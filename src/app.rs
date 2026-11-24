use std::collections::VecDeque;

use crate::{
    collector::{ProcessInfo, Snapshot},
    process::{
        filter,
        sort::{self, SortKey},
    },
};

pub const HISTORY_CAPACITY: usize = 60;

#[derive(Debug)]
pub enum Action {
    MoveUp,
    MoveDown,
    PageUp,
    PageDown,
    SortBy(SortKey),
    ToggleSortOrder,
    CycleSort,
    EnterFilter,
    FilterInput(char),
    FilterBackspace,
    FilterCommit,
    FilterCancel,
    TogglePause,
    Quit,
    Noop,
    SnapshotReceived(Box<Snapshot>),
}

#[derive(Debug)]
pub struct AppState {
    pub snapshot: Option<Snapshot>,
    pub selected_index: usize,
    pub selected_pid: Option<u32>,
    pub visible_indices: Vec<usize>,
    pub sort_key: SortKey,
    pub sort_descending: bool,
    pub filter_query: String,
    pub filter_active: bool,
    pub paused: bool,
    pub cpu_history: VecDeque<u64>,
    pub memory_history: VecDeque<u64>,
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
                if self.paused {
                    return;
                }
                let selected_pid = self.selected_pid;
                let old_index = self.selected_index;
                if let Some(cpu) = snapshot.cpu_total {
                    push_history(&mut self.cpu_history, cpu.clamp(0.0, 100.0) as u64);
                }
                if snapshot.memory.total > 0 {
                    push_history(
                        &mut self.memory_history,
                        ((snapshot.memory.used as f64 / snapshot.memory.total as f64) * 100.0)
                            .clamp(0.0, 100.0) as u64,
                    );
                }
                self.snapshot = Some(*snapshot);
                self.rebuild_visible_indices();
                self.restore_selection(selected_pid, old_index);
            }
            Action::MoveUp => {
                self.selected_index = self.selected_index.saturating_sub(1);
            }
            Action::MoveDown => {
                let last_index = self.visible_count().saturating_sub(1);
                self.selected_index = (self.selected_index + 1).min(last_index);
            }
            Action::PageUp => {
                self.selected_index = self.selected_index.saturating_sub(10);
            }
            Action::PageDown => {
                let last_index = self.visible_count().saturating_sub(1);
                self.selected_index = (self.selected_index + 10).min(last_index);
            }
            Action::SortBy(key) => {
                if self.sort_key == key {
                    self.sort_descending = !self.sort_descending;
                } else {
                    self.sort_key = key;
                    self.sort_descending = key.default_descending();
                }
                self.rebuild_visible_indices();
                self.restore_selection(self.selected_pid, self.selected_index);
            }
            Action::ToggleSortOrder => {
                self.sort_descending = !self.sort_descending;
                self.rebuild_visible_indices();
                self.restore_selection(self.selected_pid, self.selected_index);
            }
            Action::CycleSort => {
                let position = SortKey::ALL
                    .iter()
                    .position(|key| *key == self.sort_key)
                    .unwrap_or(0);
                self.sort_key = SortKey::ALL[(position + 1) % SortKey::ALL.len()];
                self.sort_descending = self.sort_key.default_descending();
                self.rebuild_visible_indices();
                self.restore_selection(self.selected_pid, self.selected_index);
            }
            Action::EnterFilter => self.filter_active = true,
            Action::FilterInput(character) => {
                self.filter_query.push(character);
                self.rebuild_visible_indices();
                self.restore_selection(self.selected_pid, self.selected_index);
            }
            Action::FilterBackspace => {
                self.filter_query.pop();
                self.rebuild_visible_indices();
                self.restore_selection(self.selected_pid, self.selected_index);
            }
            Action::FilterCommit => self.filter_active = false,
            Action::FilterCancel => {
                self.filter_query.clear();
                self.filter_active = false;
                self.rebuild_visible_indices();
                self.restore_selection(self.selected_pid, self.selected_index);
            }
            Action::TogglePause => self.paused = !self.paused,
        }
        self.selected_pid = self
            .visible_indices
            .get(self.selected_index)
            .and_then(|index| self.snapshot.as_ref()?.processes.get(*index))
            .map(|process| process.pid);
    }

    pub fn process_count(&self) -> usize {
        self.snapshot
            .as_ref()
            .map_or(0, |snapshot| snapshot.processes.len())
    }

    pub fn visible_count(&self) -> usize {
        self.visible_indices.len()
    }

    pub fn selected_process(&self) -> Option<&ProcessInfo> {
        let snapshot = self.snapshot.as_ref()?;
        let process_index = *self.visible_indices.get(self.selected_index)?;
        snapshot.processes.get(process_index)
    }

    fn rebuild_visible_indices(&mut self) {
        let Some(snapshot) = &self.snapshot else {
            self.visible_indices.clear();
            return;
        };
        let mut indices =
            sort::sorted_indices(&snapshot.processes, self.sort_key, self.sort_descending);
        indices.retain(|index| filter::matches(&snapshot.processes[*index], &self.filter_query));
        self.visible_indices = indices;
    }

    fn restore_selection(&mut self, selected_pid: Option<u32>, old_index: usize) {
        self.selected_index = selected_pid
            .and_then(|pid| {
                self.visible_indices.iter().position(|index| {
                    self.snapshot
                        .as_ref()
                        .is_some_and(|snapshot| snapshot.processes[*index].pid == pid)
                })
            })
            .unwrap_or_else(|| old_index.min(self.visible_count().saturating_sub(1)));
        self.selected_pid = self
            .visible_indices
            .get(self.selected_index)
            .and_then(|index| self.snapshot.as_ref()?.processes.get(*index))
            .map(|process| process.pid);
    }
}

