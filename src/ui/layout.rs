//! Main-screen layout geometry.
//!
//! The renderer and the page keys both read the pane sizes from here, so a
//! page always moves by the number of rows the pane actually shows.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

/// Minimum width at which the keybindings panel gets its own column.
const KEYBINDS_MIN_WIDTH: u16 = 90;

/// Header height: one line of text inside a border.
const HEADER_ROWS: u16 = 3;

/// Rows a table spends on its border (top and bottom) and column header.
const TABLE_CHROME_ROWS: u16 = 3;

/// Areas of the main screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MainLayout {
    pub header: Rect,
    /// Users or groups table.
    pub table: Rect,
    /// Details of the selected row.
    pub details: Rect,
    /// "Member of" (users tab) or "Members" (groups tab) table.
    pub members: Rect,
    /// Keybindings panel, when shown and the terminal is wide enough.
    pub keybinds: Option<Rect>,
    pub status: Rect,
}

/// Smallest terminal the main view and dialogs are drawn on.
const MIN_WIDTH: u16 = 30;
const MIN_HEIGHT: u16 = 8;

/// Below the minimum size only a resize hint is drawn: no panes, no dialog.
pub fn below_minimum(area: Rect) -> bool {
    area.width < MIN_WIDTH || area.height < MIN_HEIGHT
}

/// Split the screen into header, body panes, and status bar.
pub fn main_layout(area: Rect, show_keybinds: bool) -> MainLayout {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(HEADER_ROWS),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(area);
    let body = if show_keybinds && root[1].width >= KEYBINDS_MIN_WIDTH {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Percentage(35),
                Constraint::Percentage(20),
            ])
            .split(root[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(root[1])
    };
    let side = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(7), Constraint::Min(5)])
        .split(body[1]);
    MainLayout {
        header: root[0],
        table: body[0],
        details: side[0],
        members: side[1],
        keybinds: body.get(2).copied(),
        status: root[2],
    }
}

/// Data rows a bordered table with a column header shows in `area`.
/// Never zero, so paging always moves.
pub fn table_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(TABLE_CHROME_ROWS)).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keybinds_column_needs_the_toggle_and_width() {
        let wide = Rect::new(0, 0, 100, 40);
        assert!(main_layout(wide, true).keybinds.is_some());
        assert!(main_layout(wide, false).keybinds.is_none());
        assert!(
            main_layout(Rect::new(0, 0, 89, 40), true)
                .keybinds
                .is_none()
        );
    }

    #[test]
    fn table_rows_subtract_border_and_header() {
        let layout = main_layout(Rect::new(0, 0, 100, 40), true);
        // 40 rows - 3 header - 1 status = 36 body rows; minus border and header.
        assert_eq!(table_rows(layout.table), 33);
        assert_eq!(table_rows(Rect::new(0, 0, 10, 2)), 1);
    }
}
