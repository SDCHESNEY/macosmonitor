# rtop Technical Specification

## 1. Purpose and scope

`rtop` is a macOS-first terminal system monitor implemented in Rust. It collects
system and process data, presents it through a Ratatui/Crossterm TUI, supports
safe POSIX process signalling, and retains a portable `sysinfo` implementation
for Linux CI and fallback use.

This document describes the repository as implemented. It covers the executable
entry point, module boundaries, public types, internal functions, platform
behavior, data flow, testing, build configuration, and inter-file relationships.

## 2. System architecture

The application follows an event-driven, unidirectional flow:

```text
Collector workers
  -> Event::Snapshot(Snapshot)
  -> main event loop
  -> AppState::update(Action)
  -> Ratatui UI::draw
  -> terminal

Crossterm keyboard/mouse/resize input
  -> Event::Key/Mouse/Resize
  -> key/mouse event translation
  -> Action
  -> AppState::update
```

The sampler, input loop, and UI remain separate:

- `event.rs` owns input polling and collector sampling threads.
- `app.rs` owns mutable application state and translates actions into state changes.
- `collector::*` produces immutable `Snapshot` values.
- `ui.rs` is primarily a rendering function over `AppState`.
- `process::signal.rs` performs OS process-control operations.
- `terminal.rs` enables and restores terminal state.
- `config.rs` resolves persistent configuration.
- `main.rs` composes configuration, collection, workers, state, and rendering.

No collector implementation writes directly to the UI. UI rendering should not
mutate collection state. Process signalling is invoked by the main event loop
after the state machine enters an explicit confirmation state.

## 3. Dependency and module graph

Primary runtime dependencies are declared in `Cargo.toml`:

- `clap`: CLI argument parsing.
- `crossterm`: terminal raw mode, alternate screen, mouse, keyboard, and resize events.
- `ratatui`: widgets and terminal rendering.
- `sysinfo`: portable baseline process, CPU, memory, disk, and user collection.
- `nix` and `libc`: Unix signals and Darwin FFI support.
- `serde` and `toml`: configuration serialization.
- `directories`: platform-specific home-directory lookup.
- `tracing`, `tracing-appender`, and `tracing-subscriber`: file logging.
- `anyhow`: application error propagation.
- `thiserror`: typed process-control errors.
- `insta`: snapshot UI tests.

The Rust module graph is:

```text
main
├── app
├── cli
├── config
├── collector
│   ├── sysinfo_impl
│   ├── native
│   └── macos (macOS only)
│       └── mach
├── event
├── process
│   ├── filter
│   ├── sort
│   └── signal
├── terminal
└── ui
```

## 4. Source file reference

### 4.1 `Cargo.toml`

Defines the `rtop` binary package, edition 2024, Rust 1.95 minimum version,
dependencies, development dependencies, and test feature.

- `test-panic` enables the hidden `--test-panic` CLI path used to verify terminal
  restoration after a panic.
- Strict warnings and CI behavior are supplied through commands and the CI file,
  not by release profile settings in this manifest.

### 4.2 `src/main.rs`

Application composition root.

#### Module declarations

`main.rs` declares the application modules: `app`, `cli`, `collector`, `config`,
`event`, `process`, `terminal`, and `ui`.

#### `main()`

- Parses `Args` through Clap.
- Initializes file logging before starting the TUI.
- Calls `run`.
- Logs and returns any top-level error.

#### `run(args)`

- Loads `Config` and captures an invalid/unreadable config warning.
- Applies CLI refresh, theme, and collector options with the precedence
  CLI > config > built-in defaults.
- On macOS, native collection is selected by default unless `--collector sysinfo`
  is explicitly supplied.
- On non-macOS platforms, the native-only branch falls back to `sysinfo`.
- Enters raw mouse-enabled alternate-screen mode.
- Creates the Ratatui terminal, event worker set, application state, and
  `UnixProcessController`.
- Blocks on the event channel until `AppState.running` becomes false.
- Routes key events through `action_for_key_with_overlay`.
- Distinguishes `ConfirmKill` and invokes `AppState::confirm_kill` after the
  state action is applied.
