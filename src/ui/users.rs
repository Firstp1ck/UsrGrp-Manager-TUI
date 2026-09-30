//! Immutable users-tab rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::Style,
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Row, Table},
};

use crate::{
    app::{
        AppState, BULK_ACTIONS, CandidateList, DELETE_ROW_HOME, DELETE_ROW_NO, DELETE_ROW_YES,
        InputMode, MAX_EXPIRY_CHARS, MAX_HOME_BYTES, MAX_NAME_BYTES, ModalState, ModifyField,
        SecretInput, UsersFocus, civil_date_label,
    },
    search::AccountShadowState,
    sys::SystemUser,
    ui::components::{
        CandidateView, DetailLine, DetailPart, FORM_FOOTER, INPUT_CURSOR, LIST_FOOTER, MENU_FOOTER,
        PASSWORD_FORM_FOOTER, TEXT_FOOTER, YES_NO_FOOTER, byte_budget, candidate_dialog,
        details_plain_text, input_cursor, marker, no_match_text, nothing_loaded_lines,
        push_filter_labels, push_sort_label, render_empty_state, render_modal_frame, row_style,
        search_title, shadow_skipped_note, stale_line, text_lines, with_error_line,
    },
    ui::layout::table_rows,
};

/// Users table title: focus marker, the search query while it applies, and
/// how many users are listed out of all loaded, how many are marked, a
/// non-default row order, then the active filters and any skipped shadow
/// filter (`[Users]  /alice  12 of 45  3 marked  sort: name  [human, locked]`).
pub fn users_table_title(app: &AppState) -> String {
    let name = if app.users_focus == UsersFocus::UsersList {
        "[Users]"
    } else {
        "Users"
    };
    let mut title = search_title(
        name,
        &app.users_query,
        app.input_mode == InputMode::SearchUsers,
        app.users.len(),
        app.users_all.len(),
    );
    if !app.marked_users.is_empty() {
        title.push_str(&format!("  {} marked", app.marked_users.len()));
    }
    push_sort_label(&mut title, app.users_sort.label());
    push_filter_labels(&mut title, &app.active_user_filter_labels());
    if let Some(note) = shadow_skipped_note(app) {
        title.push_str("  ");
        title.push_str(&note);
    }
    title
}

/// What the users table says when it has no rows: nothing was loaded, or
/// the search and filters hide every user. Either way the stale label with
/// its reason follows when the last refresh failed.
pub fn users_empty_lines(app: &AppState) -> Vec<String> {
    if app.users_all.is_empty() {
        nothing_loaded_lines(app, "users")
    } else {
        let mut lines = vec![no_match_text(
            "users",
            &app.users_query,
            &app.active_user_filter_labels(),
        )];
        lines.extend(stale_line(app));
        lines
    }
}

pub fn render_users_table(frame: &mut Frame, area: Rect, app: &AppState) {
    let block = Block::default()
        .title(users_table_title(app))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    if app.users.is_empty() {
        render_empty_state(frame, area, app, block, users_empty_lines(app));
        return;
    }
    let per_page = table_rows(area);
    let start = app.selected_user_index / per_page * per_page;
    // With marks, every name gets a two-column prefix so names stay aligned.
    let marking = !app.marked_users.is_empty();
    let rows = app
        .users
        .iter()
        .skip(start)
        .take(per_page)
        .enumerate()
        .map(|(offset, user)| {
            let selected = start + offset == app.selected_user_index;
            Row::new(vec![
                Cell::from(user.uid.to_string()),
                Cell::from(match (marking, app.is_marked(user)) {
                    (false, _) => user.name.clone(),
                    (true, true) => format!("* {}", user.name),
                    (true, false) => format!("  {}", user.name),
                }),
                Cell::from(user.primary_gid.to_string()),
                Cell::from(user.home_dir.clone()),
                Cell::from(user.shell.clone()),
            ])
            .style(row_style(app, selected))
        });
    frame.render_widget(
        Table::new(
            rows,
            [
                Constraint::Length(7),
                Constraint::Length(18),
                Constraint::Length(7),
                Constraint::Percentage(38),
                Constraint::Percentage(30),
            ],
        )
        .header(
            Row::new(["UID", "USER", "GID", "HOME", "SHELL"])
                .style(Style::default().fg(app.theme.title)),
        )
        .block(block),
        area,
    );
}

