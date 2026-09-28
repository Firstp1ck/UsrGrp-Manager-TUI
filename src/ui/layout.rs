//! Main-screen layout geometry.
//!
//! The renderer and the page keys both read the pane sizes from here, so a
//! page always moves by the number of rows the pane actually shows.

use ratatui::layout::{Constraint, Direction, Layout, Rect};

use crate::app::ActiveTab;

/// Minimum width at which the keybindings panel gets its own column.
const KEYBINDS_MIN_WIDTH: u16 = 90;

/// Header height: one line of text inside a border.
const HEADER_ROWS: u16 = 3;

/// Details lines on the users tab when none of them wraps.
const USER_DETAILS_BASE_ROWS: usize = 10;

/// Details lines on the groups tab when none of them wraps.
const GROUP_DETAILS_BASE_ROWS: usize = 5;

/// Rows the details pane spends on its border (top and bottom).
const DETAILS_CHROME_ROWS: u16 = 2;

/// Smallest member list: three rows inside a border. It wins over the
/// details pane when the terminal is short.
const MEMBERS_MIN_ROWS: u16 = 5;

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

/// Split the screen into header, body panes, and status bar. The details
/// pane is `details_rows` lines tall inside its border, capped so the member
/// list keeps [`MEMBERS_MIN_ROWS`]; the member list takes the rest. The pane
/// widths do not depend on `details_rows`.
pub fn main_layout(area: Rect, show_keybinds: bool, details_rows: usize) -> MainLayout {
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
    let details_height = u16::try_from(details_rows)
        .unwrap_or(u16::MAX)
        .saturating_add(DETAILS_CHROME_ROWS)
        .min(body[1].height.saturating_sub(MEMBERS_MIN_ROWS));
    let side = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(details_height),
            Constraint::Min(MEMBERS_MIN_ROWS),
        ])
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

/// Details lines on `tab` when none of them wraps. Callers that do not
/// build the details (the page keys) size the layout with this.
pub fn details_base_rows(tab: ActiveTab) -> usize {
    match tab {
        ActiveTab::Users => USER_DETAILS_BASE_ROWS,
        ActiveTab::Groups => GROUP_DETAILS_BASE_ROWS,
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
        assert!(main_layout(wide, true, 10).keybinds.is_some());
        assert!(main_layout(wide, false, 10).keybinds.is_none());
        assert!(
            main_layout(Rect::new(0, 0, 89, 40), true, 10)
                .keybinds
                .is_none()
        );
    }

    #[test]
    fn table_rows_subtract_border_and_header() {
        let layout = main_layout(Rect::new(0, 0, 100, 40), true, 10);
        // 40 rows - 3 header - 1 status = 36 body rows; minus border and header.
        assert_eq!(table_rows(layout.table), 33);
        assert_eq!(table_rows(Rect::new(0, 0, 10, 2)), 1);
    }

    #[test]
    fn details_pane_fits_its_rows_and_members_take_the_rest() {
        let area = Rect::new(0, 0, 100, 40);
        let users = main_layout(area, false, details_base_rows(ActiveTab::Users));
        assert_eq!(users.details.height, 12);
        assert_eq!(users.members.height, 36 - 12);
        let groups = main_layout(area, false, details_base_rows(ActiveTab::Groups));
        assert_eq!(groups.details.height, 7);
        assert_eq!(groups.members.height, 36 - 7);
        assert_eq!(groups.table, users.table);
        let wrapped = main_layout(area, false, 14);
        assert_eq!(wrapped.details.height, 16);
        assert_eq!(wrapped.details.width, users.details.width);
    }

    #[test]
    fn short_terminal_keeps_three_member_rows() {
        // 12 rows - 3 header - 1 status = 8 body rows.
        let layout = main_layout(Rect::new(0, 0, 100, 12), false, 10);
        assert_eq!(layout.members.height, 5);
        assert_eq!(layout.details.height, 3);
    }
}
