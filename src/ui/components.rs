//! Shared immutable UI components.

use std::collections::{BTreeMap, BTreeSet};

use ratatui::{
    Frame,
    layout::{Alignment, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};

use crossterm::event::{KeyCode, KeyModifiers};

use crate::app::keymap::KeyAction;
use crate::app::{
    ActiveTab, AppState, CandidateList, GroupsFocus, InputMode, MAX_MEMBERSHIP_CANDIDATES,
    ModalState, UsersFocus,
};

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

/// Separator between status bar segments.
const STATUS_SEPARATOR: &str = " · ";

/// Smallest form of a stale label the status bar keeps: `STALE: …`.
const STALE_MARKER: &str = "STALE: …";

/// Status bar content before it is fitted to a width.
pub struct StatusSegments {
    /// Segments, most important first, shown left to right.
    pub segments: Vec<String>,
    /// How many leading segments always stay (search mode and the refresh
    /// or stale label).
    pub essential: usize,
    /// Segment that may be shortened down to `STALE_MARKER` (the stale
    /// label). It gets the width the other kept segments leave over.
    pub shrinkable: Option<usize>,
}

/// Status bar segments for the current state.
pub fn status_segments(app: &AppState) -> StatusSegments {
    let mut segments = Vec::new();
    match app.input_mode {
        InputMode::SearchUsers => segments.push("SEARCH(users)".to_owned()),
        InputMode::SearchGroups => segments.push("SEARCH(groups)".to_owned()),
        InputMode::Normal | InputMode::Modal => {}
    }
    let shrinkable = app
        .diagnostics
        .stale_reason
        .is_some()
        .then_some(segments.len());
    segments.push(refresh_label(app));
    let essential = segments.len();
    let users = app.users.len();
    let groups = app.groups.len();
    match app.active_tab {
        ActiveTab::Users => match app.users_focus {
            UsersFocus::UsersList => {
                segments.push(position("users", app.selected_user_index, users));
                segments.push(format!("groups {groups}"));
            }
            UsersFocus::MemberOf => {
                let member_of = app.selected_user_groups().len();
                segments.push(position(
                    "member of",
                    app.selected_user_group_index,
                    member_of,
                ));
                segments.push(format!("users {users} · groups {groups}"));
            }
        },
        ActiveTab::Groups => match app.groups_focus {
            GroupsFocus::GroupsList => {
                segments.push(position("groups", app.selected_group_index, groups));
                segments.push(format!("users {users}"));
            }
            GroupsFocus::Members => {
                let members = app
                    .groups
                    .get(app.selected_group_index)
                    .map_or(0, |group| group.members.len());
                segments.push(position(
                    "members",
                    app.selected_group_member_index,
                    members,
                ));
                segments.push(format!("users {users} · groups {groups}"));
            }
        },
    }
    match app.diagnostics.config_messages.len() {
        0 => {}
        1 => segments.push("config: 1 error (see ? help)".to_owned()),
        count => segments.push(format!("config: {count} errors (see ? help)")),
    }
    segments.push(format!(
        "shadow {}",
        app.diagnostics.shadow.availability_label()
    ));
    StatusSegments {
        segments,
        essential,
        shrinkable,
    }
}

/// `users 12/45`: one-based selected row and row count, `users 0/0` when empty.
fn position(name: &str, selected: usize, len: usize) -> String {
    let row = if len == 0 {
        0
    } else {
        selected.min(len - 1) + 1
    };
    format!("{name} {row}/{len}")
}

/// Join the segments with separators in at most `width` columns.
///
/// The shrinkable segment is counted at its `STALE: …` size while deciding
/// how many segments fit, so a long stale reason never pushes the others
/// out. Trailing segments beyond the essential ones are dropped only when
/// they do not fit next to that marker. The shrinkable segment then gets
/// the width left over and ends in `…` when cut.
pub fn fit_status_line(status: &StatusSegments, width: usize) -> String {
    let segments = &status.segments;
    let widths: Vec<usize> = segments
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            let full = text_width(segment);
            if status.shrinkable == Some(index) {
                full.min(text_width(STALE_MARKER))
            } else {
                full
            }
        })
        .collect();
    let line_width = |count: usize| {
        widths[..count].iter().sum::<usize>()
            + text_width(STATUS_SEPARATOR) * count.saturating_sub(1)
    };
    let essential = status.essential.min(segments.len());
    let mut kept = segments.len();
    while kept > essential && line_width(kept) > width {
        kept -= 1;
    }
    let mut leftover = width.saturating_sub(line_width(kept));
    let parts: Vec<String> = segments[..kept]
        .iter()
        .enumerate()
        .map(|(index, segment)| {
            if status.shrinkable == Some(index) {
                let room = widths[index] + leftover;
                leftover = 0;
                truncate_to_width(segment, room)
            } else {
                segment.clone()
            }
        })
        .collect();
    truncate_to_width(&parts.join(STATUS_SEPARATOR), width)
}

