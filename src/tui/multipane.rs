// Multi-pane renderer for simultaneous task event stream display.
// Exports render_multipane for split-pane layouts; depends on ratatui Layout.
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem, Paragraph};

pub struct PaneData {
    pub task_id: String,
    pub agent: String,
    pub status: String,
    pub prompt: String,
    pub events: Vec<(String, String, String)>,
    pub tokens: String,
    pub cost: String,
    pub model: String,
    pub milestone: String,
    pub cpu: String,
    pub memory: String,
    pub workgroup: String,
    pub worktree_branch: String,
    pub elapsed: String,
    pub scroll_offset: usize,
    pub total_events: usize,
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
        format!("Tab=pane j/k=scroll Enter=detail Esc=board q=quit | +{extra} more")
    } else {
        "Tab=pane j/k=scroll Enter=detail Esc=board q=quit".into()
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
    let is_done = matches!(pane.status.as_str(), "done" | "merged");
    let is_running = pane.status == "running";
    let is_failed = pane.status == "failed";
    let border_color = if is_done {
        Color::DarkGray
    } else if is_running {
        Color::Yellow
    } else if is_failed {
        Color::Red
    } else if is_active {
        Color::Cyan
    } else {
        Color::DarkGray
    };
    let status_color = match pane.status.as_str() {
        "done" | "merged" => Color::DarkGray,
        "running" => Color::Yellow,
        "awaiting_input" => Color::Magenta,
        "failed" => Color::Red,
        "pending" => Color::Gray,
        "skipped" => Color::Blue,
        _ => Color::White,
    };
    let title_status = match pane.status.as_str() {
        "done" | "merged" => format!("✓ {}", pane.status),
        "failed" => format!("✗ {}", pane.status),
        _ => pane.status.clone(),
    };
    let title = format!(" {} {} [{}] ", pane.task_id, pane.agent, title_status);
    let bottom_title = {
        let mut parts = vec![];
        if !pane.workgroup.is_empty() {
            parts.push(pane.workgroup.clone());
        }
        if !pane.worktree_branch.is_empty() {
            parts.push(pane.worktree_branch.clone());
        }
        if !pane.model.is_empty() && pane.model != "-" {
            parts.push(pane.model.clone());
        }
        if !pane.elapsed.is_empty() {
            let elapsed = if is_done {
                format!("Done in {}", pane.elapsed)
            } else if is_running {
                format!("▶ {}", pane.elapsed)
            } else {
                pane.elapsed.clone()
            };
            parts.push(elapsed);
        }
        format!(" {} ", parts.join(" | "))
    };
    let content_style = if is_done {
        Style::default()
            .fg(Color::DarkGray)
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    let title_style = if is_running {
        Style::default()
            .fg(status_color)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(status_color)
    };
    let prompt = if pane.prompt.len() <= 60 {
        pane.prompt.clone()
    } else {
        format!("{}...", &pane.prompt[..57])
    };
    let mut items = vec![ListItem::new(format!("Prompt: {prompt}"))];
    let summary = format!(
        "Tokens: {}  Cost: {}  CPU: {}  Mem: {}",
        pane.tokens, pane.cost, pane.cpu, pane.memory
    );
    items.push(ListItem::new(summary).style(Style::default().fg(Color::DarkGray)));
    if !pane.milestone.is_empty() {
        items.push(
            ListItem::new(format!("Progress: {}", pane.milestone))
                .style(Style::default().fg(Color::Green)),
        );
    }
    // Scrollable event window
    let header_lines = items.len();
    let pane_height: usize = 12;
    let visible_count = pane_height.saturating_sub(header_lines + 1); // +1 for scroll indicator
    let end = pane.events.len().saturating_sub(pane.scroll_offset);
    let start = end.saturating_sub(visible_count);
    let visible = &pane.events[start..end];
    for (ts, kind, detail) in visible {
        let event_style = match kind.as_str() {
            "milestone" => Style::default().fg(Color::Green),
            "error" => Style::default().fg(Color::Red),
            "reasoning" => Style::default().fg(Color::Cyan),
            "completion" => Style::default().fg(Color::DarkGray),
            _ => Style::default(),
        };
        items.push(ListItem::new(format!("{ts} [{kind}] {detail}")).style(event_style));
    }
    if pane.total_events > visible_count {
        let pos = format!(
            "[{}/{}]",
            pane.total_events.saturating_sub(pane.scroll_offset),
            pane.total_events
        );
        items.push(ListItem::new(pos).style(Style::default().fg(Color::DarkGray)));
    }
    List::new(items).style(content_style).block(
        Block::default()
            .title(title)
            .title_bottom(bottom_title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    )
}
