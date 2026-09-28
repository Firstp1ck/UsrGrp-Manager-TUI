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

/// Tallest dialog: 18 body rows, the footer row, and two border rows.
const MAX_MODAL_HEIGHT: u16 = 21;
/// Marks the highlighted row in every dialog body.
pub const CURSOR_MARKER: &str = "▶";

/// Draw a centered dialog: title, body, and a key hint footer on the last
/// interior row. Every modal renderer goes through this frame.
///
/// The dialog is as tall as its body plus the footer, capped by the
/// terminal. When the body does not fit, it scrolls so the row carrying
/// [`CURSOR_MARKER`] stays visible. The footer is drawn whenever the dialog
/// has an interior row.
pub fn render_modal_frame(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    title: &str,
    body: Vec<Line<'_>>,
    footer: &str,
) {
    let width = area.width.saturating_sub(8).min(76);
    let wrapped: Vec<Vec<Line<'static>>> = body
        .iter()
        .map(|line| wrap_line(line, usize::from(width.saturating_sub(2))))
        .collect();
    let row_counts = wrapped.iter().map(Vec::len).collect::<Vec<_>>();
    let cursor = body.iter().position(|line| {
        line.spans
            .iter()
            .any(|span| span.content.contains(CURSOR_MARKER))
    });
    let body: Vec<Line<'static>> = wrapped.into_iter().flatten().collect();
    let body_rows = u16::try_from(body.len()).unwrap_or(u16::MAX);
    let height = body_rows
        .saturating_add(3)
        .min(MAX_MODAL_HEIGHT)
        .min(area.height);
    let rect = centered_rect(width, height, area);
    let block = Block::default()
        .title(title.to_owned())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(rect);
    frame.render_widget(Clear, rect);
    frame.render_widget(block, rect);
    if inner.height == 0 {
        return;
    }
    let body_area = Rect {
        height: inner.height - 1,
        ..inner
    };
    let footer_area = Rect {
        y: inner.y + inner.height - 1,
        height: 1,
        ..inner
    };
    let scroll = cursor.map_or(0, |cursor| {
        let end: usize = row_counts[..=cursor].iter().sum();
        end.saturating_sub(usize::from(body_area.height))
    });
    // The body is already wrapped, so each Line is exactly one screen row.
    frame.render_widget(
        Paragraph::new(body).scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
        body_area,
    );
    frame.render_widget(
        Paragraph::new(footer.to_owned()).style(Style::default().fg(app.theme._muted)),
        footer_area,
    );
}

/// Break one body line into rows of at most `width` columns, splitting at
/// any character. Each piece keeps its span's style, and every row keeps the
/// line's style and alignment. A line that fits comes back unchanged as a
/// single row, and an empty line stays one empty row.
fn wrap_line(line: &Line<'_>, width: usize) -> Vec<Line<'static>> {
    let width = width.max(1);
    let row = |spans: Vec<Span<'static>>| Line {
        spans,
        style: line.style,
        alignment: line.alignment,
    };
    let mut rows = Vec::new();
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut used = 0;
    for span in &line.spans {
        let mut piece = String::new();
        for grapheme in span.styled_graphemes(Style::default()) {
            let columns = Span::raw(grapheme.symbol).width();
            if used > 0 && used + columns > width {
                if !piece.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut piece), span.style));
                }
                rows.push(row(std::mem::take(&mut spans)));
                used = 0;
            }
            piece.push_str(grapheme.symbol);
            used += columns;
        }
        if !piece.is_empty() {
            spans.push(Span::styled(piece, span.style));
        }
    }
    rows.push(row(spans));
    rows
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
        ModalState::SudoPrompt { password, error, .. } => {
            let operation = app.last_preview.first().map_or("", String::as_str);
            let body = text_lines(format!(
                "Authenticating to run: {operation}\n\nEnter sudo password:\n{}",
                "*".repeat(password.len())
            ));
            let body = with_error_line(body, error.as_deref());
            render_modal_frame(frame, area, app, "Authentication required", body, "Enter submit  Esc cancel operation");
            return;
        }
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

/// Cursor prefix for row `index` of a dialog body.
pub fn marker(selected: usize, index: usize) -> &'static str {
    if selected == index {
        CURSOR_MARKER
    } else {
        " "
    }
}
