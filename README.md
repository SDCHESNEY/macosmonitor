# rtop

`rtop` is a terminal system monitor in the spirit of `top`, `htop`, and Activity
Monitor. It is designed for macOS, with a portable Linux implementation.

## Platform support

macOS is the primary target, with Apple Silicon first and Intel macOS also
supported. Linux is a secondary target and is checked in CI. Windows is not
targeted.

## Usage

### TUI

Launch the interactive terminal monitor:

```sh
cargo run --release
# or build and run the release binary:
cargo build --release
./target/release/rtop
```

The TUI displays system and process information and refreshes once per second
by default. Use the mouse or keyboard to navigate:

| Key or mouse action | Action |
|---|---|
| `↑` / `↓` or `j` / `k` | Move the process selection |
| `Enter` | Open the selected process detail view |
| `/` | Start filtering; `Enter` applies and `Esc` clears |
| `p`, `n`, `u`, `c`, `m` | Sort by PID, name, user, CPU, or memory |
| `r` | Reverse the active sort |
| `o` | Cycle through sort fields |
| `t` | Toggle the parent/child tree view |
| `k` | Request SIGTERM for the selected process |
| `s` | Open the signal picker |
| `Space` | Pause or resume sampling |
| `?` | Open the in-app help |
| `q`, `Esc`, or `Ctrl-C` | Quit |
| Left click / scroll wheel | Select a process, sort a table header, or scroll |

Press `Enter` or `y` to confirm a signal request, or `n` or `Esc` to cancel.
The signal picker supports SIGTERM, SIGKILL, SIGINT, SIGHUP, SIGSTOP, and
SIGCONT.

### CLI

Run from the repository root. When using Cargo, put `--` before rtop options
so they are passed to the application:

```sh
cargo run --release -- --refresh-ms 500
cargo run --release -- --collector native
cargo run --release -- --collector sysinfo
cargo run --release -- --theme light
NO_COLOR=1 cargo run --release
```

If `--collector` is reported as an unexpected argument, rebuild with
`cargo build --release` and run `./target/release/rtop --help` to verify the
available options. A bare `rtop` command may resolve to an older installation
on your `PATH`; use `which -a rtop` to see the matching executables.

Available options:

| Option | Description | Default |
|---|---|---|
| `--refresh-ms <MILLISECONDS>` | Sampling interval; minimum 250 ms | 1000 ms, or `refresh_ms` from config |
| `--collector <native\|sysinfo>` | Select the data collector | Native on macOS; sysinfo on Linux |
| `--theme <dark\|light>` | Select the terminal palette | Config value or dark |
| `--help` | Print CLI usage | |
| `--version` | Print the rtop version | |

Configuration is read from `~/.config/rtop/config.toml`. CLI options take
precedence over config, and config takes precedence over built-in defaults.
Example:

```toml
refresh_ms = 1000
default_sort = "cpu"
theme = "dark"
visible_columns = ["pid", "name", "user", "cpu", "memory", "status"]
```

## Development

Requires Rust 1.95 or newer.

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
```

The interactive process list and CLI options are described in [Usage](#usage).

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

## macOS native collection

The Darwin collector combines Mach and `libproc` data with the `sysinfo`
baseline: `proc_listpids`, `proc_pidinfo(PROC_PIDTASKINFO)`, `proc_pidpath`,
Mach VM statistics, and `sysctl` values supply native process paths, resident/
virtual memory, thread counts, memory pressure and core information. Per-process
permission failures are ignored instead of aborting a snapshot.

All unsafe FFI in the native collector is confined to
[`src/collector/macos/mach.rs`](/Users/shawnchesney/Documents/vscodeprojects/macosmonitor/src/collector/macos/mach.rs),
with buffer-size and kernel-return safety comments. Miri cannot execute Mach,
libproc or sysctl FFI, so this is documented rather than claimed as Miri-tested.

## Release and Linux support

Linux remains supported with the `sysinfo` collector and is tested in CI.
Homebrew distribution should install an Apple-silicon or Intel universal binary;
for an unsigned downloaded release, macOS Gatekeeper can be cleared with:

```sh
xattr -dr com.apple.quarantine rtop
```

This project does not claim signed/notarized release artifacts unless a release
workflow provides verified Developer-ID signatures and `spctl --assess` results.

## What we learned

Darwin process accounting combines several independent APIs; `sysinfo` is useful
for portable baselines, while libproc/Mach/sysctl expose macOS-specific details.
Start-time checks reduce PID reuse races, but signal permissions remain subject
to ownership and macOS service policy. Terminal rendering is a separate,
testable state function, and keeping snapshots immutable makes refresh,
filtering and overlays predictable.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE). 
