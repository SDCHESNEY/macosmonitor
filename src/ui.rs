use std::time::Duration;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Clear, Gauge, Paragraph, Row, Sparkline, Table, TableState},
};

use crate::{
    app::AppState,
    collector::{CoreKind, MemoryPressure, ProcessInfo, Snapshot},
    process::sort::SortKey,
};

#[derive(Debug, Clone, Copy)]
pub enum Palette {
    TrueColor,
    Ansi256,
}

impl Palette {
    pub fn from_environment() -> Self {
        match std::env::var("COLORTERM").ok().as_deref() {
            Some("truecolor" | "24bit") => Self::TrueColor,
            _ => Self::Ansi256,
        }
    }

    fn accent(self) -> Color {
        match self {
            Self::TrueColor => Color::Rgb(86, 182, 194),
            Self::Ansi256 => Color::Indexed(44),
        }
    }

    fn muted(self) -> Color {
        match self {
            Self::TrueColor => Color::Rgb(150, 160, 170),
            Self::Ansi256 => Color::Indexed(245),
        }
    }

    fn performance(self) -> Color {
        match self {
            Self::TrueColor => Color::Rgb(97, 175, 239),
            Self::Ansi256 => Color::Indexed(75),
        }
    }

    fn efficiency(self) -> Color {
        match self {
            Self::TrueColor => Color::Rgb(198, 120, 221),
            Self::Ansi256 => Color::Indexed(176),
        }
    }
}

pub fn draw(frame: &mut Frame<'_>, state: &AppState) {
    draw_with_palette(frame, state, Palette::from_environment());
}

pub fn draw_with_palette(frame: &mut Frame<'_>, state: &AppState, palette: Palette) {
    let area = frame.area();
    let sections = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(2),
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area);

    frame.render_widget(
        Paragraph::new("rtop — system monitor").style(Style::default().fg(palette.accent())),
        sections[0],
    );
    draw_cpu_summary(frame, state, sections[1], palette);
    draw_core_usage(frame, state.snapshot.as_ref(), sections[2], palette);
    draw_memory_summary(frame, state.snapshot.as_ref(), sections[3], palette);
    draw_history(frame, state, sections[4], palette);
    draw_processes(frame, state, sections[5], palette);
    draw_footer(frame, state, sections[6], palette);
    draw_overlay(frame, state, area, palette);
}

fn draw_cpu_summary(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    let snapshot = state.snapshot.as_ref();
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(24), Constraint::Min(0)])
        .split(area);
    let cpu = snapshot.and_then(|snapshot| snapshot.cpu_total);
    let label = cpu.map_or_else(|| "CPU —".to_owned(), |usage| format!("CPU {usage:.1}%"));
    let gauge = Gauge::default()
        .gauge_style(Style::default().fg(palette.accent()))
        .percent(cpu.unwrap_or_default().round().clamp(0.0, 100.0) as u16)
        .label(label);
    frame.render_widget(gauge, columns[0]);

    let summary = snapshot.map_or_else(
        || "LOAD —  UP —  PROCESSES —  THREADS —".to_owned(),
        |snapshot| {
            let load = snapshot.load_avg.map_or_else(
                || "—".to_owned(),
                |(one, five, fifteen)| format!("{one:.2} {five:.2} {fifteen:.2}"),
            );
            let uptime = snapshot
                .uptime
                .map_or_else(|| "—".to_owned(), format_uptime);
            let threads = snapshot
                .thread_count
                .map_or_else(|| "—".to_owned(), |count| count.to_string());
            let age = format!("{:.1}s", snapshot.taken_at.elapsed().as_secs_f32());
            format!(
                "LOAD {load}  UP {uptime}  PROCESSES {}  THREADS {threads}  AGE {age}",
                state.process_count(),
            )
        },
    );
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().fg(palette.muted())),
        columns[1],
    );
}

