//! Event handling and explicit application effects.
//!
//! All mutations pass through the adapter-owned `OperationRequest` bridge.  The
//! application only displays redacted plans/reports; it never chooses commands,
//! receives runners, or retains sudo credentials.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{Terminal, backend::CrosstermBackend};

use crate::{
    app::{
        ActiveTab, AppState, GroupsFocus, InputMode, ModalState, ModifyField, PendingAction,
        SecretInput, UsersFocus, filterconf::FiltersConfig, keymap::KeyAction,
    },
    error::{CoreError, CoreResult, Result},
    search::apply_filters_and_search,
    sys::{OperationRequest, PasswordRecord, SecretString, UserName},
    ui,
};

/// Drive the TUI.  Construction and refresh are explicit effects; rendering is
/// immutable and does not touch the host.
pub fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut app = AppState::load_system();
    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;
        if event::poll(std::time::Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            if matches!(app.input_mode, InputMode::Normal)
                && matches!(app.keymap.resolve(&key), Some(KeyAction::Quit))
            {
                break Ok(());
            }
            handle_key(&mut app, key);
        }
    }
}

fn handle_key(app: &mut AppState, key: KeyEvent) {
    match app.input_mode {
        InputMode::Normal => handle_normal_key(app, key),
        InputMode::SearchUsers | InputMode::SearchGroups => handle_search_key(app, key),
        InputMode::Modal => handle_modal_key(app, key),
    }
}

fn handle_normal_key(app: &mut AppState, key: KeyEvent) {
    match app.keymap.resolve(&key) {
        Some(KeyAction::Quit) => {}
        Some(KeyAction::OpenHelp) => open_modal(app, ModalState::Help { scroll: 0 }),
        Some(KeyAction::ToggleKeybindsPane) => app.show_keybinds = !app.show_keybinds,
        Some(KeyAction::OpenFilterMenu) => open_modal(app, ModalState::FilterMenu { selected: 0 }),
        Some(KeyAction::StartSearch) => {
            app.search_query.clear();
            app.input_mode = match app.active_tab {
                ActiveTab::Users => InputMode::SearchUsers,
                ActiveTab::Groups => InputMode::SearchGroups,
            };
        }
        Some(KeyAction::NewUser) => match app.active_tab {
            ActiveTab::Users => open_modal(
                app,
                ModalState::UserAddInput {
                    selected: 0,
                    name: String::new(),
                    password: SecretInput::default(),
                    confirm: SecretInput::default(),
                    create_home: true,
                    add_to_wheel: false,
                },
            ),
            ActiveTab::Groups => open_modal(
                app,
                ModalState::GroupAddInput {
                    name: String::new(),
                },
            ),
        },
        Some(KeyAction::SwitchTab) => {
            app.active_tab = match app.active_tab {
                ActiveTab::Users => ActiveTab::Groups,
                ActiveTab::Groups => ActiveTab::Users,
            };
        }
        Some(KeyAction::ToggleUsersFocus | KeyAction::ToggleGroupsFocus) => match app.active_tab {
            ActiveTab::Users => {
                app.users_focus = match app.users_focus {
                    UsersFocus::UsersList => UsersFocus::MemberOf,
                    UsersFocus::MemberOf => UsersFocus::UsersList,
                }
            }
            ActiveTab::Groups => {
                app.groups_focus = match app.groups_focus {
                    GroupsFocus::GroupsList => GroupsFocus::Members,
                    GroupsFocus::Members => GroupsFocus::GroupsList,
                }
            }
        },
        Some(KeyAction::EnterAction) => open_actions(app),
        Some(KeyAction::DeleteSelection) => open_delete(app),
        Some(KeyAction::MoveUp) => move_selection(app, -1),
        Some(KeyAction::MoveDown) => move_selection(app, 1),
        Some(KeyAction::PageUp | KeyAction::MoveLeftPage) => {
            move_selection(app, -(app.rows_per_page as isize))
        }
        Some(KeyAction::PageDown | KeyAction::MoveRightPage) => {
            move_selection(app, app.rows_per_page as isize)
        }
        Some(KeyAction::Ignore) | None => {}
    }
}

fn handle_search_key(app: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.search_query.clear();
            app.input_mode = InputMode::Normal;
            apply_filters_and_search(app);
        }
        KeyCode::Enter => app.input_mode = InputMode::Normal,
        KeyCode::Backspace => {
            app.search_query.pop();
            app.sort_and_filter();
        }
        KeyCode::Char(character) if app.search_query.len() < 256 => {
            app.search_query.push(character);
            app.sort_and_filter();
        }
        _ => {}
    }
}

fn open_actions(app: &mut AppState) {
    match app.active_tab {
        ActiveTab::Users if matches!(app.users_focus, UsersFocus::UsersList) => {
            if !app.users.is_empty() {
                open_modal(app, ModalState::Actions { selected: 0 });
            }
        }
        ActiveTab::Users => {
            if let Some(group) = selected_user_group(app) {
                open_modal(
                    app,
                    ModalState::GroupsActions {
                        selected: 0,
                        target_gid: Some(group.gid),
                    },
                );
            }
        }
        ActiveTab::Groups => {
            if matches!(app.groups_focus, GroupsFocus::Members) {
                if let (Some(group), Some(member)) = (
                    app.groups.get(app.selected_group_index),
                    app.groups
                        .get(app.selected_group_index)
                        .and_then(|group| group.members.get(app.selected_group_member_index)),
                ) {
                    open_modal(
                        app,
                        ModalState::ConfirmRemoveUserFromGroup {
                            selected: 1,
                            group_name: format!("{}:{member}", group.name),
                        },
                    );
                }
            } else if !app.groups.is_empty() {
                open_modal(
                    app,
                    ModalState::GroupsActions {
                        selected: 0,
                        target_gid: None,
                    },
                );
            }
        }
    }
}

