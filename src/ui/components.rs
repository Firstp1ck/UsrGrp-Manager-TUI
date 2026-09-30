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
    ActiveTab, AppState, CandidateList, ConfigFileState, GroupsFocus, InputMode,
    MAX_MEMBERSHIP_CANDIDATES, ModalState, SecretInput, UsersFocus,
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
    /// How many leading segments always stay (a notice such as
    /// `copied "alice"`, search mode, and the refresh or stale label).
    pub essential: usize,
    /// Segment that may be shortened down to `STALE_MARKER` (the stale
    /// label). It gets the width the other kept segments leave over.
    pub shrinkable: Option<usize>,
    /// Per segment, how many trailing bytes are secondary text drawn in the
    /// muted color (`(see ? help)`, the shadow `unavailable` value).
    pub muted_tails: Vec<usize>,
}

/// Status bar segments for the current state.
pub fn status_segments(app: &AppState) -> StatusSegments {
    let mut segments = Vec::new();
    let mut muted_tails = Vec::new();
    // A one-shot notice (`copied "alice"`) comes first until the next key.
    segments.extend(app.notice.clone());
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
    muted_tails.resize(segments.len(), 0);
    const HELP_HINT: &str = "(see ? help)";
    match app.diagnostics.config_messages.len() {
        0 => {}
        1 => segments.push(format!("config: 1 error {HELP_HINT}")),
        count => segments.push(format!("config: {count} errors {HELP_HINT}")),
    }
    muted_tails.resize(segments.len(), HELP_HINT.len());
    let shadow = &app.diagnostics.shadow;
    let label = shadow.availability_label();
    segments.push(format!("shadow {label}"));
    muted_tails.push(match shadow {
        crate::search::ShadowState::Known(_) => 0,
        crate::search::ShadowState::Unavailable { .. } => label.len(),
    });
    StatusSegments {
        segments,
        essential,
        shrinkable,
        muted_tails,
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
pub fn fit_status_line(status: &StatusSegments, width: usize) -> Vec<(String, bool)> {
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
    // (text, muted) pieces in display order, separators included.
    let mut pieces = Vec::new();
    for (index, segment) in segments[..kept].iter().enumerate() {
        if index > 0 {
            pieces.push((STATUS_SEPARATOR.to_owned(), false));
        }
        if status.shrinkable == Some(index) {
            let room = widths[index] + leftover;
            leftover = 0;
            pieces.push((truncate_to_width(segment, room), false));
            continue;
        }
        let tail = status.muted_tails.get(index).copied().unwrap_or(0);
        let split = segment.len().saturating_sub(tail);
        let split = if segment.is_char_boundary(split) {
            split
        } else {
            segment.len()
        };
        let (plain, muted) = segment.split_at(split);
        pieces.push((plain.to_owned(), false));
        pieces.push((muted.to_owned(), true));
    }
    pieces.retain(|(text, _)| !text.is_empty());
    truncate_pieces(pieces, width)
}

/// Cut the joined pieces to at most `width` columns like
/// [`truncate_to_width`], keeping each surviving character's piece and
/// putting the `…` in the last kept piece's style.
fn truncate_pieces(pieces: Vec<(String, bool)>, width: usize) -> Vec<(String, bool)> {
    let joined: String = pieces.iter().map(|(text, _)| text.as_str()).collect();
    let cut = truncate_to_width(&joined, width);
    if cut == joined {
        return pieces;
    }
    let mut keep = cut.strip_suffix('…').unwrap_or(&cut).len();
    let mut out = Vec::new();
    for (text, muted) in pieces {
        if keep == 0 {
            break;
        }
        let take = keep.min(text.len());
        out.push((text[..take].to_owned(), muted));
        keep -= take;
    }
    if cut.ends_with('…') {
        match out.last_mut() {
            Some((text, _)) => text.push('…'),
            None => out.push(("…".to_owned(), false)),
        }
    }
    out
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

/// Status bar in `status_fg` on `status_bg`, with the help hint and an
/// unavailable shadow value in the muted color.
pub fn render_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let muted = Style::default().fg(app.theme.muted);
    let spans: Vec<Span<'static>> = fit_status_line(&status_segments(app), usize::from(area.width))
        .into_iter()
        .map(|(text, is_muted)| {
            if is_muted {
                Span::styled(text, muted)
            } else {
                Span::raw(text)
            }
        })
        .collect();
    frame.render_widget(
        Paragraph::new(Line::from(spans)).style(
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

/// Added to the header when the keybindings panel is on but the terminal is
/// too narrow to show it, when it fits after the title and the help hint.
pub const KEYBINDS_HIDDEN_NOTICE_FULL: &str = " (keybinds hidden: narrow terminal)";

/// Shorter form of [`KEYBINDS_HIDDEN_NOTICE_FULL`] used when the full one does
/// not fit. The header always keeps room for it.
pub const KEYBINDS_HIDDEN_NOTICE: &str = " (keybinds hidden)";

/// Columns between the title and the first key hint, and between hints.
const HINT_GAP: &str = "  ";

/// One header line: app, user, and tabs on the left, key hints on the right.
/// `width` is the header's inner width, two columns less than the terminal.
/// See [`header_parts`] for what is kept when the line does not fit.
pub fn header_line(app: &AppState, width: usize) -> String {
    let terminal_width = u16::try_from(width.saturating_add(2)).unwrap_or(u16::MAX);
    let (left, right) = header_parts(app, width, terminal_width);
    left + &right
}

/// The header line split into the title part (app, user, tabs, and the
/// hidden-panel notice) and the right-aligned key hints part.
///
/// Segments are placed by priority: the title, the help hint, the notice
/// (only when the panel is on but `terminal_width` is too narrow for it),
/// then the other hints. Room for the help hint and the compact notice is
/// kept before the title is sized, so only the title is ever shortened:
/// the user name is cut with `…` first, then the whole title. The notice
/// uses the full wording when it fits after the title and the help hint,
/// and the compact one otherwise. Other hints that do not fit are left out
/// whole and nothing after them is added.
fn header_parts(app: &AppState, width: usize, terminal_width: u16) -> (String, String) {
    let notice_shown = app.show_keybinds && !super::layout::keybinds_fit(terminal_width);
    // `header_hints` lists the help hint first when help has a key.
    let help = key_label(app, KeyAction::OpenHelp).map(|key| format!("{key} help"));
    let hints = header_hints(app)
        .into_iter()
        .skip(usize::from(help.is_some()));
    let reserved = help
        .as_deref()
        .map_or(0, |help| HINT_GAP.len() + text_width(help))
        + if notice_shown {
            text_width(KEYBINDS_HIDDEN_NOTICE)
        } else {
            0
        };
    let title = header_title(app, width.saturating_sub(reserved));

    let mut used = text_width(&title);
    let mut shown: Vec<String> = Vec::new();
    if let Some(help) = help {
        if used + HINT_GAP.len() + text_width(&help) <= width {
            used += HINT_GAP.len() + text_width(&help);
            shown.push(help);
        } else {
            return (title, String::new());
        }
    }
    let mut left = title;
    if notice_shown {
        let notice = [KEYBINDS_HIDDEN_NOTICE_FULL, KEYBINDS_HIDDEN_NOTICE]
            .into_iter()
            .find(|notice| used + text_width(notice) <= width);
        if let Some(notice) = notice {
            used += text_width(notice);
            left.push_str(notice);
        }
    }
    for hint in hints {
        let extra = HINT_GAP.len() + text_width(&hint);
        if used + extra > width {
            break;
        }
        used += extra;
        shown.push(hint);
    }
    if shown.is_empty() {
        return (left, String::new());
    }
    let right = shown.join(HINT_GAP);
    let padding = width - text_width(&left) - text_width(&right);
    (left, format!("{}{right}", " ".repeat(padding)))
}

/// `usrgrp-manager (<user>)   [Users]  Groups` in at most `cap` columns.
/// The user name is cut with `…` first; if the rest still does not fit,
/// the whole title is cut.
fn header_title(app: &AppState, cap: usize) -> String {
    let tabs = match app.active_tab {
        ActiveTab::Users => "[Users]  Groups",
        ActiveTab::Groups => "Users  [Groups]",
    };
    let title = |name: &str| format!("usrgrp-manager ({name})   {tabs}");
    let full = title(&app.current_username);
    if text_width(&full) <= cap {
        return full;
    }
    let fixed = text_width(&full) - text_width(&app.current_username);
    let name = truncate_to_width(&app.current_username, cap.saturating_sub(fixed).max(1));
    truncate_to_width(&title(&name), cap)
}

/// Header block in the theme's header colors, with the key hints in the
/// muted color.
pub fn render_header(frame: &mut Frame, area: Rect, app: &AppState) {
    let width = usize::from(area.width.saturating_sub(2));
    let (left, hints) = header_parts(app, width, area.width);
    let line = Line::from(vec![
        Span::raw(left),
        Span::styled(hints, Style::default().fg(app.theme.muted)),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .style(
                Style::default()
                    .fg(app.theme.header_fg)
                    .bg(app.theme.header_bg),
            )
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            ),
        area,
    );
}

/// Section headings of the keybindings panel, in display order.
const PANEL_SECTIONS: [&str; 3] = ["General", "Navigation", "Actions"];

/// Where an action goes in the keybindings panel: section index, position
/// in the section, and label. Actions that share a position and label are
/// listed as one entry. `Ignore` is not listed.
fn panel_entry(action: KeyAction) -> Option<(usize, usize, &'static str)> {
    use KeyAction::*;
    Some(match action {
        Quit => (0, 0, "Quit"),
        OpenHelp => (0, 1, "Help"),
        ToggleKeybindsPane => (0, 2, "Toggle keybindings"),
        Refresh => (0, 3, "Refresh"),
        StartSearch => (0, 4, "Search"),
        ClearSearch => (0, 5, "Clear search"),
        OpenFilterMenu => (0, 6, "Filters"),
        CycleSort => (0, 7, "Sort"),
        MoveUp => (1, 0, "Move up"),
        MoveDown => (1, 1, "Move down"),
        PageUp | MoveLeftPage => (1, 2, "Page up"),
        PageDown | MoveRightPage => (1, 3, "Page down"),
        SwitchTab => (1, 4, "Switch tab"),
        ToggleUsersFocus | ToggleGroupsFocus => (1, 5, "Switch pane"),
        EnterAction => (2, 0, "Open actions"),
        NewUser => (2, 1, "New user/group"),
        DeleteSelection => (2, 2, "Delete"),
        ToggleMark => (2, 3, "Mark user"),
        CopySelection => (2, 4, "Copy name"),
        Ignore => return None,
    })
}

/// Has modifiers, is a character, and label: sorts and names a panel key.
type PanelKey = (bool, bool, String);

/// A key as the keybindings panel shows it, with a sort key that puts plain
/// named keys first, then plain characters, then keys with modifiers. The
/// spellings of one key collapse to one label: `Shift+k`, `Shift+K`, and `K`
/// read `Shift+K`; `BackTab` reads `Shift+Tab`; `Ctrl+c` reads `Ctrl+C`.
fn panel_key(modifiers: KeyModifiers, code: KeyCode) -> PanelKey {
    let (modifiers, code) = match code {
        KeyCode::BackTab => (modifiers | KeyModifiers::SHIFT, KeyCode::Tab),
        KeyCode::Char(character) if character.is_alphabetic() => {
            let shifted = modifiers.contains(KeyModifiers::SHIFT) || character.is_uppercase();
            let modifiers = if shifted {
                modifiers | KeyModifiers::SHIFT
            } else {
                modifiers
            };
            let character = if modifiers.is_empty() {
                character
            } else {
                character.to_uppercase().next().unwrap_or(character)
            };
            (modifiers, KeyCode::Char(character))
        }
        other => (modifiers, other),
    };
    (
        !modifiers.is_empty(),
        matches!(code, KeyCode::Char(_)),
        crate::app::keymap::Keymap::format_key(modifiers, code),
    )
}

/// Keybindings panel lines: a heading per section, then `Label: keys` for
/// every bound action in the section. Sections without a bound action are
/// left out.
pub fn keybinds_panel_lines(app: &AppState) -> Vec<Line<'static>> {
    let mut entries: BTreeMap<(usize, usize), (&str, BTreeSet<PanelKey>)> = BTreeMap::new();
    for ((modifiers, code), action) in app.keymap.all_bindings() {
        if let Some((section, position, label)) = panel_entry(action) {
            entries
                .entry((section, position))
                .or_insert_with(|| (label, BTreeSet::new()))
                .1
                .insert(panel_key(modifiers, code));
        }
    }
    let heading = Style::default()
        .fg(app.theme.title)
        .add_modifier(Modifier::BOLD);
    let text = Style::default().fg(app.theme.text);
    let mut lines = Vec::new();
    let mut current = None;
    for ((section, _), (label, keys)) in entries {
        if current != Some(section) {
            if current.is_some() {
                lines.push(Line::default());
            }
            lines.push(Line::styled(PANEL_SECTIONS[section], heading));
            current = Some(section);
        }
        let keys = keys
            .into_iter()
            .map(|(_, _, key)| key)
            .collect::<Vec<_>>()
            .join(", ");
        lines.push(Line::styled(format!("{label}: {keys}"), text));
    }
    lines
}

pub fn render_keybinds_panel(frame: &mut Frame, area: Rect, app: &AppState) {
    frame.render_widget(
        Paragraph::new(keybinds_panel_lines(app))
            .wrap(Wrap { trim: true })
            .block(
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
/// Footer for forms with several fields, all of which have password
/// fields that Ctrl+R reveals.
pub const PASSWORD_FORM_FOOTER: &str =
    "↑↓ field  Space tick  Ctrl+R reveal  Enter on Submit  Esc back";

/// Footer for forms with a text field and checkboxes but no password.
pub const FORM_FOOTER: &str = "↑↓ field  Space tick  Enter submit  Esc back";

/// Drawn after the text of the focused input field.
pub const INPUT_CURSOR: &str = "▏";

/// [`INPUT_CURSOR`] when field `index` has the focus, else nothing.
pub fn input_cursor(selected: usize, index: usize) -> &'static str {
    if selected == index { INPUT_CURSOR } else { "" }
}

/// Bytes used out of a field's limit, as `12/32`. Limits count bytes like
/// the bridge, so the counter does too.
pub fn byte_budget(value: &str, maximum: usize) -> String {
    format!("{}/{maximum}", value.len())
}

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

/// First body line of an operation confirmation, above the preview.
pub const CONFIRM_INTRO: &str = "The following commands will run:";
/// First body line of an operation confirmation under `--dry-run`.
pub const DRY_RUN_INTRO: &str = "Dry run: nothing will be executed. These commands would run:";

/// The line an operation confirmation opens with.
pub fn confirm_intro(dry_run: bool) -> &'static str {
    if dry_run {
        DRY_RUN_INTRO
    } else {
        CONFIRM_INTRO
    }
}

/// `Confirm: <description><suffix>` cut to a dialog on a terminal
/// `area_width` columns wide. Only the description is shortened, so the
/// step range in `suffix` stays readable.
pub fn confirm_title(description: &str, suffix: &str, area_width: u16) -> String {
    const PREFIX: &str = "Confirm: ";
    let room = usize::from(modal_width(area_width).saturating_sub(2))
        .saturating_sub(text_width(PREFIX) + text_width(suffix));
    format!("{PREFIX}{}{suffix}", truncate_to_width(description, room))
}

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

/// What an operation confirmation shows of its preview.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PreviewWindow {
    /// The terminal is below the minimum size, so no dialog is drawn.
    Hidden,
    /// Not even the tallest step fits beside the intro and the choices.
    /// No step is drawn; the dialog asks for `need_rows` terminal rows.
    TooSmall { need_rows: usize },
    /// These preview lines are drawn in full.
    Steps(std::ops::Range<usize>),
}

impl PreviewWindow {
    /// The last of `len` steps is on screen.
    pub fn shows_last(&self, len: usize) -> bool {
        matches!(self, Self::Steps(shown) if shown.end >= len)
    }
}

/// Apply/Cancel row of an operation confirmation.
fn confirm_choices(selected: usize) -> String {
    format!(
        "{} Apply   {} Cancel",
        marker(selected, 0),
        marker(selected, 1)
    )
}

/// `Terminal too small to review the preview (need 13 rows)`; when no
/// height would do at this width, it asks for a wider terminal instead.
pub fn preview_too_small_message(need_rows: usize) -> String {
    if need_rows <= usize::from(MAX_MODAL_HEIGHT) {
        format!("Terminal too small to review the preview (need {need_rows} rows)")
    } else {
        "Terminal too small to review the preview (need a wider terminal)".to_owned()
    }
}

/// Preview lines an operation confirmation shows on a terminal of
/// `terminal` (width, height), starting at `scroll`.
///
/// This is the one place that decides how many steps fit: the renderer draws
/// exactly this range and the key handler scrolls by it and marks the
/// preview seen when the range reaches the last step. Rows are budgeted for
/// the intro line, the two blank lines, the review line of long previews, and
/// the choices (the frame's borders and footer are outside the body), and
/// only steps whose every wrapped row fits are counted, so the dialog frame
/// never has to scroll the body. The start is clamped so the last page is
/// full. When the tallest step cannot fit, no step is shown at all.
pub fn operation_preview_window(
    terminal: (u16, u16),
    intro: &str,
    preview: &[String],
    scroll: usize,
) -> PreviewWindow {
    let (width, height) = terminal;
    if crate::ui::layout::below_minimum(Rect::new(0, 0, width, height)) {
        return PreviewWindow::Hidden;
    }
    let inner = usize::from(modal_width(width).saturating_sub(2));
    let rows = |text: &str| wrap_line(&Line::raw(text), inner).len();
    // Intro, blank, blank before the choices, and the choices; long
    // previews also keep rows for the review line.
    let mut fixed = rows(intro) + 2 + rows(&confirm_choices(0));
    if preview.len() > PREVIEW_SEEN_IMMEDIATELY {
        fixed += rows(PREVIEW_REFUSAL);
    }
    let tallest = preview.iter().map(|line| rows(line)).max().unwrap_or(0);
    let body = modal_body_rows(height);
    if fixed + tallest > body {
        return PreviewWindow::TooSmall {
            need_rows: fixed + tallest + usize::from(MODAL_CHROME_ROWS),
        };
    }
    // Every step fits on its own, so each page holds at least one.
    let available = body - fixed;
    let mut last_start = preview.len();
    let mut used = 0;
    for (index, line) in preview.iter().enumerate().rev() {
        used += rows(line);
        if used > available {
            break;
        }
        last_start = index;
    }
    let start = scroll.min(last_start);
    let mut end = start;
    used = 0;
    for line in &preview[start..] {
        used += rows(line);
        if used > available {
            break;
        }
        end += 1;
    }
    PreviewWindow::Steps(start..end)
}

/// Marks the highlighted row in every dialog body.
pub const CURSOR_MARKER: &str = "▶";

/// Draw a centered dialog: title, body, and a key hint footer on the last
/// interior row. Every modal renderer goes through this frame.
///
/// The dialog is as tall as its body plus the footer, capped by the
/// terminal. `focus_row` is the index into `lines` of the caller's focused
/// row (the highlighted choice or the field being edited). When the body
/// does not fit, it scrolls so the last screen row of that line stays
/// visible, which keeps the cursor after a long wrapped value on screen.
/// Without a focus row the body starts at its top. The footer is drawn
/// whenever the dialog has an interior row.
pub fn render_modal_frame(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    title: &str,
    lines: Vec<Line<'_>>,
    focus_row: Option<usize>,
    footer: &str,
) {
    let width = modal_width(area.width);
    let wrapped: Vec<Vec<Line<'_>>> = lines
        .iter()
        .map(|line| wrap_line(line, usize::from(width.saturating_sub(2))))
        .collect();
    // Screen rows up to and including the focused line's last row.
    let focus_end = focus_row
        .filter(|row| *row < wrapped.len())
        .map(|row| wrapped[..=row].iter().map(Vec::len).sum::<usize>());
    let body: Vec<Line<'_>> = wrapped.into_iter().flatten().collect();
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
    let scroll = focus_end.map_or(0, |end| end.saturating_sub(usize::from(body_area.height)));
    // The body is already wrapped, so each Line is exactly one screen row.
    frame.render_widget(
        Paragraph::new(body).scroll((u16::try_from(scroll).unwrap_or(u16::MAX), 0)),
        body_area,
    );
    frame.render_widget(
        Paragraph::new(footer.to_owned()).style(Style::default().fg(app.theme.muted)),
        footer_area,
    );
}

/// Break one body line into rows of at most `width` columns, splitting at
/// any character. Each piece keeps its span's style, and every row keeps the
/// line's style and alignment. A line that fits comes back unchanged as a
/// single row, and an empty line stays one empty row.
///
/// Pieces borrow from `line` instead of copying, so a revealed password
/// held in a zeroizing buffer is never duplicated into a plain `String`.
fn wrap_line<'a>(line: &'a Line<'_>, width: usize) -> Vec<Line<'a>> {
    let width = width.max(1);
    let row = |spans: Vec<Span<'a>>| Line {
        spans,
        style: line.style,
        alignment: line.alignment,
    };
    let mut rows = Vec::new();
    let mut spans: Vec<Span<'a>> = Vec::new();
    let mut used = 0;
    for span in &line.spans {
        let content: &'a str = span.content.as_ref();
        let (mut start, mut end) = (0, 0);
        for grapheme in span.styled_graphemes(Style::default()) {
            // Graphemes are subslices of `content`; control characters
            // skipped by the iterator stay in the slice and render as nothing.
            let offset = grapheme.symbol.as_ptr().addr() - content.as_ptr().addr();
            let columns = Span::raw(grapheme.symbol).width();
            if used > 0 && used + columns > width {
                if start < end {
                    spans.push(Span::styled(&content[start..end], span.style));
                }
                rows.push(row(std::mem::take(&mut spans)));
                start = offset;
                used = 0;
            }
            end = offset + grapheme.symbol.len();
            used += columns;
        }
        if start < end {
            spans.push(Span::styled(&content[start..end], span.style));
        }
    }
    rows.push(row(spans));
    rows
}

/// Split newline-separated modal text into body lines.
pub fn text_lines(text: String) -> Vec<Line<'static>> {
    Text::from(text).lines
}

/// Table row style: the theme's highlight colors in bold for the selected
/// row, the text color for the rest.
pub fn row_style(app: &AppState, selected: bool) -> Style {
    if selected {
        Style::default()
            .fg(app.theme.highlight_fg)
            .bg(app.theme.highlight_bg)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(app.theme.text)
    }
}

/// A piece of a details pane line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DetailPart {
    /// Labels and account data, in the theme's text color.
    Text(String),
    /// Placeholder for a diagnostic the last refresh could not read
    /// (`unavailable`, `not set`), in the muted color.
    Missing(String),
}

/// One details pane line.
pub type DetailLine = Vec<DetailPart>;

/// Details pane lines styled per part: account data in the text color and
/// only [`DetailPart::Missing`] placeholders in the muted color.
pub fn details_lines(app: &AppState, lines: &[DetailLine]) -> Vec<Line<'static>> {
    let text = Style::default().fg(app.theme.text);
    let muted = Style::default().fg(app.theme.muted);
    lines
        .iter()
        .map(|line| {
            let spans: Vec<Span<'static>> = line
                .iter()
                .map(|part| match part {
                    DetailPart::Text(value) => Span::styled(value.clone(), text),
                    DetailPart::Missing(value) => Span::styled(value.clone(), muted),
                })
                .collect();
            Line::from(spans).style(text)
        })
        .collect()
}