- Routes mouse scroll and left-click events to navigation, selection, or sorting.
- Routes snapshots into `AppState::update`.
- Redraws after each handled event while the application remains active.

The `mouse.row >= 9` process-row heuristic and `mouse.row == 8` header heuristic
depend on the current fixed layout. Header-column boundaries are approximate
terminal-column thresholds and should be replaced by rendered Rectangle data if
layout behavior becomes more responsive or configurable.

#### `init_logging()`

Creates the platform-specific log directory, initializes a non-blocking rolling
file logger, and returns its worker guard so logging remains active through the
life of `run`.

#### `log_directory()`

Returns `~/Library/Logs/rtop` on macOS and `~/.local/state/rtop` on other
platforms.

### 4.3 `src/cli.rs`

Defines command-line parsing.

#### `CollectorKind`

Clap value enum containing `Native` and `Sysinfo`.

#### `ThemeKind`

Clap value enum containing `Dark` and `Light`.

#### `Args`

- `refresh_ms: Option<u64>`: optional refresh interval. Values below 250 ms are
  rejected by Clap.
- `collector: Option<CollectorKind>`: optional collector override.
- `theme: Option<ThemeKind>`: optional palette override.
- `test_panic: Option`-style hidden field: present only with the `test-panic`
  Cargo feature and used to trigger a panic after terminal setup.

Tests verify default argument absence and the 250 ms refresh lower bound.

### 4.4 `src/config.rs`

Provides TOML loading and precedence support.

#### `Config`

Serializable, deserializable configuration:

- `refresh_ms`
- `default_sort`
- `theme`
- `visible_columns`

`#[serde(default)]` and `Default` allow partial config files.

#### `Theme`

Internal theme enum with `Dark` and `Light`, plus a default of dark.

#### `effective_refresh(cli_refresh, config)`

Uses the CLI value when present, otherwise the config value, and clamps below
250 ms to the supported minimum.

#### `path()`

Uses `directories::BaseDirs` and returns
`~/.config/rtop/config.toml`.

#### `load()`

Returns `(Config, Option<String>)`:

- Missing files produce defaults and no warning.
- Valid TOML produces the parsed config.
- Invalid TOML or read failures produce defaults plus a clear status string.
- The status is currently written to the tracing log rather than a TUI status
  message because config loading occurs before TUI state exists.

Tests cover complete defaults, partial TOML, CLI precedence, and invalid TOML.

### 4.5 `src/collector/mod.rs`

Shared domain model and collector contract.

#### `Snapshot`

An immutable sample of system state. It includes collection time, aggregate and
per-core CPU, memory, swap, load, uptime, thread count, processes, and disks.
`Default` supplies a safe empty snapshot.

#### `CoreKind` and `CoreUsage`

Core kind distinguishes performance, efficiency, and unknown cores. `CoreUsage`
contains the core index and optional usage percentage.

#### `MemoryStats`

Defines aggregate memory values. On macOS, wired, compressed, cached, and
pressure values may be available. Linux leaves Apple-specific fields absent.

#### `MemoryDetails`

A copyable intermediate memory structure used while combining macOS Mach data
with portable `sysinfo` totals.

#### `MemoryPressure`

Normal, warning, or critical memory pressure state.

#### `ProcessInfo`

The central process record used by sorting, filtering, the detail view, tree
view, and signalling. Fields include PID, parent PID, name, command, executable,
username and UID, CPU, resident and virtual memory, status, start time, thread
count, and whether it is considered system-owned.

#### `DiskInfo`

Disk name, mount point, available bytes, and total bytes.

#### `SystemCollector`

Small trait with `refresh(&mut self) -> Snapshot`. It separates the UI and
samp­ling worker from the platform implementation.

#### `Collector`

Runtime enum selecting `NativeCollector` or `SysinfoCollector`; `SystemCollector`
dispatches to the selected variant.

The embedded mock collector test verifies the trait contract and call count.

### 4.6 `src/collector/sysinfo_impl.rs`