fn open_delete(app: &mut AppState) {
    match app.active_tab {
        ActiveTab::Users if matches!(app.users_focus, UsersFocus::UsersList) => {
            if let Some(user) = app.users.get(app.selected_user_index) {
                open_modal(
                    app,
                    ModalState::DeleteConfirm {
                        selected: 1,
                        allowed: !crate::app::is_default_protected_user(user),
                        delete_home: false,
                    },
                );
            }
        }
        ActiveTab::Users => {
            if let (Some(user), Some(group)) = (
                app.users.get(app.selected_user_index),
                selected_user_group(app),
            ) && group.gid != user.primary_gid
            {
                open_modal(
                    app,
                    ModalState::ConfirmRemoveUserFromGroup {
                        selected: 1,
                        group_name: format!("{}:{}", group.name, user.name),
                    },
                );
            }
        }
        ActiveTab::Groups if matches!(app.groups_focus, GroupsFocus::GroupsList) => {
            let gid = app
                .groups
                .get(app.selected_group_index)
                .map(|group| group.gid);
            if gid.is_some() {
                open_modal(
                    app,
                    ModalState::GroupDeleteConfirm {
                        selected: 1,
                        target_gid: gid,
                    },
                );
            }
        }
        ActiveTab::Groups => open_actions(app),
    }
}

fn move_selection(app: &mut AppState, delta: isize) {
    let state = (app.active_tab, app.users_focus, app.groups_focus);
    let len = match state {
        (ActiveTab::Users, UsersFocus::UsersList, _) => app.users.len(),
        (ActiveTab::Users, UsersFocus::MemberOf, _) => user_groups(app).len(),
        (ActiveTab::Groups, _, GroupsFocus::GroupsList) => app.groups.len(),
        (ActiveTab::Groups, _, GroupsFocus::Members) => app
            .groups
            .get(app.selected_group_index)
            .map_or(0, |group| group.members.len()),
    };
    let selected = match state {
        (ActiveTab::Users, UsersFocus::UsersList, _) => &mut app.selected_user_index,
        (ActiveTab::Users, UsersFocus::MemberOf, _) => &mut app.selected_user_group_index,
        (ActiveTab::Groups, _, GroupsFocus::GroupsList) => &mut app.selected_group_index,
        (ActiveTab::Groups, _, GroupsFocus::Members) => &mut app.selected_group_member_index,
    };
    move_index(selected, len, delta);
    app.capture_selection_identities();
}

fn move_index(index: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *index = 0;
        return;
    }
    let next = (*index as isize + delta).rem_euclid(len as isize);
    *index = next as usize;
}

fn handle_modal_key(app: &mut AppState, key: KeyEvent) {
    let Some(mut modal) = app.modal.take() else {
        return;
    };
    match &mut modal {
        ModalState::FilterMenu { selected } => handle_filter_modal(app, selected, key),
        ModalState::Actions { selected } => handle_user_actions(app, selected, key),
        ModalState::ModifyMenu { selected } => handle_modify_menu(app, selected, key),
        ModalState::ModifyDetailsMenu { selected } => handle_modify_details(app, selected, key),
        ModalState::ModifyTextInput { field, value } => handle_text_input(app, field, value, key),
        ModalState::ModifyShell {
            selected, shells, ..
        } => handle_shell_modal(app, selected, shells, key),
        ModalState::ModifyGroupsAdd {
            selected,
            selected_multi,
            ..
        } => handle_user_membership(app, true, selected, selected_multi, key),
        ModalState::ModifyGroupsRemove {
            selected,
            selected_multi,
            ..
        } => handle_user_membership(app, false, selected, selected_multi, key),
        ModalState::ModifyPasswordMenu { selected } => handle_password_menu(app, selected, key),
        ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
        } => handle_password_input(app, selected, password, confirm, must_change, key),
        ModalState::DeleteConfirm {
            selected,
            allowed,
            delete_home,
        } => handle_user_delete(app, selected, allowed, delete_home, key),
        ModalState::ConfirmRemoveUserFromGroup {
            selected,
            group_name,
        } => handle_member_delete(app, selected, group_name, key),
        ModalState::GroupsActions {
            selected,
            target_gid,
        } => handle_group_actions(app, selected, target_gid, key),
        ModalState::GroupAddInput { name } => handle_group_add(app, name, key),
        ModalState::GroupDeleteConfirm {
            selected,
            target_gid,
        } => handle_group_delete(app, selected, target_gid, key),
        ModalState::GroupModifyMenu {
            selected,
            target_gid,
        } => handle_group_modify(app, selected, target_gid, key),
        ModalState::GroupRenameInput { name, target_gid } => {
            handle_group_rename(app, name, target_gid, key)
        }
        ModalState::GroupModifyAddMembers {
            selected,
            target_gid,
            selected_multi,
            ..
        } => handle_group_members(app, true, selected, target_gid, selected_multi, key),
        ModalState::GroupModifyRemoveMembers {
            selected,
            target_gid,
            selected_multi,
            ..
        } => handle_group_members(app, false, selected, target_gid, selected_multi, key),
        ModalState::UserAddInput {
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
        } => handle_user_add(
            app,
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
            key,
        ),
        ModalState::OperationConfirm { selected, .. } => {
            handle_operation_confirm(app, selected, key)
        }
        ModalState::SudoPrompt {
            password, error, ..
        } => handle_sudo_prompt(app, password, error, key),
        ModalState::Info { .. } => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                close_modal(app)
            }
        }
        ModalState::Help { scroll } => match key.code {
            KeyCode::Esc | KeyCode::Enter => close_modal(app),
            KeyCode::Up => *scroll = scroll.saturating_sub(1),
            KeyCode::Down => *scroll = scroll.saturating_add(1),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
            KeyCode::PageDown => *scroll = scroll.saturating_add(10),
            _ => {}
        },
    }
    if app.modal.is_none() && matches!(app.input_mode, InputMode::Modal) {
        app.modal = Some(modal);
    }
}

