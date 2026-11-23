use crate::collector::Snapshot;

#[derive(Debug)]
pub enum Action {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    Quit,
    Noop,
    SnapshotReceived(Box<Snapshot>),
}

#[derive(Debug)]
pub struct AppState {
    pub snapshot: Option<Snapshot>,
    pub selected_index: usize,
    pub running: bool,
}

impl AppState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn update(&mut self, action: Action) {
        match action {
            Action::Quit => self.running = false,
            Action::Noop => {}
            Action::SnapshotReceived(snapshot) => {
                self.snapshot = Some(*snapshot);
                let process_count = self.process_count();
                self.selected_index = self.selected_index.min(process_count.saturating_sub(1));
            }
            Action::MoveUp => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            Action::MoveDown => {
                let last_index = self.process_count().saturating_sub(1);
                self.selected_index = (self.selected_index + 1).min(last_index);
            }
            Action::PageUp => {
                self.selected_index = self.selected_index.saturating_sub(10);
            }
            Action::PageDown => {
                let last_index = self.process_count().saturating_sub(1);
                self.selected_index = (self.selected_index + 10).min(last_index);
            }
        }
    }

    pub fn process_count(&self) -> usize {
        self.snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.processes.len())
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            snapshot: None,
            selected_index: 0,
            running: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::collector::{ProcessInfo, Snapshot, SystemCollector};

    use super::{Action, AppState};

    struct MockCollector(Snapshot);

    impl SystemCollector for MockCollector {
        fn refresh(&mut self) -> Snapshot {
            self.0.clone()
        }
    }

    fn fake_snapshot(process_count: u32) -> Snapshot {
        Snapshot {
            processes: (1..=process_count)
                .map(|pid| ProcessInfo {
                    pid,
                    name: format!("process-{pid}"),
                    user: Some("user".to_owned()),
                    cpu_percent: None,
                    mem_bytes: Some(0),
                    status: "Sleep".to_owned(),
                })
                .collect(),
            ..Snapshot::default()
        }
    }

    #[test]
    fn snapshot_received_ingests_processes_from_collector() {
        let mut collector = MockCollector(fake_snapshot(2));
        let mut state = AppState::new();

        state.update(Action::SnapshotReceived(Box::new(collector.refresh())));

        assert_eq!(state.process_count(), 2);
        assert_eq!(state.snapshot.as_ref().unwrap().processes[0].pid, 1);
        assert!(state.running);
    }

    #[test]
    fn navigation_stays_within_process_rows() {
        let mut state = AppState::new();
        state.update(Action::SnapshotReceived(Box::new(fake_snapshot(12))));

        for _ in 0..15 {
            state.update(Action::MoveDown);
        }
        assert_eq!(state.selected_index, 11);

        state.update(Action::PageUp);
        assert_eq!(state.selected_index, 1);

        for _ in 0..5 {
            state.update(Action::MoveUp);
        }
        assert_eq!(state.selected_index, 0);
    }

    #[test]
    fn quit_stops_the_app() {
        let mut state = AppState::new();

        state.update(Action::Quit);

        assert!(!state.running);
    }
}
