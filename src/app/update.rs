//! Application update loop and input handling.
//!
//! Contains the TUI render loop and all keyboard event handling, including
//! modal workflows for user and group management.
//!
use crate::error::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use std::time::Duration;

use crate::app::{
    ActiveTab, AppState, GroupsFilter, InputMode, ModalState, ModifyField, PendingAction,
    UsersFilter, UsersFocus,
};
use crate::search::apply_filters_and_search;
use crate::sys;
use crate::ui;

/// Drive the TUI: draw frames and react to keyboard input until quit.
pub fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut app = AppState::new();

    loop {
        terminal.draw(|f| {
            ui::render(f, &mut app);
        })?;

        if event::poll(Duration::from_millis(100))?
            && let Event::Key(key) = event::read()?
            && key.kind == KeyEventKind::Press
        {
            match app.input_mode {
                InputMode::Normal => match key.code {
                    KeyCode::Char('q') => break,
                    KeyCode::Esc => { /* ignore */ }
                    KeyCode::Char('f') => {
                        app.modal = Some(ModalState::FilterMenu { selected: 0 });
                        app.input_mode = InputMode::Modal;
                    }
                    KeyCode::Char('/') => {
                        app.search_query.clear();
                        app.input_mode = match app.active_tab {
                            ActiveTab::Users => InputMode::SearchUsers,
                            ActiveTab::Groups => InputMode::SearchGroups,
                        };
                    }
                    KeyCode::Char('n') => {
                        // Open create user modal; default to create home
                        app.modal = Some(ModalState::UserAddInput {
                            selected: 0,
                            name: String::new(),
                            password: String::new(),
                            confirm: String::new(),
                            create_home: true,
                            add_to_wheel: false,
                        });
                        app.input_mode = InputMode::Modal;
                    }
                    KeyCode::Tab => {
                        app.active_tab = match app.active_tab {
                            ActiveTab::Users => ActiveTab::Groups,
                            ActiveTab::Groups => ActiveTab::Users,
                        };
                    }
                    KeyCode::BackTab => {
                        if let ActiveTab::Users = app.active_tab {
                            app.users_focus = match app.users_focus {
                                UsersFocus::UsersList => UsersFocus::MemberOf,
                                UsersFocus::MemberOf => UsersFocus::UsersList,
                            };
                        }
                    }
                    KeyCode::Enter => match app.active_tab {
                        ActiveTab::Users => {
                            if !app.users.is_empty() {
                                if let UsersFocus::MemberOf = app.users_focus {
                                    if let Some(u) = app.users.get(app.selected_user_index) {
                                        let uname = u.name.clone();
                                        let pgid = u.primary_gid;
                                        let groups_for_user: Vec<sys::SystemGroup> = app
                                            .groups
                                            .iter()
                                            .filter(|g| {
                                                g.gid == pgid
                                                    || g.members.iter().any(|m| m == &uname)
                                            })
                                            .cloned()
                                            .collect();
                                        if let Some(sel_group) =
                                            groups_for_user.get(app.selected_group_index)
                                        {
                                            if let Some(idx) = app
                                                .groups
                                                .iter()
                                                .position(|g| g.gid == sel_group.gid)
                                            {
                                                app.selected_group_index = idx;
                                            }
                                            app.modal = Some(ModalState::GroupsActions {
                                                selected: 0,
                                                target_gid: Some(sel_group.gid),
                                            });
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
                                app.modal = Some(ModalState::GroupsActions {
                                    selected: 0,
                                    target_gid: None,
                                });
                                app.input_mode = InputMode::Modal;
                            }
                        }
                    },
                    KeyCode::Up | KeyCode::Char('k') => match app.active_tab {
                        ActiveTab::Users => match app.users_focus {
                            UsersFocus::UsersList => {
                                if app.selected_user_index > 0 {
                                    app.selected_user_index -= 1;
                                } else if !app.users.is_empty() {
                                    app.selected_user_index = app.users.len().saturating_sub(1);
                                }
                            }
                            UsersFocus::MemberOf => {
                                let groups_len = if let Some(u) =
                                    app.users.get(app.selected_user_index)
                                {
                                    let name = u.name.clone();
                                    let pgid = u.primary_gid;
                                    app.groups
                                        .iter()
                                        .filter(|g| {
                                            g.gid == pgid || g.members.iter().any(|m| m == &name)
                                        })
                                        .count()
                                } else {
                                    0
                                };
                                if app.selected_group_index > 0 {
                                    app.selected_group_index -= 1;
                                } else if groups_len > 0 {
                                    app.selected_group_index = groups_len.saturating_sub(1);
                                }
                            }
                        },
                        ActiveTab::Groups => {
                            if app.selected_group_index > 0 {
                                app.selected_group_index -= 1;
                            } else if !app.groups.is_empty() {
                                app.selected_group_index = app.groups.len().saturating_sub(1);
                            }
                        }
                    },
                    KeyCode::Down | KeyCode::Char('j') => match app.active_tab {
                        ActiveTab::Users => match app.users_focus {
                            UsersFocus::UsersList => {
                                if app.selected_user_index + 1 < app.users.len() {
                                    app.selected_user_index += 1;
                                } else if !app.users.is_empty() {
                                    app.selected_user_index = 0;
                                }
                            }
                            UsersFocus::MemberOf => {
                                let groups_len = if let Some(u) =
                                    app.users.get(app.selected_user_index)
                                {
                                    let name = u.name.clone();
                                    let pgid = u.primary_gid;
                                    app.groups
                                        .iter()
                                        .filter(|g| {
                                            g.gid == pgid || g.members.iter().any(|m| m == &name)
                                        })
                                        .count()
                                } else {
                                    0
                                };
                                if app.selected_group_index + 1 < groups_len {
                                    app.selected_group_index += 1;
                                } else if groups_len > 0 {
                                    app.selected_group_index = 0;
                                }
                            }
                        },
                        ActiveTab::Groups => {
                            if app.selected_group_index + 1 < app.groups.len() {
                                app.selected_group_index += 1;
                            } else if !app.groups.is_empty() {
                                app.selected_group_index = 0;
                            }
                        }
                    },
                    KeyCode::Left | KeyCode::Char('h') => {
                        let rpp = app.rows_per_page.max(1);
                        match app.active_tab {
                            ActiveTab::Users => match app.users_focus {
                                UsersFocus::UsersList => {
                                    if app.selected_user_index >= rpp {
                                        app.selected_user_index -= rpp;
                                    } else {
                                        app.selected_user_index = 0;
                                    }
                                }
                                UsersFocus::MemberOf => {
                                    if app.selected_group_index >= rpp {
                                        app.selected_group_index -= rpp;
                                    } else {
                                        app.selected_group_index = 0;
                                    }
                                }
                            },
                            ActiveTab::Groups => {
                                if app.selected_group_index >= rpp {
                                    app.selected_group_index -= rpp;
                                } else {
                                    app.selected_group_index = 0;
                                }
                            }
                        }
                    }
                    KeyCode::Right | KeyCode::Char('l') => {
                        let rpp = app.rows_per_page.max(1);
                        match app.active_tab {
                            ActiveTab::Users => match app.users_focus {
                                UsersFocus::UsersList => {
                                    let new_idx = app.selected_user_index.saturating_add(rpp);
                                    app.selected_user_index =
                                        new_idx.min(app.users.len().saturating_sub(1));
                                }
                                UsersFocus::MemberOf => {
                                    let groups_len =
                                        if let Some(u) = app.users.get(app.selected_user_index) {
                                            let name = u.name.clone();
                                            let pgid = u.primary_gid;
                                            app.groups
                                                .iter()
                                                .filter(|g| {
                                                    g.gid == pgid
                                                        || g.members.iter().any(|m| m == &name)
                                                })
                                                .count()
                                        } else {
                                            0
                                        };
                                    let new_idx = app.selected_group_index.saturating_add(rpp);
                                    app.selected_group_index =
                                        new_idx.min(groups_len.saturating_sub(1));
                                }
                            },
                            ActiveTab::Groups => {
                                let new_idx = app.selected_group_index.saturating_add(rpp);
                                app.selected_group_index =
                                    new_idx.min(app.groups.len().saturating_sub(1));
                            }
                        }
                    }
                    _ => {}
                },
                InputMode::Modal => {
                    handle_modal_key(&mut app, key);
                }
                InputMode::SearchUsers | InputMode::SearchGroups => match key.code {
                    KeyCode::Enter => {
                        apply_filters_and_search(&mut app);
                        app.input_mode = InputMode::Normal;
                    }
                    KeyCode::Esc => {
                        app.input_mode = InputMode::Normal;
                        app.search_query.clear();
                        apply_filters_and_search(&mut app);
                    }
                    KeyCode::Backspace => {
                        app.search_query.pop();
                        apply_filters_and_search(&mut app);
                    }
                    KeyCode::Char(c) => {
                        app.search_query.push(c);
                        apply_filters_and_search(&mut app);
                    }
                    _ => {}
                },
            }
        }

        let _uptime = app.started_at.elapsed();
    }

    Ok(())
}

/// Handle all key events while a modal dialog is open.
fn handle_modal_key(app: &mut AppState, key: KeyEvent) {
    match &mut app.modal {
        Some(ModalState::FilterMenu { selected }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => close_modal(app),
            KeyCode::Up | KeyCode::Char('k') => {
                let max = if matches!(app.active_tab, ActiveTab::Users) { 7 } else { 2 };
                if *selected > 0 { *selected -= 1; } else { *selected = max; }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                let max = if matches!(app.active_tab, ActiveTab::Users) { 7 } else { 2 };
                if *selected < max { *selected += 1; } else { *selected = 0; }
            }
            KeyCode::Char(' ') => {
                if let ActiveTab::Users = app.active_tab {
                    match *selected {
                        3 => app.users_filter_chips.inactive = !app.users_filter_chips.inactive,
                        4 => app.users_filter_chips.no_home = !app.users_filter_chips.no_home,
                        5 => app.users_filter_chips.locked = !app.users_filter_chips.locked,
                        6 => app.users_filter_chips.no_password = !app.users_filter_chips.no_password,
                        7 => app.users_filter_chips.expired = !app.users_filter_chips.expired,
                        _ => {}
                    }
                }
            }
            KeyCode::Enter => {
                match app.active_tab {
                    ActiveTab::Users => match *selected {
                        0 => app.users_filter = None,
                        1 => app.users_filter = Some(UsersFilter::OnlyUserIds),
                        2 => app.users_filter = Some(UsersFilter::OnlySystemIds),
                        _ => {}
                    },
                    ActiveTab::Groups => match *selected {
                        0 => app.groups_filter = None,
                        1 => app.groups_filter = Some(GroupsFilter::OnlyUserGids),
                        2 => app.groups_filter = Some(GroupsFilter::OnlySystemGids),
                        _ => {}
                    },
                }
                close_modal(app);
                apply_filters_and_search(app);
            }
            _ => {}
        },
        Some(ModalState::Actions { selected }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                } else {
                    *selected = 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected < 1 {
                    *selected += 1;
                } else {
                    *selected = 0;
                }
            }
            KeyCode::Enter => match *selected {
                0 => {
                    app.modal = Some(ModalState::ModifyMenu { selected: 0 });
                }
                1 => {
                    if let Some(user) = app.users.get(app.selected_user_index) {
                        let allowed = user.uid >= 1000 && user.uid <= 1999;
                        if allowed {
                            app.modal = Some(ModalState::DeleteConfirm {
                                selected: 1,
                                allowed,
                                delete_home: false,
                            });
                        } else {
                            app.modal = Some(ModalState::Info {
                                message: format!(
                                    "Deletion not allowed. Only UID 1000-1999 allowed: {}",
                                    user.name
                                ),
                            });
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            },
            _ => {}
        },
        Some(ModalState::ModifyMenu { selected }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                app.modal = Some(ModalState::Actions { selected: 0 });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                } else {
                    *selected = 3;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected < 3 {
                    *selected += 1;
                } else {
                    *selected = 0;
                }
            }
            KeyCode::Enter => match *selected {
                0 => {
                    app.modal = Some(ModalState::ModifyGroupsAdd {
                        selected: 0,
                        offset: 0,
                        selected_multi: Vec::new(),
                    })
                }
                1 => {
                    app.modal = Some(ModalState::ModifyGroupsRemove {
                        selected: 0,
                        offset: 0,
                        selected_multi: Vec::new(),
                    })
                }
                2 => app.modal = Some(ModalState::ModifyDetailsMenu { selected: 0 }),
                3 => app.modal = Some(ModalState::ModifyPasswordMenu { selected: 0 }),
                _ => {}
            },
            _ => {}
        },
        Some(ModalState::ModifyPasswordMenu { selected }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                app.modal = Some(ModalState::ModifyMenu { selected: 3 });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                } else {
                    *selected = 1;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected < 1 {
                    *selected += 1;
                } else {
                    *selected = 0;
                }
            }
            KeyCode::Enter => match *selected {
                0 => {
                    app.modal = Some(ModalState::ChangePassword {
                        selected: 0,
                        password: String::new(),
                        confirm: String::new(),
                        must_change: false,
                    })
                }
                1 => {
                    if let Some(user) = app.users.get(app.selected_user_index) {
                        let pending = PendingAction::ResetPassword {
                            username: user.name.clone(),
                        };
                        if let Err(_e) =
                            perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                        {
                            app.modal = Some(ModalState::SudoPrompt {
                                next: pending,
                                password: String::new(),
                                error: None,
                            });
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            },
            _ => {}
        },
        Some(ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
        }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Up => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down => {
                if *selected < 3 {
                    *selected += 1;
                }
            }
            KeyCode::Backspace => match *selected {
                0 => {
                    if password.is_empty() {
                        app.modal = Some(ModalState::ModifyPasswordMenu { selected: 0 });
                    } else {
                        password.pop();
                    }
                }
                1 => {
                    if confirm.is_empty() {
                        app.modal = Some(ModalState::ModifyPasswordMenu { selected: 0 });
                    } else {
                        confirm.pop();
                    }
                }
                _ => {}
            },
            KeyCode::Char(' ') => {
                if *selected == 2 {
                    *must_change = !*must_change;
                }
            }
            KeyCode::Char(c) => match *selected {
                0 => password.push(c),
                1 => confirm.push(c),
                _ => {}
            },
            KeyCode::Enter => {
                if *selected == 3 {
                    if password.is_empty() || password != confirm {
                        app.modal = Some(ModalState::Info {
                            message: "Passwords do not match or empty".to_string(),
                        });
                    } else if let Some(user) = app.users.get(app.selected_user_index) {
                        let pending = PendingAction::SetPassword {
                            username: user.name.clone(),
                            password: password.clone(),
                            must_change: *must_change,
                        };
                        if let Err(_e) =
                            perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                        {
                            app.modal = Some(ModalState::SudoPrompt {
                                next: pending,
                                password: String::new(),
                                error: None,
                            });
                        }
                    } else {
                        close_modal(app);
                    }
                }
            }
            _ => {}
        },
        Some(ModalState::ModifyGroupsAdd {
            selected,
            offset,
            selected_multi,
        }) => {
            let total = app.groups_all.len();
            match key.code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Backspace => {
                    app.modal = Some(ModalState::ModifyMenu { selected: 0 });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                        if *selected < *offset {
                            *offset = *selected;
                        }
                    } else if total > 0 {
                        *selected = total.saturating_sub(1);
                        *offset = *selected;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total {
                        *selected += 1;
                    } else if total > 0 {
                        *selected = 0;
                        *offset = 0;
                    }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step {
                        *selected -= step;
                    } else {
                        *selected = 0;
                    }
                    if *selected < *offset {
                        *offset = *selected;
                    }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    if let Some(pos) = selected_multi.iter().position(|&i| i == *selected) {
                        selected_multi.remove(pos);
                    } else {
                        selected_multi.push(*selected);
                    }
                }
                KeyCode::Enter => {
                    if let Some(user) = app.users.get(app.selected_user_index) {
                        if !selected_multi.is_empty() {
                            let mut names: Vec<String> = Vec::with_capacity(selected_multi.len());
                            for idx in selected_multi.iter() {
                                if let Some(g) = app.groups_all.get(*idx) {
                                    names.push(g.name.clone());
                                }
                            }
                            if !names.is_empty() {
                                let pending = PendingAction::AddUserToGroups {
                                    username: user.name.clone(),
                                    groupnames: names,
                                };
                                if let Err(_e) = perform_pending_action(
                                    app,
                                    pending.clone(),
                                    app.sudo_password.clone(),
                                ) {
                                    app.modal = Some(ModalState::SudoPrompt {
                                        next: pending,
                                        password: String::new(),
                                        error: None,
                                    });
                                }
                            } else {
                                close_modal(app);
                            }
                        } else if let Some(group_name) =
                            app.groups_all.get(*selected).map(|g| g.name.clone())
                        {
                            let pending = PendingAction::AddUserToGroup {
                                username: user.name.clone(),
                                groupname: group_name.clone(),
                            };
                            if let Err(_e) = perform_pending_action(
                                app,
                                pending.clone(),
                                app.sudo_password.clone(),
                            ) {
                                app.modal = Some(ModalState::SudoPrompt {
                                    next: pending,
                                    password: String::new(),
                                    error: None,
                                });
                            }
                        } else {
                            close_modal(app);
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyGroupsRemove {
            selected,
            offset,
            selected_multi,
        }) => {
            let (username, primary_gid) = if let Some(u) = app.users.get(app.selected_user_index) {
                (u.name.clone(), u.primary_gid)
            } else {
                (String::new(), 0)
            };
            let user_groups: Vec<sys::SystemGroup> = app
                .groups_all
                .iter()
                .filter(|g| g.gid == primary_gid || g.members.iter().any(|m| m == &username))
                .cloned()
                .collect();
            let total = user_groups.len();
            match key.code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Backspace => {
                    app.modal = Some(ModalState::ModifyMenu { selected: 1 });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                        if *selected < *offset {
                            *offset = *selected;
                        }
                    } else if total > 0 {
                        *selected = total.saturating_sub(1);
                        *offset = *selected;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total {
                        *selected += 1;
                    } else if total > 0 {
                        *selected = 0;
                        *offset = 0;
                    }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step {
                        *selected -= step;
                    } else {
                        *selected = 0;
                    }
                    if *selected < *offset {
                        *offset = *selected;
                    }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    if let Some(pos) = selected_multi.iter().position(|&i| i == *selected) {
                        selected_multi.remove(pos);
                    } else {
                        selected_multi.push(*selected);
                    }
                }
                KeyCode::Enter => {
                    if let Some(user) = app.users.get(app.selected_user_index) {
                        if !selected_multi.is_empty() {
                            // Collect group names, skipping primary group
                            let mut names: Vec<String> = Vec::new();
                            for idx in selected_multi.iter() {
                                if let Some(g) = user_groups.get(*idx)
                                    && g.gid != user.primary_gid
                                {
                                    names.push(g.name.clone());
                                }
                            }
                            if names.is_empty() {
                                app.modal = Some(ModalState::Info {
                                    message: "No valid groups selected (cannot remove primary)."
                                        .to_string(),
                                });
                            } else {
                                let pending = PendingAction::RemoveUserFromGroups {
                                    username: user.name.clone(),
                                    groupnames: names,
                                };
                                if let Err(_e) = perform_pending_action(
                                    app,
                                    pending.clone(),
                                    app.sudo_password.clone(),
                                ) {
                                    app.modal = Some(ModalState::SudoPrompt {
                                        next: pending,
                                        password: String::new(),
                                        error: None,
                                    });
                                }
                            }
                        } else if let Some(group) = user_groups.get(*selected) {
                            if group.gid == user.primary_gid {
                                app.modal = Some(ModalState::Info {
                                    message: "Cannot remove user from primary group.".to_string(),
                                });
                            } else {
                                let pending = PendingAction::RemoveUserFromGroup {
                                    username: user.name.clone(),
                                    groupname: group.name.clone(),
                                };
                                if let Err(_e) = perform_pending_action(
                                    app,
                                    pending.clone(),
                                    app.sudo_password.clone(),
                                ) {
                                    app.modal = Some(ModalState::SudoPrompt {
                                        next: pending,
                                        password: String::new(),
                                        error: None,
                                    });
                                }
                            }
                        } else {
                            close_modal(app);
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyDetailsMenu { selected }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                app.modal = Some(ModalState::ModifyMenu { selected: 2 });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                } else {
                    *selected = 2;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected < 2 {
                    *selected += 1;
                } else {
                    *selected = 0;
                }
            }
            KeyCode::Enter => match *selected {
                0 => {
                    app.modal = Some(ModalState::ModifyTextInput {
                        field: ModifyField::Username,
                        value: String::new(),
                    })
                }
                1 => {
                    app.modal = Some(ModalState::ModifyTextInput {
                        field: ModifyField::Fullname,
                        value: String::new(),
                    })
                }
                2 => {
                    let adapter = crate::sys::SystemAdapter::new();
                    let shells = adapter.list_shells().unwrap_or_default();
                    app.modal = Some(ModalState::ModifyShell {
                        selected: 0,
                        offset: 0,
                        shells,
                    });
                }
                _ => {}
            },
            _ => {}
        },
        Some(ModalState::ModifyShell {
            selected,
            offset,
            shells,
        }) => {
            let total = shells.len();
            match key.code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Backspace => {
                    app.modal = Some(ModalState::ModifyDetailsMenu { selected: 2 });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                        if *selected < *offset {
                            *offset = *selected;
                        }
                    } else if total > 0 {
                        *selected = total.saturating_sub(1);
                        *offset = *selected;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total {
                        *selected += 1;
                    } else if total > 0 {
                        *selected = 0;
                        *offset = 0;
                    }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step {
                        *selected -= step;
                    } else {
                        *selected = 0;
                    }
                    if *selected < *offset {
                        *offset = *selected;
                    }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Enter => {
                    if let (Some(user), Some(new_shell)) = (
                        app.users.get(app.selected_user_index),
                        shells.get(*selected),
                    ) {
                        let pending = PendingAction::ChangeShell {
                            username: user.name.clone(),
                            new_shell: new_shell.clone(),
                        };
                        if let Err(_e) =
                            perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                        {
                            app.modal = Some(ModalState::SudoPrompt {
                                next: pending,
                                password: String::new(),
                                error: None,
                            });
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyTextInput { field, value }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Enter => {
                if let Some(user) = app.users.get(app.selected_user_index) {
                    let pending = match field {
                        ModifyField::Username => PendingAction::ChangeUsername {
                            old_username: user.name.clone(),
                            new_username: value.clone(),
                        },
                        ModifyField::Fullname => PendingAction::ChangeFullname {
                            username: user.name.clone(),
                            new_fullname: value.clone(),
                        },
                    };
                    if let Err(_e) =
                        perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                    {
                        app.modal = Some(ModalState::SudoPrompt {
                            next: pending,
                            password: String::new(),
                            error: None,
                        });
                    }
                } else {
                    close_modal(app);
                }
            }
            KeyCode::Backspace => {
                if value.is_empty() {
                    app.modal = Some(ModalState::ModifyDetailsMenu { selected: 0 });
                } else {
                    value.pop();
                }
            }
            KeyCode::Char(c) => {
                value.push(c);
            }
            _ => {}
        },
        Some(ModalState::DeleteConfirm {
            selected,
            allowed,
            delete_home,
        }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                app.modal = Some(ModalState::Actions { selected: 1 });
            }
            KeyCode::Char(' ') => {
                *delete_home = !*delete_home;
            }
            KeyCode::Left | KeyCode::Right => {
                *selected = if *selected == 0 { 1 } else { 0 };
            }
            KeyCode::Enter => {
                if *selected == 0 {
                    if *allowed {
                        if let Some(user) = app.users.get(app.selected_user_index) {
                            let pending = PendingAction::DeleteUser {
                                username: user.name.clone(),
                                delete_home: *delete_home,
                            };
                            if let Err(_e) = perform_pending_action(
                                app,
                                pending.clone(),
                                app.sudo_password.clone(),
                            ) {
                                app.modal = Some(ModalState::SudoPrompt {
                                    next: pending,
                                    password: String::new(),
                                    error: None,
                                });
                            }
                        } else {
                            close_modal(app);
                        }
                    } else {
                        app.modal = Some(ModalState::Info {
                            message: "Deletion not allowed.".to_string(),
                        });
                    }
                } else {
                    close_modal(app);
                }
            }
            _ => {}
        },
        Some(ModalState::GroupsActions {
            selected,
            target_gid,
        }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => close_modal(app),
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                } else {
                    *selected = 2;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected < 2 {
                    *selected += 1;
                } else {
                    *selected = 0;
                }
            }
            KeyCode::Enter => match *selected {
                0 => {
                    app.modal = Some(ModalState::GroupAddInput {
                        name: String::new(),
                    })
                }
                1 => app.modal = Some(ModalState::GroupDeleteConfirm { selected: 1 }),
                2 => {
                    app.modal = Some(ModalState::GroupModifyMenu {
                        selected: 0,
                        target_gid: *target_gid,
                    })
                }
                _ => {}
            },
            _ => {}
        },
        Some(ModalState::GroupAddInput { name }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Enter => {
                let pending = PendingAction::CreateGroup {
                    groupname: name.clone(),
                };
                if let Err(_e) =
                    perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                {
                    app.modal = Some(ModalState::SudoPrompt {
                        next: pending,
                        password: String::new(),
                        error: None,
                    });
                }
            }
            KeyCode::Backspace => {
                if name.is_empty() {
                    app.modal = Some(ModalState::GroupsActions {
                        selected: 0,
                        target_gid: None,
                    });
                } else {
                    name.pop();
                }
            }
            KeyCode::Char(c) => {
                name.push(c);
            }
            _ => {}
        },
        Some(ModalState::GroupDeleteConfirm { selected }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                app.modal = Some(ModalState::GroupsActions {
                    selected: 1,
                    target_gid: None,
                });
            }
            KeyCode::Left | KeyCode::Right => {
                *selected = if *selected == 0 { 1 } else { 0 };
            }
            KeyCode::Enter => {
                if *selected == 0 {
                    let group_name_opt = app
                        .groups
                        .get(app.selected_group_index)
                        .map(|g| g.name.clone());
                    if let Some(group_name) = group_name_opt {
                        let pending = PendingAction::DeleteGroup {
                            groupname: group_name.clone(),
                        };
                        if let Err(_e) =
                            perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                        {
                            app.modal = Some(ModalState::SudoPrompt {
                                next: pending,
                                password: String::new(),
                                error: None,
                            });
                        }
                    } else {
                        close_modal(app);
                    }
                } else {
                    close_modal(app);
                }
            }
            _ => {}
        },
        Some(ModalState::GroupModifyMenu {
            selected,
            target_gid,
        }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                app.modal = Some(ModalState::GroupsActions {
                    selected: 2,
                    target_gid: *target_gid,
                });
            }
            KeyCode::Up | KeyCode::Char('k') => {
                if *selected > 0 {
                    *selected -= 1;
                } else {
                    *selected = 2;
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if *selected < 2 {
                    *selected += 1;
                } else {
                    *selected = 0;
                }
            }
            KeyCode::Enter => match *selected {
                0 => {
                    app.modal = Some(ModalState::GroupModifyAddMembers {
                        selected: 0,
                        offset: 0,
                        target_gid: *target_gid,
                        selected_multi: Vec::new(),
                    })
                }
                1 => {
                    app.modal = Some(ModalState::GroupModifyRemoveMembers {
                        selected: 0,
                        offset: 0,
                        target_gid: *target_gid,
                        selected_multi: Vec::new(),
                    })
                }
                2 => {
                    let effective_gid = if let Some(gid) = *target_gid {
                        gid
                    } else {
                        app.groups
                            .get(app.selected_group_index)
                            .map(|g| g.gid)
                            .unwrap_or(0)
                    };
                    if effective_gid < 1000 {
                        let gname = app
                            .groups
                            .iter()
                            .find(|g| g.gid == effective_gid)
                            .map(|g| g.name.clone())
                            .unwrap_or_else(|| "<unknown>".to_string());
                        app.modal = Some(ModalState::Info {
                            message: format!(
                                "Renaming system groups is disabled ({}: GID {}).",
                                gname, effective_gid
                            ),
                        });
                    } else {
                        app.modal = Some(ModalState::GroupRenameInput {
                            name: String::new(),
                            target_gid: *target_gid,
                        });
                    }
                }
                _ => {}
            },
            _ => {}
        },
        Some(ModalState::GroupRenameInput { name, target_gid }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                if name.is_empty() {
                    app.modal = Some(ModalState::GroupModifyMenu {
                        selected: 2,
                        target_gid: *target_gid,
                    });
                } else {
                    name.pop();
                }
            }
            KeyCode::Char(c) => {
                name.push(c);
            }
            KeyCode::Enter => {
                let (old_opt, gid_opt) = if let Some(gid) = *target_gid {
                    (
                        app.groups
                            .iter()
                            .find(|g| g.gid == gid)
                            .map(|g| g.name.clone()),
                        Some(gid),
                    )
                } else {
                    let opt = app.groups.get(app.selected_group_index);
                    (opt.map(|g| g.name.clone()), opt.map(|g| g.gid))
                };
                if let Some(gid) = gid_opt
                    && gid < 1000
                {
                    let gname = app
                        .groups
                        .iter()
                        .find(|g| g.gid == gid)
                        .map(|g| g.name.clone())
                        .unwrap_or_else(|| "<unknown>".to_string());
                    app.modal = Some(ModalState::Info {
                        message: format!(
                            "Renaming system groups is disabled ({}: GID {}).",
                            gname, gid
                        ),
                    });
                    return;
                }

                if let Some(old) = old_opt {
                    if name.trim().is_empty() {
                        app.modal = Some(ModalState::Info {
                            message: "Group name cannot be empty".to_string(),
                        });
                    } else {
                        let pending = PendingAction::RenameGroup {
                            old_name: old,
                            new_name: name.trim().to_string(),
                        };
                        if let Err(_e) =
                            perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                        {
                            app.modal = Some(ModalState::SudoPrompt {
                                next: pending,
                                password: String::new(),
                                error: None,
                            });
                        }
                    }
                } else {
                    close_modal(app);
                }
            }
            _ => {}
        },
        Some(ModalState::GroupModifyAddMembers {
            selected,
            offset,
            target_gid,
            selected_multi,
        }) => {
            let total = app.users_all.len();
            match key.code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Backspace => {
                    app.modal = Some(ModalState::GroupModifyMenu {
                        selected: 0,
                        target_gid: *target_gid,
                    });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                        if *selected < *offset {
                            *offset = *selected;
                        }
                    } else if total > 0 {
                        *selected = total.saturating_sub(1);
                        *offset = *selected;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total {
                        *selected += 1;
                    } else if total > 0 {
                        *selected = 0;
                        *offset = 0;
                    }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step {
                        *selected -= step;
                    } else {
                        *selected = 0;
                    }
                    if *selected < *offset {
                        *offset = *selected;
                    }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    if let Some(pos) = selected_multi.iter().position(|&i| i == *selected) {
                        selected_multi.remove(pos);
                    } else {
                        selected_multi.push(*selected);
                    }
                }
                KeyCode::Enter => {
                    let group_name = if let Some(gid) = *target_gid {
                        app.groups
                            .iter()
                            .find(|g| g.gid == gid)
                            .map(|g| g.name.clone())
                    } else {
                        app.groups
                            .get(app.selected_group_index)
                            .map(|g| g.name.clone())
                    };
                    if let Some(group_name) = group_name {
                        if !selected_multi.is_empty() {
                            let mut usernames: Vec<String> =
                                Vec::with_capacity(selected_multi.len());
                            for idx in selected_multi.iter() {
                                if let Some(u) = app.users_all.get(*idx) {
                                    usernames.push(u.name.clone());
                                }
                            }
                            if !usernames.is_empty() {
                                let pending = PendingAction::AddMembersToGroup {
                                    groupname: group_name.clone(),
                                    usernames,
                                };
                                if let Err(_e) = perform_pending_action(
                                    app,
                                    pending.clone(),
                                    app.sudo_password.clone(),
                                ) {
                                    app.modal = Some(ModalState::SudoPrompt {
                                        next: pending,
                                        password: String::new(),
                                        error: None,
                                    });
                                }
                            } else {
                                close_modal(app);
                            }
                        } else if let Some(user_name) =
                            app.users_all.get(*selected).map(|u| u.name.clone())
                        {
                            let pending = PendingAction::AddUserToGroup {
                                username: user_name.clone(),
                                groupname: group_name.clone(),
                            };
                            if let Err(_e) = perform_pending_action(
                                app,
                                pending.clone(),
                                app.sudo_password.clone(),
                            ) {
                                app.modal = Some(ModalState::SudoPrompt {
                                    next: pending,
                                    password: String::new(),
                                    error: None,
                                });
                            }
                        } else {
                            close_modal(app);
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyRemoveMembers {
            selected,
            offset,
            target_gid,
            selected_multi,
        }) => {
            let group_name = if let Some(gid) = *target_gid {
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
            let members: Vec<String> = if let Some(gid) = *target_gid {
                app.groups
                    .iter()
                    .find(|g| g.gid == gid)
                    .map(|g| g.members.clone())
                    .unwrap_or_default()
            } else {
                app.groups
                    .get(app.selected_group_index)
                    .map(|g| g.members.clone())
                    .unwrap_or_default()
            };
            let total = members.len();
            match key.code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Backspace => {
                    app.modal = Some(ModalState::GroupModifyMenu {
                        selected: 1,
                        target_gid: *target_gid,
                    });
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 {
                        *selected -= 1;
                        if *selected < *offset {
                            *offset = *selected;
                        }
                    } else if total > 0 {
                        *selected = total.saturating_sub(1);
                        *offset = *selected;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total {
                        *selected += 1;
                    } else if total > 0 {
                        *selected = 0;
                        *offset = 0;
                    }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step {
                        *selected -= step;
                    } else {
                        *selected = 0;
                    }
                    if *selected < *offset {
                        *offset = *selected;
                    }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Char(' ') => {
                    if let Some(pos) = selected_multi.iter().position(|&i| i == *selected) {
                        selected_multi.remove(pos);
                    } else {
                        selected_multi.push(*selected);
                    }
                }
                KeyCode::Enter => {
                    let gname_opt = if let Some(gid) = *target_gid {
                        app.groups
                            .iter()
                            .find(|g| g.gid == gid)
                            .map(|g| g.name.clone())
                    } else {
                        Some(group_name.clone())
                    };
                    if let Some(group_name) = gname_opt {
                        if !selected_multi.is_empty() {
                            let mut usernames: Vec<String> =
                                Vec::with_capacity(selected_multi.len());
                            for idx in selected_multi.iter() {
                                if let Some(u) = members.get(*idx) {
                                    usernames.push(u.clone());
                                }
                            }
                            if !usernames.is_empty() {
                                let pending = PendingAction::RemoveMembersFromGroup {
                                    groupname: group_name.clone(),
                                    usernames,
                                };
                                if let Err(_e) = perform_pending_action(
                                    app,
                                    pending.clone(),
                                    app.sudo_password.clone(),
                                ) {
                                    app.modal = Some(ModalState::SudoPrompt {
                                        next: pending,
                                        password: String::new(),
                                        error: None,
                                    });
                                }
                            } else {
                                close_modal(app);
                            }
                        } else if let Some(username) = members.get(*selected) {
                            let pending = PendingAction::RemoveUserFromGroup {
                                username: username.clone(),
                                groupname: group_name.clone(),
                            };
                            if let Err(_e) = perform_pending_action(
                                app,
                                pending.clone(),
                                app.sudo_password.clone(),
                            ) {
                                app.modal = Some(ModalState::SudoPrompt {
                                    next: pending,
                                    password: String::new(),
                                    error: None,
                                });
                            }
                        } else {
                            close_modal(app);
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::UserAddInput {
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
        }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Up => {
                if *selected > 0 {
                    *selected -= 1;
                }
            }
            KeyCode::Down => {
                if *selected < 5 {
                    *selected += 1;
                }
            }
            KeyCode::Backspace => match *selected {
                0 => {
                    if name.is_empty() {
                        close_modal(app);
                    } else {
                        name.pop();
                    }
                }
                1 => {
                    if password.is_empty() {
                        close_modal(app);
                    } else {
                        password.pop();
                    }
                }
                2 => {
                    if confirm.is_empty() {
                        close_modal(app);
                    } else {
                        confirm.pop();
                    }
                }
                _ => {}
            },
            KeyCode::Char(' ') => match *selected {
                3 => {
                    *create_home = !*create_home;
                }
                4 => {
                    *add_to_wheel = !*add_to_wheel;
                }
                _ => {}
            },
            KeyCode::Char(c) => match *selected {
                0 => name.push(c),
                1 => password.push(c),
                2 => confirm.push(c),
                _ => {}
            },
            KeyCode::Enter => {
                if *selected == 5 {
                    let uname = name.trim().to_string();
                    if uname.is_empty() {
                        app.modal = Some(ModalState::Info {
                            message: "Username cannot be empty".to_string(),
                        });
                    } else if (!password.is_empty() || !confirm.is_empty()) && *password != *confirm
                    {
                        app.modal = Some(ModalState::Info {
                            message: "Passwords do not match".to_string(),
                        });
                    } else {
                        let pending = PendingAction::CreateUserWithOptions {
                            username: uname,
                            password: if password.is_empty() {
                                None
                            } else {
                                Some(password.clone())
                            },
                            create_home: *create_home,
                            add_to_wheel: *add_to_wheel,
                        };
                        if let Err(_e) =
                            perform_pending_action(app, pending.clone(), app.sudo_password.clone())
                        {
                            app.modal = Some(ModalState::SudoPrompt {
                                next: pending,
                                password: String::new(),
                                error: None,
                            });
                        }
                    }
                }
            }
            _ => {}
        },
        Some(ModalState::SudoPrompt {
            next,
            password,
            error: _,
        }) => match key.code {
            KeyCode::Esc => close_modal(app),
            KeyCode::Backspace => {
                if password.is_empty() {
                    close_modal(app);
                } else {
                    password.pop();
                }
            }
            KeyCode::Enter => {
                let pw = password.clone();
                app.sudo_password = Some(pw.clone());
                let pending = next.clone();
                match perform_pending_action(app, pending.clone(), Some(pw)) {
                    Ok(_) => {}
                    Err(e) => {
                        app.modal = Some(ModalState::SudoPrompt {
                            next: pending,
                            password: String::new(),
                            error: Some(e.to_string()),
                        });
                    }
                }
            }
            KeyCode::Char(c) => {
                password.push(c);
            }
            _ => {}
        },
        Some(ModalState::Info { .. }) => match key.code {
            KeyCode::Esc | KeyCode::Enter => close_modal(app),
            _ => {}
        },
        None => {}
    }
}

/// Close the currently open modal and return to normal mode.
fn close_modal(app: &mut AppState) {
    app.modal = None;
    app.input_mode = InputMode::Normal;
}

/// Execute a queued privileged action and refresh state lists.
fn perform_pending_action(
    app: &mut AppState,
    pending: PendingAction,
    sudo_password: Option<String>,
) -> Result<()> {
    let adapter = crate::sys::SystemAdapter::with_sudo_password(sudo_password);
    match pending.clone() {
        PendingAction::AddUserToGroup {
            username,
            groupname,
        } => {
            adapter.add_user_to_group(&username, &groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Added '{}' to '{}'", username, groupname),
            });
        }
        PendingAction::RemoveUserFromGroup {
            username,
            groupname,
        } => {
            adapter.remove_user_from_group(&username, &groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Removed '{}' from '{}'", username, groupname),
            });
        }
        PendingAction::ChangeShell {
            username,
            new_shell,
        } => {
            adapter.change_user_shell(&username, &new_shell)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Changed shell to '{}'", new_shell),
            });
        }
        PendingAction::ChangeFullname {
            username,
            new_fullname,
        } => {
            adapter.change_user_fullname(&username, &new_fullname)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: "Changed successfully".to_string(),
            });
        }
        PendingAction::ChangeUsername {
            old_username,
            new_username,
        } => {
            adapter.change_username(&old_username, &new_username)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: "Changed successfully".to_string(),
            });
        }
        PendingAction::CreateGroup { groupname } => {
            adapter.create_group(&groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Created group '{}'", groupname),
            });
        }
        PendingAction::DeleteGroup { groupname } => {
            adapter.delete_group(&groupname)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Deleted group '{}'", groupname),
            });
        }
        PendingAction::RenameGroup { old_name, new_name } => {
            adapter.rename_group(&old_name, &new_name)?;
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Renamed group to '{}'", new_name),
            });
        }

        PendingAction::CreateUserWithOptions {
            username,
            password,
            create_home,
            add_to_wheel,
        } => {
            adapter.create_user(&username, create_home)?;
            let had_pw = password.is_some();
            if let Some(pw) = password {
                adapter.set_user_password(&username, &pw)?;
            }
            if add_to_wheel {
                adapter.add_user_to_group(&username, "wheel")?;
            }
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            apply_filters_and_search(app);
            let mut msg = format!(
                "Created user '{}'{}",
                username,
                if create_home { " with home" } else { "" }
            );
            if had_pw {
                msg.push_str(" with password");
            }
            if add_to_wheel {
                msg.push_str(" and wheel");
            }
            app.modal = Some(ModalState::Info { message: msg });
        }
        PendingAction::DeleteUser {
            username,
            delete_home,
        } => {
            adapter.delete_user(&username, delete_home)?;
            app.users_all = adapter.list_users().unwrap_or_default();
            app.users_all.sort_by_key(|u| u.uid);
            apply_filters_and_search(app);
            if app.selected_user_index >= app.users.len() {
                app.selected_user_index = app.users.len().saturating_sub(1);
            }
            let suffix = if delete_home { " and home" } else { "" };
            app.modal = Some(ModalState::Info {
                message: format!("Deleted user '{}'{}", username, suffix),
            });
        }
        PendingAction::SetPassword {
            username,
            password,
            must_change,
        } => {
            adapter.set_user_password(&username, &password)?;
            if must_change {
                let _ = adapter.expire_user_password(&username);
            }
            app.modal = Some(ModalState::Info {
                message: format!(
                    "Password set{}",
                    if must_change {
                        ", must change at next login"
                    } else {
                        ""
                    }
                ),
            });
        }
        PendingAction::ResetPassword { username } => {
            adapter.expire_user_password(&username)?;
            app.modal = Some(ModalState::Info {
                message: "Password reset (must change at next login)".to_string(),
            });
        }
        PendingAction::AddUserToGroups {
            username,
            groupnames,
        } => {
            for g in groupnames.iter() {
                adapter.add_user_to_group(&username, g)?;
            }
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Added '{}' to selected groups", username),
            });
        }
        PendingAction::RemoveUserFromGroups {
            username,
            groupnames,
        } => {
            for g in groupnames.iter() {
                adapter.remove_user_from_group(&username, g)?;
            }
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Removed '{}' from selected groups", username),
            });
        }
        PendingAction::AddMembersToGroup {
            groupname,
            usernames,
        } => {
            for u in usernames.iter() {
                adapter.add_user_to_group(u, &groupname)?;
            }
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Added selected users to '{}'", groupname),
            });
        }
        PendingAction::RemoveMembersFromGroup {
            groupname,
            usernames,
        } => {
            for u in usernames.iter() {
                adapter.remove_user_from_group(u, &groupname)?;
            }
            app.groups_all = adapter.list_groups().unwrap_or_default();
            app.groups_all.sort_by_key(|g| g.gid);
            apply_filters_and_search(app);
            app.modal = Some(ModalState::Info {
                message: format!("Removed selected users from '{}'", groupname),
            });
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::{KeyEvent, KeyModifiers};

    fn key(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn filter_menu_sets_users_filter_and_closes() {
        let mut app = AppState::default();
        app.active_tab = ActiveTab::Users;
        app.input_mode = InputMode::Modal;
        app.modal = Some(ModalState::FilterMenu { selected: 1 });

        handle_modal_key(&mut app, key(KeyCode::Enter));

        assert!(matches!(app.users_filter, Some(UsersFilter::OnlyUserIds)));
        assert!(app.modal.is_none());
        assert!(matches!(app.input_mode, InputMode::Normal));
    }

    #[test]
    fn actions_delete_opens_delete_confirm_with_allowed_flag() {
        let mut app = AppState::default();
        // Provide a deletable user (UID in 1000-1999)
        app.users = vec![crate::sys::SystemUser {
            uid: 1500,
            name: "testuser".to_string(),
            primary_gid: 1500,
            full_name: None,
            home_dir: "/home/testuser".to_string(),
            shell: "/bin/bash".to_string(),
        }];
        app.input_mode = InputMode::Modal;
        app.modal = Some(ModalState::Actions { selected: 1 });

        handle_modal_key(&mut app, key(KeyCode::Enter));

        match &app.modal {
            Some(ModalState::DeleteConfirm { allowed, .. }) => assert!(*allowed),
            other => panic!("unexpected modal state: {:?}", other),
        }
    }

    #[test]
    fn change_password_mismatch_shows_info() {
        let mut app = AppState::default();
        app.input_mode = InputMode::Modal;
        app.modal = Some(ModalState::ChangePassword {
            selected: 3, // Submit
            password: "secret".to_string(),
            confirm: "different".to_string(),
            must_change: false,
        });

        handle_modal_key(&mut app, key(KeyCode::Enter));

        match &app.modal {
            Some(ModalState::Info { message }) => {
                assert!(message.contains("Passwords do not match"))
            }
            other => panic!("expected Info modal, got {:?}", other),
        }
    }

    #[test]
    fn sudo_prompt_backspace_closes_when_empty() {
        let mut app = AppState::default();
        app.input_mode = InputMode::Modal;
        app.modal = Some(ModalState::SudoPrompt {
            next: PendingAction::ResetPassword {
                username: "user".to_string(),
            },
            password: String::new(),
            error: None,
        });

        handle_modal_key(&mut app, key(KeyCode::Backspace));

        assert!(app.modal.is_none());
        assert!(matches!(app.input_mode, InputMode::Normal));
    }
}
