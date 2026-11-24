use std::fmt;

use nix::errno::Errno;
use nix::sys::signal::{self as nix_signal, Signal as NixSignal};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Signal {
    Term,
    Kill,
    Int,
    Hup,
    Stop,
    Cont,
}

impl Signal {
    #[cfg(test)]
    pub const ALL: [Self; 6] = [
        Self::Term,
        Self::Kill,
        Self::Int,
        Self::Hup,
        Self::Stop,
        Self::Cont,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Term => "SIGTERM",
            Self::Kill => "SIGKILL",
            Self::Int => "SIGINT",
            Self::Hup => "SIGHUP",
            Self::Stop => "SIGSTOP",
            Self::Cont => "SIGCONT",
        }
    }

    fn nix(self) -> NixSignal {
        match self {
            Self::Term => NixSignal::SIGTERM,
            Self::Kill => NixSignal::SIGKILL,
            Self::Int => NixSignal::SIGINT,
            Self::Hup => NixSignal::SIGHUP,
            Self::Stop => NixSignal::SIGSTOP,
            Self::Cont => NixSignal::SIGCONT,
        }
    }
}

impl fmt::Display for Signal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.name())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum KillError {
    #[error("No such process")]
    NoSuchProcess,
    #[error(
        "Permission denied: owned by root or another user; not permitted without elevated privileges"
    )]
    PermissionDenied,
    #[error("The process already exited or the PID was reused")]
    PidReused,
    #[error("PID 1 (launchd) is protected")]
    ProtectedLaunchd,
    #[error("The rtop monitor process is protected")]
    ProtectedMonitor,
    #[error("Operation not permitted")]
    Other(i32),
}

impl KillError {
    pub fn status_message(self) -> String {
        match self {
            Self::NoSuchProcess => "No such process".to_owned(),
            Self::PermissionDenied => {
                "Permission denied (owned by root or another user; elevated privileges may be required)"
                    .to_owned()
            }
            Self::PidReused => "Process changed; PID reuse prevented".to_owned(),
            Self::ProtectedLaunchd => "Cannot signal PID 1 (launchd)".to_owned(),
            Self::ProtectedMonitor => "Cannot signal the rtop monitor process".to_owned(),
            Self::Other(code) => format!("Signal failed (error {code})"),
        }
    }

    fn from_errno(errno: Errno) -> Self {
        match errno {
            Errno::ESRCH => Self::NoSuchProcess,
            Errno::EPERM => Self::PermissionDenied,
            _ => Self::Other(errno as i32),
        }
    }
}

pub trait ProcessController: Send + Sync {
    fn send(&self, pid: u32, expected_start: u64, signal: Signal) -> Result<(), KillError>;
}

#[derive(Debug, Clone, Copy)]
pub struct UnixProcessController {
    monitor_pid: u32,
}

impl UnixProcessController {
    pub fn new() -> Self {
        Self {
            monitor_pid: std::process::id(),
        }
    }

    #[cfg(test)]
    pub fn with_monitor_pid(monitor_pid: u32) -> Self {
        Self { monitor_pid }
    }

    fn current_start_time(pid: u32) -> Option<u64> {
        let mut system = sysinfo::System::new().ok()?;
        system.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        system
            .process(sysinfo::Pid::from_u32(pid))
            .map(sysinfo::Process::start_time)
    }

    #[cfg(test)]
    pub fn pid_is_reused(pid: u32, expected_start: u64) -> bool {
        match Self::current_start_time(pid) {
            Some(current) => current != expected_start,
            None => expected_start != 0,
        }
    }
}

impl Default for UnixProcessController {
    fn default() -> Self {
        Self::new()
    }
}

impl ProcessController for UnixProcessController {
    fn send(&self, pid: u32, expected_start: u64, signal: Signal) -> Result<(), KillError> {
        if pid == 1 {
            return Err(KillError::ProtectedLaunchd);
        }
        if pid == self.monitor_pid {
            return Err(KillError::ProtectedMonitor);
        }
        if expected_start != 0
            && match Self::current_start_time(pid) {
                Some(current) => current != expected_start,
                None => true,
            }
        {
            return Err(KillError::PidReused);
        }

        nix_signal::kill(nix::unistd::Pid::from_raw(pid as i32), signal.nix())
            .map_err(KillError::from_errno)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct MockController;

    impl ProcessController for MockController {
        fn send(&self, pid: u32, expected_start: u64, signal: Signal) -> Result<(), KillError> {
            if pid == 999 {
                return Err(KillError::PidReused);
            }
            let _ = (expected_start, signal);
            Ok(())
        }
    }

    #[test]
    fn pid_one_and_monitor_pid_are_protected() {
        let controller = UnixProcessController::with_monitor_pid(42);
        assert_eq!(
            controller.send(1, 0, Signal::Term),
            Err(KillError::ProtectedLaunchd)
        );
        assert_eq!(
            controller.send(42, 0, Signal::Term),
            Err(KillError::ProtectedMonitor)
        );
    }

    #[test]
    fn mock_controller_can_simulate_pid_reuse() {
        let controller = MockController;
        assert_eq!(
            controller.send(999, 123, Signal::Term),
            Err(KillError::PidReused)
        );
    }

    #[test]
    fn typed_errors_have_status_messages() {
        assert_eq!(KillError::NoSuchProcess.status_message(), "No such process");
        assert!(
            KillError::PermissionDenied
                .status_message()
                .contains("Permission denied")
        );
    }

    #[test]
    fn signal_picker_contains_every_supported_signal() {
        assert_eq!(Signal::ALL.len(), 6);
        assert_eq!(Signal::ALL[5], Signal::Cont);
    }

    #[test]
    fn pid_reuse_helper_reports_nonmatching_start_time() {
        assert!(UnixProcessController::pid_is_reused(u32::MAX, 123));
    }

    #[test]
    fn real_child_process_receives_sigterm() {
        let mut child = std::process::Command::new("sleep")
            .arg("30")
            .spawn()
            .expect("spawn sleep");
        let controller = UnixProcessController::new();
        controller
            .send(child.id(), 0, Signal::Term)
            .expect("send SIGTERM");
        let status = child.wait().expect("wait for sleep");
        assert!(!status.success());
    }
}
