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
    self, Event as CrosstermEvent, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
};

use crate::collector::{Snapshot, SystemCollector};

pub enum Event {
    Key(KeyEvent),
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

pub fn action_for_key(key: KeyEvent) -> crate::app::Action {
    use crate::app::Action;

    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    match key.code {
        KeyCode::Char('q') | KeyCode::Esc => Action::Quit,
        KeyCode::Up | KeyCode::Char('k') => Action::MoveUp,
        KeyCode::Down | KeyCode::Char('j') => Action::MoveDown,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        _ => Action::Noop,
    }
}
