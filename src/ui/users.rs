//! Users screen rendering and modals.
//!
//! Contains the users table, details and member-of panels, and all user
//! modification modal dialogs.
//!
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table};

use crate::app::{AppState, ModalState, ModifyField, UsersFocus};

/// Render the users table and manage selection/pagination state.
pub fn render_users_table(f: &mut Frame, area: Rect, app: &mut AppState) {
    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 {
        app.rows_per_page = body_height;
    }

    let start = (app.selected_user_index / app.rows_per_page) * app.rows_per_page;
    let end = (start + app.rows_per_page).min(app.users.len());
    let slice = &app.users[start..end];

    let rows = slice.iter().enumerate().map(|(i, u)| {
        let absolute_index = start + i;
        let style = if absolute_index == app.selected_user_index {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let name_text = if absolute_index == app.selected_user_index {
            format!("[{}]", u.name)
        } else {
            u.name.clone()
        };
        Row::new(vec![
            Cell::from(u.uid.to_string()),
            Cell::from(name_text),
            Cell::from(u.primary_gid.to_string()),
            Cell::from(u.home_dir.clone()),
            Cell::from(u.shell.clone()),
        ])
        .style(style)
    });

    let widths = [
        Constraint::Length(8),
        Constraint::Length(24),
        Constraint::Length(8),
        Constraint::Percentage(40),
        Constraint::Percentage(40),
    ];

    let header = Row::new(vec!["UID", "USER", "GID", "HOME", "SHELL"]).style(
        Style::default()
            .fg(app.theme.title)
            .add_modifier(Modifier::BOLD),
    );

    let users_title = {
        let base = if app.users_focus == UsersFocus::UsersList {
            "[Users]"
        } else {
            "Users"
        };
        if app.users_focus == UsersFocus::UsersList {
            if let Some(u) = app.users.get(app.selected_user_index) {
                format!("{} - {}", base, u.name)
            } else {
                base.to_string()
            }
        } else {
            base.to_string()
        }
    };
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(users_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .row_highlight_style(
            Style::default()
                .fg(app.theme.highlight_fg)
                .bg(app.theme.highlight_bg)
                .add_modifier(Modifier::REVERSED),
        )
        .column_spacing(1);

    f.render_widget(table, area);
}

/// Render the details panel for the selected user.
pub fn render_user_details(f: &mut Frame, area: Rect, app: &AppState) {
    let user = app.users.get(app.selected_user_index);
    let (username, fullname, uid, gid, home, shell) = match user {
        Some(u) => (
            u.name.clone(),
            u.full_name.clone().unwrap_or_default(),
            u.uid,
            u.primary_gid,
            u.home_dir.clone(),
            u.shell.clone(),
        ),
        None => (
            String::new(),
            String::new(),
            0,
            0,
            String::new(),
            String::new(),
        ),
    };

    let text = format!(
        "Username: {username}\nFullname: {fullname}\nUID: {uid}\nGID: {gid}\nHome directory: {home}\nShell: {shell}"
    );
    let p = Paragraph::new(text)
        .style(Style::default().fg(app.theme.text))
        .block(
            Block::default()
                .title("Details")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        );
    f.render_widget(p, area);
}

/// Render the list of groups the selected user belongs to.
pub fn render_user_groups(f: &mut Frame, area: Rect, app: &mut AppState) {
    let groups = if let Some(u) = app.users.get(app.selected_user_index) {
        let name = u.name.clone();
        let pgid = u.primary_gid;
        app.groups
            .iter()
            .filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &name))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    if !groups.is_empty() {
        if app.selected_group_index >= groups.len() {
            app.selected_group_index = groups.len() - 1;
        }
    } else {
        app.selected_group_index = 0;
    }

    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 {
        app.rows_per_page = body_height;
    }
    let start = (app.selected_group_index / app.rows_per_page) * app.rows_per_page;
    let end = (start + app.rows_per_page).min(groups.len());
    let slice = &groups[start..end];

    let rows = slice.iter().enumerate().map(|(i, g)| {
        let absolute_index = start + i;
        let style = if absolute_index == app.selected_group_index {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        let name_text = if absolute_index == app.selected_group_index {
            format!("[{}]", g.name)
        } else {
            g.name.clone()
        };
        Row::new(vec![Cell::from(g.gid.to_string()), Cell::from(name_text)]).style(style)
    });

    let widths = [Constraint::Length(8), Constraint::Percentage(100)];
    let header = Row::new(vec!["GID", "Name"]).style(
        Style::default()
            .fg(app.theme.title)
            .add_modifier(Modifier::BOLD),
    );

    let groups_title = {
        let base = if app.users_focus == UsersFocus::MemberOf {
            "[Member of]"
        } else {
            "Member of"
        };
        if app.users_focus == UsersFocus::MemberOf {
            if let Some(g) = groups.get(app.selected_group_index) {
                format!("{} - {}", base, g.name)
            } else {
                base.to_string()
            }
        } else {
            base.to_string()
        }
    };
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(groups_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .column_spacing(1);
    f.render_widget(table, area);
}

/// Render user-related modal dialogs based on state.
pub fn render_user_modal(f: &mut Frame, area: Rect, app: &mut AppState, state: &ModalState) {
    match state.clone() {
        ModalState::Actions { selected } => {
            let width = 30u16;
            let height = 7u16;
            let rect = crate::ui::components::centered_rect(width, height, area);
            let options = ["Modify", "Delete"];
            let mut text = String::new();
            for (idx, label) in options.iter().enumerate() {
                if idx == selected {
                    text.push_str(&format!("▶ {}\n", label));
                } else {
                    text.push_str(&format!("  {}\n", label));
                }
            }
            let p = Paragraph::new(text).block(
                Block::default()
                    .title("Actions")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::ModifyMenu { selected } => {
            let rect = crate::ui::components::centered_rect(36, 9, area);
            let options = ["Add group", "Remove group", "Change details", "Password"];
            let mut text = String::new();
            for (idx, label) in options.iter().enumerate() {
                if idx == selected {
                    text.push_str(&format!("▶ {}\n", label));
                } else {
                    text.push_str(&format!("  {}\n", label));
                }
            }
            let p = Paragraph::new(text).block(
                Block::default()
                    .title("Modify")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::ModifyPasswordMenu { selected } => {
            let rect = crate::ui::components::centered_rect(50, 8, area);
            let options = [
                "Set/change password",
                "Reset (expire; must change next login)",
            ];
            let mut text = String::new();
            for (idx, label) in options.iter().enumerate() {
                if idx == selected {
                    text.push_str(&format!("▶ {}\n", label));
                } else {
                    text.push_str(&format!("  {}\n", label));
                }
            }
            let p = Paragraph::new(text).block(
                Block::default()
                    .title("Password")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
        } => {
            let rect = crate::ui::components::centered_rect(60, 10, area);
            let pw_mask = "*".repeat(password.len());
            let cf_mask = "*".repeat(confirm.len());
            let mc = if must_change { "[x]" } else { "[ ]" };
            let lines = [
                format!(
                    "{} New password: {}",
                    if selected == 0 { "▶" } else { " " },
                    pw_mask
                ),
                format!(
                    "{} Confirm:     {}",
                    if selected == 1 { "▶" } else { " " },
                    cf_mask
                ),
                format!(
                    "{} {} Must change at next login (Space)",
                    if selected == 2 { "▶" } else { " " },
                    mc
                ),
                format!("{} Submit", if selected == 3 { "▶" } else { " " }),
            ];
            let body = lines.join("\n");
            let p = Paragraph::new(body).block(
                Block::default()
                    .title("Set password")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::ModifyDetailsMenu { selected } => {
            let rect = crate::ui::components::centered_rect(34, 8, area);
            let options = ["Username", "Fullname", "Shell"];
            let mut text = String::new();
            for (idx, label) in options.iter().enumerate() {
                if idx == selected {
                    text.push_str(&format!("▶ {}\n", label));
                } else {
                    text.push_str(&format!("  {}\n", label));
                }
            }
            let p = Paragraph::new(text).block(
                Block::default()
                    .title("Change details")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::ModifyShell {
            selected,
            offset,
            shells,
        } => {
            let width = (area.width.saturating_sub(10)).clamp(40, 60);
            let height = (area.height.saturating_sub(6)).clamp(8, 20);
            let rect = crate::ui::components::centered_rect(width, height, area);
            let visible_capacity = rect.height.saturating_sub(2) as usize;
            let start = offset.min(shells.len());
            let end = (start + visible_capacity).min(shells.len());
            let slice = &shells[start..end];
            let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
            for (i, sh) in slice.iter().enumerate() {
                let abs_index = start + i;
                let marker = if abs_index == selected { "▶ " } else { "  " };
                items.push(ListItem::new(format!("{}{}", marker, sh)));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .title("Select shell")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(app.theme.border)),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_widget(Clear, rect);
            f.render_widget(list, rect);
        }
        ModalState::ModifyTextInput { field, value } => {
            let rect = crate::ui::components::centered_rect(50, 7, area);
            let title = match field {
                ModifyField::Username => "Change username",
                ModifyField::Fullname => "Change full name",
            };
            let msg = format!("{}:\n{}", title, value);
            let p = Paragraph::new(msg).block(
                Block::default()
                    .title("Input")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::ModifyGroupsAdd {
            selected,
            offset: _,
            selected_multi,
        } => {
            let width = (area.width.saturating_sub(10)).clamp(40, 60);
            let height = (area.height.saturating_sub(6)).clamp(8, 20);
            let rect = crate::ui::components::centered_rect(width, height, area);
            let visible_capacity = rect.height.saturating_sub(2) as usize;
            let total = app.groups_all.len();
            let max_offset = total.saturating_sub(visible_capacity);
            let mut off = selected.saturating_sub(visible_capacity / 2);
            if off > max_offset {
                off = max_offset;
            }
            let start = off.min(total);
            let end = (start + visible_capacity).min(total);
            let slice = &app.groups_all[start..end];
            let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
            for (i, g) in slice.iter().enumerate() {
                let abs_index = start + i;
                let focus = if abs_index == selected { "▶ " } else { "  " };
                let checked = if selected_multi.contains(&abs_index) {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(ListItem::new(format!(
                    "{}{}{} ({})",
                    focus, checked, g.name, g.gid
                )));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .title("Add to group")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(app.theme.border)),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_widget(Clear, rect);
            f.render_widget(list, rect);
        }
        ModalState::ModifyGroupsRemove {
            selected,
            offset: _,
            selected_multi,
        } => {
            let (username, primary_gid) = if let Some(u) = app.users.get(app.selected_user_index) {
                (u.name.clone(), u.primary_gid)
            } else {
                (String::new(), 0)
            };
            let user_groups: Vec<crate::sys::SystemGroup> = app
                .groups_all
                .iter()
                .filter(|g| g.gid == primary_gid || g.members.iter().any(|m| m == &username))
                .cloned()
                .collect();
            let width = (area.width.saturating_sub(10)).clamp(40, 60);
            let height = (area.height.saturating_sub(6)).clamp(8, 20);
            let rect = crate::ui::components::centered_rect(width, height, area);
            let visible_capacity = rect.height.saturating_sub(2) as usize;
            let total = user_groups.len();
            let max_offset = total.saturating_sub(visible_capacity);
            let mut off = selected.saturating_sub(visible_capacity / 2);
            if off > max_offset {
                off = max_offset;
            }
            let start = off.min(total);
            let end = (start + visible_capacity).min(total);
            let slice = &user_groups[start..end];
            let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
            for (i, g) in slice.iter().enumerate() {
                let abs_index = start + i;
                let focus = if abs_index == selected { "▶ " } else { "  " };
                let checked = if selected_multi.contains(&abs_index) {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(ListItem::new(format!(
                    "{}{}{} ({})",
                    focus, checked, g.name, g.gid
                )));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .title("Remove from group")
                        .borders(Borders::ALL)
                        .border_style(Style::default().fg(app.theme.border)),
                )
                .highlight_style(
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD),
                );
            f.render_widget(Clear, rect);
            f.render_widget(list, rect);
        }
        ModalState::DeleteConfirm {
            selected,
            allowed,
            delete_home,
        } => {
            let rect = crate::ui::components::centered_rect(50, 7, area);
            let (name, uid) = if let Some(u) = app.users.get(app.selected_user_index) {
                (u.name.clone(), u.uid)
            } else {
                (String::new(), 0)
            };
            let mut body = format!("Delete user '{name}' (uid {uid})?\n\n");
            if allowed {
                let yes = if selected == 0 { "[Yes]" } else { " Yes " };
                let no = if selected == 1 { "[No]" } else { " No  " };
                let checkbox = if delete_home { "[x]" } else { "[ ]" };
                body.push_str(&format!(
                    "  {}    {}\n\n{} Also delete home (Space)",
                    yes, no, checkbox
                ));
            } else {
                body.push_str("Deletion not allowed (only UID 1000-1999 allowed). Press Esc.");
            }
            let p = Paragraph::new(body).block(
                Block::default()
                    .title("Confirm delete")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::UserAddInput {
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
        } => {
            let rect = crate::ui::components::centered_rect(64, 13, area);
            let pw_mask = "*".repeat(password.len());
            let cf_mask = "*".repeat(confirm.len());
            let ch = if create_home { "[x]" } else { "[ ]" };
            let wh = if add_to_wheel { "[x]" } else { "[ ]" };
            let lines = [
                "Create new user".to_string(),
                format!(
                    "{} Username: {}",
                    if selected == 0 { "▶" } else { " " },
                    name
                ),
                format!(
                    "{} Password: {}",
                    if selected == 1 { "▶" } else { " " },
                    pw_mask
                ),
                format!(
                    "{} Confirm:  {}",
                    if selected == 2 { "▶" } else { " " },
                    cf_mask
                ),
                format!(
                    "{} {} Create home directory (Space)",
                    if selected == 3 { "▶" } else { " " },
                    ch
                ),
                format!(
                    "{} {} Add to wheel (sudo) group (Space)",
                    if selected == 4 { "▶" } else { " " },
                    wh
                ),
                format!("{} Submit", if selected == 5 { "▶" } else { " " }),
            ];
            let body = lines.join("\n");
            let p = Paragraph::new(body).block(
                Block::default()
                    .title("New user")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::Info { .. } => { /* routed to components */ }
        ModalState::SudoPrompt { .. } => { /* routed to components */ }
        _ => {}
    }
}
