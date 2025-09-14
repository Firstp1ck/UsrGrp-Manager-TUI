use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;
use std::time::Duration;

use crate::app::{AppState, InputMode, ActiveTab, UsersFocus, ModalState, ModifyField, PendingAction};
use crate::search::apply_search;
use crate::ui;
use crate::sys;

pub fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut app = AppState::new();

    loop {
        terminal.draw(|f| {
            ui::render(f, &mut app);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Esc => { /* ignore */ }
                            KeyCode::Char('/') => {
                                app.search_query.clear();
                                app.input_mode = match app.active_tab { ActiveTab::Users => InputMode::SearchUsers, ActiveTab::Groups => InputMode::SearchGroups };
                            }
                            KeyCode::Tab => {
                                app.active_tab = match app.active_tab { ActiveTab::Users => ActiveTab::Groups, ActiveTab::Groups => ActiveTab::Users };
                            }
                            KeyCode::BackTab => {
                                if let ActiveTab::Users = app.active_tab {
                                    app.users_focus = match app.users_focus { UsersFocus::UsersList => UsersFocus::MemberOf, UsersFocus::MemberOf => UsersFocus::UsersList };
                                }
                            }
                            KeyCode::Enter => {
                                match app.active_tab {
                                    ActiveTab::Users => {
                                        if !app.users.is_empty() {
                                            if let UsersFocus::MemberOf = app.users_focus {
                                                if let Some(u) = app.users.get(app.selected_user_index) {
                                                    let uname = u.name.clone();
                                                    let pgid = u.primary_gid;
                                                    let groups_for_user: Vec<sys::SystemGroup> = app
                                                        .groups
                                                        .iter()
                                                        .filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &uname))
                                                        .cloned()
                                                        .collect();
                                                    if let Some(sel_group) = groups_for_user.get(app.selected_group_index) {
                                                        if let Some(idx) = app.groups.iter().position(|g| g.gid == sel_group.gid) {
                                                            app.selected_group_index = idx;
                                                        }
                                                        app.modal = Some(ModalState::GroupsActions { selected: 0, target_gid: Some(sel_group.gid) });
                                                        app.input_mode = InputMode::Modal;
                                                    }
                                                }
                                            } else {
                                                app.modal = Some(ModalState::Actions { selected: 0 });
                                                app.input_mode = InputMode::Modal;
                                            }
                                        }
                                    }
                                    ActiveTab::Groups => {
                                        if !app.groups.is_empty() {
                                            app.modal = Some(ModalState::GroupsActions { selected: 0, target_gid: None });
                                            app.input_mode = InputMode::Modal;
                                        }
                                    }
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => match app.active_tab {
                                ActiveTab::Users => {
                                    match app.users_focus {
                                        UsersFocus::UsersList => { if app.selected_user_index > 0 { app.selected_user_index -= 1; } }
                                        UsersFocus::MemberOf => {
                                            if app.selected_group_index > 0 { app.selected_group_index -= 1; }
                                        }
                                    }
                                }
                                ActiveTab::Groups => { if app.selected_group_index > 0 { app.selected_group_index -= 1; } }
                            },
                            KeyCode::Down | KeyCode::Char('j') => match app.active_tab {
                                ActiveTab::Users => {
                                    match app.users_focus {
                                        UsersFocus::UsersList => { if app.selected_user_index + 1 < app.users.len() { app.selected_user_index += 1; } }
                                        UsersFocus::MemberOf => {
                                            let groups_len = if let Some(u) = app.users.get(app.selected_user_index) {
                                                let name = u.name.clone();
                                                let pgid = u.primary_gid;
                                                app.groups.iter().filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &name)).count()
                                            } else { 0 };
                                            if app.selected_group_index + 1 < groups_len { app.selected_group_index += 1; }
                                        }
                                    }
                                }
                                ActiveTab::Groups => { if app.selected_group_index + 1 < app.groups.len() { app.selected_group_index += 1; } }
                            },
                            KeyCode::Left | KeyCode::Char('h') => {
                                let rpp = app.rows_per_page.max(1);
                                match app.active_tab {
                                    ActiveTab::Users => match app.users_focus {
                                        UsersFocus::UsersList => { if app.selected_user_index >= rpp { app.selected_user_index -= rpp; } else { app.selected_user_index = 0; } }
                                        UsersFocus::MemberOf => { if app.selected_group_index >= rpp { app.selected_group_index -= rpp; } else { app.selected_group_index = 0; } }
                                    },
                                    ActiveTab::Groups => {
                                        if app.selected_group_index >= rpp { app.selected_group_index -= rpp; } else { app.selected_group_index = 0; }
                                    }
                                }
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                let rpp = app.rows_per_page.max(1);
                                match app.active_tab {
                                    ActiveTab::Users => match app.users_focus {
                                        UsersFocus::UsersList => {
                                            let new_idx = app.selected_user_index.saturating_add(rpp);
                                            app.selected_user_index = new_idx.min(app.users.len().saturating_sub(1));
                                        }
                                        UsersFocus::MemberOf => {
                                            let groups_len = if let Some(u) = app.users.get(app.selected_user_index) {
                                                let name = u.name.clone();
                                                let pgid = u.primary_gid;
                                                app.groups.iter().filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &name)).count()
                                            } else { 0 };
                                            let new_idx = app.selected_group_index.saturating_add(rpp);
                                            app.selected_group_index = new_idx.min(groups_len.saturating_sub(1));
                                        }
                                    },
                                    ActiveTab::Groups => {
                                        let new_idx = app.selected_group_index.saturating_add(rpp);
                                        app.selected_group_index = new_idx.min(app.groups.len().saturating_sub(1));
                                    }
                                }
                            }
                            _ => {}
                        },
                        InputMode::Modal => {
                            handle_modal_key(&mut app, key.code);
                        }
                        InputMode::SearchUsers | InputMode::SearchGroups => match key.code {
                            KeyCode::Enter => {
                                apply_search(&mut app);
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.search_query.clear();
                            }
                            KeyCode::Backspace => { app.search_query.pop(); }
                            KeyCode::Char(c) => { app.search_query.push(c); }
                            _ => {}
                        },
                    }
                }
            }
        }

        let _uptime = app.started_at.elapsed();
    }

    Ok(())
}

