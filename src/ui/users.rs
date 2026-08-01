//! Immutable users-tab rendering.

use ratatui::{
    Frame,
    layout::{Constraint, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
};

use crate::app::{AppState, ModalState, ModifyField, UsersFocus};

fn visible_rows(area: Rect) -> usize {
    usize::from(area.height.saturating_sub(3)).max(1)
}

pub fn render_users_table(frame: &mut Frame, area: Rect, app: &AppState) {
    let per_page = visible_rows(area);
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
        .block(
            Block::default()
                .title(if app.users_focus == UsersFocus::UsersList {
                    "[Users]"
                } else {
                    "Users"
                })
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        ),
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
    let per_page = visible_rows(area);
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
            .block(
                Block::default()
                    .title(if app.users_focus == UsersFocus::MemberOf {
                        "[Member of]"
                    } else {
                        "Member of"
                    })
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            ),
        area,
    );
}

fn member_groups(app: &AppState) -> Vec<&crate::sys::SystemGroup> {
    app.selected_user_groups()
}

pub fn render_user_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    let (title, body) = match modal {
        ModalState::Actions { selected } => ("Actions", choices(&["Modify", "Delete"], *selected)),
        ModalState::ModifyMenu { selected } => (
            "Modify user",
            choices(
                &["Add group", "Remove group", "Modify details", "Password"],
                *selected,
            ),
        ),
        ModalState::ModifyDetailsMenu { selected } => (
            "Modify details",
            choices(&["Username", "Full name", "Shell"], *selected),
        ),
        ModalState::ModifyTextInput { field, value } => (
            match field {
                ModifyField::Username => "Change username",
                ModifyField::Fullname => "Change full name",
            },
            value.clone(),
        ),
        ModalState::ModifyShell {
            selected, shells, ..
        } => ("Select shell", choices_page(shells, *selected)),
        ModalState::ModifyGroupsAdd {
            selected,
            selected_multi,
            ..
        } => (
            "Add groups",
            group_candidate_choices(app, true, *selected, selected_multi),
        ),
        ModalState::ModifyGroupsRemove {
            selected,
            selected_multi,
            ..
        } => (
            "Remove groups",
            group_candidate_choices(app, false, *selected, selected_multi),
        ),
        ModalState::ModifyPasswordMenu { selected } => (
            "Password",
            choices(&["Set/change password", "Expire password"], *selected),
        ),
        ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
        } => (
            "Set password",
            format!(
                "{} New password: {}\n{} Confirm: {}\n{} [{}] Must change next login\n{} Submit",
                marker(*selected, 0),
                "*".repeat(password.len()),
                marker(*selected, 1),
                "*".repeat(confirm.len()),
                marker(*selected, 2),
                if *must_change { "x" } else { " " },
                marker(*selected, 3)
            ),
        ),
        ModalState::DeleteConfirm {
            selected,
            allowed,
            delete_home,
        } => (
            "Confirm delete",
            format!(
                "Delete selected user?\n{} [{}] also delete home\n{} Yes   {} No\n{}",
                marker(*selected, 2),
                if *delete_home { "x" } else { " " },
                marker(*selected, 0),
                marker(*selected, 1),
                if *allowed { "" } else { "Root is immutable." }
            ),
        ),
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
        ),
        ModalState::UserAddInput {
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
        } => (
            "Create user",
            format!(
                "{} Username: {name}\n{} Password: {}\n{} Confirm: {}\n{} [{}] Create home\n{} [{}] Add to sudo group\n{} Submit",
                marker(*selected, 0),
                marker(*selected, 1),
                "*".repeat(password.len()),
                marker(*selected, 2),
                "*".repeat(confirm.len()),
                marker(*selected, 3),
                if *create_home { "x" } else { " " },
                marker(*selected, 4),
                if *add_to_wheel { "x" } else { " " },
                marker(*selected, 5)
            ),
        ),
        _ => return,
    };
    render_text_modal(frame, area, app, title, &body);
}

const MAX_MODAL_CANDIDATES: usize = 1024;
const MAX_MODAL_ROWS: usize = 12;

fn group_candidate_choices(
    app: &AppState,
    adding: bool,
    selected: usize,
    checked: &[usize],
) -> String {
    let Some(user) = app.users.get(app.selected_user_index) else {
        return String::new();
    };
    let start = selected / MAX_MODAL_ROWS * MAX_MODAL_ROWS;
    app.groups_all
        .iter()
        .filter(|group| {
            let member = group.members.iter().any(|name| name == &user.name);
            group.gid != user.primary_gid && if adding { !member } else { member }
        })
        .take(MAX_MODAL_CANDIDATES)
        .enumerate()
        .skip(start)
        .take(MAX_MODAL_ROWS)
        .map(|(index, group)| {
            format!(
                "{} [{}] {}",
                marker(selected, index),
                if checked.contains(&index) { "x" } else { " " },
                group.name
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
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
fn marker(selected: usize, index: usize) -> &'static str {
    if selected == index { "▶" } else { " " }
}
fn render_text_modal(frame: &mut Frame, area: Rect, app: &AppState, title: &str, body: &str) {
    let rect = crate::ui::components::centered_rect(
        area.width.saturating_sub(8).min(70),
        area.height.saturating_sub(6).min(20),
        area,
    );
    frame.render_widget(Clear, rect);
    frame.render_widget(
        Paragraph::new(body.to_owned())
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.theme.border)),
            ),
        rect,
    );
}
