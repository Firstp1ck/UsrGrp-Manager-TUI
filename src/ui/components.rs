//! Shared immutable UI components.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crate::app::{AppState, CandidateList, InputMode, MAX_MEMBERSHIP_CANDIDATES, ModalState};

/// Build a table title from its name, its search query, and the row count.
/// The `/query` part is left out when the query is empty and not being
/// edited; while editing, a `_` marks the cursor after the query.
pub fn search_title(name: &str, query: &str, editing: bool, shown: usize, total: usize) -> String {
    let mut title = name.to_owned();
    if editing || !query.is_empty() {
        title.push_str("  /");
        title.push_str(query);
        if editing {
            title.push('_');
        }
    }
    title.push_str(&format!("  {shown} of {total}"));
    title
}

/// When the account data was last refreshed, and why it is stale if the
/// last refresh failed: `refreshed 12:03:15 UTC` or
/// `STALE since 12:03:15 UTC: <reason>`.
pub fn refresh_label(app: &AppState) -> String {
    let time = app.last_refresh_label.as_str();
    match (&app.diagnostics.stale_reason, time.is_empty()) {
        (Some(reason), true) => format!("STALE: {reason}"),
        (Some(reason), false) => format!("STALE since {time}: {reason}"),
        (None, true) => "not refreshed".to_owned(),
        (None, false) => format!("refreshed {time}"),
    }
}

pub fn render_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let config = match app.diagnostics.config_messages.len() {
        0 => String::new(),
        1 => " config: 1 error (see ? help)".to_owned(),
        count => format!(" config: {count} errors (see ? help)"),
    };
    let mode = match app.input_mode {
        InputMode::SearchUsers => "SEARCH(users) ",
        InputMode::SearchGroups => "SEARCH(groups) ",
        InputMode::Normal | InputMode::Modal => "",
    };
    let text = format!(
        "{mode}users:{} groups:{} shadow:{}{} {}",
        app.users.len(),
        app.groups.len(),
        app.diagnostics.shadow.availability_label(),
        config,
        refresh_label(app)
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
/// Footer for multi-select candidate lists with at least one tick.
pub const MULTI_FOOTER: &str = "↑↓ move  Space tick  * all  / filter  Enter apply  Esc close";
/// Footer for multi-select candidate lists with nothing ticked: Enter
/// applies to the highlighted row.
pub const MULTI_HIGHLIGHTED_FOOTER: &str =
    "↑↓ move  Space tick  * all  / filter  Enter apply row  Esc close";
/// Footer for multi-select candidate lists while the filter is typed.
pub const FILTER_FOOTER: &str = "type to filter  Backspace delete  Enter/Esc done";
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
        ModalState::Info { title, message } => (
            title.as_deref().unwrap_or("Result"),
            message.clone(),
            "Enter/Esc close",
        ),
        ModalState::Help { .. } => ("Help", help_text(app), "↑↓ PgUp PgDn scroll  Esc close"),
        ModalState::SudoPrompt {
            password, error, ..
        } => {
            let operation = app.last_preview.first().map_or("", String::as_str);
            let body = text_lines(format!(
                "Authenticating to run: {operation}\n\nEnter sudo password:\n{}",
                "*".repeat(password.len())
            ));
            let body = with_error_line(body, error.as_deref());
            render_modal_frame(
                frame,
                area,
                app,
                "Authentication required",
                body,
                "Enter submit  Esc cancel operation",
            );
            return;
        }
        ModalState::OperationConfirm {
            selected,
            action,
            preview,
        } => (
            "Confirm prepared operation",
            format!(
                "{action}\n\n{}{}\n\n{} Apply   {} Cancel",
                preview
                    .iter()
                    .take(12)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("\n"),
                if preview.len() > 12 {
                    "\n… additional steps omitted from this modal"
                } else {
                    ""
                },
                marker(*selected, 0),
                marker(*selected, 1)
            ),
            YES_NO_FOOTER,
        ),
        ModalState::FilterMenu { selected } => {
            let body = filter_menu_rows(app)
                .iter()
                .enumerate()
                .map(|(index, row)| format!("{} {row}", marker(*selected, index)))
                .collect::<Vec<_>>()
                .join("\n");
            let mut title = "Filters".to_owned();
            if app.active_tab == crate::app::ActiveTab::Users
                && let Some(note) = shadow_skipped_note(app)
            {
                title.push_str("  ");
                title.push_str(&note);
            }
            render_modal_frame(
                frame,
                area,
                app,
                &title,
                text_lines(body),
                "↑↓ move  Enter/Space toggle  Backspace back  Esc close",
            );
            return;
        }
        _ => return,
    };
    render_modal_frame(frame, area, app, title, text_lines(body), footer);
}

/// Help modal text. Recorded configuration problems follow the key summary
/// in their own section.
fn help_text(app: &AppState) -> String {
    let mut text = "q quit; Tab switch tabs; Shift+Tab switch pane; / search; n create; Enter action; Delete delete; f filters".to_owned();
    if !app.diagnostics.config_messages.is_empty() {
        text.push_str("\n\n");
        text.push_str(crate::app::CONFIG_PROBLEMS_TITLE);
        for message in &app.diagnostics.config_messages {
            text.push('\n');
            text.push_str(message);
        }
    }
    text
}