/// Details pane lines as plain text, one line per row.
pub fn details_plain_text(lines: &[DetailLine]) -> String {
    lines
        .iter()
        .map(|line| {
            line.iter()
                .map(|part| match part {
                    DetailPart::Text(value) | DetailPart::Missing(value) => value.as_str(),
                })
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Details pane lines wrapped by character to `width` columns, one entry per
/// screen row, so the pane can be sized from the exact row count.
pub fn wrap_details<'a>(lines: &'a [Line<'_>], width: usize) -> Vec<Line<'a>> {
    lines
        .iter()
        .flat_map(|line| wrap_line(line, width))
        .collect()
}

/// Last row of a details pane too short for its content.
pub const DETAILS_MORE_MARKER: &str = "… more, enlarge terminal";

/// Draw a details pane from rows already wrapped to its inner width. When
/// the pane is shorter than the rows, the first rows are shown and the last
/// visible rows say that more is hidden.
pub fn render_details(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    title: &str,
    mut rows: Vec<Line<'_>>,
) {
    let block = Block::default()
        .title(title.to_owned())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);
    let visible = usize::from(inner.height);
    if rows.len() > visible {
        let muted = Style::default().fg(app.theme.muted);
        let marker = Line::raw(DETAILS_MORE_MARKER);
        let marker_rows = wrap_line(&marker, usize::from(inner.width));
        let keep = visible.saturating_sub(marker_rows.len());
        rows.truncate(keep);
        rows.extend(
            marker_rows
                .iter()
                .take(visible - keep)
                .map(|row| Line::styled(row.to_string(), muted)),
        );
    }
    frame.render_widget(Paragraph::new(rows).block(block), area);
}

/// Append a form's inline validation message as the last body line, in red.
pub fn with_error_line<'a>(mut lines: Vec<Line<'a>>, error: Option<&str>) -> Vec<Line<'a>> {
    if let Some(error) = error {
        lines.push(Line::styled(
            error.to_owned(),
            Style::default().fg(Color::Red),
        ));
    }
    lines
}

/// Text of the sudo prompt: the operation, then one `*` per typed character
/// and the [`INPUT_CURSOR`].
fn sudo_prompt_body(operation: &str, password: &SecretInput) -> String {
    format!(
        "Authenticating to run: {operation}\n\nEnter sudo password:\n{}{INPUT_CURSOR}",
        "*".repeat(password.char_count())
    )
}

pub fn render_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    let (title, body, footer) = match modal {
        ModalState::Info { title, message } => (
            title.as_deref().unwrap_or("Result"),
            message.clone(),
            "Enter/Esc close",
        ),
        ModalState::Help { scroll } => {
            render_help(frame, area, app, *scroll);
            return;
        }
        ModalState::SudoPrompt {
            password, error, ..
        } => {
            let operation = app.last_preview.first().map_or("", String::as_str);
            let body = text_lines(sudo_prompt_body(operation, password));
            let body = with_error_line(body, error.as_deref());
            // Keep the last line in view: the error when there is one,
            // otherwise the password row with its cursor.
            let focus = body.len().checked_sub(1);
            render_modal_frame(
                frame,
                area,
                app,
                "Authentication required",
                body,
                focus,
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
        ModalState::QuitConfirm { selected } => {
            let action = app
                .pending_operation
                .as_ref()
                .map_or("", |prepared| prepared.description.as_str());
            let body = text_lines(format!(
                "Quit and drop the prepared operation?\n\n{action}\n\n{} Yes   {} No",
                marker(*selected, 0),
                marker(*selected, 1)
            ));
            let focus = body.len().saturating_sub(1);
            render_modal_frame(frame, area, app, "Quit?", body, Some(focus), YES_NO_FOOTER);
            return;
        }
        ModalState::FilterMenu { selected } => {
            // Shadow chips that cannot apply (`[-]`) are drawn muted.
            let muted = Style::default().fg(app.theme.muted);
            let body = filter_menu_rows(app)
                .into_iter()
                .enumerate()
                .map(|(index, row)| {
                    let style = if row.starts_with("[-]") {
                        muted
                    } else {
                        Style::default()
                    };
                    Line::styled(format!("{} {row}", marker(*selected, index)), style)
                })
                .collect::<Vec<_>>();
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
                body,
                Some(*selected),
                "↑↓ move  Enter/Space toggle  Backspace back  Esc close",
            );
            return;
        }
        _ => return,
    };
    render_modal_frame(frame, area, app, title, text_lines(body), None, footer);
}

/// Operation confirmation titled with what the action does: the intro line,
/// the window of preview steps that fits, the review line for long previews,
/// and Apply/Cancel. `state` is the modal's (selected, scroll, seen_all).
fn render_operation_confirm(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    state: (usize, usize, bool),
    description: &str,
    preview: &[String],
) {
    let (selected, scroll, seen_all) = state;
    let intro = confirm_intro(app.dry_run);
    let window = operation_preview_window((area.width, area.height), intro, preview, scroll);
    let shown = match window {
        PreviewWindow::Hidden => return,
        PreviewWindow::TooSmall { need_rows } => {
            let title = confirm_title(
                description,
                &format!(" ({} steps)", preview.len()),
                area.width,
            );
            render_preview_too_small(frame, area, app, selected, &title, need_rows);
            return;
        }
        PreviewWindow::Steps(shown) => shown,
    };
    let suffix = if preview.is_empty() {
        String::new()
    } else {
        format!(
            " (steps {}-{} of {})",
            shown.start + 1,
            shown.end,
            preview.len()
        )
    };
    let title = confirm_title(description, &suffix, area.width);
    let reached_end = shown.end >= preview.len();
    let mut body = vec![Line::raw(intro), Line::default()];
    body.extend(preview[shown.clone()].iter().cloned().map(Line::raw));
    body.push(Line::default());
    if preview.len() > PREVIEW_SEEN_IMMEDIATELY {
        body.push(if seen_all || reached_end {
            Line::styled(
                format!("All {} steps shown.", preview.len()),
                Style::default().fg(app.theme.muted),
            )
        } else {
            Line::styled(PREVIEW_REFUSAL, Style::default().fg(Color::Yellow))
        });
    }
    body.push(Line::raw(confirm_choices(selected)));
    let footer = if shown.len() < preview.len() {
        PREVIEW_FOOTER
    } else {
        YES_NO_FOOTER
    };
    // The preview window is sized so the body never has to scroll.
    render_modal_frame(frame, area, app, &title, body, None, footer);
}

/// Confirmation on a terminal where no complete step fits: the size it
/// needs and the choices, with no partial command.
fn render_preview_too_small(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    selected: usize,
    title: &str,
    need_rows: usize,
) {
    let message = preview_too_small_message(need_rows);
    let inner = usize::from(modal_width(area.width).saturating_sub(2));
    let rows = |text: &str| wrap_line(&Line::raw(text), inner).len();
    let choices = confirm_choices(selected);
    let mut body = vec![Line::styled(
        message.clone(),
        Style::default().fg(Color::Yellow),
    )];
    // A blank line only when it does not push the choices out of view.
    if rows(&message) + 1 + rows(&choices) <= modal_body_rows(area.height) {
        body.push(Line::default());
    }
    body.push(Line::raw(choices));
    let focus = body.len() - 1;
    render_modal_frame(frame, area, app, title, body, Some(focus), YES_NO_FOOTER);
}

/// Keys bound to any of `actions`, collapsed and ordered the way the
/// keybindings panel lists them (`Shift+K`, not `K, Shift+k`), or `unbound`.
fn bound_keys(app: &AppState, actions: &[KeyAction]) -> String {
    let keys: BTreeSet<PanelKey> = app
        .keymap
        .all_bindings()
        .into_iter()
        .filter(|(_, action)| actions.contains(action))
        .map(|((modifiers, code), _)| panel_key(modifiers, code))
        .collect();
    if keys.is_empty() {
        return "unbound".to_owned();
    }
    keys.into_iter()
        .map(|(_, _, key)| key)
        .collect::<Vec<_>>()
        .join(", ")
}

/// `name: path` for config file `name`, from the paths cached when the
/// configuration was loaded: the file that was read, plus `saves to` when
/// changes go elsewhere; the save path marked `(created)` when no file
/// existed and defaults were written; or `not loaded` with the save path
/// when reading or creating the file failed. Rendering never resolves
/// config roots itself.
fn config_path_line(app: &AppState, name: &str) -> String {
    let Some(paths) = app.config_paths.iter().find(|paths| paths.name == name) else {
        return format!("{name}: not loaded");
    };
    match &paths.state {
        ConfigFileState::Loaded(loaded) if *loaded == paths.save => format!("{name}: {loaded}"),
        ConfigFileState::Loaded(loaded) => format!("{name}: {loaded}  saves to {}", paths.save),
        ConfigFileState::Created => format!("{name}: {} (created)", paths.save),
        ConfigFileState::Failed => format!("{name}: not loaded, would save to {}", paths.save),
    }
}

/// Help modal lines: recorded configuration problems first, then the keys
/// by section, then where the configuration files are. Main-view keys come
/// from the keymap, so a rebinding shows up here.
pub fn help_lines(app: &AppState) -> Vec<Line<'static>> {
    use KeyAction::*;
    let heading = Style::default()
        .fg(app.theme.title)
        .add_modifier(Modifier::BOLD);
    let text = Style::default().fg(app.theme.text);
    let mut lines = Vec::new();
    let mut section = |title: &str, entries: Vec<String>| {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::styled(title.to_owned(), heading));
        lines.extend(entries.into_iter().map(|entry| Line::styled(entry, text)));
    };
    if !app.diagnostics.config_messages.is_empty() {
        section(
            crate::app::CONFIG_PROBLEMS_TITLE,
            app.diagnostics.config_messages.clone(),
        );
    }
    let keys = |actions: &[KeyAction]| bound_keys(app, actions);
    section(
        "Navigation",
        vec![
            format!("Move up: {}", keys(&[MoveUp])),
            format!("Move down: {}", keys(&[MoveDown])),
            format!("Page up: {}", keys(&[PageUp, MoveLeftPage])),
            format!("Page down: {}", keys(&[PageDown, MoveRightPage])),
            format!("Switch tab: {}", keys(&[SwitchTab])),
            format!(
                "Switch pane: {}",
                keys(&[ToggleUsersFocus, ToggleGroupsFocus])
            ),
        ],
    );
    section(
        "General",
        vec![
            format!("Help: {}", keys(&[OpenHelp])),
            format!("Toggle keybindings panel: {}", keys(&[ToggleKeybindsPane])),
            format!("Refresh accounts: {}", keys(&[Refresh])),
            format!("Quit: {}", keys(&[Quit])),
        ],
    );
    section(
        "Search and filters",
        vec![
            format!("Search the active tab: {}", keys(&[StartSearch])),
            "While typing: Enter keeps the search, Esc restores the previous one".to_owned(),
            format!("Clear search: {}", keys(&[ClearSearch])),
            format!("Filters: {}", keys(&[OpenFilterMenu])),
            format!(
                "Sort the active tab: {} (users: UID, name, shell, home; groups: GID, name, member count)",
                keys(&[CycleSort])
            ),
        ],
    );
    let enter = keys(&[EnterAction]);
    let new = keys(&[NewUser]);
    let delete = keys(&[DeleteSelection]);
    section(
        "Users tab",
        vec![
            format!("User actions: {enter} on a user"),
            format!(
                "Mark or unmark a user for bulk actions: {}",
                keys(&[ToggleMark])
            ),
            format!(
                "Bulk actions: {enter} while users are marked (add to or remove from a group, lock, unlock)"
            ),
            format!(
                "Clear marks: {} (the next press clears the search)",
                keys(&[ClearSearch])
            ),
            format!("Group actions: {enter} in Member of"),
            format!(
                "Copy the highlighted user or group name: {} (the terminal must allow OSC 52)",
                keys(&[CopySelection])
            ),
            format!("New user: {new}"),
            format!("Delete user: {delete} on a user"),
            format!("Remove from group: {delete} in Member of"),
        ],
    );
    section(
        "Groups tab",
        vec![
            format!("Group actions: {enter} on a group"),
            format!("Member actions (go to user, remove from group): {enter} in Members"),
            format!("Remove member: {delete} in Members"),
            format!(
                "Copy the highlighted group or member name: {}",
                keys(&[CopySelection])
            ),
            format!("New group: {new}"),
            format!("Delete group: {delete} on a group"),
        ],
    );
    section(
        "Inside dialogs",
        vec![
            "Move in menus: Up, Down, j, k".to_owned(),
            "Choose in yes/no dialogs: Left, Right, h, l, Tab, or y/n".to_owned(),
            "Tick a row: Space; tick all: *".to_owned(),
            "Filter candidate lists: /".to_owned(),
            "Reveal passwords: Ctrl+R".to_owned(),
            "Back: Backspace in menus; Esc goes back one level".to_owned(),
            "Long command previews: scroll to the end before Apply".to_owned(),
        ],
    );
    section(
        "Configuration",
        vec![
            config_path_line(app, "theme.conf"),
            config_path_line(app, "keybinds.conf"),
            config_path_line(app, "filter.conf"),
            config_path_line(app, "settings.conf"),
            "--config-dir <PATH> reads and writes these files in PATH only".to_owned(),
            "--dry-run shows every command preview and never runs it".to_owned(),
            "--log-file <PATH> appends operations and results to PATH".to_owned(),
        ],
    );
    lines
}