fn draw_core_usage(
    frame: &mut Frame<'_>,
    snapshot: Option<&Snapshot>,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    let Some(snapshot) = snapshot else {
        frame.render_widget(Paragraph::new("CORES —"), area);
        return;
    };
    if snapshot.cpu_per_core.is_empty() {
        frame.render_widget(Paragraph::new("CORES —"), area);
        return;
    }
    let spans = snapshot
        .cpu_per_core
        .iter()
        .map(|core| {
            let label = match core.kind {
                CoreKind::Performance => format!("P{}:", core.index),
                CoreKind::Efficiency => {
                    let performance_count = snapshot
                        .cpu_per_core
                        .iter()
                        .filter(|item| item.kind == CoreKind::Performance)
                        .count();
                    format!("E{}:", core.index.saturating_sub(performance_count))
                }
                CoreKind::Unknown => format!("C{}:", core.index),
            };
            let bar = core.usage_percent.map_or('—', usage_glyph);
            let color = match core.kind {
                CoreKind::Performance => palette.performance(),
                CoreKind::Efficiency => palette.efficiency(),
                CoreKind::Unknown => palette.muted(),
            };
            Span::styled(format!("{label}{bar} "), Style::default().fg(color))
        })
        .collect::<Vec<_>>();
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn draw_memory_summary(
    frame: &mut Frame<'_>,
    snapshot: Option<&Snapshot>,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    let summary = snapshot.map_or_else(
        || {
            vec![
                Line::from("MEM —  WIRED —  COMPRESSED —"),
                Line::from("CACHED —  SWAP —  PRESSURE —"),
            ]
        },
        |snapshot| {
            let optional_bytes =
                |value: Option<u64>| value.map_or_else(|| "—".to_owned(), format_bytes);
            vec![
                Line::from(format!(
                    "MEM {}/{}  WIRED {}  COMPRESSED {}",
                    format_bytes(snapshot.memory.used),
                    format_bytes(snapshot.memory.total),
                    optional_bytes(snapshot.memory.wired),
                    optional_bytes(snapshot.memory.compressed)
                )),
                Line::from(format!(
                    "CACHED {}  SWAP {}/{}  PRESSURE {}",
                    optional_bytes(snapshot.memory.cached),
                    format_bytes(snapshot.swap_used),
                    format_bytes(snapshot.swap_total),
                    format_pressure(snapshot.memory.pressure)
                )),
            ]
        },
    );
    frame.render_widget(
        Paragraph::new(summary).style(Style::default().fg(palette.muted())),
        area,
    );
}

fn draw_history(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let cpu_data = state.cpu_history.iter().copied().collect::<Vec<_>>();
    let memory_data = state.memory_history.iter().copied().collect::<Vec<_>>();
    frame.render_widget(
        Sparkline::default()
            .block(Block::default().borders(Borders::ALL).title("CPU history"))
            .data(cpu_data)
            .max(100)
            .style(Style::default().fg(palette.accent())),
        columns[0],
    );
    frame.render_widget(
        Sparkline::default()
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Memory history"),
            )
            .data(memory_data)
            .max(100)
            .style(Style::default().fg(palette.efficiency())),
        columns[1],
    );
}