fn handle_modal_key(app: &mut AppState, code: KeyCode) {
    match &mut app.modal {
        Some(ModalState::Actions { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => { app.modal = Some(ModalState::ModifyMenu { selected: 0 }); }
                        1 => {
                            if let Some(user) = app.users.get(app.selected_user_index) {
                                let allowed = user.uid >= 1000 && user.uid <= 1999;
                                if allowed {
                                    app.modal = Some(ModalState::DeleteConfirm { selected: 1, allowed });
                                } else {
                                    app.modal = Some(ModalState::Info { message: format!("Deletion not allowed. Only UID 1000-1999 allowed: {}", user.name) });
                                }
                            } else { close_modal(app); }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyMenu { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::ModifyGroupsAdd { selected: 0, offset: 0 }),
                        1 => app.modal = Some(ModalState::ModifyGroupsRemove { selected: 0, offset: 0 }),
                        2 => app.modal = Some(ModalState::ModifyDetailsMenu { selected: 0 }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyGroupsAdd { selected, offset }) => {
            let total = app.groups_all.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } }
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } }
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } }
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); }
                KeyCode::Enter => {
                    let group_name = app.groups_all.get(*selected).map(|g| g.name.clone());
                    if let (Some(user), Some(group_name)) = (app.users.get(app.selected_user_index), group_name) {
                        let pending = PendingAction::AddUserToGroup { username: user.name.clone(), groupname: group_name.clone() };
                        if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                            app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyGroupsRemove { selected, offset }) => {
            let (username, primary_gid) = if let Some(u) = app.users.get(app.selected_user_index) { (u.name.clone(), u.primary_gid) } else { (String::new(), 0) };
            let user_groups: Vec<sys::SystemGroup> = app.groups_all.iter().filter(|g| g.gid == primary_gid || g.members.iter().any(|m| m == &username)).cloned().collect();
            let total = user_groups.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } }
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } }
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } }
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); }
                KeyCode::Enter => {
                    if let (Some(user), Some(group)) = (app.users.get(app.selected_user_index), user_groups.get(*selected)) {
                        if group.gid == user.primary_gid {
                            app.modal = Some(ModalState::Info { message: "Cannot remove user from primary group.".to_string() });
                        } else {
                            let pending = PendingAction::RemoveUserFromGroup { username: user.name.clone(), groupname: group.name.clone() };
                            if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                                app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                            }
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyDetailsMenu { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::ModifyTextInput { field: ModifyField::Username, value: String::new() }),
                        1 => app.modal = Some(ModalState::ModifyTextInput { field: ModifyField::Fullname, value: String::new() }),
                        2 => {
                            let adapter = crate::sys::SystemAdapter::new();
                            let shells = adapter.list_shells().unwrap_or_default();
                            app.modal = Some(ModalState::ModifyShell { selected: 0, offset: 0, shells });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyShell { selected, offset, shells }) => {
            let total = shells.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } }
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } }
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } }
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); }
                KeyCode::Enter => {
                    if let (Some(user), Some(new_shell)) = (app.users.get(app.selected_user_index), shells.get(*selected)) {
                        let pending = PendingAction::ChangeShell { username: user.name.clone(), new_shell: new_shell.clone() };
                        if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                            app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyTextInput { field, value }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Enter => {
                    if let Some(user) = app.users.get(app.selected_user_index) {
                        let pending = match field { ModifyField::Username => PendingAction::ChangeUsername { old_username: user.name.clone(), new_username: value.clone() }, ModifyField::Fullname => PendingAction::ChangeFullname { username: user.name.clone(), new_fullname: value.clone() } };
                        if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                            app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                        }
                    } else { close_modal(app); }
                }
                KeyCode::Backspace => { value.pop(); }
                KeyCode::Char(c) => { value.push(c); }
                _ => {}
            }
        }
        Some(ModalState::DeleteConfirm { selected, allowed }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Left | KeyCode::Right => { *selected = if *selected == 0 { 1 } else { 0 }; },
                KeyCode::Enter => {
                    if *selected == 0 {
                        if *allowed {
                            if let Some(user) = app.users.get(app.selected_user_index) {
                                app.modal = Some(ModalState::Info { message: format!("Would delete user '{}'(uid {}). Not implemented.", user.name, user.uid) });
                            } else { close_modal(app); }
                        } else {
                            app.modal = Some(ModalState::Info { message: "Deletion not allowed.".to_string() });
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupsActions { selected, target_gid }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::GroupAddInput { name: String::new() }),
                        1 => app.modal = Some(ModalState::GroupDeleteConfirm { selected: 1 }),
                        2 => app.modal = Some(ModalState::GroupModifyMenu { selected: 0, target_gid: *target_gid }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupAddInput { name }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Enter => {
                    let pending = PendingAction::CreateGroup { groupname: name.clone() };
                    if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                        app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                    }
                }
                KeyCode::Backspace => { name.pop(); }
                KeyCode::Char(c) => { name.push(c); }
                _ => {}
            }
        }
        Some(ModalState::GroupDeleteConfirm { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Left | KeyCode::Right => { *selected = if *selected == 0 { 1 } else { 0 }; },
                KeyCode::Enter => {
                    if *selected == 0 {
                        let group_name_opt = app.groups.get(app.selected_group_index).map(|g| g.name.clone());
                        if let Some(group_name) = group_name_opt {
                            let pending = PendingAction::DeleteGroup { groupname: group_name.clone() };
                            if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                                app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                            }
                        } else { close_modal(app); }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyMenu { selected, target_gid }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 1 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::GroupModifyAddMembers { selected: 0, offset: 0, target_gid: *target_gid }),
                        1 => app.modal = Some(ModalState::GroupModifyRemoveMembers { selected: 0, offset: 0, target_gid: *target_gid }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyAddMembers { selected, offset, target_gid }) => {
            let total = app.users_all.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } }
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } }
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } }
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); }
                KeyCode::Enter => {
                    let group_name = if let Some(gid) = *target_gid {
                        app.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone())
                    } else {
                        app.groups.get(app.selected_group_index).map(|g| g.name.clone())
                    };
                    let user_name = app.users_all.get(*selected).map(|u| u.name.clone());
                    if let (Some(group_name), Some(user_name)) = (group_name, user_name) {
                        let pending = PendingAction::AddUserToGroup { username: user_name.clone(), groupname: group_name.clone() };
                        if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                            app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyRemoveMembers { selected, offset, target_gid }) => {
            let group_name = if let Some(gid) = *target_gid { app.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone()).unwrap_or_default() } else { app.groups.get(app.selected_group_index).map(|g| g.name.clone()).unwrap_or_default() };
            let members: Vec<String> = if let Some(gid) = *target_gid { app.groups.iter().find(|g| g.gid == gid).map(|g| g.members.clone()).unwrap_or_default() } else { app.groups.get(app.selected_group_index).map(|g| g.members.clone()).unwrap_or_default() };
            let total = members.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } }
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } }
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } }
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); }
                KeyCode::Enter => {
                    if let Some(username) = members.get(*selected) {
                        let gname_opt = if let Some(gid) = *target_gid { app.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone()) } else { Some(group_name.clone()) };
                        if let Some(group_name) = gname_opt {
                            let pending = PendingAction::RemoveUserFromGroup { username: username.clone(), groupname: group_name.clone() };
                            if let Err(e) = perform_pending_action(app, pending.clone(), app.sudo_password.clone()) {
                                app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                            }
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::SudoPrompt { next, password, error: _ }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Backspace => { password.pop(); }
                KeyCode::Enter => {
                    let pw = password.clone();
                    app.sudo_password = Some(pw.clone());
                    let pending = next.clone();
                    match perform_pending_action(app, pending.clone(), Some(pw)) {
                        Ok(_) => {}
                        Err(e) => {
                            app.modal = Some(ModalState::SudoPrompt { next: pending, password: String::new(), error: Some(e.to_string()) });
                        }
                    }
                }
                KeyCode::Char(c) => { password.push(c); }
                _ => {}
            }
        }
        Some(ModalState::Info { .. }) => {
            match code {
                KeyCode::Esc | KeyCode::Enter => close_modal(app),
                _ => {}
            }
        }
        None => {}
    }
}

