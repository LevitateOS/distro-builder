//! The install screen's layout: two bordered panes side by side, the install hub on
//! the left and the docs on the right, over a one-row footer.
//!
//! The split pane owns the screen's geometry: it draws with this layout and sizes
//! each child's terminal from it, its smoke check runs the panes at these sizes, and
//! `cargo xtask check tui` renders the docs app at the size it really gets.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Each pane's share of the width, in percent.
pub const PANE_PERCENT: u16 = 50;

pub struct ScreenLayout {
    /// Left (hub) and right (docs) pane areas, borders included.
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

/// The hub's and the docs' terminal sizes on a `columns`×`rows` console.
pub fn pane_sizes(columns: u16, rows: u16) -> [(u16, u16); 2] {
    screen_layout(Rect::new(0, 0, columns, rows))
        .panes
        .map(pane_terminal_size)
}

/// The docs pane's terminal size on a `columns`×`rows` console.
pub fn docs_pane_size(columns: u16, rows: u16) -> (u16, u16) {
    pane_sizes(columns, rows)[1]
}