Portable baseline implementation.

#### `SysinfoCollector`

Stores `sysinfo::System`, user list, disk list, and whether an initial CPU
baseline exists.

`new` initializes the system, user table, and disk list.

#### `refresh_system`

Refreshes CPU, memory, disks, and process specifics. Process refresh requests
CPU, memory, command, executable, task, and user data only when supported.

#### `SystemCollector::refresh`

- Produces the first aggregate CPU result only after a baseline sample.
- Maps `sysinfo` processes into `ProcessInfo`.
- Resolves usernames through the cached user table.
- Converts optional resident, virtual, task, and UID values.
- Derives system ownership from `/System`, `/usr`, `/sbin`, and UID 0.
- Sorts processes by PID before snapshot publication.
- Gets load average and uptime, treating platform errors as optional data.
- Creates per-core usage entries and macOS P/E core classifications.
- Uses Darwin memory details on macOS and the generic used-memory value elsewhere.
- Collects a bounded list of disk entries.
- Updates `has_cpu_sample` after the baseline refresh.

Its disk refresh is repeated, and its process refresh is configured to keep
commands and executables when possible.

### 4.7 `src/collector/native.rs`

Darwin-oriented collector wrapper.

#### `NativeCollector`

Contains a `SysinfoCollector` fallback. In the current implementation, native
collection augments rather than fully replaces `sysinfo` collection.

`new` initializes the fallback collector.

#### `SystemCollector::refresh`

- Refreshes the portable snapshot.
- On macOS, retrieves a native PID list and filters the sysinfo process list.
- Calls native process detail lookup for executable path, resident memory,
  virtual memory, and thread count.
- Replaces available fields without discarding unavailable fallback values.
- Marks system paths as system-owned.
- Replaces total memory with `hw.memsize` when available.
- Ignores process-level native lookup failures, consistent with the per-process
  permission requirement.

The native implementation should be understood as hybrid: CPU and many BSD/user
fields still come from `sysinfo`, while selected native process and memory fields
come from Mach/libproc.

### 4.8 `src/collector/macos.rs`

Thin macOS bridge into the Mach module.

Exposes performance-core count, efficiency-core count, thread count,
`MemoryDetails`, physical memory size, native PID list, and native process detail
lookup. It contains no unsafe code itself.

This boundary keeps macOS API naming and unsafe implementation out of the
higher-level collector.

### 4.9 `src/collector/macos/mach.rs`

Darwin FFI and all unsafe blocks.

#### `ProcTaskInfo`

Repr-C layout used for `PROC_PIDTASKINFO` data. It includes virtual and resident
memory, thread counters, and task accounting fields.

The Rust layout must match the SDK declarations; the implementation requests
the kernel task-info size and transfers fields exposed by the SDK.

#### unsafe declarations

Direct declarations for:

- `proc_listpids`
- `proc_pidinfo`
- `proc_pidpath`

#### `sysctl_value`

Reads a fixed-size typed sysctl value. The safety contract requires a
NUL-terminated name, writable buffer, and matching supplied size. The result is
accepted only if the kernel returns success and writes the expected byte count.

#### `memory_details`

Calls `host_statistics64` into a zero-initialized, correctly sized
`vm_statistics64` structure. Safety comments document buffer initialization and
count matching. It derives wired, compressed, cached, and used memory and reads
the memory-pressure sysctl.

#### `native_process_pids`

Enumerates all PIDs with `proc_listpids`, doubling the buffer up to 65,536 PIDs.
The byte size passed to Darwin is derived from `u32` element size.

#### `native_process_details`

Calls `proc_pidinfo` for task info and `proc_pidpath` for the executable path.
It returns optional resident, virtual, and thread values without aborting the
collector when one process is inaccessible.

### 4.10 `src/process/mod.rs`

Process-domain module root. Reexports public submodules:

- `filter`
- `sort`
- `signal`

### 4.11 `src/process/filter.rs`

Process filtering.

#### `matches`

