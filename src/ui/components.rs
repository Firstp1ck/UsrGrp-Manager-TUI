//! Shared UI components (status bar, modal helpers).
//!
//! Contains small building blocks reused by users/groups screens.
//!
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{AppState, ModalState};

/// Render the bottom status bar with mode and counts.
pub fn render_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    let mode = match app.input_mode {
        crate::app::InputMode::Normal => "NORMAL",
        crate::app::InputMode::SearchUsers => "SEARCH(users)",
        crate::app::InputMode::SearchGroups => "SEARCH(groups)",
        crate::app::InputMode::Modal => "MODAL",
    };
    let msg = format!(
        "mode: {mode}  users:{}  groups:{}  rows/page:{}",
        app.users.len(),
        app.groups.len(),
        app.rows_per_page
    );
    let p = Paragraph::new(msg).style(
        Style::default()
            .fg(app.theme.status_fg)
            .bg(app.theme.status_bg),
    );
    f.render_widget(p, area);
}

/// Compute a rectangle centered within `area` with a maximum size.
pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width: width.min(area.width),
        height: height.min(area.height),
    }
}

/// Render a generic informational modal dialog.
pub fn render_info_modal(f: &mut Frame, area: Rect, app: &AppState, state: &ModalState) {
    if let ModalState::Info { message } = state {
        // Compute a sensible max width and height; wrap long text
        let max_w = area.width.saturating_sub(6).max(30);
        let min_w = 40u16.min(max_w);
        let approx_lines = (message.len() as u16 / (min_w.saturating_sub(4).max(10))).max(1);
        let max_h = area.height.saturating_sub(6).max(5);
        let height = (approx_lines + 4).min(max_h).max(5);
        let rect = centered_rect(min_w, height, area);
        let p = Paragraph::new(message.clone())
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title("Info")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
        f.render_widget(Clear, rect);
        f.render_widget(p, rect);
    }
}

/// Render the sudo password prompt modal.
pub fn render_sudo_modal(f: &mut Frame, area: Rect, app: &AppState, state: &ModalState) {
    if let ModalState::SudoPrompt {
        password, error, ..
    } = state
    {
        let width = 50u16.min(area.width.saturating_sub(4)).max(40);
        let height = if error.as_ref().map(|e| !e.is_empty()).unwrap_or(false) {
            8
        } else {
            6
        };
        let rect = centered_rect(width, height, area);
        let mut body = String::new();
        body.push_str("Enter sudo password:\n");
        let masked = "*".repeat(password.len());
        body.push_str(&format!("{}\n", masked));
        if let Some(err) = error
            && !err.is_empty()
        {
            body.push('\n');
            body.push_str(err);
        }
        let p = Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .title("Authentication required")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        );
        f.render_widget(Clear, rect);
        f.render_widget(p, rect);
    }
}
