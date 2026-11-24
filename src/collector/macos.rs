mod mach;

use super::MemoryDetails;

pub(super) fn performance_core_count() -> Option<usize> {
    mach::sysctl_value(c"hw.perflevel0.logicalcpu").map(|count: u32| count as usize)
}

pub(super) fn efficiency_core_count() -> Option<usize> {
    mach::sysctl_value(c"hw.perflevel1.logicalcpu").map(|count: u32| count as usize)
}

pub(super) fn thread_count() -> Option<usize> {
    mach::sysctl_value(c"kern.num_threads").map(|count: u32| count as usize)
}

pub(super) fn memory_details() -> MemoryDetails {
    mach::memory_details()
}

pub(super) fn memsize() -> Option<u64> {
    mach::sysctl_value::<u64>(c"hw.memsize")
}

pub(super) fn native_processes() -> Vec<u32> {
    mach::native_process_pids()
}

pub(super) fn native_process_details(
    pid: u32,
) -> (
    Option<std::path::PathBuf>,
    Option<u64>,
    Option<u64>,
    Option<usize>,
) {
    mach::native_process_details(pid)
}
