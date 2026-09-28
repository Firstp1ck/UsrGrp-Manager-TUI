//! Shared immutable UI components.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
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

/// Footer for single-choice menus that Backspace also closes.
pub const MENU_FOOTER: &str = "↑↓ move  Enter select  Backspace back  Esc close";
/// Footer for single-choice lists without a Backspace binding.
pub const LIST_FOOTER: &str = "↑↓ move  Enter select  Esc back";
/// Footer for multi-select candidate lists.
pub const MULTI_FOOTER: &str = "↑↓ move  Space tick  Enter apply  Esc close";
/// Footer for yes/no dialogs.
pub const YES_NO_FOOTER: &str = "←→ choose  y/n  Enter confirm  Esc cancel";
/// Footer for single-line text inputs.
pub const TEXT_FOOTER: &str = "type to edit  Enter submit  Esc back";
/// Footer for forms with several fields.
pub const FORM_FOOTER: &str = "↑↓ field  Space tick  Enter on Submit  Esc back";

/// Draw a centered dialog: title, body, and a key hint footer on the last
/// interior row. Every modal renderer goes through this frame.
pub fn render_modal_frame(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    title: &str,
    body: Vec<Line<'_>>,
    footer: &str,
) {
    let rect = centered_rect(
        area.width.saturating_sub(8).min(76),
        area.height.saturating_sub(6).min(20),
        area,
    );
    let block = Block::default()
        .title(title.to_owned())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    let (body_area, footer_area) = if inner.height >= 2 {
        (
            Rect {
                height: inner.height - 1,
                ..inner
            },
            Some(Rect {
                y: inner.y + inner.height - 1,
                height: 1,
                ..inner
            }),
        )
    } else {
        (inner, None)
    };
    frame.render_widget(Paragraph::new(body).wrap(Wrap { trim: false }), body_area);
    if let Some(footer_area) = footer_area {
        frame.render_widget(
            Paragraph::new(footer.to_owned()).style(Style::default().fg(app.theme._muted)),
            footer_area,
        );
    }
}

/// Split newline-separated modal text into body lines.
pub fn text_lines(text: String) -> Vec<Line<'static>> {
    Text::from(text).lines
}

/// Append a form's inline validation message as the last body line, in red.
pub fn with_error_line(mut lines: Vec<Line<'static>>, error: Option<&str>) -> Vec<Line<'static>> {
    if let Some(error) = error {
        lines.push(Line::styled(
            error.to_owned(),
            Style::default().fg(Color::Red),
        ));
    }
    lines
}

pub fn render_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    let (title, body, footer) = match modal {
        ModalState::Info { message } => ("Result", message.clone(), "Enter/Esc close"),
        ModalState::Help { .. } => (
            "Help",
            "q quit; Tab switch tabs; Shift+Tab switch pane; / search; n create; Enter action; Delete delete; f filters".to_owned(),
            "↑↓ PgUp PgDn scroll  Esc close",
        ),
        ModalState::SudoPrompt { password, error, .. } => (
            "Authentication required",
            format!(
                "Enter sudo password:\n{}{}",
                "*".repeat(password.len()),
                error.as_ref().map_or(String::new(), |error| format!("\n{error}"))
            ),
            "Enter submit  Esc cancel operation",
        ),
        ModalState::OperationConfirm { selected, action, preview } => (
            "Confirm prepared operation",
            format!(
                "{action}\n\n{}{}\n\n{} Apply   {} Cancel",
                preview.iter().take(12).cloned().collect::<Vec<_>>().join("\n"),
                if preview.len() > 12 { "\n… additional steps omitted from this modal" } else { "" },
                marker(*selected, 0),
                marker(*selected, 1)
            ),
            YES_NO_FOOTER,
        ),
        ModalState::FilterMenu { selected } => {
            let options = match app.active_tab { crate::app::ActiveTab::Users => vec!["Show all", "Human users", "System users", "Inactive shell", "No home", "Locked (shadow)", "No password (shadow)", "Expired (shadow)"], crate::app::ActiveTab::Groups => vec!["Show all", "User GIDs", "System GIDs"] };
            (
                "Filters",
                options.iter().enumerate().map(|(index, option)| format!("{} {option}", marker(*selected, index))).collect::<Vec<_>>().join("\n"),
                "↑↓ move  Enter/Space toggle  Backspace back  Esc close",
            )
        }
        _ => return,
    };
    render_modal_frame(frame, area, app, title, text_lines(body), footer);
}

fn marker(selected: usize, index: usize) -> &'static str {
    if selected == index { "▶" } else { " " }
}