fn push_history(history: &mut VecDeque<u64>, value: u64) {
    if history.len() == HISTORY_CAPACITY {
        history.pop_front();
    }
    history.push_back(value);
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            snapshot: None,
            selected_index: 0,
            selected_pid: None,
            visible_indices: Vec::new(),
            sort_key: SortKey::Cpu,
            sort_descending: true,
            filter_query: String::new(),
            filter_active: false,
            paused: false,
            cpu_history: VecDeque::with_capacity(HISTORY_CAPACITY),
            memory_history: VecDeque::with_capacity(HISTORY_CAPACITY),
            running: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        collector::{ProcessInfo, Snapshot, SystemCollector},
        process::sort::SortKey,
    };

    use super::{Action, AppState, HISTORY_CAPACITY};

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
                    command: String::new(),
                    exe_path: None,
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

    #[test]
    fn selection_tracks_pid_across_sort_and_snapshot_updates() {
        let mut state = AppState::new();
        state.update(Action::SnapshotReceived(Box::new(fake_snapshot(3))));
        state.update(Action::MoveDown);
        let selected_pid = state.selected_pid;

        state.update(Action::SortBy(SortKey::Name));
        state.update(Action::SnapshotReceived(Box::new(fake_snapshot(3))));

        assert_eq!(state.selected_pid, selected_pid);
    }

    #[test]
    fn selection_moves_to_a_valid_row_when_the_process_exits() {
        let mut state = AppState::new();
        state.update(Action::SnapshotReceived(Box::new(fake_snapshot(3))));
        state.update(Action::MoveDown);
        assert_eq!(state.selected_pid, Some(2));

        let mut next = fake_snapshot(3);
        next.processes.remove(1);
        state.update(Action::SnapshotReceived(Box::new(next)));

        assert_eq!(state.selected_index, 1);
        assert_eq!(state.selected_pid, Some(3));
    }

    #[test]
    fn selecting_the_same_sort_key_reverses_the_order() {
        let mut state = AppState::new();
        state.update(Action::SnapshotReceived(Box::new(fake_snapshot(3))));
        state.update(Action::SortBy(SortKey::Pid));
        assert!(!state.sort_descending);
        assert_eq!(state.selected_pid, Some(1));

        state.update(Action::SortBy(SortKey::Pid));
        assert!(state.sort_descending);
        assert_eq!(state.selected_pid, Some(1));
    }

    #[test]
    fn filtering_updates_live_and_cancel_clears_the_filter() {
        let mut snapshot = fake_snapshot(3);
        snapshot.processes[1].name = "Editor".to_owned();
        let mut state = AppState::new();
        state.update(Action::SnapshotReceived(Box::new(snapshot)));
        state.update(Action::EnterFilter);
        state.update(Action::FilterInput('e'));
        state.update(Action::FilterInput('D'));

        assert_eq!(state.visible_count(), 1);
        assert_eq!(
            state
                .selected_process()
                .map(|process| process.name.as_str()),
            Some("Editor")
        );

        state.update(Action::FilterCancel);
        assert_eq!(state.visible_count(), 3);
        assert!(state.filter_query.is_empty());
    }

    #[test]
    fn pause_ignores_snapshots_and_history_is_bounded() {
        let mut state = AppState::new();
        state.update(Action::TogglePause);
        state.update(Action::SnapshotReceived(Box::new(fake_snapshot(1))));

        assert!(state.snapshot.is_none());
        assert!(state.paused);

        state.update(Action::TogglePause);
        for sample in 0..HISTORY_CAPACITY + 5 {
            let mut snapshot = fake_snapshot(1);
            snapshot.cpu_total = Some(sample as f32);
            snapshot.memory.total = 100;
            snapshot.memory.used = sample as u64;
            state.update(Action::SnapshotReceived(Box::new(snapshot)));
        }
        assert_eq!(state.cpu_history.len(), HISTORY_CAPACITY);
        assert_eq!(state.memory_history.len(), HISTORY_CAPACITY);
        assert_eq!(state.cpu_history.front(), Some(&5));
    }
}
