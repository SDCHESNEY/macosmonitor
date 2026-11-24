use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc::Sender,
    },
    thread::{self, JoinHandle},
    time::Duration,
};

use crossterm::event::{
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEvent,
};

use crate::collector::{Snapshot, SystemCollector};

pub enum Event {
    Key(KeyEvent),
    Mouse(MouseEvent),
    Resize(u16, u16),
    Tick,
    Snapshot(Box<Snapshot>),
    Error(String),
}

struct Worker {
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl Worker {
    fn new(
        sender: Sender<Event>,
        worker: impl FnOnce(Sender<Event>, Arc<AtomicBool>) + Send + 'static,
    ) -> Self {
        let stop = Arc::new(AtomicBool::new(false));
        let worker_stop = Arc::clone(&stop);
        let handle = thread::spawn(move || worker(sender, worker_stop));
        Self {
            stop,
            handle: Some(handle),
        }
    }
}

impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Release);
        if let Some(handle) = self.handle.take() {
            handle.thread().unpark();
            if handle.join().is_err() {
                tracing::error!("event worker panicked");
            }
        }
    }
}

pub struct WorkerSet {
    _workers: Vec<Worker>,
}

impl WorkerSet {
    pub fn start<C: SystemCollector + 'static>(
        sender: Sender<Event>,
        collector: C,
        refresh_interval: Duration,
    ) -> Self {
        let input_sender = sender.clone();
        let input = Worker::new(input_sender, input_loop);

        let tick_sender = sender.clone();
        let tick = Worker::new(tick_sender, move |sender, stop| {
            periodic_loop(sender, stop, Duration::from_secs(1), || Event::Tick);
        });

        let sampler = Worker::new(sender, move |sender, stop| {
            sampling_loop(sender, stop, collector, refresh_interval);
        });

        Self {
            _workers: vec![input, tick, sampler],
        }
    }
}

fn input_loop(sender: Sender<Event>, stop: Arc<AtomicBool>) {
    while !stop.load(Ordering::Acquire) {
        match event::poll(Duration::from_millis(100)) {
            Ok(false) => continue,
            Ok(true) => match event::read() {
                Ok(CrosstermEvent::Key(key)) if key.kind == KeyEventKind::Press => {
                    if sender.send(Event::Key(key)).is_err() {
                        break;
                    }
                }
                Ok(CrosstermEvent::Resize(width, height)) => {
                    if sender.send(Event::Resize(width, height)).is_err() {
                        break;
                    }
                }
                Ok(CrosstermEvent::Mouse(mouse)) => {
                    if sender.send(Event::Mouse(mouse)).is_err() {
                        break;
                    }
                }
                Ok(_) => {}
                Err(error) => {
                    let _ = sender.send(Event::Error(error.to_string()));
                    break;
                }
            },
            Err(error) => {
                let _ = sender.send(Event::Error(error.to_string()));
                break;
            }
        }
    }
}

fn periodic_loop(
    sender: Sender<Event>,
    stop: Arc<AtomicBool>,
    interval: Duration,
    make_event: impl Fn() -> Event,
) {
    while !stop.load(Ordering::Acquire) {
        thread::park_timeout(interval);
        if !stop.load(Ordering::Acquire) && sender.send(make_event()).is_err() {
            break;
        }
    }
}

fn sampling_loop<C: SystemCollector>(
    sender: Sender<Event>,
    stop: Arc<AtomicBool>,
    mut collector: C,
    interval: Duration,
) {
    while !stop.load(Ordering::Acquire) {
        if sender
            .send(Event::Snapshot(Box::new(collector.refresh())))
            .is_err()
        {
            break;
        }
        thread::park_timeout(interval);
    }
}

#[derive(Debug, Clone, Copy)]
pub struct KeyBinding {
    pub key: &'static str,
    pub action: &'static str,
}

pub const KEY_BINDINGS: &[KeyBinding] = &[
    KeyBinding {
        key: "↑/↓ or j/k",
        action: "Navigate processes",
    },
    KeyBinding {
        key: "Enter",
        action: "Open process detail",
    },
    KeyBinding {
        key: "/",
        action: "Filter processes",
    },
    KeyBinding {
        key: "p/n/u/c/m",
        action: "Sort by PID/name/user/CPU/memory",
    },
    KeyBinding {
        key: "r",
        action: "Reverse sort",
    },
    KeyBinding {
        key: "o",
        action: "Cycle sort",
    },
    KeyBinding {
        key: "t",
        action: "Toggle tree view",
    },
    KeyBinding {
        key: "k",
        action: "Request SIGTERM",
    },
    KeyBinding {
        key: "s",
        action: "Open signal picker",
    },
    KeyBinding {
        key: "Space",
        action: "Pause/resume sampling",
    },
    KeyBinding {
        key: "?",
        action: "Open help",
    },
    KeyBinding {
        key: "q/Esc",
        action: "Quit or close popup",
    },
];