/// Details pane lines for the selected user, or a placeholder when
/// there is none.
pub fn selected_user_details(app: &AppState) -> Vec<DetailLine> {
    app.users.get(app.selected_user_index).map_or_else(
        || vec![vec![DetailPart::Text("No user selected.".to_owned())]],
        |user| user_details(app, user),
    )
}

/// Details pane text for `user`, one fact per line.
pub fn user_details_text(app: &AppState, user: &SystemUser) -> String {
    details_plain_text(&user_details(app, user))
}

/// Details pane lines for `user`, one fact per line. Anything the last
/// refresh could not read says "unavailable" and is a muted
/// [`DetailPart::Missing`]; account data is always [`DetailPart::Text`].
pub fn user_details(app: &AppState, user: &SystemUser) -> Vec<DetailLine> {
    const UNAVAILABLE: &str = "unavailable";
    let text = |value: &str| DetailPart::Text(value.to_owned());
    let missing = || DetailPart::Missing(UNAVAILABLE.to_owned());
    let name = match user.full_name.as_deref().map(str::trim) {
        Some(full_name) if !full_name.is_empty() => format!("{} ({full_name})", user.name),
        _ => user.name.clone(),
    };
    let primary_group = app
        .groups_all
        .iter()
        .find(|group| group.gid == user.primary_gid)
        .map_or("no such group", |group| group.name.as_str());

    let home = app.diagnostics.homes.get(&user.name);
    let exists = home.and_then(|diagnostic| diagnostic.exists);
    let permissions = match exists {
        Some(false) => text("-"),
        _ => home
            .and_then(|diagnostic| diagnostic.permissions.as_deref())
            .map_or_else(missing, text),
    };
    let count = |value: Option<usize>| value.map_or_else(missing, |n| text(&n.to_string()));
    let keys = count(home.and_then(|diagnostic| diagnostic.authorized_key_count));
    let processes = count(home.and_then(|diagnostic| diagnostic.process_count));

    let listed = app
        .account_snapshot
        .as_ref()
        .map_or_else(missing, |snapshot| {
            text(yes_no(
                snapshot
                    .shells
                    .iter()
                    .any(|shell| shell.as_ref() == user.shell),
            ))
        });

    let (password, dates) = match app.diagnostics.shadow.account_status(&user.name) {
        AccountShadowState::Known(status) => (
            vec![text(&format!(
                "locked {}  empty {}  expired {}",
                yes_no(status.locked),
                yes_no(status.no_password),
                yes_no(status.expired)
            ))],
            vec![
                text("Last change: "),
                last_change_label(status.last_change_days),
                text(&format!(
                    "  Expires: {}",
                    status
                        .expire_abs_days
                        .map_or("never".to_owned(), civil_date_label)
                )),
            ],
        ),
        AccountShadowState::Unknown { .. } => (
            vec![DetailPart::Missing(format!(
                "{UNAVAILABLE} (no shadow record)"
            ))],
            vec![
                text("Last change: "),
                missing(),
                text("  Expires: "),
                missing(),
            ],
        ),
        AccountShadowState::Unavailable { .. } => (
            vec![missing()],
            vec![
                text("Last change: "),
                missing(),
                text("  Expires: "),
                missing(),
            ],
        ),
    };

    let sudo = if is_sudo_member(app, user) {
        "member"
    } else {
        "not member"
    };
    let mut password_line = vec![text("Password: ")];
    password_line.extend(password);
    vec![
        vec![text(&format!("User: {name}"))],
        vec![text(&format!(
            "UID {}  GID {} ({primary_group})",
            user.uid, user.primary_gid
        ))],
        vec![
            text(&format!("Home: {}  exists: ", user.home_dir)),
            exists.map_or_else(missing, |value| text(yes_no(value))),
            text("  perms: "),
            permissions,
        ],
        vec![
            text(&format!("Shell: {}  listed: ", user.shell)),
            listed,
            text(&format!(
                "  interactive: {}",
                yes_no(is_interactive_shell(&user.shell))
            )),
        ],
        password_line,
        dates,
        vec![text(&format!("Sudo group ({}): {sudo}", app.sudo_group))],
        vec![text("SSH keys: "), keys],
        vec![text("Processes: "), processes],
    ]
}