fn handle_filter_modal(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    let maximum = match app.active_tab {
        ActiveTab::Users => 7,
        ActiveTab::Groups => 2,
    };
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.checked_sub(1).unwrap_or(maximum),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1) % (maximum + 1),
        KeyCode::Enter | KeyCode::Char(' ') => {
            match app.active_tab {
                ActiveTab::Users => match *selected {
                    0 => app.users_filter_chips = Default::default(),
                    1 => app.users_filter_chips.human_only = !app.users_filter_chips.human_only,
                    2 => app.users_filter_chips.system_only = !app.users_filter_chips.system_only,
                    3 => app.users_filter_chips.inactive = !app.users_filter_chips.inactive,
                    4 => app.users_filter_chips.no_home = !app.users_filter_chips.no_home,
                    5 => app.users_filter_chips.locked = !app.users_filter_chips.locked,
                    6 => app.users_filter_chips.no_password = !app.users_filter_chips.no_password,
                    7 => app.users_filter_chips.expired = !app.users_filter_chips.expired,
                    _ => {}
                },
                ActiveTab::Groups => {
                    app.groups_filter = match *selected {
                        0 => None,
                        1 => Some(crate::app::GroupsFilter::OnlyUserGids),
                        _ => Some(crate::app::GroupsFilter::OnlySystemGids),
                    }
                }
            }
            app.sort_and_filter();
            save_filters(app);
        }
        _ => {}
    }
}

fn handle_user_actions(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
            *selected = 1 - *selected
        }
        KeyCode::Enter => {
            if *selected == 0 {
                open_modal(app, ModalState::ModifyMenu { selected: 0 })
            } else {
                open_delete(app)
            }
        }
        _ => {}
    }
}

fn handle_modify_menu(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Backspace => open_modal(app, ModalState::Actions { selected: 0 }),
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.checked_sub(1).unwrap_or(3),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1) % 4,
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::ModifyGroupsAdd {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                },
            ),
            1 => open_modal(
                app,
                ModalState::ModifyGroupsRemove {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                },
            ),
            2 => open_modal(app, ModalState::ModifyDetailsMenu { selected: 0 }),
            _ => open_modal(app, ModalState::ModifyPasswordMenu { selected: 0 }),
        },
        _ => {}
    }
}

fn handle_modify_details(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Backspace => open_modal(app, ModalState::ModifyMenu { selected: 2 }),
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.checked_sub(1).unwrap_or(2),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1) % 3,
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::ModifyTextInput {
                    field: ModifyField::Username,
                    value: String::new(),
                },
            ),
            1 => open_modal(
                app,
                ModalState::ModifyTextInput {
                    field: ModifyField::Fullname,
                    value: String::new(),
                },
            ),
            _ => {
                let shells = app
                    .account_snapshot
                    .as_ref()
                    .map_or_else(Vec::new, |snapshot| {
                        snapshot
                            .shells
                            .iter()
                            .map(|shell| shell.as_str().to_owned())
                            .collect()
                    });
                open_modal(
                    app,
                    ModalState::ModifyShell {
                        selected: 0,
                        offset: 0,
                        shells,
                    },
                );
            }
        },
        _ => {}
    }
}

fn handle_text_input(app: &mut AppState, field: &ModifyField, value: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Backspace => {
            value.pop();
        }
        KeyCode::Char(character) if value.len() < 4096 => value.push(character),
        KeyCode::Enter => {
            if let Some(user) = app.users.get(app.selected_user_index) {
                let action = match field {
                    ModifyField::Username => PendingAction::ChangeUsername {
                        old_username: user.name.clone(),
                        new_username: value.clone(),
                    },
                    ModifyField::Fullname => PendingAction::ChangeFullname {
                        username: user.name.clone(),
                        new_fullname: value.clone(),
                    },
                };
                start_pending_action(app, action);
            }
        }
        _ => {}
    }
}