/// Help rows on a terminal of `area` (width, height) and the range shown
/// at `scroll`. Every help line is first wrapped by character to the
/// dialog's inner width, so each returned row is one screen row and a line
/// longer than the dialog can be scrolled through row by row. The start is
/// clamped so the last page is full. At least one row is shown.
pub fn help_window<'a>(
    lines: &'a [Line<'_>],
    area: (u16, u16),
    scroll: usize,
) -> (Vec<Line<'a>>, std::ops::Range<usize>) {
    let inner = usize::from(modal_width(area.0).saturating_sub(2));
    let rows = wrap_details(lines, inner);
    let visible = modal_body_rows(area.1).max(1);
    let start = scroll.min(rows.len().saturating_sub(visible));
    let end = (start + visible).min(rows.len());
    (rows, start..end)
}

/// Largest useful help scroll on a terminal of `area` (width, height): the
/// one that shows the last page of wrapped rows.
pub fn help_max_scroll(app: &AppState, area: (u16, u16)) -> usize {
    help_window(&help_lines(app), area, usize::MAX).1.start
}

/// Help dialog at `scroll`, titled with the visible row range.
fn render_help(frame: &mut Frame, area: Rect, app: &AppState, scroll: u16) {
    let lines = help_lines(app);
    let (rows, shown) = help_window(&lines, (area.width, area.height), usize::from(scroll));
    let title = format!(
        "Help (rows {}-{} of {})",
        shown.start + 1,
        shown.end,
        rows.len()
    );
    // The rows already fit the dialog's width, so the frame does not re-wrap them.
    let body = rows[shown].to_vec();
    render_modal_frame(
        frame,
        area,
        app,
        &title,
        body,
        None,
        "↑↓ PgUp PgDn scroll  Esc close",
    );
}

