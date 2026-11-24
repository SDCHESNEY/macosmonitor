use std::time::{Duration, Instant};

use sysinfo::ProcessStatus as SysinfoProcessStatus;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};
use tracing::warn;

use super::{CoreKind, CoreUsage, MemoryStats, ProcessInfo, Snapshot, SystemCollector};

pub struct SysinfoCollector {
    system: System,
    users: Users,
    has_cpu_sample: bool,
}

impl SysinfoCollector {
    pub fn new() -> Result<Self, sysinfo::Error> {
        Ok(Self {
            system: System::new()?,
            users: Users::new_with_refreshed_list()?,
            has_cpu_sample: false,
        })
    }

    fn refresh_system(&mut self) {
        self.system.refresh_cpu_usage();
        self.system.refresh_memory();
        self.system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_cpu()
                .with_memory()
                .with_cmd(UpdateKind::OnlyIfNotSet)
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_tasks()
                .with_user(UpdateKind::OnlyIfNotSet),
        );
    }
}

impl SystemCollector for SysinfoCollector {
    fn refresh(&mut self) -> Snapshot {
        self.refresh_system();
        let cpu_total = self.has_cpu_sample.then(|| self.system.global_cpu_usage());
        let processes = self
            .system
            .processes()
            .iter()
            .map(|(pid, process)| {
                let memory = process.memory();
                let status = process.status();
                ProcessInfo {
                    pid: pid.as_u32(),
                    parent: process.parent().map(|parent| parent.as_u32()),
                    name: process.name().to_string_lossy().into_owned(),
                    command: process
                        .cmd()
                        .iter()
                        .map(|part| part.to_string_lossy())
                        .collect::<Vec<_>>()
                        .join(" "),
                    exe_path: process.exe().map(std::path::Path::to_path_buf),
                    user: process
                        .user_id()
                        .and_then(|uid| self.users.get_user_by_id(uid))
                        .map(|user| user.name().to_owned()),
                    uid: process
                        .user_id()
                        .map(|uid| uid.to_string().parse().unwrap_or_default()),
                    cpu_percent: self.has_cpu_sample.then(|| process.cpu_usage()),
                    mem_bytes: (memory > 0).then_some(memory),
                    virtual_mem_bytes: {
                        let virtual_memory = process.virtual_memory();
                        (virtual_memory > 0).then_some(virtual_memory)
                    },
                    status: match status {
                        SysinfoProcessStatus::Unknown(_) => "Unknown".to_owned(),
                        _ => format!("{status:?}"),
                    },
                    start_time: process.start_time(),
                    thread_count: process.tasks().map(std::collections::HashSet::len),
                    is_system: process.exe().is_some_and(|path| {
                        let path = path.to_string_lossy();
                        path.starts_with("/System/")
                            || path.starts_with("/usr/")
                            || path.starts_with("/sbin/")
                    }) || process.user_id().is_some_and(|uid| uid.to_string() == "0"),
                }
            })
            .collect::<Vec<_>>();
        let mut processes = processes;
        processes.sort_unstable_by_key(|process| process.pid);

        let load_avg = match System::load_average() {
            Ok(load) => Some((load.one, load.five, load.fifteen)),
            Err(error) => {
                warn!(%error, "load average unavailable");
                None
            }
        };
        let uptime = match System::uptime() {
            Ok(seconds) => Some(Duration::from_secs(seconds)),
            Err(error) => {
                warn!(%error, "system uptime unavailable");
                None
            }
        };

        #[cfg(target_os = "macos")]
        let core_counts =
            super::macos::performance_core_count().zip(super::macos::efficiency_core_count());
        let cpu_per_core = self
            .system
            .cpus()
            .iter()
            .enumerate()
            .map(|(index, cpu)| {
                #[cfg(target_os = "macos")]
                let kind = match core_counts {
                    Some((performance, _)) if index < performance => CoreKind::Performance,
                    Some((performance, efficiency)) if index < performance + efficiency => {
                        CoreKind::Efficiency
                    }
                    _ => CoreKind::Unknown,
                };
                #[cfg(not(target_os = "macos"))]
                let kind = CoreKind::Unknown;
                CoreUsage {
                    index,
                    kind,
                    usage_percent: self.has_cpu_sample.then(|| cpu.usage()),
                }
            })
            .collect();
        #[cfg(target_os = "macos")]
        let memory_details = super::macos::memory_details();
        #[cfg(not(target_os = "macos"))]
        let memory_details = super::MemoryDetails::default();
        #[cfg(target_os = "macos")]
        let thread_count = super::macos::thread_count();
        #[cfg(not(target_os = "macos"))]
        let thread_count = Some(
            self.system
                .processes()
                .values()
                .filter_map(|process| process.tasks())
                .map(std::collections::HashSet::len)
                .sum::<usize>(),
        );

        let snapshot = Snapshot {
            taken_at: Instant::now(),
            cpu_total,
            cpu_per_core,
            memory: MemoryStats {
                total: self.system.total_memory(),
                used: memory_details
                    .used
                    .unwrap_or_else(|| self.system.used_memory()),
                wired: memory_details.wired,
                compressed: memory_details.compressed,
                cached: memory_details.cached,
                pressure: memory_details.pressure,
            },
            swap_used: self.system.used_swap(),
            swap_total: self.system.total_swap(),
            load_avg,
            uptime,
            thread_count,
            processes,
        };
        self.has_cpu_sample = true;
        snapshot
    }
}