fn handle_shell_modal(app: &mut AppState, selected: &mut usize, shells: &[String], key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Char('k') => move_index(selected, shells.len(), -1),
        KeyCode::Down | KeyCode::Char('j') => move_index(selected, shells.len(), 1),
        KeyCode::Enter => {
            if let (Some(user), Some(shell)) = (
                app.users.get(app.selected_user_index),
                shells.get(*selected),
            ) {
                start_pending_action(
                    app,
                    PendingAction::ChangeShell {
                        username: user.name.clone(),
                        new_shell: shell.clone(),
                    },
                );
            }
        }
        _ => {}
    }
}

fn handle_user_membership(
    app: &mut AppState,
    adding: bool,
    selected: &mut usize,
    selected_multi: &mut Vec<usize>,
    key: KeyEvent,
) {
    let Some(user) = app.users.get(app.selected_user_index).cloned() else {
        close_modal(app);
        return;
    };
    const MAX_MODAL_CANDIDATES: usize = 1024;
    let groups: Vec<_> = app
        .groups_all
        .iter()
        .filter(|group| {
            let member = group.members.iter().any(|member| member == &user.name);
            group.gid != user.primary_gid && if adding { !member } else { member }
        })
        .take(MAX_MODAL_CANDIDATES)
        .cloned()
        .collect();
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Char('k') => move_index(selected, groups.len(), -1),
        KeyCode::Down | KeyCode::Char('j') => move_index(selected, groups.len(), 1),
        KeyCode::Char(' ') => toggle(selected_multi, *selected),
        KeyCode::Enter => {
            let names: Vec<_> = if selected_multi.is_empty() {
                groups
                    .get(*selected)
                    .map(|group| vec![group.name.clone()])
                    .unwrap_or_default()
            } else {
                selected_multi
                    .iter()
                    .filter_map(|index| groups.get(*index).map(|group| group.name.clone()))
                    .collect()
            };
            if !names.is_empty() {
                start_pending_action(
                    app,
                    if adding {
                        PendingAction::AddUserToGroups {
                            username: user.name,
                            groupnames: names,
                        }
                    } else {
                        PendingAction::RemoveUserFromGroups {
                            username: user.name,
                            groupnames: names,
                        }
                    },
                );
            }
        }
        _ => {}
    }
}

fn handle_password_menu(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Down | KeyCode::Char('j') | KeyCode::Char('k') => {
            *selected = 1 - *selected
        }
        KeyCode::Enter => {
            if *selected == 0 {
                open_modal(
                    app,
                    ModalState::ChangePassword {
                        selected: 0,
                        password: SecretInput::default(),
                        confirm: SecretInput::default(),
                        must_change: false,
                    },
                )
            } else if let Some(user) = app.users.get(app.selected_user_index) {
                start_pending_action(
                    app,
                    PendingAction::ResetPassword {
                        username: user.name.clone(),
                    },
                )
            }
        }
        _ => {}
    }
}

fn handle_password_input(
    app: &mut AppState,
    selected: &mut usize,
    password: &mut SecretInput,
    confirm: &mut SecretInput,
    must_change: &mut bool,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down => *selected = (*selected + 1).min(3),
        KeyCode::Char(' ') if *selected == 2 => *must_change = !*must_change,
        KeyCode::Backspace => match *selected {
            0 => {
                password.pop();
            }
            1 => {
                confirm.pop();
            }
            _ => {}
        },
        KeyCode::Char(character) if *selected < 2 && password.len().max(confirm.len()) < 1024 => {
            if *selected == 0 {
                password.push(character)
            } else {
                confirm.push(character)
            }
        }
        KeyCode::Enter if *selected == 3 => {
            if password.is_empty() || !password.matches(confirm) {
                open_modal(
                    app,
                    ModalState::Info {
                        message: "Passwords do not match or are empty.".to_owned(),
                    },
                )
            } else if let Some(user) = app.users.get(app.selected_user_index) {
                let username = user.name.clone();
                match PasswordRecord::new(
                    UserName::new(&username).expect("observed username remains valid"),
                    SecretString::new(password.take()),
                ) {
                    Ok(record) => start_password_action(
                        app,
                        PendingAction::SetPassword {
                            username,
                            must_change: *must_change,
                        },
                        record,
                    ),
                    Err(error) => open_modal(
                        app,
                        ModalState::Info {
                            message: classified_error_message(&error),
                        },
                    ),
                }
            }
        }
        _ => {}
    }
}

fn handle_user_delete(
    app: &mut AppState,
    selected: &mut usize,
    allowed: &mut bool,
    delete_home: &mut bool,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Left | KeyCode::Right => *selected = 1 - *selected,
        KeyCode::Char(' ') => *delete_home = !*delete_home,
        KeyCode::Enter if *selected == 0 && *allowed => {
            if let Some(user) = app.users.get(app.selected_user_index) {
                start_pending_action(
                    app,
                    PendingAction::DeleteUser {
                        username: user.name.clone(),
                        delete_home: *delete_home,
                    },
                )
            }
        }
        KeyCode::Enter if !*allowed => open_modal(
            app,
            ModalState::Info {
                message: "Protected identities require explicit policy allowlisting.".to_owned(),
            },
        ),
        _ => {}
    }
}

