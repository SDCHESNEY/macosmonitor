mod app;
mod cli;
mod collector;
mod config;
mod event;
mod process;
mod terminal;
mod ui;

use std::{fs, io, path::PathBuf, sync::mpsc, time::Duration};

use anyhow::{Context, Result};
use clap::Parser;
use crossterm::event::{MouseButton, MouseEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};
use tracing_appender::non_blocking::WorkerGuard;

use crate::{
    app::{Action, AppState},
    cli::Args,
    collector::{Collector, native::NativeCollector, sysinfo_impl::SysinfoCollector},
    event::{Event, WorkerSet, action_for_key_with_overlay},
    process::signal::UnixProcessController,
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
    let (config, config_warning) = config::load();
    if let Some(warning) = config_warning {
        tracing::warn!("{warning}");
    }
    let refresh_ms = config::effective_refresh(args.refresh_ms, &config);
    let theme = args
        .theme
        .map(|theme| match theme {
            cli::ThemeKind::Dark => config::Theme::Dark,
            cli::ThemeKind::Light => config::Theme::Light,
        })
        .unwrap_or_else(|| match config.theme.to_lowercase().as_str() {
            "light" => config::Theme::Light,
            _ => config::Theme::Dark,
        });
    #[cfg(target_os = "macos")]
    let use_native = args
        .collector
        .map(|collector| matches!(collector, cli::CollectorKind::Native))
        .unwrap_or(true);
    #[cfg(not(target_os = "macos"))]
    let use_native = args
        .collector
        .is_some_and(|collector| matches!(collector, cli::CollectorKind::Native));
    let collector = if use_native {
        #[cfg(target_os = "macos")]
        {
            Collector::Native(NativeCollector::new().context("initialize native collector")?)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Collector::Sysinfo(SysinfoCollector::new().context("initialize system collector")?)
        }
    } else {
        Collector::Sysinfo(SysinfoCollector::new().context("initialize system collector")?)
    };
    let _terminal_session = terminal::TerminalSession::enter()?;

    #[cfg(feature = "test-panic")]
    if args.test_panic {
        panic!("test panic requested");
    }

    let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))
        .context("initialize terminal renderer")?;
    let (sender, receiver) = mpsc::channel();
    let _workers = WorkerSet::start(sender, collector, Duration::from_millis(refresh_ms));
    let mut state = AppState::new();
    state.theme = theme;
    let process_controller = UnixProcessController::new();

    while state.running {
        let event = receiver
            .recv()
            .context("event workers stopped unexpectedly")?;
        match event {
            Event::Key(key) => {
                let action = action_for_key_with_overlay(key, state.filter_active, state.overlay);
                let confirm = matches!(action, Action::ConfirmKill);
                state.update(action);
                if confirm {
                    state.confirm_kill(&process_controller);
                }
            }
            Event::Mouse(mouse) => match mouse.kind {
                MouseEventKind::ScrollUp => state.update(Action::MoveUp),
                MouseEventKind::ScrollDown => state.update(Action::MoveDown),
                MouseEventKind::Down(MouseButton::Left) => {
                    if mouse.row >= 9 {
                        let index = (mouse.row.saturating_sub(9)) as usize;
                        state.update(Action::SelectVisibleIndex(index));
                    } else if mouse.row == 8 {
                        let key = if mouse.column < 8 {
                            process::sort::SortKey::Pid
                        } else if mouse.column < 40 {
                            process::sort::SortKey::Name
                        } else if mouse.column < 55 {
                            process::sort::SortKey::User
                        } else if mouse.column < 65 {
                            process::sort::SortKey::Cpu
                        } else if mouse.column < 85 {
                            process::sort::SortKey::Memory
                        } else {
                            process::sort::SortKey::Status
                        };
                        state.update(Action::SortBy(key));
                    }
                }
                _ => {}
            },
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
