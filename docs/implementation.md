# System Monitor TUI in Rust — macOS-First Implementation Plan

A terminal system monitor (in the spirit of `top`, `htop` and Activity Monitor) that shows processes, CPU, memory and system information, with sorting, filtering and safe process termination.

**Working name:** `rtop` (rename freely)
**Primary target:** macOS (Apple Silicon `aarch64-apple-darwin` first, Intel `x86_64-apple-darwin` second)
**Secondary target:** Linux (kept working through the portability layer, validated in CI; not the design driver)
**Not targeted:** Windows
**Estimated effort:** 4 phases, roughly 3–5 weeks part-time.

---

## 1. Goals and Non-Goals

### Goals
- Demonstrate macOS/Darwin knowledge: process inspection through `libproc`, Mach host statistics, `sysctl`, signals, and the permission model (SIP, ownership, entitlements).
- Demonstrate tool-building craft: clean architecture, testing, CLI ergonomics, packaging and documentation.
- Be a native-feeling macOS terminal tool: works well in Terminal.app, iTerm2 and Ghostty, respects the system appearance, and installs through Homebrew.

### Non-Goals
- Replacing `htop`, `btop` or Activity Monitor.
- Remote monitoring or a network daemon.
- A GUI or menu-bar app.
- Windows support.
- Reading GPU or SMC sensor data (private or unstable APIs; see Phase 4 stretch items).

### macOS vs Linux: what changes in this plan
| Area | macOS (primary) | Linux (secondary) |
|---|---|---|
| Process data | `sysinfo` first; direct `libproc` calls in Phase 4 | `sysinfo`; optional `/proc` collector is dropped from the plan |
| Memory model | Wired, active, inactive, compressed and free pages, plus memory pressure | Used/free/cache/buffers |
| CPU | Apple Silicon performance and efficiency cores; per-core ticks via Mach | Uniform cores |
| Signals | POSIX signals via `nix`/`libc`; SIP and ownership limit targets | Same, plus capabilities |
| Permissions | Other users' processes, system daemons and SIP-protected binaries are restricted | Mostly ownership/root |
| Distribution | Homebrew tap, universal binary, code signing and notarization | Static musl binary, optional |
| CI | `macos-latest` (arm64) and an Intel runner are the main jobs | One Ubuntu job |

---

## 2. Recommended Libraries

| Crate | Role | macOS notes |
|---|---|---|
| `ratatui` | TUI widgets, layout, rendering | Works well with 24-bit colour in iTerm2/Ghostty; Terminal.app is 256-colour only, so provide a fallback palette |
| `crossterm` | Terminal backend and input events | Mouse and resize events work in the major macOS terminals. The Option key may need terminal configuration to send Meta, so avoid Alt-based bindings |
| `sysinfo` | Process, CPU, memory, disk and network data | Supported on macOS. Temperatures and some process fields are limited, especially on Apple Silicon |
| `libproc` *(Phase 4)* | Safe wrapper over Darwin `libproc` (`proc_pidinfo`, `proc_pidpath`, `proc_listpids`) | macOS-only (also builds on Linux with reduced function); used for the native collector |
| `mach2` *(Phase 4)* | Bindings to Mach APIs (`host_statistics64`, `host_processor_info`) | macOS-only; needs `unsafe`, so isolate it in one module |
| `libc` / `nix` | `kill`, `sysctlbyname`, signal constants | `nix` gives typed signals; use raw `libc::sysctlbyname` for `hw.*` and `vm.*` values |
| `clap` (derive) | CLI arguments | Generate shell completions and a man page for the Homebrew formula |
| `anyhow` | Error handling in `main` and app glue | |
| `thiserror` | Typed errors in library modules | e.g. `KillError::PermissionDenied`, `KillError::ProtectedBySip` |
| `tracing` + `tracing-appender` | File logging | Log to `~/Library/Logs/rtop/rtop.log`, the macOS convention |
| `serde` + `toml` + `directories` | Config | Use `directories` for platform-correct paths. Decide explicitly between `~/Library/Application Support/rtop` and `~/.config/rtop`; recommend `~/.config/rtop` (XDG) because terminal-tool users expect it, and document the choice |
| `insta` | Snapshot tests of rendered buffers | Uses ratatui's `TestBackend` |
| `proptest` *(optional)* | Property tests for sort and filter | |
| `cargo-dist` | Release builds and Homebrew formula generation | Handles macOS arm64/x86_64 artifacts |

