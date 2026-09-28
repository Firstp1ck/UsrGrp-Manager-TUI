//! Immutable groups-tab rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{AppState, CandidateList, GroupsFocus, InputMode, ModalState},
    ui::components::{
        CandidateView, MENU_FOOTER, TEXT_FOOTER, YES_NO_FOOTER, candidate_dialog, marker,
        render_modal_frame, search_title, text_lines, with_error_line,
    },
    ui::layout::table_rows,
};

/// Groups table title: focus marker, the search query while it applies, and
/// how many groups are listed out of all loaded (`[Groups]  /dev  3 of 89`).
pub fn groups_table_title(app: &AppState) -> String {
    let name = if app.groups_focus == GroupsFocus::GroupsList {
        "[Groups]"
    } else {
        "Groups"
    };
    search_title(
        name,
        &app.groups_query,
        app.input_mode == InputMode::SearchGroups,
        app.groups.len(),
        app.groups_all.len(),
    )
}

pub fn render_groups_table(frame: &mut Frame, area: Rect, app: &AppState) {
    let per_page = table_rows(area);
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
                    .title(groups_table_title(app))
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
    let per_page = table_rows(area);
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
    let (title, body, footer) = match modal {
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
            MENU_FOOTER,
        ),
        ModalState::GroupAddInput { name, .. } => {
            ("Create group", format!("Group name:\n{name}"), TEXT_FOOTER)
        }
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
                YES_NO_FOOTER,
            )
        }
        ModalState::GroupModifyMenu { selected, .. } => (
            "Modify group",
            choices(&["Add members", "Remove members", "Rename"], *selected),
            MENU_FOOTER,
        ),
        ModalState::GroupRenameInput { name, .. } => {
            ("Rename group", format!("New name:\n{name}"), TEXT_FOOTER)
        }
        ModalState::GroupModifyAddMembers {
            selected,
            target_gid,
            selected_multi,
            filter,
            filtering,
            ..
        }
        | ModalState::GroupModifyRemoveMembers {
            selected,
            target_gid,
            selected_multi,
            filter,
            filtering,
            ..
        } => {
            let adding = matches!(modal, ModalState::GroupModifyAddMembers { .. });
            let list = member_candidates(app, *target_gid, adding, filter);
            let (title, body, footer) = candidate_dialog(&CandidateView {
                name: if adding {
                    "Add members"
                } else {
                    "Remove members"
                },
                list: &list,
                selected: *selected,
                ticked: selected_multi,
                filter,
                filtering: *filtering,
            });
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(frame, area, app, &title, body, footer);
            return;
        }
        _ => return,
    };
    let body = with_error_line(text_lines(body), modal.form_error());
    render_modal_frame(frame, area, app, title, body, footer);
}

fn choices(options: &[&str], selected: usize) -> String {
    options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("{} {option}", marker(selected, index)))
        .collect::<Vec<_>>()
        .join("\n")
}
/// Candidates for the members dialog of the targeted group (or the
/// selected one), resolved the same way as `handle_group_members`.
fn member_candidates(
    app: &AppState,
    target_gid: Option<u32>,
    adding: bool,
    filter: &str,
) -> CandidateList {
    target_gid
        .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
        .or_else(|| app.groups.get(app.selected_group_index))
        .map_or_else(CandidateList::default, |group| {
            app.group_member_candidate_list(group.gid, adding, filter)
        })
}
