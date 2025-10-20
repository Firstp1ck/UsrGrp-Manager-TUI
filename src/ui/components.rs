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
    let mut chips = Vec::new();
    if app.users_filter_chips.human_only {
        chips.push("human");
    }
    if app.users_filter_chips.system_only {
        chips.push("system");
    }
    if app.users_filter_chips.inactive {
        chips.push("inactive");
    }
    if app.users_filter_chips.no_home {
        chips.push("no_home");
    }
    if app.users_filter_chips.locked {
        chips.push("locked");
    }
    if app.users_filter_chips.no_password {
        chips.push("no_password");
    }
    if app.users_filter_chips.expired {
        chips.push("expired");
    }
    let chips_str = if chips.is_empty() { String::new() } else { format!("  filters:[{}]", chips.join(",")) };
    let msg = format!(
        "mode: {mode}  users:{}  groups:{}  rows/page:{}{}",
        app.users.len(),
        app.groups.len(),
        app.rows_per_page,
        chips_str
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

/// Render filter selection modal depending on active tab.
pub fn render_filter_modal(f: &mut Frame, area: Rect, app: &AppState, state: &ModalState) {
    if let ModalState::FilterMenu { selected } = state {
        match app.active_tab {
            crate::app::ActiveTab::Users => {
                let width = 64u16.min(area.width.saturating_sub(4)).max(44);
                let height = 14u16.min(area.height.saturating_sub(4)).max(10);
                let rect = centered_rect(width, height, area);
                let opts: [&str; 8] = [
                    "Show all",
                    "Only show User IDs (>=1000)",
                    "Only show System IDs (<1000)",
                    "Inactive shell (nologin/false)",
                    "No home directory",
                    "Locked account",
                    "No password set",
                    "Password expired",
                ];
                let mut text = String::new();
                for (idx, label) in opts.iter().enumerate() {
                    let marker = if idx == *selected { "▶" } else { " " };
                    // For chip options (idx >= 3) show checkbox from state
                    let checkbox = if idx >= 3 {
                        let checked = match idx {
                            3 => app.users_filter_chips.inactive,
                            4 => app.users_filter_chips.no_home,
                            5 => app.users_filter_chips.locked,
                            6 => app.users_filter_chips.no_password,
                            7 => app.users_filter_chips.expired,
                            _ => false,
                        };
                        if checked { "[x] " } else { "[ ] " }
                    } else {
                        ""
                    };
                    text.push_str(&format!("{} {}{}\n", marker, checkbox, label));
                }
                let p = Paragraph::new(text).block(
                    Block::default()
                        .title("Filter users")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(app.theme.border)),
                );
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            crate::app::ActiveTab::Groups => {
                let width = 56u16.min(area.width.saturating_sub(4)).max(40);
                let height = 9u16;
                let rect = centered_rect(width, height, area);
                let options: [&str; 3] = [
                    "Show all",
                    "Only show User GIDs (>=1000)",
                    "Only show System GIDs (<1000)",
                ];
                let mut text = String::new();
                for (idx, label) in options.iter().enumerate() {
                    if idx == *selected {
                        text.push_str(&format!("▶ {}\n", label));
                    } else {
                        text.push_str(&format!("  {}\n", label));
                    }
                }
                let p = Paragraph::new(text).block(
                    Block::default()
                        .title("Filter groups")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(app.theme.border)),
                );
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
        }
    }
}
