//! Immutable users-tab rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
};

use crate::{
    app::{
        AppState, CandidateList, DELETE_ROW_HOME, DELETE_ROW_NO, DELETE_ROW_YES, InputMode,
        MAX_NAME_CHARS, ModalState, ModifyField, SecretInput, UsersFocus,
    },
    ui::components::{
        CandidateView, INPUT_CURSOR, LIST_FOOTER, MENU_FOOTER, PASSWORD_FORM_FOOTER, TEXT_FOOTER,
        YES_NO_FOOTER, candidate_dialog, char_budget, input_cursor, marker, no_match_text,
        nothing_loaded_lines, push_filter_labels, render_empty_state, render_modal_frame,
        search_title, shadow_skipped_note, text_lines, with_error_line,
    },
    ui::layout::table_rows,
};

/// Users table title: focus marker, the search query while it applies, and
/// how many users are listed out of all loaded, then the active filters and
/// any skipped shadow filter
/// (`[Users]  /alice  12 of 45  [human, locked]`).
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
    push_filter_labels(&mut title, &app.active_user_filter_labels());
    if let Some(note) = shadow_skipped_note(app) {
        title.push_str("  ");
        title.push_str(&note);
    }
    title
}

/// What the users table says when it has no rows: nothing was loaded
/// (with the stale reason), or the search and filters hide every user.
pub fn users_empty_lines(app: &AppState) -> Vec<String> {
    if app.users_all.is_empty() {
        nothing_loaded_lines(app, "users")
    } else {
        vec![no_match_text(
            "users",
            &app.users_query,
            &app.active_user_filter_labels(),
        )]
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
                Cell::from(user.name.clone()),
                Cell::from(user.primary_gid.to_string()),
                Cell::from(user.home_dir.clone()),
                Cell::from(user.shell.clone()),
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

pub fn render_user_details(frame: &mut Frame, area: Rect, app: &AppState) {
    let text = if let Some(user) = app.users.get(app.selected_user_index) {
        let home = app.diagnostics.homes.get(&user.name);
        let home_exists = home
            .and_then(|diagnostic| diagnostic.exists)
            .map_or("unavailable".to_owned(), |exists| exists.to_string());
        let permissions = home
            .and_then(|diagnostic| diagnostic.permissions.as_deref())
            .unwrap_or("unavailable");
        let keys = home
            .and_then(|diagnostic| diagnostic.authorized_key_count)
            .map_or("unavailable".to_owned(), |count| count.to_string());
        let shadow = app.diagnostics.shadow.status(&user.name);
        let shadow_text = shadow.map_or_else(
            || app.diagnostics.shadow.availability_label().to_string(),
            |status| {
                format!(
                    "locked={} empty={} expired={}",
                    status.locked, status.no_password, status.expired
                )
            },
        );
        format!(
            "User: {}\nUID: {}  GID: {}\nHome: {} (exists: {}, perms: {})\nShell: {}\nShadow: {}\nAuthorized keys: {}",
            user.name,
            user.uid,
            user.primary_gid,
            user.home_dir,
            home_exists,
            permissions,
            user.shell,
            shadow_text,
            keys
        )
    } else {
        "No user selected.".to_owned()
    };
    frame.render_widget(
        Paragraph::new(text).wrap(Wrap { trim: true }).block(
            Block::default()
                .title("Details")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        area,
    );
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
            .style(if selected {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            })
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
        ModalState::ModifyMenu { selected } => (
            "Modify user",
            choices(
                &["Add group", "Remove group", "Modify details", "Password"],
                *selected,
            ),
            Some(*selected),
            MENU_FOOTER,
        ),
        ModalState::ModifyDetailsMenu { selected } => (
            "Modify details",
            choices(&["Username", "Full name", "Shell"], *selected),
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
                    char_budget(value, field.max_chars())
                ),
                ModifyField::Fullname => format!(
                    "{value}{INPUT_CURSOR}  {}",
                    char_budget(value, field.max_chars())
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
            let (title, body, focus, footer) = candidate_dialog(&CandidateView {
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
            });
            let body = with_error_line(body, modal.form_error());
            render_modal_frame(frame, area, app, &title, body, focus, footer);
            return;
        }
        ModalState::ModifyPasswordMenu { selected } => (
            "Password",
            choices(&["Set/change password", "Expire password"], *selected),
            Some(*selected),
            MENU_FOOTER,
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
                "{} [{}] Must change next login\n{} Submit",
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
            group_name,
        } => (
            "Confirm membership removal",
            format!(
                "Remove {group_name}?\n{} Yes   {} No",
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
                char_budget(name, MAX_NAME_CHARS),
            ));
            body.extend(password_rows(
                *selected,
                (1, "Password", password, &shown),
                (2, "Confirm", confirm, &shown_confirm),
            ));
            body.extend(text_lines(format!(
                "{} [{}] Create home\n{} [{}] Add to sudo group\n{} Submit",
                marker(*selected, 3),
                if *create_home { "x" } else { " " },
                marker(*selected, 4),
                if *add_to_wheel { "x" } else { " " },
                marker(*selected, 5)
            )));
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
        format!(
            "{yes} Cannot delete: root and system accounts (UID below 1000) are protected.\n{no} No"
        )
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