fn draw_processes(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    if state.snapshot.is_some()
        && state.visible_indices.is_empty()
        && !state.filter_query.is_empty()
    {
        frame.render_widget(
            Paragraph::new("No matching processes")
                .block(Block::default().borders(Borders::ALL).title("Processes")),
            area,
        );
        return;
    }

    let wide = area.width >= 120;
    let compact = area.width < 80;
    let headers = table_headers(state.sort_key, state.sort_descending, compact, wide);
    let rows = state
        .visible_indices
        .iter()
        .enumerate()
        .filter_map(|(row_index, index)| {
            let process = state
                .snapshot
                .as_ref()
                .and_then(|snapshot| snapshot.processes.get(*index))?;
            let depth = state.tree_depths.get(row_index).copied().unwrap_or(0);
            Some(process_row(process, compact, wide, area.width, depth))
        })
        .collect::<Vec<_>>();
    let constraints = table_constraints(compact, wide);
    let header = Row::new(headers).style(
        Style::default()
            .fg(palette.accent())
            .add_modifier(Modifier::BOLD),
    );
    let table = Table::new(rows, constraints)
        .header(header)
        .block(Block::default().borders(Borders::ALL).title("Processes"))
        .row_highlight_style(
            Style::default()
                .bg(Color::DarkGray)
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol("› ");
    let mut table_state = TableState::default();
    table_state.select((state.visible_count() > 0).then_some(state.selected_index));
    frame.render_stateful_widget(table, area, &mut table_state);
}

fn table_headers(sort_key: SortKey, descending: bool, compact: bool, wide: bool) -> Vec<String> {
    let keys = if compact {
        vec![SortKey::Pid, SortKey::Name, SortKey::Cpu, SortKey::Memory]
    } else {
        vec![
            SortKey::Pid,
            SortKey::Name,
            SortKey::User,
            SortKey::Cpu,
            SortKey::Memory,
            SortKey::Status,
        ]
    };
    let mut headers = keys
        .into_iter()
        .map(|key| {
            if key == sort_key {
                format!("{} {}", key.label(), if descending { '▼' } else { '▲' })
            } else {
                key.label().to_owned()
            }
        })
        .collect::<Vec<_>>();
    if wide {
        headers.push("Command".to_owned());
    }
    headers
}

fn table_constraints(compact: bool, wide: bool) -> Vec<Constraint> {
    if compact {
        vec![
            Constraint::Length(7),
            Constraint::Percentage(50),
            Constraint::Length(8),
            Constraint::Percentage(25),
        ]
    } else if wide {
        vec![
            Constraint::Length(7),
            Constraint::Percentage(20),
            Constraint::Percentage(12),
            Constraint::Length(9),
            Constraint::Percentage(12),
            Constraint::Percentage(12),
            Constraint::Min(18),
        ]
    } else {
        vec![
            Constraint::Length(7),
            Constraint::Percentage(27),
            Constraint::Percentage(15),
            Constraint::Length(9),
            Constraint::Percentage(18),
            Constraint::Percentage(18),
        ]
    }
}

fn process_row(
    process: &ProcessInfo,
    compact: bool,
    wide: bool,
    width: u16,
    depth: usize,
) -> Row<'static> {
    let cpu = process
        .cpu_percent
        .map_or_else(|| "—".to_owned(), |usage| format!("{usage:.1}"));
    let memory = process
        .mem_bytes
        .map_or_else(|| "—".to_owned(), format_bytes);
    let mut cells = vec![
        Cell::from(process.pid.to_string()),
        Cell::from(truncate(
            &format!("{}{}", "  ".repeat(depth), process.name),
            if compact { 22 } else { 24 },
        )),
    ];
    if wide {
        cells.push(Cell::from(
            process.user.as_deref().unwrap_or("—").to_owned(),
        ));
        cells.push(Cell::from(cpu));
        cells.push(Cell::from(memory));
        cells.push(Cell::from(process.status.clone()));
        cells.push(Cell::from(truncate(
            &process.command,
            width.saturating_sub(100) as usize,
        )));
    } else if compact {
        cells.push(Cell::from(cpu));
        cells.push(Cell::from(memory));
    } else {
        cells.push(Cell::from(
            process.user.as_deref().unwrap_or("—").to_owned(),
        ));
        cells.push(Cell::from(cpu));
        cells.push(Cell::from(memory));
        cells.push(Cell::from(process.status.clone()));
    }
    Row::new(cells)
}

fn draw_footer(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    let mut hints = if state.filter_active {
        format!("FILTER: {}▏  Enter apply • Esc clear", state.filter_query)
    } else {
        "p/n/u/c/m/r sort • / filter • Enter detail • k kill • s signal • t tree • Space pause • q quit".to_owned()
    };
    if !state.filter_active && !state.filter_query.is_empty() {
        hints.push_str(&format!("  FILTER: {}", state.filter_query));
    }
    if state.paused {
        hints.push_str("  PAUSED");
    }
    if state.tree_view {
        hints.push_str("  TREE");
    }
    if let Some(status) = &state.status_message {
        hints.push_str(&format!("  {status}"));
    }
    if let Some(process) = state.selected_process() {
        hints.push_str(&format!(
            "  PID {} {}",
            process.pid,
            truncate(&process.name, 20)
        ));
    }
    frame.render_widget(
        Paragraph::new(hints).style(Style::default().fg(palette.muted())),
        area,
    );
}

fn usage_glyph(usage: f32) -> char {
    const GLYPHS: [char; 9] = ['▁', '▂', '▃', '▄', '▅', '▆', '▇', '█', '█'];
    let index = (usage.clamp(0.0, 100.0) / 12.5).floor() as usize;
    GLYPHS[index.min(GLYPHS.len() - 1)]
}

fn format_pressure(pressure: Option<MemoryPressure>) -> &'static str {
    match pressure {
        Some(MemoryPressure::Normal) => "Normal",
        Some(MemoryPressure::Warning) => "Warning",
        Some(MemoryPressure::Critical) => "Critical",
        None => "—",
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KiB", "MiB", "GiB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    format!("{size:.1} {}", UNITS[unit])
}

