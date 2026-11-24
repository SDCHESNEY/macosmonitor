use std::{ffi::CStr, mem::size_of, ptr};

use tracing::warn;

use crate::collector::{MemoryDetails, MemoryPressure};

#[repr(C)]
struct ProcTaskInfo {
    virtual_size: u64,
    resident_size: u64,
    total_user: u64,
    total_system: u64,
    threads_user: u64,
    threads_system: u64,
    policy: i32,
    suspend_count: i32,
    thread_count: i32,
    fault_count: i32,
    pageins: i32,
    cow_faults: i32,
    messages_sent: i32,
    messages_received: i32,
    syscalls_mach: i32,
    syscalls_unix: i32,
    csw: i32,
    page_count: i32,
}

unsafe extern "C" {
    fn proc_listpids(proc_type: u32, type_info: u32, buffer: *mut u32, buffer_size: i32) -> i32;
    fn proc_pidinfo(
        pid: i32,
        flavor: i32,
        arg: u64,
        buffer: *mut std::ffi::c_void,
        buffer_size: i32,
    ) -> i32;
    fn proc_pidpath(pid: i32, buffer: *mut u8, buffer_size: u32) -> i32;
}

pub(super) fn sysctl_value<T: Copy + Default>(name: &CStr) -> Option<T> {
    let mut value = T::default();
    let mut size = size_of::<T>();
    // SAFETY: `name` is NUL-terminated, `value` is writable for exactly `size`,
    // and sysctlbyname only writes through those two kernel-provided lengths.
    let result = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&mut value as *mut T).cast(),
            &mut size,
            ptr::null_mut(),
            0,
        )
    };
    (result == 0 && size == size_of::<T>()).then_some(value)
}

#[allow(deprecated)]
pub(super) fn memory_details() -> MemoryDetails {
    let mut details = MemoryDetails::default();

    // SAFETY: host_statistics64 receives a zero-initialized, correctly sized
    // vm_statistics64 buffer and a matching HOST_VM_INFO64_COUNT argument.
    unsafe {
        let mut stats: libc::vm_statistics64 = std::mem::zeroed();
        let mut count = libc::HOST_VM_INFO64_COUNT;
        let result = libc::host_statistics64(
            libc::mach_host_self(),
            libc::HOST_VM_INFO64,
            (&mut stats as *mut libc::vm_statistics64).cast(),
            &mut count,
        );
        if result == libc::KERN_SUCCESS {
            let page_size = libc::sysconf(libc::_SC_PAGESIZE);
            if page_size > 0 {
                let page_size = page_size as u64;
                details.wired = Some(u64::from(stats.wire_count).saturating_mul(page_size));
                details.compressed =
                    Some(u64::from(stats.compressor_page_count).saturating_mul(page_size));
                details.cached =
                    Some(u64::from(stats.external_page_count).saturating_mul(page_size));
                details.used = Some(
                    u64::from(stats.wire_count)
                        .saturating_add(u64::from(stats.active_count))
                        .saturating_add(u64::from(stats.compressor_page_count))
                        .saturating_mul(page_size),
                );
            }
        } else {
            warn!(result, "Mach memory statistics unavailable");
        }
    }

    details.pressure =
        sysctl_value::<u32>(c"kern.memorystatus_vm_pressure_level").and_then(|level| match level {
            1 => Some(MemoryPressure::Normal),
            2 => Some(MemoryPressure::Warning),
            4 => Some(MemoryPressure::Critical),
            _ => None,
        });
    details
}

pub(super) fn native_process_pids() -> Vec<u32> {
    const PROC_ALL_PIDS: u32 = 1;
    let mut capacity = 4096;
    loop {
        let mut pids = vec![0_u32; capacity];
        // SAFETY: buffer is writable and its byte size is passed exactly to proc_listpids.
        let result = unsafe {
            proc_listpids(
                PROC_ALL_PIDS,
                0,
                pids.as_mut_ptr(),
                (pids.len() * size_of::<u32>()) as i32,
            )
        };
        if result <= 0 {
            return Vec::new();
        }
        let count = (result as usize) / size_of::<u32>();
        pids.truncate(count);
        pids.retain(|pid| *pid != 0);
        if count < capacity {
            return pids;
        }
        capacity = capacity.saturating_mul(2).min(65_536);
    }
}

pub(super) fn native_process_details(
    pid: u32,
) -> (
    Option<std::path::PathBuf>,
    Option<u64>,
    Option<u64>,
    Option<usize>,
) {
    let mut task = ProcTaskInfo {
        virtual_size: 0,
        resident_size: 0,
        total_user: 0,
        total_system: 0,
        threads_user: 0,
        threads_system: 0,
        policy: 0,
        suspend_count: 0,
        thread_count: 0,
        fault_count: 0,
        pageins: 0,
        cow_faults: 0,
        messages_sent: 0,
        messages_received: 0,
        syscalls_mach: 0,
        syscalls_unix: 0,
        csw: 0,
        page_count: 0,
    };
    // SAFETY: PROC_PIDTASKINFO_SIZE is matched by this initialized repr(C) output.
    let task_result = unsafe {
        proc_pidinfo(
            pid as i32,
            4,
            0,
            (&mut task as *mut ProcTaskInfo).cast(),
            size_of::<ProcTaskInfo>() as i32,
        )
    };

    let mut path_bytes = vec![0_u8; 4096];
    // SAFETY: the output buffer is writable and 4096 is passed as its byte size.
    let path_result =
        unsafe { proc_pidpath(pid as i32, path_bytes.as_mut_ptr(), path_bytes.len() as u32) };
    let exe_path = (path_result > 0).then(|| {
        let length = path_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(path_bytes.len());
        std::path::PathBuf::from(String::from_utf8_lossy(&path_bytes[..length]).into_owned())
    });

    if task_result > 0 {
        (
            exe_path,
            (task.resident_size > 0).then_some(task.resident_size),
            (task.virtual_size > 0).then_some(task.virtual_size),
            (task.thread_count > 0).then_some(task.thread_count as usize),
        )
    } else {
        (exe_path, None, None, None)
    }
}
