//! Immutable groups-tab rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::{
    app::{
        AppState, CandidateList, GroupDiagnostics, GroupsFilter, GroupsFocus, InputMode,
        MAX_NAME_BYTES, MEMBER_ACTIONS, ModalState, civil_date_label,
    },
    search::ShadowState,
    sys::SystemGroup,
    ui::components::{
        CandidateView, DetailLine, DetailPart, INPUT_CURSOR, MENU_FOOTER, TEXT_FOOTER,
        YES_NO_FOOTER, byte_budget, candidate_dialog, details_plain_text, marker, no_match_text,
        nothing_loaded_lines, push_filter_labels, push_sort_label, render_empty_state,
        render_modal_frame, row_style, search_title, stale_line, text_lines, with_error_line,
    },
    ui::layout::table_rows,
};

/// Groups table title: focus marker, the search query while it applies, and
/// how many groups are listed out of all loaded, a non-default row order,
/// then the active filter (`[Groups]  /dev  3 of 89  sort: name  [system]`).
pub fn groups_table_title(app: &AppState) -> String {
    let name = if app.groups_focus == GroupsFocus::GroupsList {
        "[Groups]"
    } else {
        "Groups"
    };
    let mut title = search_title(
        name,
        &app.groups_query,
        app.input_mode == InputMode::SearchGroups,
        app.groups.len(),
        app.groups_all.len(),
    );
    push_sort_label(&mut title, app.groups_sort.label());
    push_filter_labels(&mut title, groups_filter_label(app).as_slice());
    title
}

/// Short name of the active groups filter, if any.
fn groups_filter_label(app: &AppState) -> Option<&'static str> {
    match app.groups_filter {
        None => None,
        Some(GroupsFilter::OnlyUserGids) => Some("user"),
        Some(GroupsFilter::OnlySystemGids) => Some("system"),
    }
}

/// What the groups table says when it has no rows: nothing was loaded, or
/// the search and filter hide every group. Either way the stale label with
/// its reason follows when the last refresh failed.
pub fn groups_empty_lines(app: &AppState) -> Vec<String> {
    if app.groups_all.is_empty() {
        nothing_loaded_lines(app, "groups")
    } else {
        let mut lines = vec![no_match_text(
            "groups",
            &app.groups_query,
            groups_filter_label(app).as_slice(),
        )];
        lines.extend(stale_line(app));
        lines
    }
}

pub fn render_groups_table(frame: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .title(groups_table_title(app))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    if app.groups.is_empty() {
        render_empty_state(frame, area, app, block, groups_empty_lines(app));
        return;
    }
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
            .style(row_style(app, selected))
        });
    frame.render_widget(
        Table::new(rows, [Constraint::Length(8), Constraint::Percentage(100)])
            .header(Row::new(["GID", "GROUP"]).style(Style::default().fg(app.theme.title)))
            .block(block),
        area,
    );
}

/// Details pane lines for the selected group, or a placeholder when
/// there is none.
pub fn selected_group_details(app: &AppState) -> Vec<DetailLine> {
    app.groups.get(app.selected_group_index).map_or_else(
        || vec![vec![DetailPart::Text("No group selected.".to_owned())]],
        |group| group_details(app, group),
    )
}

/// Details pane text for `group`, one fact per line.
pub fn group_details_text(app: &AppState, group: &SystemGroup) -> String {
    details_plain_text(&group_details(app, group))
}

/// Details pane lines for `group`, one fact per line. Anything the last
/// refresh could not read says "unavailable" and is a muted
/// [`DetailPart::Missing`]; account data is always [`DetailPart::Text`].
pub fn group_details(app: &AppState, group: &SystemGroup) -> Vec<DetailLine> {
    let text = |value: String| DetailPart::Text(value);
    let diagnostics = app.diagnostics.groups.get(&group.gid);
    let count = |field: fn(&GroupDiagnostics) -> usize| diagnostics.map_or(0, field);
    let kind = if group.gid < 1000 { "system" } else { "user" };
    let protected = if crate::app::is_default_protected_group(group) {
        ", protected"
    } else {
        ""
    };
    let passwords = match app.diagnostics.shadow {
        ShadowState::Known(_) => text(format!(
            "{} locked, {} empty, {} expired (shadow known)",
            count(|value| value.locked_members),
            count(|value| value.empty_password_members),
            count(|value| value.expired_members)
        )),
        ShadowState::Unavailable { .. } => DetailPart::Missing("unavailable".to_owned()),
    };
    let mut lines = vec![
        vec![text(format!(
            "Group: {} (GID {}, {kind}{protected})",
            group.name, group.gid
        ))],
        vec![text(format!(
            "Members: {} primary, {} secondary, {} orphaned",
            count(|value| value.primary_members),
            group.members.len(),
            count(|value| value.orphan_members)
        ))],
        vec![text("Password status of members: ".to_owned()), passwords],
        vec![
            text("/etc/group changed: ".to_owned()),
            changed_label(
                app.diagnostics.group_mtime_days,
                app.diagnostics.refreshed_days,
            ),
        ],
    ];
    if diagnostics.is_some_and(|value| value.members_truncated) {
        lines.push(vec![text(
            "Member diagnostics bounded at 100000 records.".to_owned(),
        )]);
    }
    lines
}

