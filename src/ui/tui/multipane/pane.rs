// Individual multipane task rendering.
// Builds pane titles, summaries, and the scrollable event window.

use super::{PaneData, truncate_visible};
use ratatui::prelude::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, List, ListItem};

pub(super) fn render_pane(pane: &PaneData, is_active: bool, area_height: u16) -> List<'static> {
    let is_done = matches!(pane.status.as_str(), "done" | "merged");
    let is_running = pane.status == "running";
    let border_color = pane_border_color(pane, is_active, is_done, is_running);
    let status_color = pane_status_color(pane);
    let title_status = pane_title_status(pane);
    let title = format!(" {} {} [{}] ", pane.task_id, pane.agent, title_status);
    let bottom_title = pane_bottom_title(pane, is_done, is_running);
    let content_style = if is_done {
        Style::default()
            .fg(Color::Indexed(240))
            .add_modifier(Modifier::DIM)
    } else {
        Style::default()
    };
    let title_style = if is_running {
        Style::default()
            .fg(status_color)
            .add_modifier(Modifier::BOLD)
    } else if is_done {
        Style::default()
            .fg(Color::Indexed(240))
            .add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(status_color)
    };
    let mut items = pane_header_items(pane);
    append_pane_events(&mut items, pane, area_height, is_done, content_style);
    List::new(items).style(content_style).block(
        Block::default()
            .title(title)
            .title_bottom(bottom_title)
            .title_style(title_style)
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color)),
    )
}

fn pane_bottom_title(pane: &PaneData, is_done: bool, is_running: bool) -> String {
    let bottom_title = {
        let mut parts = vec![];
        if !pane.workgroup.is_empty() {
            parts.push(pane.workgroup.clone());
        }
        if !pane.worktree_branch.is_empty() {
            parts.push(pane.worktree_branch.clone());
        }
        if !pane.model.is_empty() && pane.model != "unknown" {
            parts.push(pane.model.clone());
        }
        if !pane.created.is_empty() {
            parts.push(format!("Created {}", pane.created));
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
    bottom_title
}

fn append_pane_events(
    items: &mut Vec<ListItem<'static>>,
    pane: &PaneData,
    area_height: u16,
    is_done: bool,
    content_style: Style,
) {
    // Scrollable event window sized to the real pane height (not a fixed 12).
    let header_lines = items.len();
    let inner_h = (area_height as usize).saturating_sub(2); // borders
    let visible_count = inner_h.saturating_sub(header_lines + 1).max(1); // +1 indicator
    let max_offset = pane.events.len().saturating_sub(1);
    let scroll = pane.scroll_offset.min(max_offset);
    let end = pane.events.len().saturating_sub(scroll);
    let start = end.saturating_sub(visible_count);
    let visible = &pane.events[start..end];
    for (ts, kind, detail) in visible {
        let event_style = if is_done {
            content_style
        } else {
            match kind.as_str() {
                "milestone" => Style::default().fg(Color::Green),
                "error" => Style::default().fg(Color::Red),
                "reasoning" => Style::default().fg(Color::Cyan),
                "completion" => Style::default().fg(Color::Indexed(243)),
                _ => Style::default(),
            }
        };
        // Cap detail so a single long line cannot stretch the pane layout.
        let detail = truncate_visible(detail, 72);
        items.push(ListItem::new(format!("{ts} [{kind}] {detail}")).style(event_style));
    }
    if pane.total_events > visible_count {
        let pos = format!(
            "[{}/{}]",
            pane.total_events.saturating_sub(scroll),
            pane.total_events
        );
        items.push(ListItem::new(pos).style(Style::default().fg(Color::Indexed(243))));
    }
}

fn pane_header_items(pane: &PaneData) -> Vec<ListItem<'static>> {
    // Ellipsis truncation is intentional and visible ("...").
    let prompt = truncate_visible(&pane.prompt, 60);
    let mut items = vec![ListItem::new(format!("Prompt: {prompt}"))];
    items.push(ListItem::new(format!("State: {}", pane.activity)));
    let summary = format!(
        "Tokens: {}  Cost: {}  CPU: {}  Mem: {}",
        pane.tokens, pane.cost, pane.cpu, pane.memory
    );
    items.push(ListItem::new(summary).style(Style::default().fg(Color::Indexed(243))));
    if !pane.milestone.is_empty() {
        items.push(
            ListItem::new(format!(
                "Progress: {}",
                truncate_visible(&pane.milestone, 48)
            ))
            .style(Style::default().fg(Color::Green)),
        );
    }
    items
}

fn pane_border_color(pane: &PaneData, is_active: bool, is_done: bool, is_running: bool) -> Color {
    let is_failed = pane.status == "failed";
    let border_color = if is_active {
        Color::Cyan
    } else if is_running {
        Color::Yellow
    } else if is_failed {
        Color::Red
    } else if is_done {
        Color::Indexed(236)
    } else {
        Color::Indexed(240)
    };
    border_color
}

fn pane_status_color(pane: &PaneData) -> Color {
    let status_color = match pane.status.as_str() {
        "done" | "merged" => Color::Green,
        "running" => Color::Yellow,
        "awaiting_input" => Color::Magenta,
        "failed" => Color::Red,
        "pending" => Color::Indexed(250),
        "skipped" => Color::Blue,
        _ => Color::White,
    };
    status_color
}

fn pane_title_status(pane: &PaneData) -> String {
    let title_status = match pane.status.as_str() {
        "done" | "merged" => format!("✓ {}", pane.status),
        "failed" => format!("✗ {}", pane.status),
        _ => pane.status.clone(),
    };
    title_status
}