fn close_modal(app: &mut AppState) {
    app.modal = None;
    app.input_mode = InputMode::Normal;
}

fn perform_pending_action(app: &mut AppState, pending: PendingAction, sudo_password: Option<String>) -> Result<()> {
    let adapter = crate::sys::SystemAdapter::with_sudo_password(sudo_password);
    match pending.clone() {
        PendingAction::AddUserToGroup { username, groupname } => {
            adapter.add_user_to_group(&username, &groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups = app.groups_all.clone();
            app.modal = Some(ModalState::Info { message: format!("Added '{}' to '{}'", username, groupname) });
        }
        PendingAction::RemoveUserFromGroup { username, groupname } => {
            adapter.remove_user_from_group(&username, &groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups = app.groups_all.clone();
            app.modal = Some(ModalState::Info { message: format!("Removed '{}' from '{}'", username, groupname) });
        }
        PendingAction::ChangeShell { username, new_shell } => {
            adapter.change_user_shell(&username, &new_shell)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            app.users = app.users_all.clone();
            app.modal = Some(ModalState::Info { message: format!("Changed shell to '{}'", new_shell) });
        }
        PendingAction::ChangeFullname { username, new_fullname } => {
            adapter.change_user_fullname(&username, &new_fullname)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            app.users = app.users_all.clone();
            app.modal = Some(ModalState::Info { message: "Changed successfully".to_string() });
        }
        PendingAction::ChangeUsername { old_username, new_username } => {
            adapter.change_username(&old_username, &new_username)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            app.users = app.users_all.clone();
            app.modal = Some(ModalState::Info { message: "Changed successfully".to_string() });
        }
        PendingAction::CreateGroup { groupname } => {
            adapter.create_group(&groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups = app.groups_all.clone();
            app.modal = Some(ModalState::Info { message: format!("Created group '{}'", groupname) });
        }
        PendingAction::DeleteGroup { groupname } => {
            adapter.delete_group(&groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups = app.groups_all.clone();
            app.modal = Some(ModalState::Info { message: format!("Deleted group '{}'", groupname) });
        }
    }
    Ok(())
}