/// Filter menu rows for the active tab, without the cursor. User chips read
/// `[x] Human users`; shadow chips read `[-] Locked (shadow unavailable:
/// <reason>)` while shadow data is unavailable. The groups menu is a radio:
/// `(•) User GIDs`.
pub fn filter_menu_rows(app: &AppState) -> Vec<String> {
    use crate::app::{ActiveTab, GroupsFilter};
    use crate::search::ShadowState;
    match app.active_tab {
        ActiveTab::Users => {
            let chips = &app.users_filter_chips;
            let check = |on: bool, label: &str| format!("[{}] {label}", if on { "x" } else { " " });
            let shadow = |on: bool, label: &str| match &app.diagnostics.shadow {
                ShadowState::Known(_) => check(on, &format!("{label} (shadow)")),
                ShadowState::Unavailable { reason } => {
                    format!("[-] {label} (shadow unavailable: {reason})")
                }
            };
            vec![
                "Show all".to_owned(),
                check(app.human_filter_on(), "Human users"),
                check(app.system_filter_on(), "System users"),
                check(chips.inactive, "Inactive shell"),
                check(chips.no_home, "No home"),
                shadow(chips.locked, "Locked"),
                shadow(chips.no_password, "No password"),
                shadow(chips.expired, "Expired"),
            ]
        }
        ActiveTab::Groups => [
            (None, "Show all"),
            (Some(GroupsFilter::OnlyUserGids), "User GIDs"),
            (Some(GroupsFilter::OnlySystemGids), "System GIDs"),
        ]
        .into_iter()
        .map(|(filter, label)| {
            let dot = if app.groups_filter == filter {
                "•"
            } else {
                " "
            };
            format!("({dot}) {label}")
        })
        .collect(),
    }
}

/// `(shadow filter skipped: 3 users without record)` when the last filter
/// pass left the shadow chips unapplied.
pub fn shadow_skipped_note(app: &AppState) -> Option<String> {
    app.shadow_filter_skipped.map(|count| {
        let users = if count == 1 { "user" } else { "users" };
        format!("(shadow filter skipped: {count} {users} without record)")
    })
}

/// Append `  [human, locked]` to a table title when any filter is on.
pub fn push_filter_labels(title: &mut String, labels: &[&str]) {
    if !labels.is_empty() {
        title.push_str(&format!("  [{}]", labels.join(", ")));
    }
}

/// Cursor prefix for row `index` of a dialog body.
pub fn marker(selected: usize, index: usize) -> &'static str {
    if selected == index {
        CURSOR_MARKER
    } else {
        " "
    }
}

/// Candidate rows shown per page in a multi-select dialog.
const CANDIDATE_ROWS: usize = 12;

/// The page of a multi-select dialog that holds `selected`, one
/// `▶ [x] name` row per candidate. `candidates` must be the same list the
/// key handler indexes (see `AppState::group_member_candidate_list` and
/// `AppState::user_group_candidate_list`); `ticked` holds names.
pub fn candidate_page(candidates: &[String], selected: usize, ticked: &[String]) -> String {
    let start = selected / CANDIDATE_ROWS * CANDIDATE_ROWS;
    candidates
        .iter()
        .enumerate()
        .skip(start)
        .take(CANDIDATE_ROWS)
        .map(|(index, name)| {
            format!(
                "{} [{}] {name}",
                marker(selected, index),
                if ticked.contains(name) { "x" } else { " " }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// What a multi-select dialog shows, borrowed from its modal state.
pub struct CandidateView<'a> {
    /// Dialog name, such as "Add groups".
    pub name: &'a str,
    /// The filtered list the key handler indexes.
    pub list: &'a CandidateList,
    pub selected: usize,
    /// Ticked names, including ones the filter hides.
    pub ticked: &'a [String],
    pub filter: &'a str,
    pub filtering: bool,
}

/// Title, body, and footer of a multi-select dialog. The title reads
/// `Add groups  page 2/9  3 selected` and notes when the list was capped.
/// The body starts with the filter line when a filter is set or being
/// typed.
pub fn candidate_dialog(view: &CandidateView<'_>) -> (String, Vec<Line<'static>>, &'static str) {
    let len = view.list.names.len();
    let pages = len.div_ceil(CANDIDATE_ROWS).max(1);
    let page = (view.selected / CANDIDATE_ROWS + 1).min(pages);
    let mut title = format!(
        "{}  page {page}/{pages}  {} selected",
        view.name,
        view.ticked.len()
    );
    if view.list.is_capped() {
        title.push_str(&format!(
            "  showing first {MAX_MEMBERSHIP_CANDIDATES} of {}",
            view.list.total
        ));
    }
    let mut body = Vec::new();
    if view.filtering || !view.filter.is_empty() {
        body.push(Line::from(format!(
            "Filter: {}{}",
            view.filter,
            if view.filtering { "_" } else { "" }
        )));
    }
    if len == 0 && !view.filter.is_empty() {
        body.push(Line::from("No matches"));
    }
    body.extend(text_lines(candidate_page(
        &view.list.names,
        view.selected,
        view.ticked,
    )));
    let footer = if view.filtering {
        FILTER_FOOTER
    } else if view.ticked.is_empty() {
        MULTI_HIGHLIGHTED_FOOTER
    } else {
        MULTI_FOOTER
    };
    (title, body, footer)
}
