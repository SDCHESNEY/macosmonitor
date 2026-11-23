# rtop

`rtop` is a terminal system monitor in the spirit of `top`, `htop`, and Activity
Monitor. It is designed for macOS, with a portable Linux implementation.

> **Status:** Phase 1 is implemented: a read-only process monitor with live
> system and process data.

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
keys or `j`/`k` to navigate, and `q`, `Esc`, or `Ctrl-C` to exit.

The event loop uses `std::thread` and `std::sync::mpsc`; an async runtime is not
needed for this blocking workload. The first CPU reading is intentionally shown
as `—` while the collector gathers a baseline. Fields the OS does not expose
are shown as `—`. On macOS, logs are written to
`~/Library/Logs/rtop/rtop.log`; on Linux, they are written to
`~/.local/state/rtop/rtop.log`.

The hidden `test-panic` feature provides a panic path for checking terminal
restoration: run with `cargo run --features test-panic -- --test-panic`.

The UI reports the memory totals provided by `sysinfo`; they are not intended
to be an exact substitute for Activity Monitor's macOS memory breakdown.

On this Apple Silicon (`arm64`) host, `top -l 2` reported 0.8% CPU for the
optimized monitor at a 1-second refresh in a short spot check. This is a
single-session observation, not a benchmark.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE). 