fn format_uptime(uptime: Duration) -> String {
    let seconds = uptime.as_secs();
    let days = seconds / 86_400;
    let hours = (seconds % 86_400) / 3_600;
    let minutes = (seconds % 3_600) / 60;
    if days > 0 {
        format!("{days}d {hours:02}h")
    } else {
        format!("{hours}h {minutes:02}m")
    }
}

fn draw_overlay(
    frame: &mut Frame<'_>,
    state: &AppState,
    area: ratatui::layout::Rect,
    palette: Palette,
) {
    use crate::app::OverlayMode;

    let popup_width = area.width.saturating_sub(12).clamp(40, 90);
    let popup_height = match state.overlay {
        OverlayMode::Detail => 12,
        OverlayMode::SignalPicker => 10,
        _ => 9,
    };
    let popup = ratatui::layout::Rect {
        x: area.x + (area.width.saturating_sub(popup_width)) / 2,
        y: area.y + (area.height.saturating_sub(popup_height)) / 2,
        width: popup_width,
        height: popup_height,
    };

    match state.overlay {
        OverlayMode::None => {}
        OverlayMode::Detail => {
            let Some(process) = state.selected_process() else {
                return;
            };
            let children = state
                .snapshot
                .as_ref()
                .map(|snapshot| {
                    snapshot
                        .processes
                        .iter()
                        .filter(|child| child.parent == Some(process.pid))
                        .map(|child| child.pid.to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                })
                .filter(|children| !children.is_empty())
                .unwrap_or_else(|| "—".to_owned());
            let start = if process.start_time == 0 {
                "—".to_owned()
            } else {
                process.start_time.to_string()
            };
            let text = vec![
                Line::from(format!("PID: {}", process.pid)),
                Line::from(format!("Name: {}", process.name)),
                Line::from(format!("Command: {}", process.command)),
                Line::from(format!(
                    "Executable: {}",
                    process
                        .exe_path
                        .as_ref()
                        .map_or_else(|| "—".to_owned(), |path| path.display().to_string())
                )),
                Line::from(format!(
                    "Parent PID: {}",
                    process
                        .parent
                        .map_or_else(|| "—".to_owned(), |pid| pid.to_string())
                )),
                Line::from(format!("User: {}", process.user.as_deref().unwrap_or("—"))),
                Line::from(format!("Start time: {start}")),
                Line::from(format!(
                    "Threads: {}",
                    process
                        .thread_count
                        .map_or_else(|| "—".to_owned(), |count| count.to_string())
                )),
                Line::from(format!(
                    "Resident: {}  Virtual: {}",
                    process
                        .mem_bytes
                        .map_or_else(|| "—".to_owned(), format_bytes),
                    process
                        .virtual_mem_bytes
                        .map_or_else(|| "—".to_owned(), format_bytes)
                )),
                Line::from(format!("Children: {children}")),
                Line::from("Esc closes"),
            ];
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Process Detail"),
                ),
                popup,
            );
        }
        OverlayMode::KillConfirm | OverlayMode::ExtraConfirm => {
            let Some(process) = state.selected_process() else {
                return;
            };
            let warning = if state.overlay == OverlayMode::ExtraConfirm {
                "\nThis process is root-owned or under a protected system path.\n"
            } else {
                ""
            };
            let text = format!(
                "Send {} to process {} ({})?\nUser: {}\nExecutable: {}{}Enter/y confirm  Esc/n cancel",
                state.pending_signal,
                process.pid,
                process.name,
                process.user.as_deref().unwrap_or("—"),
                process
                    .exe_path
                    .as_ref()
                    .map_or("—".to_owned(), |path| path.display().to_string()),
                warning,
            );
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(text)
                    .style(Style::default().fg(palette.accent()))
                    .block(
                        Block::default()
                            .borders(Borders::ALL)
                            .title("Confirm Signal"),
                    ),
                popup,
            );
        }
        OverlayMode::SignalPicker => {
            let lines = [
                "Select a signal:",
                "1  SIGTERM",
                "2  SIGKILL",
                "3  SIGINT",
                "4  SIGHUP",
                "5  SIGSTOP",
                "6  SIGCONT",
                "Esc cancels",
            ]
            .iter()
            .copied()
            .map(Line::from)
            .collect::<Vec<_>>();
            frame.render_widget(Clear, popup);
            frame.render_widget(
                Paragraph::new(lines).block(
                    Block::default()
                        .borders(Borders::ALL)
                        .title("Signal Picker"),
                ),
                popup,
            );
        }
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_owned();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    format!(
        "{}...",
        value.chars().take(max_chars - 3).collect::<String>()
    )
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use insta::assert_snapshot;
    use ratatui::{Terminal, backend::TestBackend};

    use crate::{
        app::AppState,
        collector::{CoreKind, CoreUsage, MemoryPressure, MemoryStats, ProcessInfo, Snapshot},
    };

    use super::{Palette, draw, draw_with_palette, truncate};

    #[test]
    fn draw_handles_a_tiny_terminal() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");

        terminal
            .draw(|frame| draw(frame, &AppState::new()))
            .expect("small terminal should render");
    }

    #[test]
    fn long_text_is_truncated() {
        assert_eq!(truncate("a very long process name", 12), "a very lo...");
        assert_eq!(truncate("abc", 3), "abc");
    }

    #[test]
    fn empty_filter_results_are_visible() {
        let mut state = AppState::new();
        state.update(crate::app::Action::SnapshotReceived(Box::new(snapshot())));
        state.update(crate::app::Action::EnterFilter);
        state.update(crate::app::Action::FilterInput('z'));
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");

        terminal
            .draw(|frame| draw_with_palette(frame, &state, Palette::TrueColor))
            .expect("empty results should render");
        let text = terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>();

        assert!(text.contains("No matching processes"), "{text}");
    }

    fn snapshot() -> Snapshot {
        Snapshot {
            cpu_total: Some(23.5),
            cpu_per_core: vec![
                CoreUsage {
                    index: 0,
                    kind: CoreKind::Performance,
                    usage_percent: Some(42.0),
                },
                CoreUsage {
                    index: 1,
                    kind: CoreKind::Efficiency,
                    usage_percent: Some(18.0),
                },
            ],
            memory: MemoryStats {
                total: 16 * 1024 * 1024 * 1024,
                used: 7 * 1024 * 1024 * 1024,
                wired: Some(2 * 1024 * 1024 * 1024),
                compressed: Some(512 * 1024 * 1024),
                cached: Some(1024_u64.pow(3)),
                pressure: Some(MemoryPressure::Normal),
            },
            swap_used: 256 * 1024 * 1024,
            swap_total: 2 * 1024 * 1024 * 1024,
            load_avg: Some((1.25, 0.75, 0.5)),
            uptime: Some(Duration::from_secs(90_061)),
            thread_count: Some(420),
            processes: vec![ProcessInfo {
                pid: 123,
                parent: Some(1),
                name: "sample-process".to_owned(),
                command: "/usr/bin/sample-process --option value".to_owned(),
                exe_path: Some(PathBuf::from("/usr/bin/sample-process")),
                user: Some("user".to_owned()),
                uid: Some(501),
                cpu_percent: Some(4.5),
                mem_bytes: Some(128 * 1024 * 1024),
                virtual_mem_bytes: Some(256 * 1024 * 1024),
                status: "Sleep".to_owned(),
                start_time: 100,
                thread_count: Some(8),
                is_system: false,
            }],
            ..Snapshot::default()
        }
    }

    fn render_snapshot(width: u16, height: u16, palette: Palette) -> String {
        let backend = TestBackend::new(width, height);
        let mut terminal = Terminal::new(backend).expect("test backend should initialize");
        let mut state = AppState::new();
        state.update(crate::app::Action::SnapshotReceived(Box::new(snapshot())));
        state.cpu_history.extend([10, 25, 55, 24]);
        state.memory_history.extend([40, 42, 44, 43]);
        terminal
            .draw(|frame| draw_with_palette(frame, &state, palette))
            .expect("snapshot should render");
        let buffer = terminal.backend().buffer();
        let title_color = buffer.content()[0].fg;
        let width = buffer.area.width as usize;
        let rendered = buffer
            .content()
            .chunks(width)
            .map(|row| {
                row.iter()
                    .map(|cell| cell.symbol())
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n");
        format!("title color: {title_color:?}\n{rendered}")
    }

    #[test]
    fn main_screen_80_by_24_true_color() {
        assert_snapshot!(render_snapshot(80, 24, Palette::TrueColor));
    }

    #[test]
    fn main_screen_80_by_24_256_color() {
        assert_snapshot!(render_snapshot(80, 24, Palette::Ansi256));
    }

    #[test]
    fn main_screen_120_by_40_256_color() {
        assert_snapshot!(render_snapshot(120, 40, Palette::Ansi256));
    }

    #[test]
    fn main_screen_120_by_40_true_color() {
        assert_snapshot!(render_snapshot(120, 40, Palette::TrueColor));
    }
}
