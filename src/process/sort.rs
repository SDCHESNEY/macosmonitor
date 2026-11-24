use std::cmp::Ordering;

use crate::collector::ProcessInfo;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortKey {
    Pid,
    Name,
    User,
    Cpu,
    Memory,
    Status,
}

impl SortKey {
    pub const ALL: [Self; 6] = [
        Self::Pid,
        Self::Name,
        Self::User,
        Self::Cpu,
        Self::Memory,
        Self::Status,
    ];

    pub fn default_descending(self) -> bool {
        matches!(self, Self::Cpu | Self::Memory)
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Pid => "PID",
            Self::Name => "Name",
            Self::User => "User",
            Self::Cpu => "CPU",
            Self::Memory => "Memory",
            Self::Status => "Status",
        }
    }
}

pub fn sorted_indices(processes: &[ProcessInfo], key: SortKey, descending: bool) -> Vec<usize> {
    let mut indices = (0..processes.len()).collect::<Vec<_>>();
    indices.sort_by(|left, right| {
        let left_process = &processes[*left];
        let right_process = &processes[*right];
        compare(left_process, right_process, key, descending)
            .then_with(|| left_process.pid.cmp(&right_process.pid))
    });
    indices
}

fn compare(left: &ProcessInfo, right: &ProcessInfo, key: SortKey, descending: bool) -> Ordering {
    match key {
        SortKey::Pid => direction(left.pid.cmp(&right.pid), descending),
        SortKey::Name => direction(
            left.name.to_lowercase().cmp(&right.name.to_lowercase()),
            descending,
        ),
        SortKey::User => compare_optional_order(
            left.user.as_deref().map(str::to_lowercase),
            right.user.as_deref().map(str::to_lowercase),
            descending,
        ),
        SortKey::Cpu => compare_optional_float(left.cpu_percent, right.cpu_percent, descending),
        SortKey::Memory => compare_optional_order(left.mem_bytes, right.mem_bytes, descending),
        SortKey::Status => direction(left.status.cmp(&right.status), descending),
    }
}

fn direction(ordering: Ordering, descending: bool) -> Ordering {
    if descending {
        ordering.reverse()
    } else {
        ordering
    }
}

fn compare_optional_order<T: Ord>(left: Option<T>, right: Option<T>, descending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => direction(left.cmp(&right), descending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

fn compare_optional_float(left: Option<f32>, right: Option<f32>, descending: bool) -> Ordering {
    match (left, right) {
        (Some(left), Some(right)) => direction(left.total_cmp(&right), descending),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

#[cfg(test)]
mod tests {
    use crate::collector::ProcessInfo;

    use super::{SortKey, sorted_indices};

    fn process(pid: u32, name: &str, cpu: f32, memory: u64) -> ProcessInfo {
        ProcessInfo {
            pid,
            parent: Some(1),
            name: name.to_owned(),
            command: String::new(),
            exe_path: None,
            user: Some(format!("user-{pid}")),
            uid: Some(1000),
            cpu_percent: Some(cpu),
            mem_bytes: Some(memory),
            virtual_mem_bytes: Some(memory * 2),
            status: if pid == 2 { "Run" } else { "Sleep" }.to_owned(),
            start_time: pid as u64,
            thread_count: Some(1),
            is_system: false,
        }
    }

    fn pids(processes: &[ProcessInfo], key: SortKey, descending: bool) -> Vec<u32> {
        sorted_indices(processes, key, descending)
            .into_iter()
            .map(|index| processes[index].pid)
            .collect()
    }

    #[test]
    fn sorting_is_repeatable_and_keeps_pid_ties_ascending() {
        let processes = vec![
            process(3, "same", 10.0, 100),
            process(1, "same", 10.0, 100),
            process(2, "other", 20.0, 50),
        ];

        assert_eq!(
            pids(&processes, SortKey::Name, false),
            pids(&processes, SortKey::Name, false)
        );
        assert_eq!(pids(&processes, SortKey::Name, false), vec![2, 1, 3]);
        for key in SortKey::ALL {
            let mut sorted = pids(&processes, key, false);
            sorted.sort_unstable();
            assert_eq!(sorted, vec![1, 2, 3]);
        }
    }

    #[test]
    fn every_sort_key_works_in_both_directions() {
        let processes = vec![
            process(3, "charlie", 10.0, 100),
            process(1, "alpha", 30.0, 300),
            process(2, "bravo", 20.0, 200),
        ];
        let expected = [
            (SortKey::Pid, vec![1, 2, 3], vec![3, 2, 1]),
            (SortKey::Name, vec![1, 2, 3], vec![3, 2, 1]),
            (SortKey::User, vec![1, 2, 3], vec![3, 2, 1]),
            (SortKey::Cpu, vec![3, 2, 1], vec![1, 2, 3]),
            (SortKey::Memory, vec![3, 2, 1], vec![1, 2, 3]),
            (SortKey::Status, vec![2, 1, 3], vec![1, 3, 2]),
        ];
        for (key, ascending, descending) in expected {
            assert_eq!(pids(&processes, key, false), ascending, "ascending {key:?}");
            assert_eq!(
                pids(&processes, key, true),
                descending,
                "descending {key:?}"
            );
        }
    }

    #[test]
    fn missing_metrics_stay_last_in_both_directions() {
        let mut processes = vec![
            process(1, "one", 10.0, 100),
            process(2, "two", 20.0, 200),
            process(3, "three", 30.0, 300),
        ];
        processes[1].cpu_percent = None;
        processes[1].mem_bytes = None;

        assert_eq!(pids(&processes, SortKey::Cpu, true), vec![3, 1, 2]);
        assert_eq!(pids(&processes, SortKey::Memory, false), vec![1, 3, 2]);
    }

    #[test]
    fn sorting_is_idempotent_and_preserves_every_pid_across_input_orders() {
        let originals = [
            process(3, "charlie", 10.0, 100),
            process(1, "alpha", 30.0, 300),
            process(2, "bravo", 20.0, 200),
        ];
        let mut input_orders = Vec::new();
        for first in 0..originals.len() {
            for second in 0..originals.len() {
                for third in 0..originals.len() {
                    if first != second && first != third && second != third {
                        input_orders.push(vec![
                            originals[first].clone(),
                            originals[second].clone(),
                            originals[third].clone(),
                        ]);
                    }
                }
            }
        }

        for processes in input_orders {
            for key in SortKey::ALL {
                for descending in [false, true] {
                    let sorted = sorted_indices(&processes, key, descending)
                        .into_iter()
                        .map(|index| processes[index].clone())
                        .collect::<Vec<_>>();
                    let sorted_pids = sorted.iter().map(|process| process.pid).collect::<Vec<_>>();
                    let mut preserved_pids = sorted_pids.clone();
                    preserved_pids.sort_unstable();
                    assert_eq!(preserved_pids, vec![1, 2, 3]);
                    assert_eq!(
                        pids(&sorted, key, descending),
                        sorted_pids,
                        "idempotence for {key:?}, descending={descending}"
                    );
                }
            }
        }
    }
}