fn yes_no(value: bool) -> &'static str {
    if value { "yes" } else { "no" }
}

/// A shell that only refuses logins (`nologin`, `false`) is not interactive.
fn is_interactive_shell(shell: &str) -> bool {
    let program = shell.rsplit('/').next().unwrap_or(shell);
    !matches!(program, "nologin" | "false")
}

/// Shadow last-change day: day zero forces a change at the next login and an
/// empty field turns password aging off.
fn last_change_label(days: Option<i64>) -> DetailPart {
    match days {
        Some(0) => DetailPart::Text("must change at next login".to_owned()),
        Some(days) => DetailPart::Text(civil_date_label(days)),
        None => DetailPart::Missing("not set".to_owned()),
    }
}

/// Whether `user` is in the configured sudo group, as a listed member or
/// through their primary group.
fn is_sudo_member(app: &AppState, user: &SystemUser) -> bool {
    app.groups_all
        .iter()
        .filter(|group| group.name == app.sudo_group)
        .any(|group| group.gid == user.primary_gid || group.members.contains(&user.name))
}

pub fn render_user_groups(frame: &mut Frame, area: Rect, app: &AppState) {
    let groups = member_groups(app);
    let block = Block::default()
        .title(if app.users_focus == UsersFocus::MemberOf {
            "[Member of]"
        } else {
            "Member of"
        })
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border));
    if groups.is_empty() {
        let text = if app.users.get(app.selected_user_index).is_some() {
            "No supplementary groups"
        } else {
            "No user selected"
        };
        render_empty_state(frame, area, app, block, vec![text.to_owned()]);
        return;
    }
    let per_page = table_rows(area);
    let start = app.selected_user_group_index / per_page * per_page;
    let rows = groups
        .iter()
        .skip(start)
        .take(per_page)
        .enumerate()
        .map(|(offset, group)| {
            let selected = start + offset == app.selected_user_group_index;
            Row::new([
                Cell::from(group.gid.to_string()),
                Cell::from(group.name.clone()),
            ])
            .style(row_style(app, selected))
        });
    frame.render_widget(
        Table::new(rows, [Constraint::Length(7), Constraint::Percentage(100)])
            .header(Row::new(["GID", "GROUP"]).style(Style::default().fg(app.theme.title)))
            .block(block),
        area,
    );
}

fn member_groups(app: &AppState) -> Vec<&crate::sys::SystemGroup> {
    app.selected_user_groups()
}

