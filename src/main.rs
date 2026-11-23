mod app;
mod cli;
mod collector;
mod event;
mod terminal;
mod ui;

use std::{fs, io, path::PathBuf, sync::mpsc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing_appender::non_blocking::WorkerGuard;

use crate::{
    app::{Action, AppState},
    cli::Args,
    collector::sysinfo_impl::SysinfoCollector,
    event::{Event, WorkerSet, action_for_key},
};

fn main() -> Result<()> {
    let args = Args::parse();
    let _log_guard = init_logging()?;
    if let Err(error) = run(args) {
        tracing::error!("{error:#}");
        return Err(error);
    }
    Ok(())
}

fn run(args: Args) -> Result<()> {
    let collector = SysinfoCollector::new().context("initialize system collector")?;
    let _terminal_session = terminal::TerminalSession::enter()?;

    #[cfg(feature = "test-panic")]
    if args.test_panic {
        panic!("test panic requested");
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
        .context("initialize terminal renderer")?;
    let (sender, receiver) = mpsc::channel();
    let _workers = WorkerSet::start(sender, collector, Duration::from_millis(args.refresh_ms));
    let mut state = AppState::new();

    while state.running {
        let event = receiver
            .recv()
            .context("event workers stopped unexpectedly")?;
        match event {
            Event::Key(key) => state.update(action_for_key(key)),
            Event::Resize(width, height) => {
                tracing::debug!(width, height, "terminal resized");
            }
            Event::Tick => {}
            Event::Snapshot(snapshot) => {
                state.update(Action::SnapshotReceived(snapshot));
            }
            Event::Error(message) => anyhow::bail!("terminal input failed: {message}"),
        }

        if state.running {
            terminal
                .draw(|frame| ui::draw(frame, &state))
                .context("draw process list")?;
        }
    }

    Ok(())
}

fn init_logging() -> Result<WorkerGuard> {
    let log_dir = log_directory()?;
    fs::create_dir_all(&log_dir)
        .with_context(|| format!("create log directory {}", log_dir.display()))?;
    let file_appender = tracing_appender::rolling::never(&log_dir, "rtop.log");
    let (writer, guard) = tracing_appender::non_blocking(file_appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .map_err(|error| anyhow::anyhow!("initialize file logging: {error}"))?;
    Ok(guard)
}

fn log_directory() -> Result<PathBuf> {
    let base_dirs = directories::BaseDirs::new().context("determine home directory")?;
    #[cfg(target_os = "macos")]
    let path = base_dirs.home_dir().join("Library/Logs/rtop");
    #[cfg(not(target_os = "macos"))]
    let path = base_dirs.home_dir().join(".local/state/rtop");
    Ok(path)
}