pub fn action_for_key(key: KeyEvent, filter_active: bool) -> crate::app::Action {
    use crate::app::Action;
    use crate::process::sort::SortKey;

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }

    if filter_active {
        return match key.code {
            KeyCode::Esc => Action::FilterCancel,
            KeyCode::Enter => Action::FilterCommit,
            KeyCode::Backspace => Action::FilterBackspace,
            KeyCode::Char(character) if !key.modifiers.contains(KeyModifiers::CONTROL) => {
                Action::FilterInput(character)
            }
            _ => Action::Noop,
        };
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Up => Action::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => Action::MoveDown,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::Char('/') => Action::EnterFilter,
        KeyCode::Char(' ') => Action::TogglePause,
        KeyCode::Char('p') => Action::SortBy(SortKey::Pid),
        KeyCode::Char('n') => Action::SortBy(SortKey::Name),
        KeyCode::Char('u') => Action::SortBy(SortKey::User),
        KeyCode::Char('c') => Action::SortBy(SortKey::Cpu),
        KeyCode::Char('m') => Action::SortBy(SortKey::Memory),
        KeyCode::Char('t') => Action::ToggleTree,
        KeyCode::Char('r') => Action::ToggleSortOrder,
        KeyCode::Char('o') => Action::CycleSort,
        KeyCode::Enter => Action::OpenDetail,
        KeyCode::Char('k') => Action::RequestKill,
        KeyCode::Char('s') => Action::OpenSignalPicker,
        KeyCode::Char('?') => Action::OpenHelp,
        _ => Action::Noop,
    }
}

pub fn action_for_key_with_overlay(
    key: KeyEvent,
    filter_active: bool,
    overlay: crate::app::OverlayMode,
) -> crate::app::Action {
    use crate::app::{Action, OverlayMode};

    match overlay {
        OverlayMode::Detail | OverlayMode::Help => match key.code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Enter => Action::CloseOverlay,
            _ => Action::Noop,
        },
        OverlayMode::KillConfirm | OverlayMode::ExtraConfirm => match key.code {
            KeyCode::Enter | KeyCode::Char('y') => Action::ConfirmKill,
            KeyCode::Esc | KeyCode::Char('n') => Action::CancelKill,
            _ => Action::Noop,
        },
        OverlayMode::SignalPicker => match key.code {
            KeyCode::Esc | KeyCode::Char('n') => Action::CancelKill,
            KeyCode::Char('1') => Action::SelectSignal(crate::process::signal::Signal::Term),
            KeyCode::Char('2') => Action::SelectSignal(crate::process::signal::Signal::Kill),
            KeyCode::Char('3') => Action::SelectSignal(crate::process::signal::Signal::Int),
            KeyCode::Char('4') => Action::SelectSignal(crate::process::signal::Signal::Hup),
            KeyCode::Char('5') => Action::SelectSignal(crate::process::signal::Signal::Stop),
            KeyCode::Char('6') => Action::SelectSignal(crate::process::signal::Signal::Cont),
            _ => Action::Noop,
        },
        OverlayMode::None => action_for_key(key, filter_active),
    }
}

#[cfg(test)]
mod tests {
    use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

    use crate::{app::Action, process::sort::SortKey};

    use super::KEY_BINDINGS;
    use super::action_for_key;

    #[test]
    fn help_bindings_include_every_action_row() {
        assert!(!KEY_BINDINGS.is_empty());
        assert!(KEY_BINDINGS.iter().any(|binding| binding.key == "?"));
        assert!(KEY_BINDINGS.iter().any(|binding| binding.key == "k"));
        assert!(KEY_BINDINGS.iter().any(|binding| binding.key == "s"));
    }

    fn key(code: KeyCode, modifiers: KeyModifiers) -> KeyEvent {
        KeyEvent::new(code, modifiers)
    }

    #[test]
    fn phase_two_key_bindings_cover_filter_sort_pause_and_quit() {
        assert!(matches!(
            action_for_key(key(KeyCode::Char('/'), KeyModifiers::NONE), false),
            Action::EnterFilter
        ));
        assert!(matches!(
            action_for_key(key(KeyCode::Char('q'), KeyModifiers::NONE), true),
            Action::FilterInput('q')
        ));
        assert!(matches!(
            action_for_key(key(KeyCode::Esc, KeyModifiers::NONE), true),
            Action::FilterCancel
        ));
        assert!(matches!(
            action_for_key(key(KeyCode::Char('m'), KeyModifiers::NONE), false),
            Action::SortBy(SortKey::Memory)
        ));
        assert!(matches!(
            action_for_key(key(KeyCode::Char(' '), KeyModifiers::NONE), false),
            Action::TogglePause
        ));
        assert!(matches!(
            action_for_key(key(KeyCode::Esc, KeyModifiers::NONE), false),
            Action::Quit
        ));
    }
}
