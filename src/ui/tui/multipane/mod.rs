// Multi-pane renderer for simultaneous task event stream display.
// Exports render_multipane for split-pane layouts; depends on ratatui Layout.
mod pane;
use pane::render_pane;

use super::app::App;
use super::status_bar::{StatusBarMode, render_status_bar};
use ratatui::layout::{Constraint, Direction, Layout, Rect};

pub struct PaneData {
    pub task_id: String,
    pub agent: String,
    pub status: String,
    pub activity: String,
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
    pub created: String,
    pub elapsed: String,
    pub scroll_offset: usize,
    pub total_events: usize,
}

pub fn render_multipane(
    frame: &mut ratatui::Frame<'_>,
    panes: &[PaneData],
    app: &App,
    active_pane: usize,
) {
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
        frame.render_widget(
            render_pane(pane, index == active_pane, pane_area.height),
            *pane_area,
        );
    }
    let extra = panes.len().saturating_sub(6);
    render_status_bar(
        frame,
        chunks[1],
        app,
        StatusBarMode::Multipane { extra_panes: extra },
    );
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
        4 => four_pane_layout(area),
        5 | 6 => six_pane_layout(area, count),
        _ => vec![],
    }
}

fn truncate_visible(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_string();
    }
    let end = max.saturating_sub(3);
    let clipped: String = value.chars().take(end).collect();
    format!("{clipped}...")
}

fn four_pane_layout(area: Rect) -> Vec<Rect> {
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

fn six_pane_layout(area: Rect, count: usize) -> Vec<Rect> {
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
