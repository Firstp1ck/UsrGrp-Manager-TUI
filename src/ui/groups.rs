//! Immutable groups-tab rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::app::{AppState, GroupsFocus, ModalState};

fn visible_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(3)).max(1)
}

pub fn render_groups_table(frame: &mut Frame, area: Rect, app: &AppState) {
    let per_page = visible_rows(area);
    let start = app.selected_group_index / per_page * per_page;
    let rows = app
        .groups
        .iter()
        .skip(start)
        .take(per_page)
        .enumerate()
        .map(|(offset, group)| {
            let selected = start + offset == app.selected_group_index;
            Row::new([
                Cell::from(group.gid.to_string()),
                Cell::from(group.name.clone()),
            ])
            .style(if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        Table::new(rows, [Constraint::Length(8), Constraint::Percentage(100)])
            .header(Row::new(["GID", "GROUP"]).style(Style::default().fg(app.theme.title)))
            .block(
                Block::default()
                    .title(if app.groups_focus == GroupsFocus::GroupsList {
                        "[Groups]"
                    } else {
                        "Groups"
                    })
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            ),
        area,
    );
}

pub fn render_group_details(frame: &mut Frame, area: Rect, app: &AppState) {
    let text = if let Some(group) = app.groups.get(app.selected_group_index) {
        let diagnostics = app.diagnostics.groups.get(&group.gid);
        let mtime = app
            .diagnostics
            .group_mtime_days
            .map_or("unavailable".to_owned(), |days| days.to_string());
        let protected = if crate::app::is_default_protected_group(group) {
            " protected by default policy"
        } else {
            ""
        };
        format!(
            "Group: {}\nGID: {} ({}){}\nPrimary members: {}\nSecondary members: {}\nOrphan members: {}\nShadow ({}) locked={} empty={} expired={}\n/etc/group mtime days: {}{}",
            group.name,
            group.gid,
            if group.gid < 1000 { "system" } else { "user" },
            protected,
            diagnostics.map_or(0, |value| value.primary_members),
            group.members.len(),
            diagnostics.map_or(0, |value| value.orphan_members),
            app.diagnostics.shadow.availability_label(),
            diagnostics.map_or(0, |value| value.locked_members),
            diagnostics.map_or(0, |value| value.empty_password_members),
            diagnostics.map_or(0, |value| value.expired_members),
            mtime,
            if diagnostics.is_some_and(|value| value.members_truncated) {
                "\nMember diagnostics bounded at 100000 records."
            } else {
                ""
            }
        )
    } else {
        "No group selected.".to_owned()
    };
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Group details")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        area,
    );
}

pub fn render_group_members(frame: &mut Frame, area: Rect, app: &AppState) {
    let members = app
        .groups
        .get(app.selected_group_index)
        .map_or(&[][..], |group| group.members.as_slice());
    let per_page = visible_rows(area);
    let start = app.selected_group_member_index / per_page * per_page;
    let rows = members
        .iter()
        .skip(start)
        .take(per_page)
        .enumerate()
        .map(|(offset, member)| {
            let selected = start + offset == app.selected_group_member_index;
            Row::new([Cell::from(member.clone())]).style(if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
        });
    frame.render_widget(
        Table::new(rows, [Constraint::Percentage(100)])
            .header(Row::new(["MEMBER"]).style(Style::default().fg(app.theme.title)))
            .block(
                Block::default()
                    .title(if app.groups_focus == GroupsFocus::Members {
                        "[Members]"
                    } else {
                        "Members"
                    })
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            ),
        area,
    );
}

pub fn render_group_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    let (title, body) = match modal {
        ModalState::GroupsActions {
            selected,
            target_gid,
        } => (
            "Group actions",
            if target_gid.is_some() {
                choices(&["Modify", "Delete"], *selected)
            } else {
                choices(&["Create", "Delete", "Modify"], *selected)
            },
        ),
        ModalState::GroupAddInput { name } => ("Create group", format!("Group name:\n{name}")),
        ModalState::GroupDeleteConfirm {
            selected,
            target_gid,
        } => {
            let group = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index));
            (
                "Confirm delete",
                format!(
                    "Delete {}?\n{} Yes   {} No",
                    group.map_or("<missing>", |group| group.name.as_str()),
                    marker(*selected, 0),
                    marker(*selected, 1)
                ),
            )
        }
        ModalState::GroupModifyMenu { selected, .. } => (
            "Modify group",
            choices(&["Add members", "Remove members", "Rename"], *selected),
        ),
        ModalState::GroupRenameInput { name, .. } => ("Rename group", format!("New name:\n{name}")),
        ModalState::GroupModifyAddMembers {
            selected,
            selected_multi,
            ..
        } => (
            "Add members",
            user_candidate_choices(
                app.users_all.iter().map(|user| user.name.as_str()),
                *selected,
                selected_multi,
            ),
        ),
        ModalState::GroupModifyRemoveMembers {
            selected,
            target_gid,
            selected_multi,
            ..
        } => {
            let members = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index))
                .map_or(&[][..], |group| group.members.as_slice());
            (
                "Remove members",
                user_candidate_choices(
                    members.iter().map(String::as_str),
                    *selected,
                    selected_multi,
                ),
            )
        }
        _ => return,
    };
    let rect = crate::ui::components::centered_rect(
        area.width.saturating_sub(8).min(70),
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

fn choices(options: &[&str], selected: usize) -> String {
    options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("{} {option}", marker(selected, index)))
        .collect::<Vec<_>>()
        .join("\n")
}
const MAX_MODAL_CANDIDATES: usize = 1024;
const MAX_MODAL_ROWS: usize = 12;

fn user_candidate_choices<'a>(
    users: impl Iterator<Item = &'a str>,
    selected: usize,
    checked: &[usize],
) -> String {
    let start = selected / MAX_MODAL_ROWS * MAX_MODAL_ROWS;
    users
        .take(MAX_MODAL_CANDIDATES)
        .enumerate()
        .skip(start)
        .take(MAX_MODAL_ROWS)
        .map(|(index, user)| {
            format!(
                "{} [{}] {user}",
                marker(selected, index),
                if checked.contains(&index) { "x" } else { " " }
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn marker(selected: usize, index: usize) -> &'static str {
    if selected == index { "▶" } else { " " }
}
