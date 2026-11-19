# rtop

`rtop` is a terminal system monitor in the spirit of `top`, `htop`, and Activity
Monitor. It is designed for macOS, with a portable Linux implementation.

> **Status:** Project setup is complete. The interactive monitor is planned for
> Phase 1.

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

The planned event loop uses `std::thread` and `std::sync::mpsc`; an async runtime
is not needed for this blocking workload. User configuration is planned for
`~/.config/rtop`.

## License

This project is licensed under the MIT License. See [LICENSE](LICENSE).