**Concurrency decision:** plain `std::thread` plus `std::sync::mpsc` (one input thread, one sampler thread, one UI loop). The workload is small and blocking, so an async runtime adds complexity without benefit. If you prefer `tokio` for portfolio reasons, the `SystemCollector` trait isolates the change to `event.rs`, since `sysinfo` calls are blocking and would need `spawn_blocking` anyway. Record the decision in the README either way.

---

## 3. Recommended Architecture

### 3.1 Pattern
An event-driven, unidirectional data flow (Elm/Redux style):

```
 ┌──────────────┐  Event::Key/Resize   ┌────────────┐
 │ Input thread │ ────────────────────▶│            │
 └──────────────┘                      │            │  Action   ┌──────────┐
 ┌──────────────┐  Event::Snapshot     │ Event loop │ ────────▶ │   App    │
 │Sampler thread│ ────────────────────▶│   (main)   │           │ (state + │
 └──────────────┘                      │            │ ◀──────── │ update)  │
 ┌──────────────┐  Event::Tick         │            │  &State   └──────────┘
 │  Tick timer  │ ────────────────────▶└─────┬──────┘
 └──────────────┘                            │ draw(&State)
                                             ▼
                                        ┌─────────┐
                                        │   UI    │ (pure rendering)
                                        └─────────┘
```

Principles:
1. **Data collection never touches the UI.** The sampler produces immutable `Snapshot` values.
2. **The UI is a pure function of state.** `ui::draw(frame, &AppState)` has no side effects and is snapshot-testable.
3. **State changes happen in one place.** `App::update(Action)` is the only mutation point.
4. **Platform code sits behind traits, with macOS as the reference implementation.** The `Snapshot` model is designed around what macOS can report; Linux maps onto it, not the reverse.

### 3.2 Module Layout

```
rtop/
├── Cargo.toml
├── README.md
├── implementation.md
├── src/
│   ├── main.rs              # CLI parsing, terminal setup/teardown, run()
│   ├── cli.rs               # clap definitions
│   ├── app.rs               # AppState, Action enum, update()
│   ├── event.rs             # Event enum, input + tick threads
│   ├── terminal.rs          # init/restore, panic hook, colour-capability detection
│   ├── collector/
│   │   ├── mod.rs           # SystemCollector trait + Snapshot types
│   │   ├── sysinfo_impl.rs  # cross-platform baseline (Phases 1–3)
│   │   ├── macos/           # (Phase 4) native Darwin collector
│   │   │   ├── mod.rs
│   │   │   ├── libproc.rs   # per-process info
│   │   │   ├── mach.rs      # CPU ticks, vm statistics (all unsafe lives here)
│   │   │   └── sysctl.rs    # hw.*, vm.*, kern.* values
│   │   └── history.rs       # ring buffers for sparklines
│   ├── process/
│   │   ├── mod.rs           # ProcessInfo, ProcessStatus
│   │   ├── sort.rs
│   │   ├── filter.rs
│   │   └── signal.rs        # ProcessController trait, KillError, Unix impl
│   ├── ui/
│   │   ├── mod.rs
│   │   ├── header.rs        # CPU bars, memory breakdown, uptime, load avg
│   │   ├── process_table.rs
│   │   ├── footer.rs
│   │   ├── popup.rs         # confirm-kill, signal picker, help
│   │   └── theme.rs         # palettes incl. 256-colour fallback
│   └── config.rs            # (Phase 4)
└── tests/
    ├── sort_filter.rs
    ├── app_update.rs
    └── ui_snapshots.rs
```

All `#[cfg(target_os = "macos")]` code lives under `collector/macos/` and in small, clearly marked parts of `signal.rs`. Everything else is platform-neutral.

### 3.3 Core Types (sketch)

