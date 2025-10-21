//! Shared UI components (status bar, modal helpers).
//!
//! Contains small building blocks reused by users/groups screens.
//!
use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::app::{AppState, ModalState};
use std::collections::{BTreeMap, BTreeSet};

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
    let chips_str = if chips.is_empty() {
        String::new()
    } else {
        format!("  filters:[{}]", chips.join(","))
    };
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

/// Render the right-side keybinds viewer with grouped sections.
pub fn render_keybinds_panel(f: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .title("Keybindings")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    let inner = block.inner(area);

    // We'll collect styled lines instead of plain string body

    // Build friendly maps of actions -> keys
    let mut general: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();
    let mut navigation: BTreeMap<&'static str, BTreeSet<String>> = BTreeMap::new();

    for ((mods, code), action) in app.keymap.all_bindings().into_iter() {
        // Normalize display for certain combos
        let key = match code {
            crossterm::event::KeyCode::BackTab => "Shift+Tab".to_string(),
            crossterm::event::KeyCode::Tab
                if mods.contains(crossterm::event::KeyModifiers::SHIFT) =>
            {
                "Shift+Tab".to_string()
            }
            _ => crate::app::keymap::Keymap::format_key(mods, code),
        };

        match action {
            // General app commands
            crate::app::keymap::KeyAction::Quit => {
                general.entry("Quit").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::SwitchTab => {
                general.entry("Switch tab").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::OpenFilterMenu => {
                general.entry("Open filter menu").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::StartSearch => {
                general.entry("Search").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::DeleteSelection => {
                general.entry("Delete selection").or_default().insert(key);
            }

            // Navigation
            crate::app::keymap::KeyAction::MoveUp => {
                navigation.entry("Move up").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::MoveDown => {
                navigation.entry("Move down").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::MoveLeftPage => {
                navigation.entry("Move left").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::MoveRightPage => {
                navigation.entry("Move right").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::PageUp => {
                navigation.entry("Page up").or_default().insert(key);
            }
            crate::app::keymap::KeyAction::PageDown => {
                navigation.entry("Page down").or_default().insert(key);
            }

            // Shown in contextual/tab sections below; skip in general list
            crate::app::keymap::KeyAction::EnterAction
            | crate::app::keymap::KeyAction::ToggleUsersFocus
            | crate::app::keymap::KeyAction::ToggleGroupsFocus
            | crate::app::keymap::KeyAction::ToggleKeybindsPane
            | crate::app::keymap::KeyAction::OpenHelp
            | crate::app::keymap::KeyAction::NewUser
            | crate::app::keymap::KeyAction::Ignore => {}
        }
    }

    // Compute column widths based on inner area and render rows with alignment
    let total_w = inner.width as usize;
    let sep = " │ ";
    let sep_w = sep.chars().count();

    let mut max_label = 0usize;
    for k in general.keys() {
        max_label = max_label.max(k.len());
    }
    for k in navigation.keys() {
        max_label = max_label.max(k.len());
    }
    max_label = max_label.max("Cancel / Close".len());
    max_label = max_label.max("Toggle checkbox / multi-select".len());
    max_label = max_label.max("Confirm / Apply".len());
    max_label = max_label.max("Toggle pane".len());
    max_label = max_label.max("New user".len());
    max_label = max_label.max("New group".len());

    let col1_w = std::cmp::min(max_label, total_w.saturating_sub(sep_w + 8));
    let _col2_w = total_w.saturating_sub(sep_w + col1_w);

    let push_row = |label: &str, value: &str| -> (String, String) {
        let mut lbl = label.to_string();
        if lbl.chars().count() > col1_w {
            // Truncate safely on char boundaries
            lbl = lbl.chars().take(col1_w).collect();
        }
        let label_aligned = format!("{:>width$}", lbl, width = col1_w);
        let left = format!("  {}{}", label_aligned, sep);
        (left, value.to_string())
    };

    // Build styled lines instead of a raw string
    let mut lines: Vec<Line> = Vec::new();

    // General
    lines.push(Line::from(Span::styled(
        "General:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for (label, keys) in general.iter() {
        let joined = keys.iter().cloned().collect::<Vec<_>>().join(", ");
        let (left, right) = push_row(label, &joined);
        lines.push(Line::from(vec![
            Span::raw(left),
            Span::styled(right, Style::default().add_modifier(Modifier::ITALIC)),
        ]));
    }

    // Navigation
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Navigation:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for (label, keys) in navigation.iter() {
        let joined = keys.iter().cloned().collect::<Vec<_>>().join(", ");
        let (left, right) = push_row(label, &joined);
        lines.push(Line::from(vec![
            Span::raw(left),
            Span::styled(right, Style::default().add_modifier(Modifier::ITALIC)),
        ]));
    }

    // Contextual
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Contextual:",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    for (label, value) in [
        ("Cancel / Close", "Esc, Backspace"),
        ("Toggle checkbox / multi-select", "Space"),
        ("Confirm / Apply", "Enter"),
        ("Help", "?"),
    ] {
        let (left, right) = push_row(label, value);
        lines.push(Line::from(vec![
            Span::raw(left),
            Span::styled(right, Style::default().add_modifier(Modifier::ITALIC)),
        ]));
    }

    // Tab-specific
    lines.push(Line::raw(""));
    match app.active_tab {
        crate::app::ActiveTab::Users => {
            lines.push(Line::from(Span::styled(
                "Users tab:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for (label, value) in [
                ("Toggle pane", "Shift+Tab"),
                ("New user", "n"),
                ("Toggle keybindings", "Shift+K"),
            ] {
                let (left, right) = push_row(label, value);
                lines.push(Line::from(vec![
                    Span::raw(left),
                    Span::styled(right, Style::default().add_modifier(Modifier::ITALIC)),
                ]));
            }
        }
        crate::app::ActiveTab::Groups => {
            lines.push(Line::from(Span::styled(
                "Groups tab:",
                Style::default().add_modifier(Modifier::BOLD),
            )));
            for (label, value) in [
                ("Toggle pane", "Shift+Tab"),
                ("New group", "n"),
                ("Toggle keybindings", "Shift+K"),
            ] {
                let (left, right) = push_row(label, value);
                lines.push(Line::from(vec![
                    Span::raw(left),
                    Span::styled(right, Style::default().add_modifier(Modifier::ITALIC)),
                ]));
            }
        }
    }

    let p = Paragraph::new(lines).wrap(Wrap { trim: false });
    f.render_widget(block, area);
    f.render_widget(p, inner);
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

/// Render the help modal with important usage information and key tips.
pub fn render_help_modal(f: &mut Frame, area: Rect, app: &AppState, scroll: u16) {
    let width = 80u16.min(area.width.saturating_sub(4)).max(60);
    let height = 22u16.min(area.height.saturating_sub(4)).max(14);
    let rect = centered_rect(width, height, area);

    let mut lines: Vec<Line> = vec![
        Line::from(Span::styled(
            "Help",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::raw(""),
    ];
    lines.push(Line::from(vec![
        Span::raw("Navigation: "),
        Span::styled(
            "Arrow keys / h j k l",
            Style::default().add_modifier(Modifier::ITALIC),
        ),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Search: "),
        Span::styled("/", Style::default().add_modifier(Modifier::ITALIC)),
        Span::raw(" to start; type and Enter to apply; Esc to cancel"),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Switch tab: "),
        Span::styled("Tab", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Toggle right pane: "),
        Span::styled("Shift+Tab", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Open filter menu: "),
        Span::styled("f", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Open keybindings panel: "),
        Span::styled("Shift+K", Style::default().add_modifier(Modifier::ITALIC)),
        Span::raw(" (toggle)"),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Open this help: "),
        Span::styled("?", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Quit: "),
        Span::styled("q", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Users tab",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::raw("Open actions / modify: "),
        Span::styled("Enter", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Create user: "),
        Span::styled("n", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Delete user / remove from group: "),
        Span::styled("Delete", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "Groups tab",
        Style::default().add_modifier(Modifier::BOLD),
    )));
    lines.push(Line::from(vec![
        Span::raw("Open actions: "),
        Span::styled("Enter", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Create group: "),
        Span::styled("n", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::from(vec![
        Span::raw("Delete group: "),
        Span::styled("Delete", Style::default().add_modifier(Modifier::ITALIC)),
    ]));
    lines.push(Line::raw(""));
    lines.push(Line::from(vec![
        Span::raw("Close help: "),
        Span::styled(
            "Esc / Enter",
            Style::default().add_modifier(Modifier::ITALIC),
        ),
    ]));

    let p = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(
            Block::default()
                .title("Help")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        );
    f.render_widget(Clear, rect);
    f.render_widget(p, rect);
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
                    "Human users only (uid >= 1000)",
                    "System users only (uid < 1000)",
                    "Inactive shell (nologin/false)",
                    "No home directory",
                    "Locked account",
                    "No password set",
                    "Password expired",
                ];
                let mut text = String::new();
                for (idx, label) in opts.iter().enumerate() {
                    let marker = if idx == *selected { "▶" } else { " " };
                    // For chip options (idx >= 1) show checkbox from state
                    let checkbox = if idx >= 1 {
                        let checked = match idx {
                            1 => app.users_filter_chips.human_only,
                            2 => app.users_filter_chips.system_only,
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
