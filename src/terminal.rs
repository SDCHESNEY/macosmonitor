use std::{
    io::{self, Write},
    panic,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};

pub struct TerminalSession {
    restored: Arc<AtomicBool>,
}

impl TerminalSession {
    pub fn enter() -> Result<Self> {
        enable_raw_mode().context("enable terminal raw mode")?;
        if let Err(error) = execute!(io::stdout(), EnterAlternateScreen, crossterm::cursor::Hide) {
            let _ = restore();
            return Err(error).context("enter alternate screen");
        }
        let restored = Arc::new(AtomicBool::new(false));
        install_panic_hook(Arc::clone(&restored));
        Ok(Self { restored })
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        if !self.restored.swap(true, Ordering::AcqRel)
            && let Err(error) = restore()
        {
            tracing::error!(%error, "restore terminal");
        }
    }
}

fn restore() -> io::Result<()> {
    let raw_mode = disable_raw_mode();
    let screen = execute!(
        io::stdout(),
        LeaveAlternateScreen,
        crossterm::cursor::Show,
        crossterm::style::ResetColor
    );
    raw_mode?;
    screen?;
    io::stdout().flush()
}

fn install_panic_hook(restored: Arc<AtomicBool>) {
    let previous_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        if !restored.swap(true, Ordering::AcqRel) {
            let _ = restore();
        }
        previous_hook(panic_info);
    }));
}