fn handle_member_delete(app: &mut AppState, selected: &mut usize, encoded: &str, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Left | KeyCode::Right => *selected = 1 - *selected,
        KeyCode::Enter if *selected == 0 => {
            if let Some((group, user)) = encoded.split_once(':') {
                start_pending_action(
                    app,
                    PendingAction::RemoveUserFromGroup {
                        username: user.to_owned(),
                        groupname: group.to_owned(),
                    },
                )
            }
        }
        _ => {}
    }
}

fn handle_group_actions(
    app: &mut AppState,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    let limit = if target_gid.is_some() { 1 } else { 2 };
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.checked_sub(1).unwrap_or(limit),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1) % (limit + 1),
        KeyCode::Enter => match (*selected, *target_gid) {
            (0, _) if target_gid.is_none() => open_modal(
                app,
                ModalState::GroupAddInput {
                    name: String::new(),
                },
            ),
            (1, target) => open_modal(
                app,
                ModalState::GroupDeleteConfirm {
                    selected: 1,
                    target_gid: target.or_else(|| {
                        app.groups
                            .get(app.selected_group_index)
                            .map(|group| group.gid)
                    }),
                },
            ),
            (_, target) => open_modal(
                app,
                ModalState::GroupModifyMenu {
                    selected: 0,
                    target_gid: target.or_else(|| {
                        app.groups
                            .get(app.selected_group_index)
                            .map(|group| group.gid)
                    }),
                },
            ),
        },
        _ => {}
    }
}

fn handle_group_add(app: &mut AppState, name: &mut String, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Backspace => {
            name.pop();
        }
        KeyCode::Char(character) if name.len() < 32 => name.push(character),
        KeyCode::Enter => start_pending_action(
            app,
            PendingAction::CreateGroup {
                groupname: name.clone(),
            },
        ),
        _ => {}
    }
}

fn handle_group_delete(
    app: &mut AppState,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Left | KeyCode::Right => *selected = 1 - *selected,
        KeyCode::Enter if *selected == 0 => {
            let group = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index));
            if let Some(group) = group {
                if crate::app::is_default_protected_group(group) {
                    open_modal(
                        app,
                        ModalState::Info {
                            message:
                                "Protected group mutations require explicit policy allowlisting."
                                    .to_owned(),
                        },
                    );
                } else {
                    start_pending_action(
                        app,
                        PendingAction::DeleteGroup {
                            groupname: group.name.clone(),
                        },
                    );
                }
            }
        }
        _ => {}
    }
}

fn handle_group_modify(
    app: &mut AppState,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Char('k') => *selected = selected.checked_sub(1).unwrap_or(2),
        KeyCode::Down | KeyCode::Char('j') => *selected = (*selected + 1) % 3,
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::GroupModifyAddMembers {
                    selected: 0,
                    offset: 0,
                    target_gid: *target_gid,
                    selected_multi: vec![],
                },
            ),
            1 => open_modal(
                app,
                ModalState::GroupModifyRemoveMembers {
                    selected: 0,
                    offset: 0,
                    target_gid: *target_gid,
                    selected_multi: vec![],
                },
            ),
            _ => open_modal(
                app,
                ModalState::GroupRenameInput {
                    name: String::new(),
                    target_gid: *target_gid,
                },
            ),
        },
        _ => {}
    }
}

fn handle_group_rename(
    app: &mut AppState,
    name: &mut String,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Backspace => {
            name.pop();
        }
        KeyCode::Char(character) if name.len() < 32 => name.push(character),
        KeyCode::Enter => {
            let group = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index));
            if let Some(group) = group {
                if crate::app::is_default_protected_group(group) {
                    open_modal(
                        app,
                        ModalState::Info {
                            message:
                                "Protected group mutations require explicit policy allowlisting."
                                    .to_owned(),
                        },
                    );
                } else {
                    start_pending_action(
                        app,
                        PendingAction::RenameGroup {
                            old_name: group.name.clone(),
                            new_name: name.clone(),
                        },
                    );
                }
            }
        }
        _ => {}
    }
}

fn handle_group_members(
    app: &mut AppState,
    adding: bool,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    selected_multi: &mut Vec<usize>,
    key: KeyEvent,
) {
    let group = target_gid
        .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
        .or_else(|| app.groups.get(app.selected_group_index))
        .cloned();
    let Some(group) = group else {
        close_modal(app);
        return;
    };
    const MAX_MODAL_CANDIDATES: usize = 1024;
    let users: Vec<String> = if adding {
        app.users_all
            .iter()
            .filter(|user| !group.members.contains(&user.name))
            .take(MAX_MODAL_CANDIDATES)
            .map(|user| user.name.clone())
            .collect()
    } else {
        group
            .members
            .into_iter()
            .take(MAX_MODAL_CANDIDATES)
            .collect()
    };
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up | KeyCode::Char('k') => move_index(selected, users.len(), -1),
        KeyCode::Down | KeyCode::Char('j') => move_index(selected, users.len(), 1),
        KeyCode::Char(' ') => toggle(selected_multi, *selected),
        KeyCode::Enter => {
            let names: Vec<_> = if selected_multi.is_empty() {
                users.get(*selected).cloned().into_iter().collect()
            } else {
                selected_multi
                    .iter()
                    .filter_map(|index| users.get(*index).cloned())
                    .collect()
            };
            if !names.is_empty() {
                start_pending_action(
                    app,
                    if adding {
                        PendingAction::AddMembersToGroup {
                            groupname: group.name,
                            usernames: names,
                        }
                    } else {
                        PendingAction::RemoveMembersFromGroup {
                            groupname: group.name,
                            usernames: names,
                        }
                    },
                )
            }
        }
        _ => {}
    }
}