- Empty or whitespace-only queries match every process.
- `pid:<value>`, `user:<value>`, and `path:<value>` use typed matching.
- Unknown fields fall back to name and command matching.
- Ordinary queries match name or command.
- String matching is case-insensitive.

The tests cover name, command, PID, user, executable path, and false positives.

### 4.12 `src/process/sort.rs`

Stable process ordering support.

#### `SortKey`

Defines PID, name, user, CPU, memory, and status keys. `ALL` supports cycling,
`default_descending` makes CPU and memory descending, and `label` provides
column titles.

#### `sorted_indices`

Returns indices into the original process slice rather than rearranging the
snapshot. This preserves snapshot ownership and makes selection restoration
safer.

Sorting uses the requested key and resolves ties by ascending PID.

Optional CPU and memory values sort missing values consistently; compare helpers
also preserve deterministic behavior for floats and strings.

Tests verify repeatability, input-order independence, tie behavior, missing
metrics, and both sort directions.

### 4.13 `src/process/signal.rs`

Safe Unix process signalling.

#### `Signal`

Maps application signal choices to `SIGTERM`, `SIGKILL`, `SIGINT`, `SIGHUP`,
`SIGSTOP`, and `SIGCONT`. Its `name` and nix mapping methods provide display and
OS representations.

#### `KillError`

Typed outcome:

- no such process
- permission denied
- PID reuse detected
- launchd PID 1 protected
- rtop monitor PID protected
- other OS error code

`status_message` converts errors into footer text.
`from_errno` maps `ESRCH` and `EPERM` while retaining other errno values.

#### `ProcessController`

Mockable trait for sending a signal with PID and expected start time.

#### `UnixProcessController`

Stores the monitor PID.

- `new` captures `std::process::id`.
- `current_start_time` refreshes process state and reads start time.
- `send` refuses PID 1 and the monitor PID.
- `send` checks process start time before signalling.
- `nix::sys::signal::kill` delivers the selected signal.
- OS errno values are translated into `KillError`.

The test-only `pid_is_reused` helper and mock monitor constructor support unit
tests.

Tests cover PID 1, self-protection, errno/status text, PID reuse, signal names,
and real child-process SIGTERM delivery.

### 4.14 `src/app.rs`

Central application state machine.

#### `Action`

The complete UI action interface: navigation, sorting, filtering, pause,
detail/help/kill overlays, signal selection, tree mode, mouse index selection,
quit, no-op, and snapshot ingestion.

#### `OverlayMode`

Disjoint overlay states: none, detail, kill confirmation, extra confirmation,
signal picker, and help.

#### `AppState`

Stores snapshot, selection, visible indices, sorting/filtering state, histories,
pause/running flags, overlays, pending signal, status text, tree depths, and
theme. It is mutated only through `update`.

#### `update`

- Snapshot ingestion updates bounded CPU/memory histories.
- Navigation clamps indices.
- Sorting rebuilds visible indices and restores selection by PID.
- Filtering rebuilds and restores selection.
- Pause drops incoming snapshots.
- Overlay actions open or close mode-specific UI.
- Tree mode rebuilds hierarchy-aware indices.
- Mouse selection updates the selected PID.

#### Selection helpers

- `process_count`, `visible_count`, and `selected_process` expose derived state.
- `rebuild_visible_indices` applies sort and filter and optionally tree expansion.
- `restore_selection` preserves a process by PID where possible or uses a valid
  old index.

#### Kill flow

`request_kill` starts confirmation and chooses extra confirmation for system or
root-owned targets. `kill_requires_extra_confirmation` uses `is_system` or UID 0.
`confirm_kill` enforces PID 1/self guards and calls `ProcessController.send`.
Success and typed failures both update status text and close the popup.

#### `build_tree_indices`

Builds a depth-first view:

- Collects filtered PIDs.
- Groups processes by parent.
- Sorts sibling groups using the active sort key.
- Starts from launchd/PID 1.
- Attaches orphan or missing-parent processes after the root pass.
- Returns flattened indices and a parallel depth vector.

#### `push_history`

Keeps CPU and memory histories bounded to `HISTORY_CAPACITY`.

