pub mod sysinfo_impl;

use std::{time::Duration, time::Instant};

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub taken_at: Instant,
    pub cpu_total: Option<f32>,
    pub memory: MemoryStats,
    pub swap_used: u64,
    pub swap_total: u64,
    pub load_avg: Option<(f64, f64, f64)>,
    pub uptime: Option<Duration>,
    pub processes: Vec<ProcessInfo>,
}

impl Default for Snapshot {
    fn default() -> Self {
        Self {
            taken_at: Instant::now(),
            cpu_total: None,
            memory: MemoryStats::default(),
            swap_used: 0,
            swap_total: 0,
            load_avg: None,
            uptime: None,
            processes: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryStats {
    pub total: u64,
    pub used: u64,
}

#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub user: Option<String>,
    pub cpu_percent: Option<f32>,
    pub mem_bytes: Option<u64>,
    pub status: String,
}

pub trait SystemCollector: Send {
    fn refresh(&mut self) -> Snapshot;
}

#[cfg(test)]
mod tests {
    use super::{Snapshot, SystemCollector};

    struct MockCollector {
        snapshot: Snapshot,
        calls: usize,
    }

    impl SystemCollector for MockCollector {
        fn refresh(&mut self) -> Snapshot {
            self.calls += 1;
            self.snapshot.clone()
        }
    }

    #[test]
    fn collector_contract_returns_snapshots() {
        let expected = Snapshot::default();
        let mut collector = MockCollector {
            snapshot: expected.clone(),
            calls: 0,
        };

        let actual = collector.refresh();

        assert_eq!(collector.calls, 1);
        assert!(actual.processes.is_empty());
        assert_eq!(actual.cpu_total, expected.cpu_total);
    }
}
