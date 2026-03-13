// Multi-pane renderer for simultaneous task event stream display.
// Exports render_multipane for split-pane layouts; depends on ratatui Layout.
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

pub struct PaneData {
    pub task_id: String,
    pub agent: String,
    pub status: String,
    pub prompt: String,
    pub events: Vec<(String, String, String)>,
    pub duration: String,
    pub tokens: String,
    pub cost: String,
    pub model: String,
    pub milestone: String,
    pub cpu: String,
    pub memory: String,
}

pub fn render_multipane(frame: &mut ratatui::Frame<'_>, panes: &[PaneData], active_pane: usize) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(5), Constraint::Length(1)])
        .split(frame.area());
    let visible_count = panes.len().min(6);
    let pane_areas = compute_pane_layout(chunks[0], visible_count);
    for (index, (pane, pane_area)) in panes
        .iter()
        .take(visible_count)
        .zip(pane_areas.iter())
        .enumerate()
    {
        frame.render_widget(render_pane(pane, index == active_pane), *pane_area);
    }
    let extra = panes.len().saturating_sub(6);
    let footer = if extra > 0 {
        format!("m=board Tab=next pane q=quit | +{extra} more")
    } else {
        "m=board Tab=next pane q=quit".into()
    };
    frame.render_widget(Paragraph::new(footer), chunks[1]);
}

fn compute_pane_layout(area: Rect, count: usize) -> Vec<Rect> {
    match count {
        0 => vec![],
        1 => vec![area],
        2 => Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(area)
            .to_vec(),
        3 => {
            let tb = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            let lr = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(tb[0]);
            vec![lr[0], lr[1], tb[1]]
        }
        4 => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            let top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(rows[0]);
            let bot = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(rows[1]);
            vec![top[0], top[1], bot[0], bot[1]]
        }
        5 | 6 => {
            let rows = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(area);
            let top = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                ])
                .split(rows[0]);
            let bot = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([
                    Constraint::Percentage(33),
                    Constraint::Percentage(33),
                    Constraint::Percentage(34),
                ])
                .split(rows[1]);
            let mut v = vec![top[0], top[1], top[2], bot[0], bot[1], bot[2]];
            v.truncate(count);
            v
        }
        _ => vec![],
    }
}

fn render_pane(pane: &PaneData, is_active: bool) -> List<'static> {
    let border_color = if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let status_color = match pane.status.as_str() {
        "done" | "merged" => Color::Green,
        "running" => Color::Yellow,
        "awaiting_input" => Color::Magenta,
        "failed" => Color::Red,
        "pending" => Color::Gray,
        "skipped" => Color::Blue,
        _ => Color::White,
    };
    let title = format!(" {} {} {} ", pane.task_id, pane.agent, pane.status);
    let prompt = if pane.prompt.len() <= 60 {
        pane.prompt.clone()
    } else {
        format!("{}...", &pane.prompt[..57])
    };
    let mut items = vec![ListItem::new(format!("Prompt: {prompt}"))];
    let summary = format!(
        "Duration: {}  Tokens: {}  Cost: {}  Model: {}  CPU: {}  Mem: {}",
        pane.duration, pane.tokens, pane.cost, pane.model, pane.cpu, pane.memory
    );
    items.insert(
        1,
        ListItem::new(summary).style(Style::default().fg(Color::DarkGray)),
    );
    if !pane.milestone.is_empty() {
        items.insert(
            2,
            ListItem::new(format!("Progress: {}", pane.milestone))
                .style(Style::default().fg(Color::Green)),
        );
    }
    let recent_events = if pane.events.len() > 20 {
        &pane.events[pane.events.len() - 20..]
    } else {
        &pane.events
    };
    for (ts, kind, detail) in recent_events {
        let event_style = match kind.as_str() {
            "milestone" => Style::default().fg(Color::Green),
            "error" => Style::default().fg(Color::Red),
            "reasoning" => Style::default().fg(Color::Cyan),
            "completion" => Style::default().fg(Color::DarkGray),
            _ => Style::default(),
        };
        items.push(ListItem::new(format!("{ts} [{kind}] {detail}")).style(event_style));
    }
    List::new(items).block(
        Block::default()
            .title(title)
            .title_style(Style::default().fg(status_color))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    )
}