Tests include snapshot ingestion, navigation, PID tracking, filtering, pause,
bounded histories, kill state transitions, extra confirmation, tree nesting,
PID 1/system protection, and a 5,000-process synthetic snapshot.

### 4.15 `src/event.rs`

Threading and input/action translation.

#### `Event`

Describes key, mouse, resize, tick, snapshot, and terminal input-error events.

#### `Worker` and `WorkerSet`

`Worker` owns a stop flag and join handle. Drop sets the flag and joins the
worker. `WorkerSet::start` creates input, tick, and sampler workers.

#### `input_loop`

Polls Crossterm every 100 ms, dispatching key presses, mouse events, and resize
events to the channel.

#### `periodic_loop`

Sends one-second `Tick` events through `thread::park_timeout`.

#### `sampling_loop`

Calls `SystemCollector.refresh`, sends `Event::Snapshot`, and parks for the
configured refresh interval.

#### `KeyBinding`

Defines the shared key-table metadata used by help rendering and help tests.

#### `KEY_BINDINGS`

Single key-binding source for navigation, detail, filter, sorting, tree, process
control, pause, help, and quit operations.

#### `action_for_key`

Maps normal Crossterm key events to `Action`. It handles filter input separately,
Ctrl-C globally, sort keys, tree mode, detail, kill, signal picker, help, and quit.

#### `action_for_key_with_overlay`

Maps key events according to the active overlay. Only confirmation/cancellation,
signal selection, or close actions are accepted in modal states.

Tests verify key mapping and that every tested key binding is represented.

### 4.16 `src/terminal.rs`

Terminal lifecycle and panic restoration.

#### `TerminalSession`

Stores an atomic restored flag. `enter` enables raw mode, enables mouse capture,
enters alternate screen, hides cursor, and installs a panic hook.

#### `Drop`

Calls the restoration function once.

#### `restore`

Disables raw mode, returns to the normal screen, disables mouse capture, shows
the cursor, resets terminal style, and flushes stdout.

#### `install_panic_hook`

Ensures terminal restoration is attempted before the previous panic hook runs.

### 4.17 `src/ui.rs`

Pure Ratatui rendering surface.

#### `Palette`

Supports true color, ANSI 256, no color, and light variants. `from_environment`
honors `NO_COLOR`, `COLORTERM`, and `AppState.theme`.

#### `draw` / `draw_with_palette`

`draw` chooses environment palette; `draw_with_palette` is testable and divides
the frame into title, CPU, core, memory/disk, history, process, footer, and
overlay regions.

#### Rendering helpers

- `draw_cpu_summary`: gauge plus load, uptime, process/thread, and snapshot age.
- `draw_core_usage`: performance/efficiency/unknown core bars.
- `draw_memory_summary`: memory, wired, compressed, cache, swap, pressure, and
  first disk entry.
- `draw_history`: bounded CPU and memory sparklines.
- `draw_processes`: responsive process table and no-match message.
- `table_headers`: drops or adds columns based on terminal width.
- `table_constraints`: compact, normal, and wide layout constraints.
- `process_row`: process table cells, including tree indentation.
- `draw_footer`: filter, pause, tree, status, PID/name hints.
- `draw_overlay`: detail, confirmation, signal picker, and help overlays.
- `usage_glyph`, `format_pressure`, `format_bytes`, `format_uptime`, `truncate`:
  small formatting helpers.

UI tests use Ratatui `TestBackend` and Insta snapshots for 80 and 120-column
layouts in true-color and 256-color modes. Additional tests cover tiny terminals,
long text, empty filtering, and overlay-independent rendering.

### 4.18 `scripts/build-universal.sh`

Shell release script.

- Creates `dist`.
- Adds Apple Silicon and Intel Rust targets.
- Builds both release binaries with Cargo and `--locked`.
- Uses `lipo` to create a universal binary.
- Runs a strict ad-hoc/certificate codesign verification command.

The script depends on macOS SDK/toolchain, Homebrew or system Rust tooling,
`rustup`, and Xcode command-line tools. It is not an actual notarization workflow.