/// Display width of `text` in terminal columns.
fn text_width(text: &str) -> usize {
    Span::raw(text).width()
}

/// Cut `text` to at most `width` columns, ending in `…` when shortened.
fn truncate_to_width(text: &str, width: usize) -> String {
    if text_width(text) <= width {
        return text.to_owned();
    }
    let mut out = String::new();
    let mut used = 0;
    for character in text.chars() {
        let mut buffer = [0; 4];
        let columns = text_width(character.encode_utf8(&mut buffer));
        if used + columns + 1 > width {
            break;
        }
        out.push(character);
        used += columns;
    }
    if width > 0 {
        out.push('…');
    }
    out
}

pub fn render_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let text = fit_status_line(&status_segments(app), usize::from(area.width));
    frame.render_widget(
        Paragraph::new(text).style(
            Style::default()
                .fg(app.theme.status_fg)
                .bg(app.theme.status_bg),
        ),
        area,
    );
}

/// The key bound to `action` as the keybindings panel names it. Shift with
/// an upper-case letter (`Shift+K`) is preferred, then the fewest modifiers.
fn key_label(app: &AppState, action: KeyAction) -> Option<String> {
    app.keymap
        .all_bindings()
        .into_iter()
        .filter(|(_, bound)| *bound == action)
        .map(|((modifiers, code), _)| {
            let shifted_upper = modifiers == KeyModifiers::SHIFT
                && matches!(code, KeyCode::Char(character) if character.is_uppercase());
            let rank = if shifted_upper {
                0
            } else {
                1 + modifiers.bits().count_ones()
            };
            let label = crate::app::keymap::Keymap::format_key(modifiers, code);
            (rank, label.len(), label)
        })
        .min()
        .map(|(_, _, label)| label)
}

/// Key hints for the header, most important first.
pub fn header_hints(app: &AppState) -> Vec<String> {
    [
        (KeyAction::OpenHelp, "help"),
        (KeyAction::StartSearch, "search"),
        (KeyAction::OpenFilterMenu, "filter"),
        (KeyAction::ToggleKeybindsPane, "keys"),
    ]
    .into_iter()
    .filter_map(|(action, name)| key_label(app, action).map(|key| format!("{key} {name}")))
    .collect()
}

/// One header line: app, user, and tabs on the left, key hints on the right.
/// Hints that do not fit are dropped from the end.
pub fn header_line(app: &AppState, width: usize) -> String {
    let tabs = match app.active_tab {
        ActiveTab::Users => "[Users]  Groups",
        ActiveTab::Groups => "Users  [Groups]",
    };
    let left = format!("usrgrp-manager ({})   {tabs}", app.current_username);
    let mut hints = header_hints(app);
    while !hints.is_empty() {
        let right = hints.join("  ");
        let used = text_width(&left) + text_width(&right);
        if used + 2 <= width {
            return format!("{left}{}{right}", " ".repeat(width - used));
        }
        hints.pop();
    }
    left
}

pub fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let width = usize::from(area.width.saturating_sub(2));
    frame.render_widget(
        Paragraph::new(header_line(app, width))
            .style(Style::default().fg(app.theme.header_fg))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
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

/// Footer for an operation confirmation whose preview scrolls.
pub const PREVIEW_FOOTER: &str = "↑↓ PgUp PgDn scroll  ←→ choose  y/n  Enter confirm  Esc cancel";

/// Tallest dialog: 18 body rows, the footer row, and two border rows.
const MAX_MODAL_HEIGHT: u16 = 21;
/// Rows a dialog spends outside its body: two borders and the footer.
const MODAL_CHROME_ROWS: u16 = 3;

/// Previews with at most this many steps count as reviewed as soon as the
/// confirmation opens.
pub const PREVIEW_SEEN_IMMEDIATELY: usize = 12;
/// Shown under a longer preview until its last step has been on screen.
pub const PREVIEW_REFUSAL: &str = "Scroll to the end to review every step before applying.";

/// Dialog width on a terminal `area_width` columns wide, borders included.
fn modal_width(area_width: u16) -> u16 {
    area_width.saturating_sub(8).min(76)
}

/// Body rows of the tallest dialog that fits a terminal `area_height` rows
/// high.
fn modal_body_rows(area_height: u16) -> usize {
    usize::from(
        MAX_MODAL_HEIGHT
            .min(area_height)
            .saturating_sub(MODAL_CHROME_ROWS),
    )
}

/// Preview lines an operation confirmation shows on a terminal of
/// `terminal` (width, height), starting at `scroll`.
///
/// This is the one place that decides how many steps fit: the renderer draws
/// exactly this range and the key handler scrolls by it and marks the
/// preview seen when the range reaches the last step. The start is clamped
/// so the last page is full, and at least one line is always shown so every
/// step can be reached on any terminal. Wrapped lines count by their rows.
pub fn operation_preview_window(
    terminal: (u16, u16),
    action: &str,
    preview: &[String],
    scroll: usize,
) -> std::ops::Range<usize> {
    let (width, height) = terminal;
    let inner = usize::from(modal_width(width).saturating_sub(2));
    let rows = |text: &str| wrap_line(&Line::raw(text), inner).len();
    // Action, blank, blank before the choices, and the choices; long
    // previews also keep a row for the review line.
    let mut fixed = rows(action) + 3;
    if preview.len() > PREVIEW_SEEN_IMMEDIATELY {
        fixed += rows(PREVIEW_REFUSAL);
    }
    let available = modal_body_rows(height).saturating_sub(fixed);
    let fits = |used: usize, next: usize, any: bool| !any || used + next <= available;
    let mut last_start = preview.len();
    let mut used = 0;
    for (index, line) in preview.iter().enumerate().rev() {
        let next = rows(line);
        if !fits(used, next, used > 0) {
            break;
        }
        used += next;
        last_start = index;
    }
    let start = scroll.min(last_start);
    let mut end = start;
    used = 0;
    for line in &preview[start..] {
        let next = rows(line);
        if !fits(used, next, end > start) {
            break;
        }
        used += next;
        end += 1;
    }
    start..end
}
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
    let width = modal_width(area.width);
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
        .saturating_add(MODAL_CHROME_ROWS)
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
            scroll,
            seen_all,
        } => {
            render_operation_confirm(
                frame,
                area,
                app,
                (*selected, *scroll, *seen_all),
                action,
                preview,
            );
            return;
        }
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