/// Filter menu rows for the active tab, without the cursor. User chips read
/// `[x] Human users`; shadow chips read `[x] Locked (shadow)`, or
/// `[-] Locked (<reason>)` while `shadow_chips_blocked` gives a reason
/// (`shadow unavailable: ...`, `2 users without shadow record`). The groups
/// menu is a radio: `(•) User GIDs`.
pub fn filter_menu_rows(app: &AppState) -> Vec<String> {
    use crate::app::{ActiveTab, GroupsFilter};
    match app.active_tab {
        ActiveTab::Users => {
            let chips = &app.users_filter_chips;
            let check = |on: bool, label: &str| format!("[{}] {label}", if on { "x" } else { " " });
            let blocked = app.shadow_chips_blocked();
            let shadow = |on: bool, label: &str| match &blocked {
                None => check(on, &format!("{label} (shadow)")),
                Some(reason) => format!("[-] {label} ({reason})"),
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
/// Append a non-default row order to a table title: `  sort: name`.
pub fn push_sort_label(title: &mut String, label: Option<&str>) {
    if let Some(label) = label {
        title.push_str("  sort: ");
        title.push_str(label);
    }
}

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

/// The stale label with its reason (`STALE since 22:13:20 UTC: boom`) when
/// the last refresh failed, for the second line of a table's empty state.
pub fn stale_line(app: &AppState) -> Option<String> {
    app.diagnostics
        .stale_reason
        .is_some()
        .then(|| refresh_label(app))
}

/// Empty-state lines for a table with nothing loaded: `No users loaded`,
/// then the stale label with its reason when the last refresh failed.
pub fn nothing_loaded_lines(app: &AppState, noun: &str) -> Vec<String> {
    let mut lines = vec![format!("No {noun} loaded")];
    lines.extend(stale_line(app));
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
    let lines: Vec<Line<'_>> = lines.into_iter().map(Line::from).collect();
    let rows: Vec<Line<'_>> = lines
        .iter()
        .flat_map(|line| wrap_line(line, usize::from(inner.width)))
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
            .style(Style::default().fg(app.theme.muted)),
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
/// `▶ [x] name` row per candidate, with ` (blocked: <reason>)` after names
/// the policy refuses. `candidates` must be the same list the key handler
/// indexes (see `AppState::group_member_candidate_list` and
/// `AppState::user_group_candidate_list`); `ticked` holds names.
/// Blocked rows are drawn in the muted color.
pub fn candidate_page(
    app: &AppState,
    candidates: &CandidateList,
    selected: usize,
    ticked: &[String],
) -> Vec<Line<'static>> {
    let start = selected / CANDIDATE_ROWS * CANDIDATE_ROWS;
    let muted = Style::default().fg(app.theme.muted);
    candidates
        .names
        .iter()
        .enumerate()
        .skip(start)
        .take(CANDIDATE_ROWS)
        .map(|(index, name)| {
            let reason = candidates.blocked_reason(index);
            let blocked = reason
                .map(|reason| format!(" (blocked: {reason})"))
                .unwrap_or_default();
            let row = format!(
                "{} [{}] {name}{blocked}",
                marker(selected, index),
                if ticked.contains(name) { "x" } else { " " }
            );
            if reason.is_some() {
                Line::styled(row, muted)
            } else {
                Line::from(row)
            }
        })
        .collect()
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

/// Title, body, focus row, and footer of a multi-select dialog. The title
/// reads `Add groups  page 2/9  3 selected` and notes when the list was
/// capped. The body starts with the filter line when a filter is set or
/// being typed. The focus row is the highlighted candidate's body line.
pub fn candidate_dialog(
    app: &AppState,
    view: &CandidateView<'_>,
) -> (String, Vec<Line<'static>>, Option<usize>, &'static str) {
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
    let focus = (len > 0).then(|| body.len() + view.selected % CANDIDATE_ROWS);
    body.extend(candidate_page(app, view.list, view.selected, view.ticked));
    let footer = if view.filtering {
        FILTER_FOOTER
    } else if view.ticked.is_empty() {
        MULTI_HIGHLIGHTED_FOOTER
    } else {
        MULTI_FOOTER
    };
    (title, body, focus, footer)
}

#[cfg(test)]
mod tests {
    use super::{INPUT_CURSOR, SecretInput, sudo_prompt_body};

    #[test]
    fn sudo_prompt_shows_one_star_per_character_then_the_cursor() {
        let mut password = SecretInput::default();
        for character in "éa€".chars() {
            password.push(character);
        }
        let body = sudo_prompt_body("usermod -s /bin/bash alice", &password);
        assert!(body.ends_with(&format!("\n***{INPUT_CURSOR}")), "{body}");

        let empty = sudo_prompt_body("usermod -s /bin/bash alice", &SecretInput::default());
        assert!(
            empty.ends_with(&format!("password:\n{INPUT_CURSOR}")),
            "{empty}"
        );
    }
}
