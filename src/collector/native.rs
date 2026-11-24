use super::sysinfo_impl::SysinfoCollector;
use super::{Snapshot, SystemCollector};

pub struct NativeCollector {
    fallback: SysinfoCollector,
}

impl NativeCollector {
    pub fn new() -> Result<Self, sysinfo::Error> {
        Ok(Self {
            fallback: SysinfoCollector::new()?,
        })
    }
}

impl SystemCollector for NativeCollector {
    fn refresh(&mut self) -> Snapshot {
        let mut snapshot = self.fallback.refresh();
        #[cfg(target_os = "macos")]
        {
            let native_pids = super::macos::native_processes();
            snapshot
                .processes
                .retain(|process| native_pids.contains(&process.pid));
            for process in &mut snapshot.processes {
                let (exe_path, resident, virtual_memory, threads) =
                    super::macos::native_process_details(process.pid);
                if let Some(exe_path) = exe_path {
                    process.exe_path = Some(exe_path);
                    let path = process.exe_path.as_ref().expect("just assigned");
                    let path_text = path.to_string_lossy();
                    process.is_system = process.is_system
                        || path_text.starts_with("/System/")
                        || path_text.starts_with("/usr/")
                        || path_text.starts_with("/sbin/");
                }
                if resident.is_some() {
                    process.mem_bytes = resident;
                }
                if virtual_memory.is_some() {
                    process.virtual_mem_bytes = virtual_memory;
                }
                if threads.is_some() {
                    process.thread_count = threads;
                }
            }
            if let Some(memsize) = super::macos::memsize() {
                snapshot.memory.total = memsize;
            }
        }
        snapshot
    }
}

#[cfg(test)]
mod tests {
    use super::NativeCollector;

    #[test]
    fn native_collector_initializes() {
        assert!(NativeCollector::new().is_ok());
    }
}