#[allow(clippy::too_many_arguments)] // Modal fields are borrowed independently from a taken modal.
fn handle_user_add(
    app: &mut AppState,
    selected: &mut usize,
    name: &mut String,
    password: &mut SecretInput,
    confirm: &mut SecretInput,
    create_home: &mut bool,
    add_to_wheel: &mut bool,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => close_modal(app),
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down => *selected = (*selected + 1).min(5),
        KeyCode::Char(' ') if *selected == 3 => *create_home = !*create_home,
        KeyCode::Char(' ') if *selected == 4 => *add_to_wheel = !*add_to_wheel,
        KeyCode::Backspace => match *selected {
            0 => {
                name.pop();
            }
            1 => {
                password.pop();
            }
            2 => {
                confirm.pop();
            }
            _ => {}
        },
        KeyCode::Char(character) if *selected == 0 && name.len() < 32 => name.push(character),
        KeyCode::Char(character)
            if (*selected == 1 || *selected == 2) && password.len().max(confirm.len()) < 1024 =>
        {
            if *selected == 1 {
                password.push(character)
            } else {
                confirm.push(character)
            }
        }
        KeyCode::Enter if *selected == 5 => {
            if name.is_empty() || (!password.is_empty() && !password.matches(confirm)) {
                open_modal(
                    app,
                    ModalState::Info {
                        message: "Username is required and passwords must match.".to_owned(),
                    },
                )
            } else {
                let action = PendingAction::CreateUserWithOptions {
                    username: name.clone(),
                    set_password: !password.is_empty(),
                    create_home: *create_home,
                    add_to_wheel: *add_to_wheel,
                };
                if password.is_empty() {
                    start_pending_action(app, action);
                } else {
                    match UserName::new(name.clone()).and_then(|username| {
                        PasswordRecord::new(username, SecretString::new(password.take()))
                    }) {
                        Ok(record) => start_password_action(app, action, record),
                        Err(error) => open_modal(
                            app,
                            ModalState::Info {
                                message: classified_error_message(&error),
                            },
                        ),
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_operation_confirm(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.clear_pending_operation();
            close_modal(app);
        }
        KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => *selected = 1 - *selected,
        KeyCode::Enter if *selected == 0 => execute_pending_plan(app),
        KeyCode::Enter => {
            app.clear_pending_operation();
            close_modal(app);
        }
        _ => {}
    }
}

fn handle_sudo_prompt(
    app: &mut AppState,
    password: &mut SecretInput,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => {
            app.clear_pending_operation();
            close_modal(app);
        }
        KeyCode::Backspace => {
            password.pop();
        }
        KeyCode::Char(character) if password.len() < 1024 => password.push(character),
        KeyCode::Enter => {
            if password.is_empty() {
                *error = Some("A password is required.".to_owned());
                return;
            }
            app.adapter
                .set_elevation_secret(SecretString::new(password.take()));
            execute_pending_plan(app);
        }
        _ => {}
    }
}

fn start_password_action(app: &mut AppState, action: PendingAction, record: PasswordRecord) {
    app.set_password_capability(record);
    start_pending_action(app, action);
}

fn start_pending_action(app: &mut AppState, action: PendingAction) {
    match operation_request(app, action) {
        Ok(request) => prepare_request(app, request),
        Err(error) => {
            app.clear_pending_operation();
            open_modal(
                app,
                ModalState::Info {
                    message: classified_error_message(&error),
                },
            )
        }
    }
}

fn prepare_request(app: &mut AppState, request: OperationRequest) {
    match app.adapter.prepare_operation(request) {
        Ok(plan) => {
            let prepared = crate::app::PreparedOperation { plan };
            let preview = prepared.preview();
            app.last_preview = preview.clone();
            app.pending_operation = Some(prepared);
            open_modal(
                app,
                ModalState::OperationConfirm {
                    selected: 1,
                    action: "Apply the exact prepared operation".to_owned(),
                    preview,
                },
            );
        }
        Err(error) => {
            app.clear_pending_operation();
            open_modal(
                app,
                ModalState::Info {
                    message: classified_error_message(&error),
                },
            );
        }
    }
}

fn execute_pending_plan(app: &mut AppState) {
    let Some(prepared) = app.pending_operation.as_ref() else {
        open_modal(
            app,
            ModalState::Info {
                message: "No prepared operation is available.".to_owned(),
            },
        );
        return;
    };
    match app.adapter.execute_prepared_operation(&prepared.plan) {
        Ok(report) => {
            let message = report_message(&report);
            app.last_report = Some(report);
            app.pending_operation = None;
            app.refresh_accounts();
            open_modal(app, ModalState::Info { message });
        }
        Err(CoreError::AuthenticationRequired) => {
            open_modal(
                app,
                ModalState::SudoPrompt {
                    password: SecretInput::default(),
                    error: None,
                },
            );
        }
        Err(error) => {
            app.clear_pending_operation();
            open_modal(
                app,
                ModalState::Info {
                    message: classified_error_message(&error),
                },
            );
        }
    }
}

fn operation_request(app: &mut AppState, action: PendingAction) -> CoreResult<OperationRequest> {
    let mut requests = match action {
        PendingAction::AddUserToGroup {
            username,
            groupname,
        } => vec![OperationRequest::AddUserToGroup {
            username,
            groupname,
        }],
        PendingAction::RemoveUserFromGroup {
            username,
            groupname,
        } => vec![OperationRequest::RemoveUserFromGroup {
            username,
            groupname,
        }],
        PendingAction::AddUserToGroups {
            username,
            groupnames,
        } => groupnames
            .into_iter()
            .map(|groupname| OperationRequest::AddUserToGroup {
                username: username.clone(),
                groupname,
            })
            .collect(),
        PendingAction::RemoveUserFromGroups {
            username,
            groupnames,
        } => groupnames
            .into_iter()
            .map(|groupname| OperationRequest::RemoveUserFromGroup {
                username: username.clone(),
                groupname,
            })
            .collect(),
        PendingAction::AddMembersToGroup {
            groupname,
            usernames,
        } => usernames
            .into_iter()
            .map(|username| OperationRequest::AddUserToGroup {
                username,
                groupname: groupname.clone(),
            })
            .collect(),
        PendingAction::RemoveMembersFromGroup {
            groupname,
            usernames,
        } => usernames
            .into_iter()
            .map(|username| OperationRequest::RemoveUserFromGroup {
                username,
                groupname: groupname.clone(),
            })
            .collect(),
        PendingAction::ChangeShell {
            username,
            new_shell,
        } => vec![OperationRequest::ChangeUserShell {
            username,
            shell: new_shell,
        }],
        PendingAction::ChangeFullname {
            username,
            new_fullname,
        } => vec![OperationRequest::ChangeUserGecos {
            username,
            gecos: new_fullname,
        }],
        PendingAction::ChangeUsername {
            old_username,
            new_username,
        } => vec![OperationRequest::RenameUser {
            old_username,
            new_username,
        }],
        PendingAction::CreateGroup { groupname } => {
            vec![OperationRequest::CreateGroup { groupname }]
        }
        PendingAction::DeleteGroup { groupname } => {
            vec![OperationRequest::DeleteGroup { groupname }]
        }
        PendingAction::RenameGroup { old_name, new_name } => {
            vec![OperationRequest::RenameGroup { old_name, new_name }]
        }
        PendingAction::CreateUserWithOptions {
            username,
            set_password,
            create_home,
            add_to_wheel,
        } => {
            let mut requests = vec![OperationRequest::CreateUser {
                username: username.clone(),
                create_home,
            }];
            if set_password {
                let record = app
                    .take_password_capability()
                    .ok_or(CoreError::Validation {
                        field: "password capability",
                        reason: "expired before operation preparation",
                    })?;
                requests.push(OperationRequest::SetUserPassword { record });
            }
            if add_to_wheel {
                requests.push(OperationRequest::AddUserToGroup {
                    username,
                    groupname: crate::app::sudo_group_name(),
                });
            }
            requests
        }
        PendingAction::DeleteUser {
            username,
            delete_home,
        } => vec![OperationRequest::DeleteUser {
            username,
            delete_home,
        }],
        PendingAction::SetPassword {
            username,
            must_change,
        } => {
            let record = app
                .take_password_capability()
                .ok_or(CoreError::Validation {
                    field: "password capability",
                    reason: "expired before operation preparation",
                })?;
            let mut requests = vec![OperationRequest::SetUserPassword { record }];
            if must_change {
                requests.push(OperationRequest::ExpireUserPassword { username });
            }
            requests
        }
        PendingAction::ResetPassword { username } => {
            vec![OperationRequest::ExpireUserPassword { username }]
        }
    };
    Ok(if requests.len() == 1 {
        requests.pop().expect("one request remains")
    } else {
        OperationRequest::Composite { requests }
    })
}

fn report_message(report: &crate::sys::OperationReport) -> String {
    let mut message = format!(
        "completed: {}, skipped: {}",
        report.completed.len(),
        report.skipped.len()
    );
    if let Some(failed) = &report.failed {
        message.push_str(&format!(
            "\nfailed step {}: {}",
            failed.id,
            classified_error_message(&failed.error)
        ));
    }
    match &report.reconciliation {
        crate::sys::ReconciliationStatus::Verified => {
            message.push_str("\nreconciliation: verified")
        }
        crate::sys::ReconciliationStatus::Partial { detail } => {
            message.push_str(&format!("\npartial outcome: {detail}"))
        }
        crate::sys::ReconciliationStatus::Unavailable { detail } => {
            message.push_str(&format!("\nreconciliation unavailable: {detail}"))
        }
    }
    message
}

/// Stable, bounded diagnostics shown by the application. `CoreError` never
/// carries a credential, command output, or password record.
fn classified_error_message(error: &CoreError) -> String {
    let code = match error {
        CoreError::AuthenticationRequired => "E-AUTH-REQUIRED",
        CoreError::AuthenticationDenied => "E-AUTH-DENIED",
        CoreError::AuthenticationCapability => "E-AUTH-CAPABILITY",
        CoreError::UnsupportedPlatform => "E-PLATFORM",
        CoreError::MissingExecutable { .. } => "E-EXEC-MISSING",
        CoreError::Timeout { .. } => "E-EXEC-TIMEOUT",
        CoreError::OutputLimit { .. } => "E-EXEC-OUTPUT",
        CoreError::ExitStatus { .. } => "E-EXEC-STATUS",
        CoreError::Validation { .. } => "E-VALIDATION",
        CoreError::Refresh { .. } => "E-REFRESH",
        CoreError::Io { .. } => "E-IO",
        CoreError::PartialCompletion { .. } => "E-PARTIAL",
        CoreError::PostconditionFailed { .. } => "E-POSTCONDITION",
    };
    format!("{code}: {error}")
}

fn save_filters(app: &mut AppState) {
    let path = app.configuration_write_path("filter.conf");
    if let Err(error) = FiltersConfig::save_from_app(app, &path) {
        app.record_config_message("filter", &error);
    }
}

fn open_modal(app: &mut AppState, modal: ModalState) {
    app.modal = Some(modal);
    app.input_mode = InputMode::Modal;
}

fn close_modal(app: &mut AppState) {
    app.modal = None;
    app.input_mode = InputMode::Normal;
}

fn toggle(values: &mut Vec<usize>, selected: usize) {
    if let Some(position) = values.iter().position(|value| *value == selected) {
        values.remove(position);
    } else {
        values.push(selected);
    }
}

fn user_groups(app: &AppState) -> Vec<crate::sys::SystemGroup> {
    app.users
        .get(app.selected_user_index)
        .map_or_else(Vec::new, |user| {
            app.groups
                .iter()
                .filter(|group| {
                    group.gid == user.primary_gid
                        || group.members.iter().any(|member| member == &user.name)
                })
                .cloned()
                .collect()
        })
}

fn selected_user_group(app: &AppState) -> Option<crate::sys::SystemGroup> {
    user_groups(app).get(app.selected_user_group_index).cloned()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::{
        error::CoreResult,
        sys::{
            AccountDataSource, AccountGroup, AccountSnapshot, AccountUser, CommandResult,
            CommandRunner, ElevationGrant, FixedIdentityProvider, Gecos, Gid, GroupName, ShellPath,
            SystemAdapter, Uid,
        },
    };

    #[test]
    fn index_movement_wraps_without_stateful_rendering() {
        let mut index = 0;
        move_index(&mut index, 3, -1);
        assert_eq!(index, 2);
        move_index(&mut index, 3, 2);
        assert_eq!(index, 1);
    }

    #[test]
    fn report_text_marks_unavailable_reconciliation() {
        let report = crate::sys::OperationReport {
            completed: vec![],
            skipped: vec![],
            compensated: vec![],
            failed: None,
            reconciliation: crate::sys::ReconciliationStatus::Unavailable {
                detail: "shadow unavailable".to_owned(),
            },
        };
        assert!(report_message(&report).contains("unavailable"));
    }

    #[test]
    fn app_prepares_exact_redacted_plan_before_authentication_prompt() {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = AppState::with_adapter(adapter, snapshot);
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        assert!(app.pending_operation.is_some());
        assert_eq!(app.last_preview, ["usermod -s /bin/bash alice"]);
        execute_pending_plan(&mut app);
        assert!(matches!(app.modal, Some(ModalState::SudoPrompt { .. })));
    }

    #[test]
    fn password_expiry_is_one_composite_request_and_consumes_its_capability() {
        let mut app = AppState::new();
        let record = PasswordRecord::new(
            UserName::new("alice").unwrap(),
            SecretString::new("fixture-password"),
        )
        .unwrap();
        app.set_password_capability(record);
        let request = operation_request(
            &mut app,
            PendingAction::SetPassword {
                username: "alice".into(),
                must_change: true,
            },
        )
        .unwrap();
        assert!(app.pending_password.is_none());
        assert!(matches!(
            request,
            OperationRequest::Composite { ref requests } if requests.len() == 2
        ));
    }

    fn fixture_snapshot() -> AccountSnapshot {
        AccountSnapshot {
            users: vec![AccountUser {
                uid: Uid(1000),
                name: UserName::new("alice").unwrap(),
                primary_gid: Gid(1000),
                full_name: Some(Gecos::new("Alice").unwrap()),
                home_dir: "/home/alice".into(),
                shell: ShellPath::new("/bin/sh").unwrap(),
            }],
            groups: vec![AccountGroup {
                gid: Gid(1000),
                name: GroupName::new("dev").unwrap(),
                members: vec![],
            }],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        }
    }

    struct StaticSource(AccountSnapshot);
    impl AccountDataSource for StaticSource {
        fn refresh(&self) -> CoreResult<AccountSnapshot> {
            Ok(self.0.clone())
        }
    }

    struct NoRun;
    impl CommandRunner for NoRun {
        fn authenticate(&self, _: SecretString) -> CoreResult<ElevationGrant> {
            unreachable!("authentication is not attempted without a supplied secret")
        }
        fn run(&self, _: ElevationGrant, _: &crate::sys::CommandSpec) -> CoreResult<CommandResult> {
            unreachable!("authentication required must precede command execution")
        }
    }
}