### 4.19 `.github/workflows/ci.yml`

CI matrix for macOS latest, an Intel macOS runner, and Ubuntu.

Steps format-check, build, strict-clippy, test, and run `cargo doc --no-deps`.
Both macOS native branches and the Linux fallback are exercised by source builds,
subject to runner availability.

## 5. Runtime data flow

1. `main` loads config and CLI settings.
2. `main` chooses `NativeCollector` or `SysinfoCollector`.
3. `WorkerSet` starts input and sampler threads.
4. The sampler refreshes the collector and sends `Snapshot` through `mpsc`.
5. The input worker sends key, mouse, and resize events.
6. `main` translates key and mouse events into `Action`.
7. `AppState::update` mutates only the application state and derived visible indices.
8. Ratatui redraws the complete frame from state.
9. Process-control actions enter confirmation overlays before
   `UnixProcessController.send` is called.
10. Terminal state is restored by `TerminalSession` Drop or the panic hook.

## 6. Process-control security model

The implementation has multiple guard layers:

- Explicit confirmation is required for every signal.
- PID 1 is rejected.
- The monitor PID is rejected.
- Start-time comparison reduces PID reuse races.
- Root UID and protected executable paths trigger extra confirmation.
- `ESRCH` becomes "No such process".
- `EPERM` becomes a permission-denied status with elevated-privilege guidance.
- Unsupported errno values retain the OS error code.

The OS remains authoritative: a successfully delivered signal is not proof that
a process remains exited, because launchd or an application service manager may
restart the target.

## 7. Platform behavior

### macOS

- Native collector is the default.
- Native memory pressure and core classification use Mach/sysctl support.
- `proc_listpids`, `proc_pidinfo`, and `proc_pidpath` augment process details.
- Process ownership, SIP, and terminal privileges can limit command paths or
  signalling operations.
- File and disk values depend on `sysinfo` and are not fully Darwin-native.

### Linux

- `sysinfo` remains the portable implementation.
- Apple memory pressure and P/E core fields are unavailable.
- Native collector selection is deliberately mapped to `sysinfo`.
- Signals use the same nix/libc-oriented POSIX controller.

### Windows

Not targeted. The Unix-only assumptions in signalling, paths, and CI are not
designated for Windows support.

## 8. Testing strategy

Tests are primarily embedded in the source modules:

- Clap parsing tests verify refresh values.
- Config tests verify defaults, partial TOML, precedence, and invalid TOML.
- Collector tests verify contract behavior.
- Sort/filter tests verify deterministic behavior.
- App tests verify state transitions and 5,000-process indexing.
- Signal tests verify guards, typed errors, mock PID reuse, and real child
  SIGTERM delivery.
- Event tests verify key mapping and the shared help table.
- UI tests verify small layouts and Insta snapshots.
- Native collector initialization is checked on supported builds.

Integration tests should remain platform-aware. Linux can validate POSIX signal
delivery, while macOS tests should validate launchd-sensitive behavior separately.

## 9. Build and release

Normal development commands:

```sh
cargo build
cargo test
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo doc --no-deps
```

The universal release script cross-builds Apple Silicon and Intel targets and
combines them with `lipo`. Signing verification is not notarization. Shell
completions and a formal man page are not currently generated by the repository
implementation.

## 10. Known technical constraints

- Native collection is hybrid rather than a complete standalone libproc/Mach
  implementation.
- Native PID lookup and process detail results are used as enhancements over
  `sysinfo`, not a replacement for every process field.
- Mouse row/column mapping relies on fixed layout assumptions.
- Config `default_sort` and `visible_columns` are currently documented but are
  not fully applied to every UI state path.
- Disk output shows only the first disk in the memory summary.
- Native process CPU still depends on the fallback collector.
- Miri cannot execute Mach, libproc, sysctl, or other system FFI; unsafe review
  remains source- and Darwin-platform-specific.
- Benchmark, 30-minute memory, and macOS cross-check acceptance criteria require
  separate recorded host measurements and are not established solely by unit tests.
