# rtop

`rtop` is a terminal system monitor in the spirit of `top`, `htop`, and Activity
Monitor. It is designed for macOS, with a portable Linux implementation.

> **Status:** Phase 3 is implemented: process control, PID-safe signalling,
> process details and a parent/child tree view.

## Platform support

macOS is the primary target, with Apple Silicon first and Intel macOS also
supported. Linux is a secondary target and is checked in CI. Windows is not
targeted.

## Development

Requires Rust 1.95 or newer.

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

Run the interactive process list with `cargo run --release`. Sampling defaults
to one second; set `--refresh-ms` to change it (minimum 250 ms). Use the arrow
keys or `j`/`k` to navigate; `p`, `n`, `u`, `c`, `m`, and `t` sort by PID, name,
user, CPU, memory, and status. Press the active sort key again to reverse the
order. `/` filters by name or command; use `pid:`, `user:`, or `path:` for a
specific field. `Enter` applies the filter and `Esc` clears it. Press `Space`
to pause/resume updates, and `q`, `Esc`, or `Ctrl-C` to exit.
Press `Enter` for the selected process detail view, `k` to request SIGTERM,
and `s` to choose SIGTERM, SIGKILL, SIGINT, SIGHUP, SIGSTOP or SIGCONT. Signal
requests require `Enter`/`y` confirmation (or `n`/`Esc` cancellation). Press
`t` to switch between sorted and parent/child tree views.

The event loop uses `std::thread` and `std::sync::mpsc`; an async runtime is not
needed for this blocking workload. The first CPU reading is intentionally shown
as `—` while the collector gathers a baseline. Fields the OS does not expose
are shown as `—`. On macOS, logs are written to
`~/Library/Logs/rtop/rtop.log`; on Linux, they are written to
`~/.local/state/rtop/rtop.log`.

The hidden `test-panic` feature provides a panic path for checking terminal
restoration: run with `cargo run --features test-panic -- --test-panic`.

On macOS, memory used is wired + active + compressed; wired and compressor
pages come from Mach VM statistics, cached memory is reported from Darwin's
external-page count, swap comes from `sysinfo`, and pressure comes from the
kernel pressure-level sysctl. On Linux, unavailable Apple-specific fields show
as `—`. CPU and memory history retain at most 60 samples.

On this Apple Silicon (`arm64`) host, `top -l 2` reported 0.8% CPU for the
optimized monitor at a 1-second refresh in a short spot check. This is a
single-session observation, not a benchmark.

## Process control and macOS permissions

Sending a signal always requires explicit confirmation. `k` requests SIGTERM;
`s` opens the signal picker. rtop refuses PID 1 and its own PID, and verifies
the process start time before signalling to reduce PID-reuse races. Root-owned
processes and processes whose executable is under `/System`, `/usr`, or `/sbin`
require a second warning confirmation. Processes owned by another user display
a permission warning and are typically rejected with `EPERM` by macOS unless
the terminal is running with suitable privileges.

SIGTERM allows a graphical application or service to handle shutdown normally.
SIGKILL is immediate and cannot be handled. Stopped processes may be resumed
with SIGCONT. launchd-managed daemons can be restarted automatically after a
signal, depending on their service configuration.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE). 
