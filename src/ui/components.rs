//! Shared immutable UI components.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{AppState, ModalState};

pub fn render_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let stale = app
        .diagnostics
        .stale_reason
        .as_ref()
        .map_or("fresh", |_| "STALE");
    let config = if app.diagnostics.config_messages.is_empty() {
        ""
    } else {
        " config-error"
    };
    let text = format!(
        "users:{} groups:{} rows/page:{} data:{} shadow:{}{}",
        app.users.len(),
        app.groups.len(),
        app.rows_per_page,
        stale,
        app.diagnostics.shadow.availability_label(),
        config
    );
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .fg(app.theme.status_fg)
                .bg(app.theme.status_bg),
        ),
        area,
    );
}

pub fn render_keybinds_panel(frame: &mut Frame, area: Rect, app: &AppState) {
    let mut bindings: BTreeMap<&str, BTreeSet<String>> = BTreeMap::new();
    for ((modifiers, code), action) in app.keymap.all_bindings() {
        bindings
            .entry(crate::app::keymap::format_action(action))
            .or_default()
            .insert(crate::app::keymap::Keymap::format_key(modifiers, code));
    }
    let lines = bindings
        .into_iter()
        .map(|(action, keys)| {
            Line::from(vec![
                Span::styled(
                    format!("{action}: "),
                    Style::default().add_modifier(Modifier::BOLD),
                ),
                Span::raw(keys.into_iter().collect::<Vec<_>>().join(", ")),
            ])
        })
        .collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Keybindings")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        area,
    );
}

pub fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

pub fn render_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    let (title, body) = match modal {
        ModalState::Info { message } => ("Result", message.clone()),
        ModalState::Help { .. } => ("Help", "q quit; Tab switch tabs; Shift+Tab switch pane; / search; n create; Enter action; Delete delete; f filters".to_owned()),
        ModalState::SudoPrompt { password, error, .. } => ("Authentication required", format!("Enter sudo password:\n{}{}", "*".repeat(password.len()), error.as_ref().map_or(String::new(), |error| format!("\n{error}")))),
        ModalState::OperationConfirm { selected, action, preview } => (
            "Confirm prepared operation",
            format!(
                "{action}\n\n{}{}\n\n{} Apply   {} Cancel",
                preview.iter().take(12).cloned().collect::<Vec<_>>().join("\n"),
                if preview.len() > 12 { "\n… additional steps omitted from this modal" } else { "" },
                marker(*selected, 0),
                marker(*selected, 1)
            ),
        ),
        ModalState::FilterMenu { selected } => {
            let options = match app.active_tab { crate::app::ActiveTab::Users => vec!["Show all", "Human users", "System users", "Inactive shell", "No home", "Locked (shadow)", "No password (shadow)", "Expired (shadow)"], crate::app::ActiveTab::Groups => vec!["Show all", "User GIDs", "System GIDs"] };
            ("Filters", options.iter().enumerate().map(|(index, option)| format!("{} {option}", marker(*selected, index))).collect::<Vec<_>>().join("\n"))
        }
        _ => return,
    };
    let rect = centered_rect(
        area.width.saturating_sub(8).min(76),
        area.height.saturating_sub(6).min(20),
        area,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(body).wrap(Wrap { trim: false }).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        rect,
    );
}

fn marker(selected: usize, index: usize) -> &'static str {
    if selected == index { "▶" } else { " " }
}
