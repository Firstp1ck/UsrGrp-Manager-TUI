//! Groups screen rendering and modals.
//!
//! Contains the groups table, details panel, members list, and group-related
//! modal dialogs for add/remove/rename actions.
//!
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table};

use crate::app::{AppState, GroupsFocus, ModalState};

/// Render the groups table and manage selection/pagination state.
pub fn render_groups_table(f: &mut Frame, area: Rect, app: &mut AppState) {
    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 {
        app.rows_per_page = body_height;
    }

    let start = (app.selected_group_index / app.rows_per_page) * app.rows_per_page;
    let end = (start + app.rows_per_page).min(app.groups.len());
    let slice = &app.groups[start..end];

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
    let header = Row::new(vec!["GID", "GROUP"]).style(
        Style::default()
            .fg(app.theme.title)
            .add_modifier(Modifier::BOLD),
    );

    let groups_title = {
        let base = if matches!(app.groups_focus, GroupsFocus::GroupsList) {
            "[Groups]"
        } else {
            "Groups"
        };
        if let Some(g) = app.groups.get(app.selected_group_index) {
            format!("{} - {}", base, g.name)
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

/// Render the selected group's summary details.
pub fn render_group_details(f: &mut Frame, area: Rect, app: &AppState) {
    let group = app.groups.get(app.selected_group_index);
    let (
        name,
        gid,
        members,
        is_system,
        primary_count,
        _secondary_count,
        sudo_flag,
        members_preview,
        shell_interactive,
        shell_noninteractive,
        uid_system_count,
        uid_user_count,
        locked_count,
        nopass_count,
        expired_count,
        orphan_count,
        group_mtime_days,
    ) = match group {
        Some(g) => {
            let is_system = g.gid < 1000;

            // Build lookup for users by name and collect primary members
            let mut user_by_name: std::collections::HashMap<&str, &crate::sys::SystemUser> =
                std::collections::HashMap::new();
            for u in &app.users_all {
                user_by_name.insert(&u.name, u);
            }
            let primary_usernames: Vec<&str> = app
                .users_all
                .iter()
                .filter(|u| u.primary_gid == g.gid)
                .map(|u| u.name.as_str())
                .collect();
            let primaries = primary_usernames.len();

            // Secondary members are from the group membership list
            let secondary_count = g.members.len();

            // Configurable sudo flag
            let sudo_group = crate::app::sudo_group_name();
            let sudo_flag = if g.name == sudo_group { "sudo" } else { "-" };

            // Combined member set (primary + secondary) for distributions
            let mut member_set: std::collections::BTreeSet<String> =
                std::collections::BTreeSet::new();
            for n in &g.members {
                member_set.insert(n.clone());
            }
            for n in primary_usernames {
                member_set.insert(n.to_string());
            }

            // Shell distribution and UID class counts
            let mut shell_interactive = 0usize;
            let mut shell_noninteractive = 0usize;
            let mut uid_system_count = 0usize;
            let mut uid_user_count = 0usize;
            let mut orphan_count = 0usize;
            for name in member_set.iter() {
                if let Some(u) = user_by_name.get(name.as_str()) {
                    let sh = u.shell.as_str();
                    let non = sh.ends_with("/nologin") || sh.ends_with("/false");
                    if non {
                        shell_noninteractive += 1;
                    } else {
                        shell_interactive += 1;
                    }
                    if u.uid < 1000 {
                        uid_system_count += 1;
                    } else {
                        uid_user_count += 1;
                    }
                } else {
                    orphan_count += 1;
                }
            }

            // Shadow status counts (best-effort)
            let mut locked_count = 0usize;
            let mut nopass_count = 0usize;
            let mut expired_count = 0usize;
            for name in member_set.iter() {
                if let Some(sh) = crate::search::user_shadow_status(name) {
                    if sh.locked {
                        locked_count += 1;
                    }
                    if sh.no_password {
                        nopass_count += 1;
                    }
                    if sh.expired {
                        expired_count += 1;
                    }
                }
            }

            // Alphabetical top-N preview of member names (secondary list only)
            let mut names = g.members.clone();
            names.sort_by_key(|a| a.to_lowercase());
            let n: usize = 10;
            let total = names.len();
            let shown: Vec<String> = names.into_iter().take(n).collect();
            let mut preview = if shown.is_empty() {
                "-".to_string()
            } else {
                shown.join(", ")
            };
            if total > n {
                let more = total - n;
                preview.push_str(&format!(" (+{} more)", more));
            }

            // /etc/group mtime in days since epoch (proxy for membership change)
            let group_mtime_days = std::fs::metadata("/etc/group")
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs() / 86_400)
                .unwrap_or(0);

            (
                g.name.clone(),
                g.gid,
                g.members.len(),
                is_system,
                primaries,
                secondary_count,
                sudo_flag.to_string(),
                preview,
                shell_interactive,
                shell_noninteractive,
                uid_system_count,
                uid_user_count,
                locked_count,
                nopass_count,
                expired_count,
                orphan_count,
                group_mtime_days,
            )
        }
        None => (
            String::new(),
            0,
            0,
            false,
            0,
            0,
            String::new(),
            String::new(),
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
            0,
        ),
    };
    let text = format!(
        "Group: {name}\nGID: {gid} ({})\nMembers (secondary): {members}\nPrimary members: {primary_count}\nPrivilege: {sudo_flag}\nMembers preview: {members_preview}\nShells: interactive={}, noninteractive={}\nUID class: system={}, user={}\nAccounts: locked={}, no_password={}, expired={}\nOrphan secondary members: {}\n/etc/group mtime (days since epoch): {}",
        if is_system { "system" } else { "user" },
        shell_interactive,
        shell_noninteractive,
        uid_system_count,
        uid_user_count,
        locked_count,
        nopass_count,
        expired_count,
        orphan_count,
        group_mtime_days,
    );
    let p = Paragraph::new(text)
        .style(Style::default().fg(app.theme.text))
        .block(
            Block::default()
                .title("Group Details")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        );
    f.render_widget(p, area);
}

/// Render the selected group's members list.
pub fn render_group_members(f: &mut Frame, area: Rect, app: &mut AppState) {
    let members = app
        .groups
        .get(app.selected_group_index)
        .map(|g| g.members.clone())
        .unwrap_or_default();

    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 {
        app.rows_per_page = body_height;
    }
    let start = 0;
    let end = members.len().min(app.rows_per_page);
    let slice = &members[start..end];

    let rows = slice.iter().enumerate().map(|(i, m)| {
        let absolute_index = start + i;
        let mut style = Style::default();
        if absolute_index == app.selected_group_member_index {
            style = style.fg(Color::Yellow).add_modifier(Modifier::BOLD);
        }
        let text = if absolute_index == app.selected_group_member_index {
            format!("[{}]", m)
        } else {
            m.clone()
        };
        Row::new(vec![Cell::from(text)]).style(style)
    });
    let widths = [Constraint::Percentage(100)];
    let header = Row::new(vec!["Members"]).style(
        Style::default()
            .fg(app.theme.title)
            .add_modifier(Modifier::BOLD),
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(if matches!(app.groups_focus, GroupsFocus::Members) {
                    "[Group Members]"
                } else {
                    "Group Members"
                })
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .column_spacing(1);
    f.render_widget(table, area);
}

/// Render group-related modal dialogs based on state.
pub fn render_group_modal(f: &mut Frame, area: Rect, app: &mut AppState, state: &ModalState) {
    match state.clone() {
        ModalState::GroupsActions {
            selected,
            target_gid,
        } => {
            let rect = crate::ui::components::centered_rect(36, 8, area);
            let (options, title) = if let Some(gid) = target_gid {
                let name = app
                    .groups
                    .iter()
                    .find(|g| g.gid == gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_else(|| "<unknown>".to_string());
                (
                    ["Modify group", "Remove group"].as_slice(),
                    &*format!("Group actions - {}", name),
                )
            } else {
                (
                    ["Add group", "Remove group", "Modify group (members)"].as_slice(),
                    "Group actions",
                )
            };
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
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::GroupAddInput { name } => {
            let rect = crate::ui::components::centered_rect(48, 7, area);
            let msg = format!("New group name:\n{}", name);
            let p = Paragraph::new(msg).block(
                Block::default()
                    .title("Create group")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::GroupDeleteConfirm {
            selected,
            target_gid,
        } => {
            let rect = crate::ui::components::centered_rect(50, 7, area);
            let (name, gid) = if let Some(tgid) = target_gid {
                app.groups
                    .iter()
                    .find(|g| g.gid == tgid)
                    .map(|g| (g.name.clone(), g.gid))
                    .unwrap_or_else(|| (String::new(), 0))
            } else {
                app.groups
                    .get(app.selected_group_index)
                    .map(|g| (g.name.clone(), g.gid))
                    .unwrap_or_else(|| (String::new(), 0))
            };
            let mut body = format!("Delete group '{}' ?\n\n", name);
            // Show a caution if this looks like a system group
            if gid < 1000 && gid != 0 {
                body.push_str(&format!("WARNING: '{}' appears to be a system group (GID {}).\nDeleting may break the system.\n\n", name, gid));
            }
            let yes = if selected == 0 { "[Yes]" } else { " Yes " };
            let no = if selected == 1 { "[No]" } else { " No  " };
            body.push_str(&format!("  {}    {}", yes, no));
            let p = Paragraph::new(body).block(
                Block::default()
                    .title("Confirm delete")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::GroupModifyMenu {
            selected,
            target_gid,
        } => {
            let rect = crate::ui::components::centered_rect(40, 9, area);
            let options = ["Add member", "Remove member", "Rename group"];
            let mut text = String::new();
            for (idx, label) in options.iter().enumerate() {
                if idx == selected {
                    text.push_str(&format!("▶ {}\n", label));
                } else {
                    text.push_str(&format!("  {}\n", label));
                }
            }
            let title_suffix = if let Some(gid) = target_gid {
                app.groups
                    .iter()
                    .find(|g| g.gid == gid)
                    .map(|g| format!(" - {}", g.name))
                    .unwrap_or_default()
            } else {
                app.groups
                    .get(app.selected_group_index)
                    .map(|g| format!(" - {}", g.name))
                    .unwrap_or_default()
            };
            let p = Paragraph::new(text).block(
                Block::default()
                    .title(format!("Modify group{}", title_suffix))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::GroupRenameInput { name, target_gid } => {
            let rect = crate::ui::components::centered_rect(48, 7, area);
            let current = if let Some(gid) = target_gid {
                app.groups
                    .iter()
                    .find(|g| g.gid == gid)
                    .map(|g| g.name.clone())
                    .unwrap_or_default()
            } else {
                app.groups
                    .get(app.selected_group_index)
                    .map(|g| g.name.clone())
                    .unwrap_or_default()
            };
            let msg = format!("Current: {}\nNew name: {}", current, name);
            let p = Paragraph::new(msg).block(
                Block::default()
                    .title("Rename group")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            );
            f.render_widget(Clear, rect);
            f.render_widget(p, rect);
        }
        ModalState::GroupModifyAddMembers {
            selected,
            offset: _,
            selected_multi,
            ..
        } => {
            let users = &app.users_all;
            let width = (area.width.saturating_sub(10)).clamp(40, 60);
            let height = (area.height.saturating_sub(6)).clamp(8, 20);
            let rect = crate::ui::components::centered_rect(width, height, area);
            let visible_capacity = rect.height.saturating_sub(2) as usize;
            let total = users.len();
            let max_offset = total.saturating_sub(visible_capacity);
            let mut off = selected.saturating_sub(visible_capacity / 2);
            if off > max_offset {
                off = max_offset;
            }
            let start = off.min(total);
            let end = (start + visible_capacity).min(total);
            let slice = &users[start..end];
            let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
            for (i, u) in slice.iter().enumerate() {
                let abs_index = start + i;
                let focus = if abs_index == selected { "▶ " } else { "  " };
                let checked = if selected_multi.contains(&abs_index) {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(ListItem::new(format!(
                    "{}{}{} ({})",
                    focus, checked, u.name, u.uid
                )));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .title("Add member to group")
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
        ModalState::GroupModifyRemoveMembers {
            selected,
            offset: _,
            selected_multi,
            ..
        } => {
            let name = app
                .groups
                .get(app.selected_group_index)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let members = app
                .groups
                .get(app.selected_group_index)
                .map(|g| g.members.clone())
                .unwrap_or_default();
            let width = (area.width.saturating_sub(10)).clamp(40, 60);
            let height = (area.height.saturating_sub(6)).clamp(8, 20);
            let rect = crate::ui::components::centered_rect(width, height, area);
            let visible_capacity = rect.height.saturating_sub(2) as usize;
            let total = members.len();
            let max_offset = total.saturating_sub(visible_capacity);
            let mut off = selected.saturating_sub(visible_capacity / 2);
            if off > max_offset {
                off = max_offset;
            }
            let start = off.min(total);
            let end = (start + visible_capacity).min(total);
            let slice = &members[start..end];
            let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
            for (i, m) in slice.iter().enumerate() {
                let abs_index = start + i;
                let focus = if abs_index == selected { "▶ " } else { "  " };
                let checked = if selected_multi.contains(&abs_index) {
                    "[x] "
                } else {
                    "[ ] "
                };
                items.push(ListItem::new(format!("{}{}{}", focus, checked, m)));
            }
            let list = List::new(items)
                .block(
                    Block::default()
                        .title(format!("Remove member from '{}'", name))
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
        _ => {}
    }
}