pub fn render_user_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    // The focus row indexes the body's lines: the highlighted choice or the
    // edited value.
    let (title, body, focus, footer) = match modal {
        ModalState::Actions { selected } => (
            "Actions",
            choices(&["Modify", "Delete"], *selected),
            Some(*selected),
            MENU_FOOTER,
        ),
        ModalState::BulkActions { selected, .. } => {
            let mut body = vec![Line::from(format!(
                "Marked: {}",
                crate::app::describe_names(&app.marked_usernames())
            ))];
            body.extend(text_lines(choices(&BULK_ACTIONS, *selected)));
            let body = with_error_line(body, modal.form_error());
            let title = format!("Bulk actions ({} marked)", app.marked_users.len());
            render_modal_frame(
                frame,
                area,
                app,
                &title,
                body,
                Some(1 + *selected),
                MENU_FOOTER,
            );
            return;
        }
        ModalState::BulkGroupPicker {
            adding,
            selected,
            groups,
            ..
        } => {
            let mut body = vec![Line::from(format!(
                "Marked: {}",
                crate::app::describe_names(&app.marked_usernames())
            ))];
            if !*adding {
                body.push(Line::from("Only groups with a marked member are listed."));
            }
            let focus = (!groups.names.is_empty()).then(|| body.len() + selected % MAX_MODAL_ROWS);
            body.extend(group_choice_page(app, groups, *selected, None));
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(
                frame,
                area,
                app,
                if *adding {
                    "Add marked users to a group"
                } else {
                    "Remove marked users from a group"
                },
                body,
                focus,
                LIST_FOOTER,
            );
            return;
        }
        ModalState::ModifyMenu { selected } => {
            let labels: Vec<&str> = app
                .modify_menu_items()
                .into_iter()
                .map(|item| item.label())
                .collect();
            (
                "Modify user",
                choices(&labels, *selected),
                Some(*selected),
                MENU_FOOTER,
            )
        }
        ModalState::ModifyDetailsMenu { selected } => (
            "Modify details",
            choices(
                &[
                    "Username",
                    "Full name",
                    "Shell",
                    "Primary group",
                    "Home directory",
                ],
                *selected,
            ),
            Some(*selected),
            MENU_FOOTER,
        ),
        ModalState::ModifyTextInput { field, value, .. } => (
            match field {
                ModifyField::Username => "Change username",
                ModifyField::Fullname => "Change full name",
            },
            match field {
                ModifyField::Username => format!(
                    "{value}{INPUT_CURSOR}  {}\nThe home directory keeps its current name.",
                    byte_budget(value, field.max_bytes())
                ),
                ModifyField::Fullname => format!(
                    "{value}{INPUT_CURSOR}  {}",
                    byte_budget(value, field.max_bytes())
                ),
            },
            Some(0),
            TEXT_FOOTER,
        ),
        ModalState::ModifyShell {
            selected,
            shells,
            current,
            current_unlisted,
            current_label,
            ..
        } => {
            let labels: Vec<String> = shells
                .iter()
                .enumerate()
                .map(|(index, shell)| {
                    if Some(index) == *current && *current_unlisted {
                        format!("{shell} (current, not in /etc/shells)")
                    } else if Some(index) == *current {
                        format!("{shell} (current)")
                    } else {
                        shell.clone()
                    }
                })
                .collect();
            // A current shell that is only a label cannot be chosen, so it
            // sits above the list as a note.
            let mut body = String::new();
            if let Some(label) = current_label {
                body.push_str(&format!("Current shell: {label}\n"));
            }
            let note_rows = usize::from(current_label.is_some());
            body.push_str(&choices_page(&labels, *selected));
            (
                "Select shell",
                body,
                Some(note_rows + selected % MAX_MODAL_ROWS),
                LIST_FOOTER,
            )
        }
        ModalState::ModifyPrimaryGroup {
            selected,
            groups,
            current,
            current_gid,
            ..
        } => {
            // A primary GID without a group entry cannot be marked in the
            // list, so it sits above it as a note.
            let mut body = Vec::new();
            if let Some(gid) = current_gid {
                body.push(Line::from(format!(
                    "Current primary group: GID {gid} (no group entry)"
                )));
            }
            let focus = (!groups.names.is_empty()).then(|| body.len() + selected % MAX_MODAL_ROWS);
            body.extend(group_choice_page(app, groups, *selected, *current));
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(
                frame,
                area,
                app,
                "Select primary group",
                body,
                focus,
                LIST_FOOTER,
            );
            return;
        }
        ModalState::ModifyHome {
            selected,
            value,
            move_contents,
            current,
            ..
        } => {
            let body = text_lines(format!(
                "Current: {current}\n{} Path: {value}{}  {}\n{} [{}] move contents",
                marker(*selected, 0),
                input_cursor(*selected, 0),
                byte_budget(value, MAX_HOME_BYTES),
                marker(*selected, 1),
                if *move_contents { "x" } else { " " },
            ));
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(
                frame,
                area,
                app,
                "Change home directory",
                body,
                Some(1 + *selected),
                FORM_FOOTER,
            );
            return;
        }
        ModalState::ModifyGroupsAdd {
            selected,
            selected_multi,
            filter,
            filtering,
            ..
        }
        | ModalState::ModifyGroupsRemove {
            selected,
            selected_multi,
            filter,
            filtering,
            ..
        } => {
            let adding = matches!(modal, ModalState::ModifyGroupsAdd { .. });
            let list = group_candidates(app, adding, filter);
            let (title, body, focus, footer) = candidate_dialog(
                app,
                &CandidateView {
                    name: if adding {
                        "Add groups"
                    } else {
                        "Remove groups"
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
        ModalState::ModifyPasswordMenu { selected } => (
            "Password",
            choices(
                &[
                    "Set or change password",
                    "Force change at next login",
                    "Account expiry date",
                ],
                *selected,
            ),
            Some(*selected),
            MENU_FOOTER,
        ),
        ModalState::ModifyExpiry { value, current, .. } => (
            "Account expiry date",
            format!(
                "Current: {}\nExpires: {value}{INPUT_CURSOR}  {}\nEmpty = never expires. Format YYYY-MM-DD.",
                match current {
                    Some(Some(date)) => date.as_str(),
                    Some(None) => "never",
                    None => "unknown",
                },
                byte_budget(value, MAX_EXPIRY_CHARS),
            ),
            Some(1),
            TEXT_FOOTER,
        ),
        ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
            reveal,
            ..
        } => {
            let (shown, shown_confirm) = (password.display(*reveal), confirm.display(*reveal));
            let mut body = password_rows(
                *selected,
                (0, "New password", password, &shown),
                (1, "Confirm", confirm, &shown_confirm),
            );
            body.extend(text_lines(format!(
                "{} [{}] Must change at next login\n{} Submit",
                marker(*selected, 2),
                if *must_change { "x" } else { " " },
                marker(*selected, 3)
            )));
            let focus = password_form_focus(*selected, 0, password);
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(
                frame,
                area,
                app,
                "Set password",
                body,
                Some(focus),
                PASSWORD_FORM_FOOTER,
            );
            return;
        }
        ModalState::DeleteConfirm { .. } => {
            let (body, focus) = delete_confirm_body(modal);
            render_modal_frame(
                frame,
                area,
                app,
                "Confirm delete",
                body,
                focus,
                "↑↓ row  ←→ choose  Space tick  y/n  Enter confirm  Esc cancel",
            );
            return;
        }
        ModalState::ConfirmRemoveUserFromGroup {
            selected,
            username,
            groupname,
        } => (
            "Confirm membership removal",
            format!(
                "Remove {username} from {groupname}?\n{} Yes   {} No",
                marker(*selected, 0),
                marker(*selected, 1)
            ),
            Some(1),
            YES_NO_FOOTER,
        ),
        ModalState::UserAddInput {
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
            reveal,
            ..
        } => {
            let (shown, shown_confirm) = (password.display(*reveal), confirm.display(*reveal));
            let mut body = text_lines(format!(
                "{} Username: {name}{}  {}",
                marker(*selected, 0),
                input_cursor(*selected, 0),
                byte_budget(name, MAX_NAME_BYTES),
            ));
            body.extend(password_rows(
                *selected,
                (1, "Password", password, &shown),
                (2, "Confirm", confirm, &shown_confirm),
            ));
            // A blocked group has no checkbox. One ticked before the block
            // appeared keeps its box so it can still be cleared.
            let group = &app.sudo_group;
            let (sudo_row, blocked) = match (app.membership_block(group), *add_to_wheel) {
                (Some(reason), false) => (
                    format!("    Add to sudo group ({group}): blocked, {reason}"),
                    true,
                ),
                (Some(reason), true) => (
                    format!("[x] Add to sudo group ({group}): blocked, {reason}"),
                    true,
                ),
                (None, ticked) => (
                    format!(
                        "[{}] Add to sudo group ({group})",
                        if ticked { "x" } else { " " }
                    ),
                    false,
                ),
            };
            // A blocked sudo row is drawn in the muted color.
            let sudo_style = if blocked {
                Style::default().fg(app.theme.muted)
            } else {
                Style::default()
            };
            body.push(Line::from(format!(
                "{} [{}] Create home",
                marker(*selected, 3),
                if *create_home { "x" } else { " " }
            )));
            body.push(Line::styled(
                format!("{} {sudo_row}", marker(*selected, 4)),
                sudo_style,
            ));
            body.push(Line::from(format!("{} Submit", marker(*selected, 5))));
            let focus = password_form_focus(*selected, 1, password);
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(
                frame,
                area,
                app,
                "Create user",
                body,
                Some(focus),
                PASSWORD_FORM_FOOTER,
            );
            return;
        }
        _ => return,
    };
    let body = with_error_line(text_lines(body), modal.form_error());
    render_modal_frame(frame, area, app, title, body, focus, footer);
}

/// Body of the user delete dialog: who is deleted, what happens to the
/// home, warnings in red, then the checkbox and the Yes/No row. Also
/// returns the line of the highlighted row.
fn delete_confirm_body(modal: &ModalState) -> (Vec<Line<'static>>, Option<usize>) {
    let ModalState::DeleteConfirm {
        selected,
        allowed,
        delete_home,
        username,
        uid,
        home,
        is_self,
        process_count,
    } = modal
    else {
        return (Vec::new(), None);
    };
    let mut lines = text_lines(format!(
        "Delete user {username} (UID {uid})?\nHome: {home}{}",
        if *delete_home { " will be removed" } else { "" }
    ));
    lines = with_error_line(lines, is_self.then_some("This is your own account."));
    let processes = process_count.filter(|count| *count > 0).map(|count| {
        if count == 1 {
            "1 running process belongs to this user.".to_owned()
        } else {
            format!("{count} running processes belong to this user.")
        }
    });
    lines = with_error_line(lines, processes.as_deref());
    let yes = marker(*selected, DELETE_ROW_YES);
    let no = marker(*selected, DELETE_ROW_NO);
    let buttons = if *allowed {
        format!("{yes} Yes   {no} No")
    } else {
        format!("{yes} {}\n{no} No", crate::app::PROTECTED_USER_MESSAGE)
    };
    // A blank line, the checkbox, then the buttons: Yes and No share a
    // line unless the refusal puts No on its own.
    let home_row = lines.len() + 1;
    let focus = match *selected {
        DELETE_ROW_HOME => home_row,
        DELETE_ROW_NO if !*allowed => home_row + 2,
        _ => home_row + 1,
    };
    lines.extend(text_lines(format!(
        "\n{} [{}] also delete home\n{buttons}",
        marker(*selected, DELETE_ROW_HOME),
        if *delete_home { "x" } else { " " },
    )));
    (lines, Some(focus))
}

/// Body line of form row `selected` when the password field is row
/// `password_row`: rows after it move down one line while its strength
/// hint is shown (see [`password_rows`]).
fn password_form_focus(selected: usize, password_row: usize, password: &SecretInput) -> usize {
    if selected > password_row && !password.is_empty() {
        selected + 1
    } else {
        selected
    }
}

/// A password field and its confirmation, each `(row, label, input,
/// shown)` where `shown` is [`SecretInput::display`]: the cursor after the
/// focused one, and the strength hint under the first once something is
/// typed. `shown` is borrowed into its own span, never copied.
fn password_rows<'a>(
    selected: usize,
    first: (usize, &str, &SecretInput, &'a str),
    second: (usize, &str, &SecretInput, &'a str),
) -> Vec<Line<'a>> {
    let row = |(index, label, _, shown): (usize, &str, &SecretInput, &'a str)| {
        Line::from(vec![
            Span::raw(format!("{} {label}: ", marker(selected, index))),
            Span::raw(shown),
            Span::raw(input_cursor(selected, index)),
        ])
    };
    let mut lines = vec![row(first)];
    if !first.2.is_empty() {
        lines.push(Line::raw(format!("    Strength: {}", first.2.strength())));
    }
    lines.push(row(second));
    lines
}

const MAX_MODAL_CANDIDATES: usize = 1024;
const MAX_MODAL_ROWS: usize = 12;

/// Candidates for the selected user's add/remove groups dialog, the same
/// list `handle_user_membership` indexes.
fn group_candidates(app: &AppState, adding: bool, filter: &str) -> CandidateList {
    app.users
        .get(app.selected_user_index)
        .map_or_else(CandidateList::default, |user| {
            app.user_group_candidate_list(user.uid, adding, filter)
        })
}

/// The page of a single-select group picker (primary group, bulk
/// membership) that holds `selected`: one row per group, the current one
/// marked ` (current)`, and groups the policy refuses muted with
/// ` (blocked: <reason>)`. `groups` is the list the key handler indexes.
fn group_choice_page(
    app: &AppState,
    groups: &CandidateList,
    selected: usize,
    current: Option<usize>,
) -> Vec<Line<'static>> {
    let start = selected / MAX_MODAL_ROWS * MAX_MODAL_ROWS;
    let muted = Style::default().fg(app.theme.muted);
    groups
        .names
        .iter()
        .enumerate()
        .skip(start)
        .take(MAX_MODAL_ROWS)
        .map(|(index, name)| {
            let reason = groups.blocked_reason(index);
            let row = format!(
                "{} {name}{}{}",
                marker(selected, index),
                if Some(index) == current {
                    " (current)"
                } else {
                    ""
                },
                reason
                    .map(|reason| format!(" (blocked: {reason})"))
                    .unwrap_or_default(),
            );
            if reason.is_some() {
                Line::styled(row, muted)
            } else {
                Line::from(row)
            }
        })
        .collect()
}

fn choices(options: &[&str], selected: usize) -> String {
    options
        .iter()
        .enumerate()
        .map(|(index, option)| format!("{} {option}", marker(selected, index)))
        .collect::<Vec<_>>()
        .join("\n")
}
fn choices_page(options: &[String], selected: usize) -> String {
    let start = selected / MAX_MODAL_ROWS * MAX_MODAL_ROWS;
    options
        .iter()
        .take(MAX_MODAL_CANDIDATES)
        .enumerate()
        .skip(start)
        .take(MAX_MODAL_ROWS)
        .map(|(index, option)| format!("{} {option}", marker(selected, index)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// One loaded user `alice` and a search that matches nobody.
    fn filtered_out_app() -> AppState {
        let mut app = AppState::new();
        app.users_all = vec![SystemUser {
            uid: 1000,
            name: "alice".to_owned(),
            primary_gid: 100,
            full_name: None,
            home_dir: "/home/alice".to_owned(),
            shell: "/bin/sh".to_owned(),
        }];
        app.users_query = "zzz".to_owned();
        app.sort_and_filter();
        assert!(app.users.is_empty());
        app
    }

    #[test]
    fn filtered_out_users_show_the_stale_reason_after_a_failed_refresh() {
        let mut app = filtered_out_app();
        app.last_refresh_label = "22:13:20 UTC".to_owned();
        app.diagnostics.stale_reason = Some("boom".to_owned());
        assert_eq!(
            users_empty_lines(&app),
            ["No users match /zzz", "STALE since 22:13:20 UTC: boom"]
        );

        app.diagnostics.stale_reason = None;
        assert_eq!(users_empty_lines(&app), ["No users match /zzz"]);
    }
}
