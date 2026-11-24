use std::{ffi::CStr, mem::size_of, ptr};

use tracing::warn;

use super::{MemoryDetails, MemoryPressure};

pub(super) fn performance_core_count() -> Option<usize> {
    sysctl_value(c"hw.perflevel0.logicalcpu").map(|count: u32| count as usize)
}

pub(super) fn efficiency_core_count() -> Option<usize> {
    sysctl_value(c"hw.perflevel1.logicalcpu").map(|count: u32| count as usize)
}

pub(super) fn thread_count() -> Option<usize> {
    sysctl_value(c"kern.num_threads").map(|count: u32| count as usize)
}

#[allow(deprecated)]
pub(super) fn memory_details() -> MemoryDetails {
    let mut details = MemoryDetails::default();

    // SAFETY: the Mach API writes a vm_statistics64 value into this correctly sized,
    // initialized output buffer.
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

fn sysctl_value<T: Copy + Default>(name: &CStr) -> Option<T> {
    let mut value = T::default();
    let mut size = size_of::<T>();
    // SAFETY: `name` is a NUL-terminated sysctl name, and `value` is a writable
    // buffer whose exact size is supplied to the kernel.
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
