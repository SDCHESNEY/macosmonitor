use std::time::{Duration, Instant};

use sysinfo::ProcessStatus as SysinfoProcessStatus;
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind, Users};
use tracing::warn;

use super::{MemoryStats, ProcessInfo, Snapshot, SystemCollector};

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
                    name: process.name().to_string_lossy().into_owned(),
                    user: process
                        .user_id()
                        .and_then(|uid| self.users.get_user_by_id(uid))
                        .map(|user| user.name().to_owned()),
                    cpu_percent: self.has_cpu_sample.then(|| process.cpu_usage()),
                    mem_bytes: (memory > 0).then_some(memory),
                    status: match status {
                        SysinfoProcessStatus::Unknown(_) => "Unknown".to_owned(),
                        _ => format!("{status:?}"),
                    },
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

        let snapshot = Snapshot {
            taken_at: Instant::now(),
            cpu_total,
            memory: MemoryStats {
                total: self.system.total_memory(),
                used: self.system.used_memory(),
            },
            swap_used: self.system.used_swap(),
            swap_total: self.system.total_swap(),
            load_avg,
            uptime,
            processes,
        };
        self.has_cpu_sample = true;
        snapshot
    }
}
