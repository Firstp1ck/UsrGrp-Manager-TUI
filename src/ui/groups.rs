//! Groups screen rendering and modals.
//!
//! Contains the groups table, details panel, members list, and group-related
//! modal dialogs for add/remove/rename actions.
//!
use ratatui::Frame;
use ratatui::layout::{Constraint, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Clear, List, ListItem, Paragraph, Row, Table};

use crate::app::{AppState, ModalState};

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
        Row::new(vec![
            Cell::from(g.gid.to_string()),
            Cell::from(g.name.clone()),
        ])
        .style(style)
    });

    let widths = [Constraint::Length(8), Constraint::Percentage(100)];
    let header = Row::new(vec!["GID", "GROUP"]).style(
        Style::default()
            .fg(app.theme.title)
            .add_modifier(Modifier::BOLD),
    );

    let groups_title = if let Some(g) = app.groups.get(app.selected_group_index) {
        format!("Groups - {}", g.name)
    } else {
        "Groups".to_string()
    };
    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title(groups_title)
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

/// Render the selected group's summary details.
pub fn render_group_details(f: &mut Frame, area: Rect, app: &AppState) {
    let group = app.groups.get(app.selected_group_index);
    let (name, gid, members) = match group {
        Some(g) => (g.name.clone(), g.gid, g.members.len()),
        None => (String::new(), 0, 0),
    };
    let text = format!("Group: {name}\nGID: {gid}\nMembers: {members}");
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

    let rows = slice
        .iter()
        .map(|m| Row::new(vec![Cell::from(m.clone())]).style(Style::default()));
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
                .title("Group Members")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .column_spacing(1);
    f.render_widget(table, area);
}

/// Render group-related modal dialogs based on state.
pub fn render_group_modal(f: &mut Frame, area: Rect, app: &mut AppState, state: &ModalState) {
    match state.clone() {
        ModalState::GroupsActions { selected, .. } => {
            let rect = crate::ui::components::centered_rect(36, 8, area);
            let options = ["Add group", "Remove group", "Modify group (members)"];
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
                    .title("Group actions")
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
        ModalState::GroupDeleteConfirm { selected } => {
            let rect = crate::ui::components::centered_rect(50, 7, area);
            let name = app
                .groups
                .get(app.selected_group_index)
                .map(|g| g.name.clone())
                .unwrap_or_default();
            let mut body = format!("Delete group '{}' ?\n\n", name);
            // Show a caution if this looks like a system group
            if let Some(g) = app.groups.get(app.selected_group_index)
                && g.gid < 1000
            {
                body.push_str(&format!("WARNING: '{}' appears to be a system group (GID {}).\nDeleting may break the system.\n\n", g.name, g.gid));
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
        ModalState::GroupModifyMenu { selected, .. } => {
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
            let p = Paragraph::new(text).block(
                Block::default()
                    .title("Modify group")
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