/// Operation confirmation: the action, the window of preview steps that
/// fits, the review line for long previews, and Apply/Cancel. `state` is
/// the modal's (selected, scroll, seen_all).
fn render_operation_confirm(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    state: (usize, usize, bool),
    action: &str,
    preview: &[String],
) {
    let (selected, scroll, seen_all) = state;
    let shown = operation_preview_window((area.width, area.height), action, preview, scroll);
    let title = if preview.is_empty() {
        "Confirm".to_owned()
    } else {
        format!(
            "Confirm (steps {}-{} of {})",
            shown.start + 1,
            shown.end,
            preview.len()
        )
    };
    let reached_end = shown.end >= preview.len();
    let mut body = vec![Line::raw(action.to_owned()), Line::default()];
    body.extend(preview[shown.clone()].iter().cloned().map(Line::raw));
    body.push(Line::default());
    if preview.len() > PREVIEW_SEEN_IMMEDIATELY {
        body.push(if seen_all || reached_end {
            Line::styled(
                format!("All {} steps shown.", preview.len()),
                Style::default().fg(app.theme._muted),
            )
        } else {
            Line::styled(PREVIEW_REFUSAL, Style::default().fg(Color::Yellow))
        });
    }
    body.push(Line::raw(format!(
        "{} Apply   {} Cancel",
        marker(selected, 0),
        marker(selected, 1)
    )));
    let footer = if shown.len() < preview.len() {
        PREVIEW_FOOTER
    } else {
        YES_NO_FOOTER
    };
    render_modal_frame(frame, area, app, &title, body, footer);
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

/// Empty-state text for a table whose loaded rows were all filtered out:
/// `No users match /alice, filters: locked`. The query and filter parts
/// are left out when they do not apply.
pub fn no_match_text(noun: &str, query: &str, filters: &[&str]) -> String {
    let mut parts = Vec::new();
    if !query.is_empty() {
        parts.push(format!("/{query}"));
    }
    if !filters.is_empty() {
        parts.push(format!("filters: {}", filters.join(", ")));
    }
    let mut text = format!("No {noun} match");
    if !parts.is_empty() {
        text.push(' ');
        text.push_str(&parts.join(", "));
    }
    text
}

/// Empty-state lines for a table with nothing loaded: `No users loaded`,
/// then the stale label with its reason when the last refresh failed.
pub fn nothing_loaded_lines(app: &AppState, noun: &str) -> Vec<String> {
    let mut lines = vec![format!("No {noun} loaded")];
    if app.diagnostics.stale_reason.is_some() {
        lines.push(refresh_label(app));
    }
    lines
}

/// Draw `block` with `lines` centered inside it in the muted color, in
/// place of a table that has no rows.
pub fn render_empty_state(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    block: Block<'_>,
    lines: Vec<String>,
) {
    let inner = block.inner(area);
    frame.render_widget(block, area);
    // Wrap by character up front, the way dialog bodies are, so the row
    // count is exact and the text is centered from its real height.
    let rows: Vec<Line<'static>> = lines
        .into_iter()
        .flat_map(|line| wrap_line(&Line::from(line), usize::from(inner.width)))
        .collect();
    let height = u16::try_from(rows.len())
        .unwrap_or(u16::MAX)
        .min(inner.height);
    let text_area = Rect {
        y: inner.y + (inner.height - height) / 2,
        height,
        ..inner
    };
    frame.render_widget(
        Paragraph::new(rows)
            .alignment(Alignment::Center)
            .style(Style::default().fg(app.theme._muted)),
        text_area,
    );
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
