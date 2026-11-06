//! The install screen's layout: two bordered panes side by side over a one-row footer.
//!
//! The split pane draws with it and sizes each child's terminal from it, and
//! `cargo xtask check tui` uses it to render the docs app at the size it really gets.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Each pane's share of the width, in percent.
const PANE_PERCENT: u16 = 50;

pub struct ScreenLayout {
    /// Left (shell) and right (docs) pane areas, borders included.
    pub panes: [Rect; 2],
    pub footer: Rect,
}

pub fn screen_layout(area: Rect) -> ScreenLayout {
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(1)])
        .split(area);
    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(PANE_PERCENT),
            Constraint::Percentage(100 - PANE_PERCENT),
        ])
        .split(rows[0]);
    ScreenLayout {
        panes: [panes[0], panes[1]],
        footer: rows[1],
    }
}

/// The terminal a pane's child sees: the pane minus its one-cell border.
pub fn pane_terminal_size(pane: Rect) -> (u16, u16) {
    (
        pane.width.saturating_sub(2).max(1),
        pane.height.saturating_sub(2).max(1),
    )
}

/// The docs pane's terminal size on a `columns`×`rows` console.
pub fn docs_pane_size(columns: u16, rows: u16) -> (u16, u16) {
    pane_terminal_size(screen_layout(Rect::new(0, 0, columns, rows)).panes[1])
}