/// `2026-09-01 (27 days ago)` for a file changed on day `changed`, counted
/// back from the refresh day. No age is shown when the change day is later
/// than the refresh day.
fn changed_label(changed: Option<u64>, refreshed: u64) -> DetailPart {
    let Some(changed) = changed else {
        return DetailPart::Missing("unavailable".to_owned());
    };
    let date = civil_date_label(i64::try_from(changed).unwrap_or(i64::MAX));
    DetailPart::Text(match refreshed.checked_sub(changed) {
        Some(0) => format!("{date} (today)"),
        Some(1) => format!("{date} (1 day ago)"),
        Some(days) => format!("{date} ({days} days ago)"),
        None => date,
    })
}

pub fn render_group_members(frame: &mut Frame, area: Rect, app: &AppState) {
    let group = app.groups.get(app.selected_group_index);
    let members = group.map_or(&[][..], |group| group.members.as_slice());
    let block = Block::default()
        .title(if app.groups_focus == GroupsFocus::Members {
            "[Members]"
        } else {
            "Members"
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    if members.is_empty() {
        let text = if group.is_some() {
            "No members"
        } else {
            "No group selected"
        };
        render_empty_state(frame, area, app, block, vec![text.to_owned()]);
        return;
    }
    let per_page = table_rows(area);
    let start = app.selected_group_member_index / per_page * per_page;
    let rows = members
        .iter()
        .skip(start)
        .take(per_page)
        .enumerate()
        .map(|(offset, member)| {
            let selected = start + offset == app.selected_group_member_index;
            Row::new([Cell::from(member.clone())]).style(row_style(app, selected))
        });
    frame.render_widget(
        Table::new(rows, [Constraint::Percentage(100)])
            .header(Row::new(["MEMBER"]).style(Style::default().fg(app.theme.title)))
            .block(block),
        area,
    );
}

pub fn render_group_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    // The focus row indexes the body's lines: the highlighted choice or the
    // edited value.
    let (title, body, focus, footer) = match modal {
        // The same two rows from the groups list and from Member of; new
        // groups come from the NewUser key.
        ModalState::GroupsActions { selected, .. } => (
            "Group actions",
            choices(&["Modify", "Delete"], *selected),
            Some(*selected),
            MENU_FOOTER,
        ),
        // The first line names the member; the choices follow it.
        ModalState::MemberActions {
            selected,
            username,
            groupname,
            ..
        } => (
            "Member actions",
            format!(
                "{username} in {groupname}\n{}",
                choices(&MEMBER_ACTIONS, *selected)
            ),
            Some(*selected + 1),
            MENU_FOOTER,
        ),
        ModalState::GroupAddInput { name, .. } => (
            "Create group",
            format!(
                "Group name:\n{name}{INPUT_CURSOR}  {}",
                byte_budget(name, MAX_NAME_BYTES)
            ),
            Some(1),
            TEXT_FOOTER,
        ),
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
                Some(1),
                YES_NO_FOOTER,
            )
        }
        ModalState::GroupModifyMenu { selected, .. } => (
            "Modify group",
            choices(&["Add members", "Remove members", "Rename"], *selected),
            Some(*selected),
            MENU_FOOTER,
        ),
        ModalState::GroupRenameInput { name, .. } => (
            "Rename group",
            format!(
                "New name:\n{name}{INPUT_CURSOR}  {}",
                byte_budget(name, MAX_NAME_BYTES)
            ),
            Some(1),
            TEXT_FOOTER,
        ),
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
            let (title, body, focus, footer) = candidate_dialog(
                app,
                &CandidateView {
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
                },
            );
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(frame, area, app, &title, body, focus, footer);
            return;
        }
        _ => return,
    };
    let body = with_error_line(text_lines(body), modal.form_error());
    render_modal_frame(frame, area, app, title, body, focus, footer);
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

#[cfg(test)]
mod tests {
    use super::*;

    /// One loaded group `staff` and a search that matches none.
    fn filtered_out_app() -> AppState {
        let mut app = AppState::new();
        app.groups_all = vec![SystemGroup {
            gid: 100,
            name: "staff".to_owned(),
            members: vec![],
        }];
        app.groups_query = "zzz".to_owned();
        app.sort_and_filter();
        assert!(app.groups.is_empty());
        app
    }

    #[test]
    fn filtered_out_groups_show_the_stale_reason_after_a_failed_refresh() {
        let mut app = filtered_out_app();
        app.last_refresh_label = "22:13:20 UTC".to_owned();
        app.diagnostics.stale_reason = Some("boom".to_owned());
        assert_eq!(
            groups_empty_lines(&app),
            ["No groups match /zzz", "STALE since 22:13:20 UTC: boom"]
        );

        app.diagnostics.stale_reason = None;
        assert_eq!(groups_empty_lines(&app), ["No groups match /zzz"]);
    }
}