```rust
pub struct Snapshot {
    pub taken_at: Instant,
    pub cpu_per_core: Vec<CoreUsage>,     // includes CoreKind::{Performance, Efficiency, Unknown}
    pub cpu_total: f32,
    pub memory: MemoryStats,
    pub swap_used: u64,
    pub swap_total: u64,
    pub load_avg: (f64, f64, f64),        // available on macOS and Linux
    pub uptime: Duration,
    pub processes: Vec<ProcessInfo>,
}

pub struct MemoryStats {
    pub total: u64,
    pub used: u64,                         // definition documented; see 3.4
    pub wired: Option<u64>,                // macOS
    pub compressed: Option<u64>,           // macOS
    pub cached: Option<u64>,
    pub pressure: Option<MemoryPressure>,  // Normal / Warn / Critical (macOS)
}

pub struct ProcessInfo {
    pub pid: u32,
    pub parent: Option<u32>,
    pub name: String,
    pub cmd: String,
    pub exe_path: Option<PathBuf>,
    pub user: Option<String>,
    pub cpu_percent: f32,
    pub mem_bytes: u64,                    // resident size
    pub status: ProcessStatus,
    pub start_time: u64,
    pub is_system: bool,                   // root/system-owned or under /System, /usr, /sbin
}

pub trait SystemCollector: Send {
    fn refresh(&mut self) -> Snapshot;
}

pub trait ProcessController {
    fn send(&self, pid: u32, expected_start: u64, signal: Signal) -> Result<(), KillError>;
}

pub enum Event { Key(KeyEvent), Mouse(MouseEvent), Resize(u16, u16), Tick, Snapshot(Box<Snapshot>) }

pub enum Action {
    MoveUp, MoveDown, PageUp, PageDown, Top, Bottom,
    CycleSort, ToggleSortOrder,
    EnterFilter, FilterInput(char), FilterBackspace, FilterCommit, FilterCancel,
    RequestKill, ConfirmKill, CancelKill, SelectSignal(Signal),
    ToggleTree, ToggleDetail, ToggleHelp, TogglePause, Quit,
    SnapshotReceived(Box<Snapshot>),
}
```

### 3.4 Key Design Decisions

