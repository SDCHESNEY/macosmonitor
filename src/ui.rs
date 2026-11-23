use std::time::Duration;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    text::Line,
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState},
};

use crate::{app::AppState, collector::ProcessInfo};

pub fn draw(frame: &mut Frame<'_>, state: &AppState) {
    let layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(frame.area());

    let title = Paragraph::new("rtop — system monitor").style(Style::default().fg(Color::Cyan));
    frame.render_widget(title, layout[0]);

    let metrics = state
        .snapshot
        .as_ref()
        .map_or_else(|| "Collecting system data…".to_owned(), metrics_line);
    frame.render_widget(Paragraph::new(metrics), layout[1]);

    let rows = state
        .snapshot
        .as_ref()
        .map(|snapshot| {
            snapshot
                .processes
                .iter()
                .map(process_row)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let header = Row::new(["PID", "Name", "User", "CPU %", "Memory", "Status"])
        .style(Style::default().add_modifier(Modifier::BOLD));
    let table = Table::new(
        rows,
        [
            Constraint::Length(7),
            Constraint::Percentage(30),
            Constraint::Percentage(16),
            Constraint::Length(8),
            Constraint::Percentage(20),
            Constraint::Percentage(18),
        ],
    )
    .header(header)
    .block(Block::default().borders(Borders::ALL).title("Processes"))
    .row_highlight_style(
        Style::default()
            .bg(Color::DarkGray)
            .add_modifier(Modifier::BOLD),
    )
    .highlight_symbol("› ");
    let mut table_state = TableState::default();
    table_state.select((state.process_count() > 0).then_some(state.selected_index));
    frame.render_stateful_widget(table, layout[2], &mut table_state);

    frame.render_widget(
        Paragraph::new(Line::from("↑/↓ or j/k navigate • q/Esc quit")),
        layout[3],
    );
}

fn process_row(process: &ProcessInfo) -> Row<'static> {
    let cpu = process
        .cpu_percent
        .map_or_else(|| "—".to_owned(), |usage| format!("{usage:.1}"));
    let memory = process
        .mem_bytes
        .map_or_else(|| "—".to_owned(), format_bytes);
    Row::new(vec![
        Cell::from(process.pid.to_string()),
        Cell::from(process.name.clone()),
        Cell::from(process.user.as_deref().unwrap_or("—").to_owned()),
        Cell::from(cpu),
        Cell::from(memory),
        Cell::from(process.status.clone()),
    ])
}

fn metrics_line(snapshot: &crate::collector::Snapshot) -> String {
    let cpu = snapshot
        .cpu_total
        .map_or_else(|| "—".to_owned(), |usage| format!("{usage:.1}%"));
    let memory = format!(
        "{}/{}",
        format_bytes(snapshot.memory.used),
        format_bytes(snapshot.memory.total)
    );
    let swap = format!(
        "{}/{}",
        format_bytes(snapshot.swap_used),
        format_bytes(snapshot.swap_total)
    );
    let load = snapshot.load_avg.map_or_else(
        || "—".to_owned(),
        |(one, five, fifteen)| format!("{one:.2} {five:.2} {fifteen:.2}"),
    );
    let uptime = snapshot
        .uptime
        .map_or_else(|| "—".to_owned(), format_uptime);
    let age = snapshot.taken_at.elapsed().as_secs_f32();
    format!("CPU {cpu}  MEM {memory}  SWAP {swap}  LOAD {load}  UP {uptime}  AGE {age:.1}s")
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

#[cfg(test)]
mod tests {
    use ratatui::{Terminal, backend::TestBackend};

    use crate::app::AppState;

    use super::draw;

    #[test]
    fn draw_handles_a_tiny_terminal() {
        let backend = TestBackend::new(20, 5);
        let mut terminal = Terminal::new(backend).expect("test terminal should initialize");

        terminal
            .draw(|frame| draw(frame, &AppState::new()))
            .expect("small terminal should render");
    }
}