| Decision | Choice | Rationale |
|---|---|---|
| Selection tracking | By **PID**, not row index | Rows reorder each refresh |
| CPU measurement | Needs two samples | The first reading is meaningless; discard it or show `—` |
| CPU percent convention | Per-core scale (a process can exceed 100%), matching `top`; offer a toggle to normalise | Users compare against `top` and Activity Monitor |
| Memory "used" | Document the definition. Show wired + active + compressed (close to Activity Monitor's "Memory Used") rather than "total − free" | macOS keeps free memory near zero by design, so a naive formula looks alarming |
| Memory pressure | Display Normal/Warn/Critical alongside the numbers | This is the metric that matters on macOS |
| Core types | Show performance and efficiency cores separately on Apple Silicon (read `hw.perflevel0/1.*` via `sysctl`) | Distinctive, Mac-specific feature |
| Terminal safety | Panic hook plus RAII guard restores the terminal | Otherwise the shell is left in raw mode |
| Kill safety | Confirmation popup, SIGTERM by default, SIGKILL as an explicit second step | Prevent accidental data loss |
| Kill target safety | Start-time check before signalling; protected-PID list | Guards against PID reuse and critical system processes (`launchd` PID 1) |
| Missing data | `Option<T>` in the model, `—` in the UI | Some fields are unavailable for other users' processes without privileges |
| Terminal colours | Detect `COLORTERM=truecolor`; fall back to the 256-colour palette | Terminal.app lacks true colour |

---

## 4. macOS Platform Notes (read before Phase 1)

These shape the acceptance criteria below.

1. **Permissions and SIP.** An unprivileged process can list all processes, but detailed info (e.g. memory or executable path) for other users' and system processes may be unavailable. Signalling processes owned by other users fails with `EPERM`. System Integrity Protection does not block `kill(2)` itself, but system daemons are owned by root, so they surface as permission errors. Treat these as normal, expected outcomes, not bugs.
2. **No `/proc`.** Process data comes from `libproc`, `sysctl` (`KERN_PROC`) and Mach APIs. `sysinfo` wraps these; Phase 4 calls them directly.
3. **Terminal capabilities.** Test in Terminal.app, iTerm2 and one modern terminal (Ghostty, WezTerm or Alacritty). Verify mouse support, resize, colours and Unicode block characters in each.
4. **Apple Silicon.** CPU core layout is heterogeneous; temperature and power sensors are not reliably accessible to unprivileged programs. Do not promise them.
5. **Privacy prompts.** A terminal app generally does not need special permission to list processes. Document this finding after testing (and note any Full Disk Access caveats if you observe them).
6. **Signing and notarization.** Binaries downloaded from the internet are quarantined by Gatekeeper. A Homebrew-built binary avoids this; a downloaded release binary needs a Developer ID signature and notarization, or users must clear the quarantine attribute. Plan for this in Phase 4.

---

## 5. Phased Implementation

Each phase ends in a **usable, committed, tagged** state (`v0.1` … `v1.0`). Do not start a phase until the previous phase's acceptance criteria pass. Unless stated, acceptance criteria are verified **on macOS first**; Linux checks are run through CI.

---

### Phase 0 — Project Setup (½ day)

**Tasks**
- `cargo new rtop`; add dependencies; set edition and MSRV.
- Configure `rustfmt.toml` and `clippy` (`-D warnings`).
- GitHub Actions: build, fmt, clippy and test on `macos-latest` (Apple Silicon) and an Intel macOS runner, plus one Ubuntu job.
- Create a README skeleton, `.gitignore`, license and `CHANGELOG.md`.
- Install helpful tooling: `cargo-nextest`, `cargo-insta`.

**Acceptance criteria**
- [ ] `cargo build`, `cargo test`, `cargo fmt --check` and `cargo clippy -- -D warnings` pass locally on macOS.
- [ ] CI is green on Apple Silicon macOS, Intel macOS and Linux.
- [ ] README states the purpose and the macOS-primary, Linux-secondary support policy.

---

### Phase 1 — Foundation: Data Collection and Read-Only Process List (v0.1)

**Objective:** Prove the pipeline end to end on macOS: sample the OS, render a table, quit cleanly.

**Tasks**
1. Implement `terminal.rs`: enter and leave the alternate screen and raw mode; install a panic hook.
2. Implement `SystemCollector` with `sysinfo`, producing `Snapshot` (including `load_avg`, uptime and memory).
3. Implement `event.rs`: input thread, tick timer and sampler thread feeding one `mpsc` channel.
4. Implement `AppState` and `update()` for `Quit`, `SnapshotReceived` and navigation.
5. Render a `ratatui::Table`: PID, Name, User, CPU %, Memory, Status.
6. CLI via `clap`: `--refresh-ms` (default 1000, minimum 250).
7. Log to `~/Library/Logs/rtop/rtop.log` with `tracing`.

**Acceptance criteria**
- [ ] Running `rtop` on macOS shows all running processes with PID, name, user, CPU %, memory and status.
- [ ] Values refresh at the configured interval; `--refresh-ms 500` visibly doubles the update rate.
- [ ] `q`, `Esc` and `Ctrl-C` exit and restore the terminal (cursor visible, no raw mode, no leftover alternate screen) in Terminal.app **and** iTerm2.
- [ ] A forced panic (behind a test-only feature flag) leaves the terminal usable.
- [ ] The first CPU sample is discarded or shown as `—`.
- [ ] Resizing the terminal (including to 20×5) never crashes.
- [ ] Processes the user cannot inspect (root/system) are listed with `—` for unavailable fields rather than being dropped or causing errors.
- [ ] The monitor's own CPU use is under ~3% at 1 s refresh on an Apple Silicon Mac (measured, recorded in the README).
- [ ] CPU percentages for a known load (e.g. `yes > /dev/null`) agree with `top -l 2` within a reasonable tolerance (±5 points), verified manually.
- [ ] Unit tests cover `update()` navigation and snapshot ingestion; the collector is tested through a mock.
- [ ] No `unwrap()` on fallible OS calls in non-test code.

---

### Phase 2 — macOS-Accurate Overview, Sorting and Filtering (v0.2)

**Objective:** Make the tool informative, and make its numbers meaningful on macOS.

**Tasks**
1. **Header panel:** per-core CPU bars, aggregate gauge, load average, uptime, process and thread counts.
2. **Apple Silicon core grouping:** read `hw.perflevel0.*` / `hw.perflevel1.*` through `sysctlbyname` to label performance vs efficiency cores. On Intel Macs or unavailable values, fall back to a single group.
3. **Memory panel:** used (as defined in 3.4), wired, compressed, cached, swap, and a **memory pressure** indicator.
4. **History:** a bounded ring buffer (e.g. 60 samples) for CPU and memory, shown as `Sparkline`s.
5. **Sorting:** keys for PID, Name, User, CPU, Memory and Status. The same key again reverses the order; show ▲/▼ in the header.
6. **Filtering:** `/` enters filter mode; live, case-insensitive match on name or command; `pid:`, `user:` and `path:` prefixes; `Enter` commits, `Esc` clears.
7. **Selection by PID** persisting across refreshes and re-sorting.
8. **Pause** with `Space`.
9. **Footer** with key hints and a status area. Use only keys that work on macOS terminals by default (no Option/Alt dependency).
10. Human-readable units; truncate long names and commands.

**Acceptance criteria**
- [ ] The header shows per-core CPU, load average, uptime and process count; values agree with `top`/`sysctl vm.loadavg` within tolerance (±2 points CPU, ±1 load-average digit).
- [ ] On an Apple Silicon Mac, cores are grouped as performance and efficiency, with counts matching `sysctl hw.perflevel0.logicalcpu` and `hw.perflevel1.logicalcpu`. On other hardware, the UI shows a single group without errors.
- [ ] Memory figures agree with Activity Monitor's "Memory Used", "Wired Memory" and "Compressed" within ~5% (spot-checked manually; record the comparison).
- [ ] The memory pressure indicator matches Activity Monitor's pressure state in a normal-load scenario and changes under induced load (e.g. a memory-hungry test process).
- [ ] Sparklines show the last 60 samples and scroll as new data arrives; buffers are bounded.
- [ ] Every sort key works both ways; the sort is stable, with PID as the tie-breaker.
- [ ] Sort and filter logic are pure functions with unit and property tests (sorting is idempotent and preserves the PID set).
- [ ] Filtering updates on each keystroke; an empty result shows "No matching processes".
- [ ] Selection persists across refreshes and re-sorts; when the process exits, selection moves sensibly and nothing panics.
- [ ] Pause stops data updates but not input; a `PAUSED` indicator is shown.
- [ ] Snapshot tests (`insta` + `TestBackend`) cover the main screen at 80×24 and 120×40, plus both the true-colour and 256-colour palettes.
- [ ] Layout degrades gracefully below 80 columns by dropping low-priority columns.
- [ ] Linux CI still passes; Apple-only features (core grouping, memory pressure) degrade to `—`/single group there.

---

### Phase 3 — Process Control and Detail View (v0.3)

**Objective:** Add the OS interaction that makes the project stand out: sending signals safely on macOS.

**Tasks**
1. **`ProcessController` trait** with a Unix implementation using `nix::sys::signal::kill` (or `libc::kill`).
2. **Kill flow:**
   - `k` opens a confirmation popup showing PID, name, user and executable path; default signal is SIGTERM.
   - `Enter`/`y` confirms; `Esc`/`n` cancels.
   - A signal picker (`s`) offers SIGTERM, SIGKILL, SIGINT, SIGHUP, SIGSTOP and SIGCONT.
3. **Typed errors** mapped to clear status-bar messages: `ESRCH` (no such process), `EPERM` (permission denied, with a hint such as "owned by root; not permitted without elevated privileges") and others.
4. **Safeguards:**
   - Refuse to signal PID 1 (`launchd`) and the monitor's own PID.
   - Require an extra confirmation for root-owned processes and processes under `/System`, `/usr` or `/sbin`.
   - Warn when the target is owned by another user.
5. **PID reuse protection:** compare the process start time before sending the signal.
6. **Detail view:** full command line, executable path, parent PID, user, start time, thread count, resident and virtual memory, and child processes.
7. **Tree view toggle (`t`):** parent/child hierarchy rooted at `launchd`.
8. **Document macOS behaviour:** what you observe when signalling GUI apps (e.g. SIGTERM versus SIGKILL on a test app), and how `launchd`-managed daemons may be restarted automatically after being killed.

**Acceptance criteria**
- [ ] Killing a spawned test process (e.g. `sleep 600`) with SIGTERM removes it from the list within two refresh cycles on macOS.
- [ ] A process that ignores SIGTERM can be ended with SIGKILL via the signal picker.
- [ ] No signal is sent without an explicit confirmation step.
- [ ] Attempting to kill a root-owned process as a normal user shows a "Permission denied" message with an explanatory hint; the app does not crash and the process stays listed.
- [ ] Attempting to signal an already-exited process shows "No such process" without crashing.
- [ ] PID 1 and the monitor's own PID are protected; the extra confirmation for system-path and root-owned processes is implemented. Both are covered by tests.
- [ ] The start-time check is implemented and tested with a mock controller that simulates PID reuse.
- [ ] SIGSTOP/SIGCONT work, and a stopped process shows a stopped status in the table.
- [ ] The detail view shows correct data for a known test process (command line, executable path, PPID, user) and closes with `Esc`.
- [ ] Tree view nests children correctly under parents, with `launchd` as root; orphans attach to PID 1; sorting applies within sibling groups.
- [ ] Kill-flow state transitions (request → confirm → result; request → cancel) are unit-tested with a mocked `ProcessController`.
- [ ] Integration tests spawn real child processes and verify signal delivery on macOS and Linux CI.
- [ ] The README documents the macOS permission model as observed and the observed `launchd` auto-restart behaviour.

---

### Phase 4 — Native macOS Collector, Polish and Release (v1.0)

**Objective:** Go beyond a wrapper library, then ship a portfolio-grade, Homebrew-installable tool.

**Tasks**
1. **Native Darwin collector** implementing `SystemCollector` directly:
   - `libproc`: `proc_listpids`, `proc_pidinfo` (task info, BSD info) and `proc_pidpath`.
   - Mach: `host_processor_info` for per-core ticks and `host_statistics64` for VM statistics.
   - `sysctl`: `kern.boottime`, `hw.memsize`, `vm.swapusage`, `hw.perflevel*`.
   - Confine all `unsafe` to `collector/macos/mach.rs` with documented safety invariants.
   - Selectable with `--collector native|sysinfo` (default `native` on macOS if all acceptance criteria below pass, otherwise `sysinfo`).
2. **Config file** (`~/.config/rtop/config.toml`): refresh rate, default sort, theme, visible columns. Precedence: CLI > config > defaults.
3. **Themes:** dark and light, with automatic selection where the terminal reports its background, `NO_COLOR` support, and the 256-colour fallback.
4. **Mouse support:** click to select, scroll to move, click a header to sort.
5. **Help overlay** (`?`) generated from the single key-binding table.
6. **Extra panels (pick one or two):** disk usage, network throughput, per-process disk I/O, thermal state. Only include what works reliably without privileges, and document what does not.
7. **Performance pass:** profile (`cargo flamegraph` or Instruments Time Profiler); remove per-frame allocations; test with 1,000+ real processes and a synthetic 5,000-process snapshot.
8. **Packaging and release:**
   - Build a universal binary (arm64 + x86_64) with `lipo`, or ship separate artifacts via `cargo-dist`.
   - Create a Homebrew tap and formula.
   - Generate shell completions and a man page.
   - Optional: Developer ID signing and notarization for downloaded binaries.
9. **Documentation:** README with a demo GIF (`vhs` or `asciinema`), features, key bindings, architecture diagram, design decisions, known limitations, and a "What I learned" section.

**Acceptance criteria**
- [ ] Native collector: for a set of reference processes, PID, name, executable path, resident memory and CPU agree with the `sysinfo` collector within tolerance (CPU ±3 points, memory ±5%); comparison tests exist.
- [ ] Native collector: memory totals and pressure match Activity Monitor within the tolerances from Phase 2.
- [ ] All `unsafe` blocks are in one module, each with a safety comment, and the module passes `cargo clippy` and `cargo miri`-free review where applicable (Miri cannot run FFI; note this in the README).
- [ ] The native collector handles permission failures per process without aborting the sample (verified by listing all processes as a normal user).
- [ ] Native collector overhead is equal to or lower than the `sysinfo` collector (measured, recorded).
- [ ] Config precedence (CLI > config > defaults) is tested; an invalid config gives a clear error and a documented fallback.
- [ ] `NO_COLOR=1 rtop` renders without colour; the light and dark themes are legible in Terminal.app and iTerm2.
- [ ] Mouse selection, scrolling and header-click sorting work in Terminal.app and iTerm2; keyboard-only operation remains complete.
- [ ] The help overlay lists every key binding, with a test that fails if a binding is missing from the shared table.
- [ ] With a synthetic 5,000-process snapshot, each frame renders in under ~16 ms in release mode (measured, recorded).
- [ ] Memory usage stays flat over a 30-minute run (no growth trend).
- [ ] Release artifacts run on Apple Silicon and Intel Macs; `brew install <tap>/rtop` installs and runs the tool on a clean machine.
- [ ] Shell completions and a man page are installed by the formula.
- [ ] If signing and notarization are done, `spctl --assess` and `codesign --verify` pass on the release binary; otherwise the README documents the Gatekeeper workaround.
- [ ] Linux build still passes CI using the `sysinfo` collector; the README states Linux is supported on a best-effort basis.
- [ ] `cargo clippy -- -D warnings` is clean, core-logic test coverage is ≥ 70%, and `cargo doc` has no warnings.
- [ ] A fresh clone builds and runs on a Mac with only the Rust toolchain installed, following the README alone.

---

## 6. Testing Strategy

| Layer | Approach | Tools |
|---|---|---|
| Pure logic (sort, filter, history) | Unit and property tests | built-in, `proptest` |
| State machine (`App::update`) | Table-driven tests with synthetic `Action`s and `Snapshot`s | built-in |
| Rendering | Snapshot tests on `TestBackend` buffers (both palettes) | `insta` |
| OS interaction | Mock `SystemCollector` and `ProcessController`; a small set of real-process integration tests | `std::process::Command`, `serial_test` |
| Native collector | Cross-check against `sysinfo`, `top -l 2`, `ps`, and `sysctl` output | comparison tests plus manual checklist |
| Terminal compatibility | Manual matrix: Terminal.app, iTerm2, one modern terminal | checklist in the README |
| CI | macOS arm64 and Intel (primary), Ubuntu (secondary) | GitHub Actions |

---

## 7. Risks and Mitigations

| Risk | Impact | Mitigation |
|---|---|---|
| `sysinfo` macOS behaviour or API changes | Build breaks or wrong values | Pin versions; isolate in `sysinfo_impl.rs`; cross-check against `top` |
| Direct Mach/libproc FFI mistakes | Crashes, undefined behaviour | Confine `unsafe` to one module; compare with `sysinfo`; keep `sysinfo` as a fallback collector |
| Private or undocumented Darwin behaviour | Values differ between macOS versions | Test on at least two macOS versions; document supported versions |
| Memory numbers look wrong vs Activity Monitor | Loss of user trust | Define "used" explicitly; show pressure; record the comparison |
| Permission errors on other users' processes | Missing data or failed kills | `Option` fields, typed errors, explanatory messages |
| PID reuse races | Killing the wrong process | Start-time verification before signalling |
| Terminal.app limitations (colour, key handling) | Poor first impression | 256-colour fallback; avoid Alt bindings; test in Terminal.app each release |
| Gatekeeper blocks downloaded binaries | Users cannot run releases | Homebrew-first distribution; optional notarization; documented workaround |
| No Mac available for some CI cases (Intel, older macOS) | Untested targets | Use CI runners; state supported versions explicitly |
| Scope creep (extra panels) | Never reaching v1.0 | Finish each phase's criteria first; at most two extra panels |

---

## 8. Definition of Done (Project Level)

- [ ] All four phases' acceptance criteria are met and tagged `v0.1` through `v1.0`.
- [ ] CI is green on macOS (Apple Silicon and Intel) and Linux; releases are published.
- [ ] Homebrew installation works.
- [ ] The README lets a stranger understand, install and run the tool in under five minutes.
- [ ] The README includes a "What I learned" section covering Darwin process accounting, signals and the macOS permission model, memory pressure, terminal handling and architecture trade-offs.

---

## 9. Suggested Timeline

| Phase | Focus | Estimated effort |
|---|---|---|
| 0 | Setup and CI | ½ day |
| 1 | Foundation and process list | 4–6 days |
| 2 | macOS-accurate overview, sort, filter | 5–7 days |
| 3 | Process control and detail | 4–6 days |
| 4 | Native collector, polish, Homebrew release | 6–9 days |
