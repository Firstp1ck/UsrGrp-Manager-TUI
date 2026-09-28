//! Event handling and explicit application effects.
//!
//! All mutations pass through the adapter-owned `OperationRequest` bridge.  The
//! application only displays redacted plans/reports; it never chooses commands,
//! receives runners, or retains sudo credentials.

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};

use crate::{
    app::{
        ActiveTab, AppState, DELETE_ROW_HOME, DELETE_ROW_NO, DELETE_ROW_YES, GroupsFocus,
        InputMode, ModalState, ModifyField, PendingAction, RunOptions, SecretInput, UsersFocus,
        filterconf::FiltersConfig, keymap::KeyAction,
    },
    error::{CoreError, CoreResult, Result},
    sys::{GroupName, OperationRequest, PasswordRecord, SecretString, UserName},
    ui,
};

/// Drive the TUI.  Construction and refresh are explicit effects; rendering is
/// immutable and does not touch the host.
pub fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    run_app_with(terminal, RunOptions::default())
}

/// Drive the TUI with command-line options (config root, dry run, log file).
pub fn run_app_with(
    terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>,
    options: RunOptions,
) -> Result<()> {
    let mut app = AppState::load_system_with(options);
    app.show_config_problems();
    let size = terminal.size()?;
    app.terminal_size = (size.width, size.height);
    loop {
        terminal.draw(|frame| ui::render(frame, &app))?;
        if event::poll(std::time::Duration::from_millis(100))?
            && handle_event(&mut app, event::read()?)
        {
            break Ok(());
        }
    }
}

/// Dispatch one terminal event. Returns `true` when the app should quit.
///
/// Only key presses reach the key handlers. A resize records the new size
/// for page keys; the next loop iteration redraws at that size. Mouse capture
/// is not enabled, so mouse, focus, and paste events are ignored.
fn handle_event(app: &mut AppState, event: Event) -> bool {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            if matches!(app.input_mode, InputMode::Normal)
                && matches!(app.keymap.resolve(&key), Some(KeyAction::Quit))
            {
                return true;
            }
            handle_key(app, key);
            false
        }
        Event::Resize(width, height) => {
            app.terminal_size = (width, height);
            false
        }
        _ => false,
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
        Some(KeyAction::StartSearch) => app.start_search(),
        Some(KeyAction::ClearSearch) => app.clear_search(),
        Some(KeyAction::Refresh) => app.refresh_accounts(),
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
                    password_reentry_required: false,
                    name_touched: false,
                    error: None,
                },
            ),
            ActiveTab::Groups => open_modal(
                app,
                ModalState::GroupAddInput {
                    name: String::new(),
                    error: None,
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
            move_selection(app, -(page_size(app) as isize))
        }
        Some(KeyAction::PageDown | KeyAction::MoveRightPage) => {
            move_selection(app, page_size(app) as isize)
        }
        Some(KeyAction::Ignore) | None => {}
    }
}

/// Keys while editing a table's query. Enter keeps the query, Esc restores
/// the one from before `/`; the filter applies on every keystroke.
fn handle_search_key(app: &mut AppState, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.cancel_search(),
        KeyCode::Enter => app.finish_search(),
        KeyCode::Backspace => app.edit_search(|query| {
            query.pop();
        }),
        KeyCode::Char(character)
            if !character.is_control() && app.query(app.search_tab()).len() < 256 =>
        {
            app.edit_search(|query| query.push(character))
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
                let modal = ModalState::DeleteConfirm {
                    selected: DELETE_ROW_NO,
                    allowed: !crate::app::is_default_protected_user(user),
                    delete_home: false,
                    username: user.name.clone(),
                    uid: user.uid,
                    home: user.home_dir.clone(),
                    is_self: user.name == app.current_username,
                    process_count: app
                        .diagnostics
                        .homes
                        .get(&user.name)
                        .and_then(|home| home.process_count),
                };
                open_modal(app, modal);
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

/// Rows the focused pane shows at the current terminal size, taken from the
/// same layout the renderer uses.
fn page_size(app: &AppState) -> usize {
    let (width, height) = app.terminal_size;
    let layout = ui::layout::main_layout(Rect::new(0, 0, width, height), app.show_keybinds);
    let pane = match (app.active_tab, app.users_focus, app.groups_focus) {
        (ActiveTab::Users, UsersFocus::UsersList, _)
        | (ActiveTab::Groups, _, GroupsFocus::GroupsList) => layout.table,
        (ActiveTab::Users, UsersFocus::MemberOf, _)
        | (ActiveTab::Groups, _, GroupsFocus::Members) => layout.members,
    };
    ui::layout::table_rows(pane)
}

fn move_selection(app: &mut AppState, delta: isize) {
    let state = (app.active_tab, app.users_focus, app.groups_focus);
    let len = match state {
        (ActiveTab::Users, UsersFocus::UsersList, _) => app.users.len(),
        (ActiveTab::Users, UsersFocus::MemberOf, _) => app.selected_user_groups().len(),
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
    let before = *selected;
    move_index(selected, len, delta);
    let parent_changed = *selected != before;
    // A new parent row has its own child list; start its cursor at the top
    // so the child pane never points past the end of the new list.
    if parent_changed {
        match state {
            (ActiveTab::Users, UsersFocus::UsersList, _) => app.selected_user_group_index = 0,
            (ActiveTab::Groups, _, GroupsFocus::GroupsList) => app.selected_group_member_index = 0,
            (ActiveTab::Users, UsersFocus::MemberOf, _)
            | (ActiveTab::Groups, _, GroupsFocus::Members) => {}
        }
    }
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

/// Choice lists move with Up/Down and k/j.
fn list_step(key: KeyEvent) -> Option<isize> {
    match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(-1),
        KeyCode::Down | KeyCode::Char('j') => Some(1),
        _ => None,
    }
}

/// What a key means in a yes/no dialog. Row 0 is Yes, row 1 is No.
enum Answer {
    /// Enter on the highlighted choice, or `y`.
    Confirm,
    /// `n`: choose No directly.
    Decline,
    /// Esc.
    Cancel,
    /// Movement or an unbound key.
    Other,
}

/// Left/Right, h/l, and Tab move between Yes and No; `y` and `n` choose
/// directly; Enter confirms the highlighted choice.
fn yes_no_answer(selected: &mut usize, key: KeyEvent) -> Answer {
    match key.code {
        KeyCode::Left
        | KeyCode::Right
        | KeyCode::Tab
        | KeyCode::BackTab
        | KeyCode::Char('h')
        | KeyCode::Char('l') => {
            *selected = usize::from(*selected == 0);
            Answer::Other
        }
        KeyCode::Char('y') => {
            *selected = 0;
            Answer::Confirm
        }
        KeyCode::Char('n') => {
            *selected = 1;
            Answer::Decline
        }
        KeyCode::Enter => Answer::Confirm,
        KeyCode::Esc => Answer::Cancel,
        _ => Answer::Other,
    }
}

fn handle_modal_key(app: &mut AppState, key: KeyEvent) {
    let Some((mut modal, detached)) = app.detach_top_modal() else {
        return;
    };
    match &mut modal {
        ModalState::FilterMenu { selected } => handle_filter_modal(app, selected, key),
        ModalState::Actions { selected } => handle_user_actions(app, selected, key),
        ModalState::ModifyMenu { selected } => handle_modify_menu(app, selected, key),
        ModalState::ModifyDetailsMenu { selected } => handle_modify_details(app, selected, key),
        ModalState::ModifyTextInput {
            field,
            value,
            error,
        } => handle_text_input(app, field, value, error, key),
        ModalState::ModifyShell {
            selected, shells, ..
        } => handle_shell_modal(app, selected, shells, key),
        ModalState::ModifyGroupsAdd {
            selected,
            selected_multi,
            filter,
            filtering,
            ..
        } => handle_user_membership(
            app,
            true,
            CandidateDialog {
                selected,
                selected_multi,
                filter,
                filtering,
            },
            key,
        ),
        ModalState::ModifyGroupsRemove {
            selected,
            selected_multi,
            filter,
            filtering,
            ..
        } => handle_user_membership(
            app,
            false,
            CandidateDialog {
                selected,
                selected_multi,
                filter,
                filtering,
            },
            key,
        ),
        ModalState::ModifyPasswordMenu { selected } => handle_password_menu(app, selected, key),
        ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
            error,
        } => handle_password_input(app, selected, password, confirm, must_change, error, key),
        ModalState::DeleteConfirm {
            selected,
            allowed,
            delete_home,
            username,
            ..
        } => handle_user_delete(app, selected, *allowed, username, delete_home, key),
        ModalState::ConfirmRemoveUserFromGroup {
            selected,
            group_name,
        } => handle_member_delete(app, selected, group_name, key),
        ModalState::GroupsActions {
            selected,
            target_gid,
        } => handle_group_actions(app, selected, target_gid, key),
        ModalState::GroupAddInput { name, error } => handle_group_add(app, name, error, key),
        ModalState::GroupDeleteConfirm {
            selected,
            target_gid,
        } => handle_group_delete(app, selected, target_gid, key),
        ModalState::GroupModifyMenu {
            selected,
            target_gid,
        } => handle_group_modify(app, selected, target_gid, key),
        ModalState::GroupRenameInput {
            name,
            target_gid,
            error,
        } => handle_group_rename(app, name, target_gid, error, key),
        ModalState::GroupModifyAddMembers {
            selected,
            target_gid,
            selected_multi,
            filter,
            filtering,
            ..
        } => handle_group_members(
            app,
            true,
            *target_gid,
            CandidateDialog {
                selected,
                selected_multi,
                filter,
                filtering,
            },
            key,
        ),
        ModalState::GroupModifyRemoveMembers {
            selected,
            target_gid,
            selected_multi,
            filter,
            filtering,
            ..
        } => handle_group_members(
            app,
            false,
            *target_gid,
            CandidateDialog {
                selected,
                selected_multi,
                filter,
                filtering,
            },
            key,
        ),
        ModalState::UserAddInput {
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
            password_reentry_required,
            name_touched,
            error,
        } => handle_user_add(
            app,
            selected,
            name,
            password,
            confirm,
            create_home,
            add_to_wheel,
            password_reentry_required,
            name_touched,
            error,
            key,
        ),
        ModalState::OperationConfirm { selected, .. } => {
            handle_operation_confirm(app, selected, key)
        }
        ModalState::SudoPrompt {
            password,
            error,
            attempts,
        } => handle_sudo_prompt(app, password, error, *attempts, key),
        ModalState::Info { .. } => {
            if matches!(key.code, KeyCode::Esc | KeyCode::Enter) {
                back(app)
            }
        }
        ModalState::Help { scroll } => match key.code {
            KeyCode::Esc | KeyCode::Enter => back(app),
            KeyCode::Up | KeyCode::Char('k') => *scroll = scroll.saturating_sub(1),
            KeyCode::Down | KeyCode::Char('j') => *scroll = scroll.saturating_add(1),
            KeyCode::PageUp => *scroll = scroll.saturating_sub(10),
            KeyCode::PageDown => *scroll = scroll.saturating_add(10),
            _ => {}
        },
    }
    app.reattach_modal(modal, detached);
}

fn handle_filter_modal(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    let maximum = match app.active_tab {
        ActiveTab::Users => 7,
        ActiveTab::Groups => 2,
    };
    if let Some(step) = list_step(key) {
        move_index(selected, maximum + 1, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter | KeyCode::Char(' ') => {
            let changed = match app.active_tab {
                ActiveTab::Users => toggle_user_filter(app, *selected),
                ActiveTab::Groups => {
                    app.groups_filter = match *selected {
                        0 => None,
                        1 => Some(crate::app::GroupsFilter::OnlyUserGids),
                        _ => Some(crate::app::GroupsFilter::OnlySystemGids),
                    };
                    true
                }
            };
            if changed {
                app.sort_and_filter();
                save_filters(app);
            }
        }
        _ => {}
    }
}

/// Apply Enter/Space on row `selected` of the users filter menu. Returns
/// false when the row is disabled: the shadow chips while shadow data is
/// unavailable.
fn toggle_user_filter(app: &mut AppState, selected: usize) -> bool {
    let shadow_known = matches!(app.diagnostics.shadow, crate::search::ShadowState::Known(_));
    let chips = &mut app.users_filter_chips;
    match selected {
        0 => {
            *chips = Default::default();
            app.users_filter = None;
        }
        // Human and system users exclude each other: turning one on turns the other off.
        1 => {
            let human = !app.human_filter_on();
            let system = !human && app.system_filter_on();
            app.set_uid_range_filters(human, system);
        }
        2 => {
            let system = !app.system_filter_on();
            let human = !system && app.human_filter_on();
            app.set_uid_range_filters(human, system);
        }
        3 => chips.inactive = !chips.inactive,
        4 => chips.no_home = !chips.no_home,
        5..=7 if !shadow_known => return false,
        5 => chips.locked = !chips.locked,
        6 => chips.no_password = !chips.no_password,
        7 => chips.expired = !chips.expired,
        _ => return false,
    }
    true
}

fn handle_user_actions(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    if let Some(step) = list_step(key) {
        move_index(selected, 2, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
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
    if let Some(step) = list_step(key) {
        move_index(selected, 4, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::ModifyGroupsAdd {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                },
            ),
            1 => open_modal(
                app,
                ModalState::ModifyGroupsRemove {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                },
            ),
            2 => open_modal(app, ModalState::ModifyDetailsMenu { selected: 0 }),
            _ => open_modal(app, ModalState::ModifyPasswordMenu { selected: 0 }),
        },
        _ => {}
    }
}

fn handle_modify_details(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    if let Some(step) = list_step(key) {
        move_index(selected, 3, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::ModifyTextInput {
                    field: ModifyField::Username,
                    value: String::new(),
                    error: None,
                },
            ),
            1 => open_modal(
                app,
                ModalState::ModifyTextInput {
                    field: ModifyField::Fullname,
                    value: String::new(),
                    error: None,
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

fn handle_text_input(
    app: &mut AppState,
    field: &ModifyField,
    value: &mut String,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    let live_error = |value: &str| match field {
        ModifyField::Username => live_name_error(UserName::new(value)),
        ModifyField::Fullname => None,
    };
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Backspace => {
            value.pop();
            *error = live_error(value);
        }
        KeyCode::Char(character) if !character.is_control() && value.len() < 4096 => {
            value.push(character);
            *error = live_error(value);
        }
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
                start_form_action(app, action, error);
            }
        }
        _ => {}
    }
}

fn handle_shell_modal(app: &mut AppState, selected: &mut usize, shells: &[String], key: KeyEvent) {
    if let Some(step) = list_step(key) {
        move_index(selected, shells.len(), step);
        return;
    }
    match key.code {
        KeyCode::Esc => back(app),
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
    mut dialog: CandidateDialog<'_>,
    key: KeyEvent,
) {
    let Some(user) = app.users.get(app.selected_user_index).cloned() else {
        back(app);
        return;
    };
    let groups = app
        .user_group_candidate_list(user.uid, adding, dialog.filter)
        .names;
    match candidate_key(&mut dialog, &groups, key) {
        CandidateKey::Handled => {}
        CandidateKey::Close => back(app),
        CandidateKey::Apply => {
            let names = if dialog.selected_multi.is_empty() {
                groups.get(*dialog.selected).cloned().into_iter().collect()
            } else {
                app.offered_user_groups(user.uid, adding, dialog.selected_multi)
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
    }
}

/// Longest filter a multi-select dialog accepts, in characters. Account
/// names are at most 32.
const MAX_CANDIDATE_FILTER: usize = 32;

/// The fields of a multi-select candidate dialog, borrowed from the modal.
struct CandidateDialog<'a> {
    /// Highlighted row of the filtered list.
    selected: &'a mut usize,
    /// Ticked names, in tick order.
    selected_multi: &'a mut Vec<String>,
    filter: &'a mut String,
    filtering: &'a mut bool,
}

/// What is left for the handler after `candidate_key`.
enum CandidateKey {
    /// The key moved, ticked, or edited the filter.
    Handled,
    /// Esc outside filter typing.
    Close,
    /// Enter outside filter typing.
    Apply,
}

/// Keys shared by the four multi-select dialogs. `list` is the filtered
/// candidate list the dialog shows.
///
/// `/` starts filter typing: printable characters and Backspace edit the
/// filter, Up/Down still move, and Enter or Esc stop typing. Otherwise
/// Up/Down/j/k move, Space ticks the highlighted row, and `*` ticks every
/// listed row (or clears them when all are ticked already).
fn candidate_key(dialog: &mut CandidateDialog<'_>, list: &[String], key: KeyEvent) -> CandidateKey {
    if *dialog.filtering {
        match key.code {
            KeyCode::Enter | KeyCode::Esc => *dialog.filtering = false,
            KeyCode::Up => move_index(dialog.selected, list.len(), -1),
            KeyCode::Down => move_index(dialog.selected, list.len(), 1),
            KeyCode::Backspace => {
                if dialog.filter.pop().is_some() {
                    *dialog.selected = 0;
                }
            }
            KeyCode::Char(character)
                if !character.is_control()
                    && dialog.filter.chars().count() < MAX_CANDIDATE_FILTER =>
            {
                dialog.filter.push(character);
                *dialog.selected = 0;
            }
            _ => {}
        }
        return CandidateKey::Handled;
    }
    if let Some(step) = list_step(key) {
        move_index(dialog.selected, list.len(), step);
        return CandidateKey::Handled;
    }
    match key.code {
        KeyCode::Esc => return CandidateKey::Close,
        KeyCode::Enter => return CandidateKey::Apply,
        KeyCode::Char('/') => *dialog.filtering = true,
        KeyCode::Char(' ') => {
            if let Some(name) = list.get(*dialog.selected) {
                toggle(dialog.selected_multi, name);
            }
        }
        KeyCode::Char('*') => toggle_all(dialog.selected_multi, list),
        _ => {}
    }
    CandidateKey::Handled
}

fn handle_password_menu(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    if let Some(step) = list_step(key) {
        move_index(selected, 2, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => {
            if *selected == 0 {
                open_modal(
                    app,
                    ModalState::ChangePassword {
                        selected: 0,
                        password: SecretInput::default(),
                        confirm: SecretInput::default(),
                        must_change: false,
                        error: None,
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
    error: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down => *selected = (*selected + 1).min(3),
        KeyCode::Char(' ') if *selected == 2 => *must_change = !*must_change,
        KeyCode::Backspace => match *selected {
            0 => {
                password.pop();
                *error = None;
            }
            1 => {
                confirm.pop();
                *error = None;
            }
            _ => {}
        },
        KeyCode::Char(character)
            if !character.is_control()
                && *selected < 2
                && password.len().max(confirm.len()) < 1024 =>
        {
            if *selected == 0 {
                password.push(character)
            } else {
                confirm.push(character)
            }
            *error = None;
        }
        KeyCode::Enter if *selected == 3 => {
            if password.is_empty() || !password.matches(confirm) {
                *error = Some("Passwords do not match or are empty.".to_owned());
            } else if let Some(user) = app.users.get(app.selected_user_index) {
                let username = user.name.clone();
                let secret = SecretString::new(password.take());
                // The password has left the form. If the submission fails,
                // the form stays open under the error, so empty the confirm
                // field too and make the user type both again.
                confirm.clear();
                match PasswordRecord::new(
                    UserName::new(&username).expect("observed username remains valid"),
                    secret,
                ) {
                    Ok(record) => start_password_action(
                        app,
                        PendingAction::SetPassword {
                            username,
                            must_change: *must_change,
                        },
                        record,
                        error,
                    ),
                    Err(problem) => show_form_error(app, error, &problem),
                }
            }
        }
        _ => {}
    }
}

fn handle_user_delete(
    app: &mut AppState,
    selected: &mut usize,
    allowed: bool,
    username: &str,
    delete_home: &mut bool,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Char(' ') => {
            if *selected == DELETE_ROW_HOME {
                *delete_home = !*delete_home;
            }
            return;
        }
        KeyCode::Enter if *selected == DELETE_ROW_HOME => return,
        _ => {}
    }
    if list_step(key).is_some() {
        // Two visual rows: the checkbox and the Yes/No line. Moving down to
        // the buttons lands on No, never on Yes.
        *selected = if *selected == DELETE_ROW_HOME {
            DELETE_ROW_NO
        } else {
            DELETE_ROW_HOME
        };
        return;
    }
    // `yes_no_answer` counts Yes as 0 and No as 1. From the checkbox a
    // horizontal move toggles 0 to 1, so it also lands on No.
    let before = usize::from(*selected == DELETE_ROW_NO);
    let mut choice = before;
    let answer = yes_no_answer(&mut choice, key);
    if choice != before || !matches!(answer, Answer::Other) {
        *selected = if choice == 0 {
            DELETE_ROW_YES
        } else {
            DELETE_ROW_NO
        };
    }
    match answer {
        Answer::Cancel | Answer::Decline => back(app),
        // Enter on No closes the dialog, like `n`, even for protected users.
        Answer::Confirm if *selected == DELETE_ROW_NO => back(app),
        Answer::Confirm if allowed => start_pending_action(
            app,
            PendingAction::DeleteUser {
                username: username.to_owned(),
                delete_home: *delete_home,
            },
        ),
        Answer::Confirm => open_modal(
            app,
            ModalState::Info {
                title: None,
                message: "Protected identities require explicit policy allowlisting.".to_owned(),
            },
        ),
        Answer::Other => {}
    }
}

fn handle_member_delete(app: &mut AppState, selected: &mut usize, encoded: &str, key: KeyEvent) {
    match yes_no_answer(selected, key) {
        Answer::Cancel | Answer::Decline => back(app),
        Answer::Confirm if *selected == 0 => {
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
        // Enter on No closes the dialog, like `n`.
        Answer::Confirm => back(app),
        Answer::Other => {}
    }
}

fn handle_group_actions(
    app: &mut AppState,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    let limit = if target_gid.is_some() { 1 } else { 2 };
    if let Some(step) = list_step(key) {
        move_index(selected, limit + 1, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match (*selected, *target_gid) {
            (0, _) if target_gid.is_none() => open_modal(
                app,
                ModalState::GroupAddInput {
                    name: String::new(),
                    error: None,
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

fn handle_group_add(
    app: &mut AppState,
    name: &mut String,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Backspace => {
            name.pop();
            *error = live_name_error(GroupName::new(&*name));
        }
        KeyCode::Char(character) if !character.is_control() && name.len() < 32 => {
            name.push(character);
            *error = live_name_error(GroupName::new(&*name));
        }
        KeyCode::Enter => start_form_action(
            app,
            PendingAction::CreateGroup {
                groupname: name.clone(),
            },
            error,
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
    match yes_no_answer(selected, key) {
        Answer::Cancel | Answer::Decline => back(app),
        Answer::Confirm if *selected == 0 => {
            let group = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index));
            if let Some(group) = group {
                if crate::app::is_default_protected_group(group) {
                    open_modal(
                        app,
                        ModalState::Info {
                            title: None,
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
        // Enter on No closes the dialog, like `n`.
        Answer::Confirm => back(app),
        Answer::Other => {}
    }
}

fn handle_group_modify(
    app: &mut AppState,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    if let Some(step) = list_step(key) {
        move_index(selected, 3, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::GroupModifyAddMembers {
                    selected: 0,
                    offset: 0,
                    target_gid: *target_gid,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                },
            ),
            1 => open_modal(
                app,
                ModalState::GroupModifyRemoveMembers {
                    selected: 0,
                    offset: 0,
                    target_gid: *target_gid,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                },
            ),
            _ => open_modal(
                app,
                ModalState::GroupRenameInput {
                    name: String::new(),
                    target_gid: *target_gid,
                    error: None,
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
    error: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Backspace => {
            name.pop();
            *error = live_name_error(GroupName::new(&*name));
        }
        KeyCode::Char(character) if !character.is_control() && name.len() < 32 => {
            name.push(character);
            *error = live_name_error(GroupName::new(&*name));
        }
        KeyCode::Enter => {
            let group = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index));
            if let Some(group) = group {
                if crate::app::is_default_protected_group(group) {
                    *error = Some(
                        "Protected group mutations require explicit policy allowlisting."
                            .to_owned(),
                    );
                } else {
                    start_form_action(
                        app,
                        PendingAction::RenameGroup {
                            old_name: group.name.clone(),
                            new_name: name.clone(),
                        },
                        error,
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
    target_gid: Option<u32>,
    mut dialog: CandidateDialog<'_>,
    key: KeyEvent,
) {
    let group = target_gid
        .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
        .or_else(|| app.groups.get(app.selected_group_index))
        .cloned();
    let Some(group) = group else {
        back(app);
        return;
    };
    let users = app
        .group_member_candidate_list(group.gid, adding, dialog.filter)
        .names;
    match candidate_key(&mut dialog, &users, key) {
        CandidateKey::Handled => {}
        CandidateKey::Close => back(app),
        CandidateKey::Apply => {
            let names = if dialog.selected_multi.is_empty() {
                users.get(*dialog.selected).cloned().into_iter().collect()
            } else {
                app.offered_group_members(group.gid, adding, dialog.selected_multi)
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
    password_reentry_required: &mut bool,
    name_touched: &mut bool,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    // Edits refresh the message: a bad username, including one emptied
    // again, stays visible; anything else is cleared once the user changes
    // the input. An untouched empty name is not an error yet.
    let live_error = |name: &str, touched: bool| {
        if touched {
            live_name_error(UserName::new(name))
        } else {
            None
        }
    };
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down => *selected = (*selected + 1).min(5),
        KeyCode::Char(' ') if *selected == 3 => *create_home = !*create_home,
        KeyCode::Char(' ') if *selected == 4 => *add_to_wheel = !*add_to_wheel,
        KeyCode::Backspace => match *selected {
            0 => {
                name.pop();
                *name_touched = true;
                *error = live_error(name, *name_touched);
            }
            1 => {
                password.pop();
                *error = live_error(name, *name_touched);
            }
            2 => {
                confirm.pop();
                *error = live_error(name, *name_touched);
            }
            _ => {}
        },
        KeyCode::Char(character)
            if !character.is_control() && *selected == 0 && name.len() < 32 =>
        {
            name.push(character);
            *name_touched = true;
            *error = live_error(name, *name_touched);
        }
        KeyCode::Char(character)
            if !character.is_control()
                && (*selected == 1 || *selected == 2)
                && password.len().max(confirm.len()) < 1024 =>
        {
            if *selected == 1 {
                password.push(character);
                *password_reentry_required = false;
            } else {
                confirm.push(character)
            }
            *error = live_error(name, *name_touched);
        }
        KeyCode::Enter if *selected == 5 && *password_reentry_required && password.is_empty() => {
            show_form_error(
                app,
                error,
                &CoreError::Validation {
                    field: "password",
                    reason: "Re-enter the password before submitting.",
                },
            )
        }
        KeyCode::Enter if *selected == 5 => {
            // Submitting counts as touching the name, so later password
            // edits keep reporting an empty one instead of clearing it.
            *name_touched = true;
            // Both fields empty means no password. Otherwise both must be
            // filled and equal: a filled confirm with an empty password is
            // an error, not an account without a password.
            if name.is_empty() || !password.matches(confirm) {
                *error = Some("Username is required and passwords must match.".to_owned());
            } else {
                let action = PendingAction::CreateUserWithOptions {
                    username: name.clone(),
                    set_password: !password.is_empty(),
                    create_home: *create_home,
                    add_to_wheel: *add_to_wheel,
                };
                if password.is_empty() {
                    start_form_action(app, action, error);
                } else {
                    let secret = SecretString::new(password.take());
                    // The password has left the form. If the submission
                    // fails, the form stays open under the error, so empty
                    // the confirm field too and refuse to submit until a new
                    // password is typed.
                    confirm.clear();
                    *password_reentry_required = true;
                    match UserName::new(name.clone())
                        .and_then(|username| PasswordRecord::new(username, secret))
                    {
                        Ok(record) => start_password_action(app, action, record, error),
                        Err(problem) => show_form_error(app, error, &problem),
                    }
                }
            }
        }
        _ => {}
    }
}

fn handle_operation_confirm(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    match yes_no_answer(selected, key) {
        Answer::Cancel | Answer::Decline => cancel_operation(app),
        Answer::Confirm if *selected == 0 => execute_pending_plan(app),
        Answer::Confirm => cancel_operation(app),
        Answer::Other => {}
    }
}

fn handle_sudo_prompt(
    app: &mut AppState,
    password: &mut SecretInput,
    error: &mut Option<String>,
    attempts: u8,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => cancel_operation(app),
        KeyCode::Backspace => {
            password.pop();
        }
        KeyCode::Char(character) if !character.is_control() && password.len() < 1024 => {
            password.push(character)
        }
        KeyCode::Enter => {
            if password.is_empty() {
                *error = Some("A password is required.".to_owned());
                return;
            }
            app.adapter
                .set_elevation_secret(SecretString::new(password.take()));
            // The prompt has done its job; if authentication is still
            // required or the password was rejected, execution opens a fresh
            // one in its place that carries the attempt count forward.
            app.pop_modal();
            run_pending_plan(app, attempts);
        }
        _ => {}
    }
}

/// Start an action from a form that just consumed a password.
fn start_password_action(
    app: &mut AppState,
    action: PendingAction,
    record: PasswordRecord,
    error: &mut Option<String>,
) {
    app.set_password_capability(record);
    start_form_action(app, action, error);
}

fn start_pending_action(app: &mut AppState, action: PendingAction) {
    if let Err(error) = try_start_pending_action(app, action) {
        show_error(app, &error);
    }
}

/// Start an action submitted from a form dialog. Validation problems stay in
/// the form as its inline error; anything else is shown above it.
fn start_form_action(app: &mut AppState, action: PendingAction, error: &mut Option<String>) {
    match try_start_pending_action(app, action) {
        Ok(()) => *error = None,
        Err(problem @ CoreError::Validation { .. }) => show_form_error(app, error, &problem),
        Err(problem) => show_error(app, &problem),
    }
}

/// Build and prepare the request, opening its confirmation on success. On
/// failure nothing stays pending and the caller reports the error.
fn try_start_pending_action(app: &mut AppState, action: PendingAction) -> CoreResult<()> {
    let result = operation_request(app, action).and_then(|request| prepare_request(app, request));
    if result.is_err() {
        app.clear_pending_operation();
    }
    result
}

fn prepare_request(app: &mut AppState, request: OperationRequest) -> CoreResult<()> {
    match app.adapter.prepare_operation(request) {
        Ok(plan) => {
            let prepared = crate::app::PreparedOperation { plan };
            let preview = prepared.preview();
            app.log_event("prepared", &preview.join("; "));
            app.last_preview = preview.clone();
            app.pending_operation = Some(prepared);
            let action = if app.dry_run {
                "Dry run: preview only, nothing will be executed"
            } else {
                "Apply the exact prepared operation"
            };
            open_modal(
                app,
                ModalState::OperationConfirm {
                    selected: 1,
                    action: action.to_owned(),
                    preview,
                },
            );
            Ok(())
        }
        Err(error) => Err(error),
    }
}

/// Rejected sudo passwords allowed before the pending operation is dropped.
const MAX_SUDO_ATTEMPTS: u8 = 3;

fn execute_pending_plan(app: &mut AppState) {
    run_pending_plan(app, 0);
}

/// Execute the pending plan; `attempts` counts passwords already rejected
/// for it, so a rejection reopens the prompt with the next count.
fn run_pending_plan(app: &mut AppState, attempts: u8) {
    let Some(prepared) = app.pending_operation.as_ref() else {
        show_outcome(app, "No prepared operation is available.".to_owned());
        return;
    };
    let description = prepared.preview().join("; ");
    if app.dry_run {
        // The adapter is never consulted, so no sudo prompt and no command.
        app.clear_pending_operation();
        app.log_event("dry-run", &description);
        show_outcome(app, "Dry run: nothing was executed.".to_owned());
        return;
    }
    match app.adapter.execute_prepared_operation(&prepared.plan) {
        Ok(report) => {
            let message = report_message(&report);
            app.log_event("executed", &format!("{description} => {message}"));
            if let Some(failed) = &report.failed {
                // A long preview can push the failure past the line cap, so
                // the classified error also gets a line of its own.
                app.log_event("error", &classified_error_message(&failed.error));
            }
            app.last_report = Some(report);
            app.pending_operation = None;
            app.refresh_accounts();
            show_outcome(app, message);
        }
        Err(CoreError::AuthenticationRequired) => {
            open_modal(
                app,
                ModalState::SudoPrompt {
                    password: SecretInput::default(),
                    error: None,
                    attempts,
                },
            );
        }
        Err(error @ CoreError::AuthenticationDenied) => {
            log_error(app, &error);
            let attempts = attempts.saturating_add(1);
            if attempts >= MAX_SUDO_ATTEMPTS {
                app.clear_pending_operation();
                show_outcome(
                    app,
                    "Authentication failed three times. The operation was not run.".to_owned(),
                );
                return;
            }
            // The prepared operation stays pending so only the password
            // has to be typed again.
            open_modal(
                app,
                ModalState::SudoPrompt {
                    password: SecretInput::default(),
                    error: Some(format!(
                        "Password rejected (attempt {attempts} of {MAX_SUDO_ATTEMPTS})"
                    )),
                    attempts,
                },
            );
        }
        Err(error) => {
            app.clear_pending_operation();
            let message = log_error(app, &error);
            show_outcome(app, message);
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

/// Show a classified error above the current dialog and append it to the
/// operation log. Esc returns to the dialog so the input can be corrected.
fn show_error(app: &mut AppState, error: &CoreError) {
    let message = log_error(app, error);
    open_modal(
        app,
        ModalState::Info {
            title: None,
            message,
        },
    );
}

/// Keep a validation problem in the form that caused it, logged the same
/// way `show_error` logs.
fn show_form_error(app: &mut AppState, error: &mut Option<String>, problem: &CoreError) {
    *error = Some(log_error(app, problem));
}

/// Inline message for a name typed so far: the rule it breaks, or None.
fn live_name_error<T>(result: CoreResult<T>) -> Option<String> {
    match result {
        Ok(_) => None,
        Err(CoreError::Validation { reason, .. }) => Some(reason.to_owned()),
        Err(error) => Some(error.to_string()),
    }
}

fn log_error(app: &mut AppState, error: &CoreError) -> String {
    let message = classified_error_message(error);
    app.log_event("error", &message);
    message
}

/// Report how an operation ended. Every dialog that led to it is closed
/// first, so Esc from the result lands on the main view.
fn show_outcome(app: &mut AppState, message: String) {
    app.close_modals();
    open_modal(
        app,
        ModalState::Info {
            title: None,
            message,
        },
    );
}

fn save_filters(app: &mut AppState) {
    let path = app.configuration_write_path("filter.conf");
    if let Err(error) = FiltersConfig::save_from_app(app, &path) {
        app.record_config_message("filter", &error);
        open_modal(
            app,
            ModalState::Info {
                title: Some(crate::app::CONFIG_PROBLEMS_TITLE.to_owned()),
                message: format!(
                    "The filter change applies now but was not saved.\nfilter configuration: {error}"
                ),
            },
        );
    }
}

fn open_modal(app: &mut AppState, modal: ModalState) {
    app.push_modal(modal);
}

/// Close the current dialog and return to the one below it.
fn back(app: &mut AppState) {
    app.pop_modal();
}

/// Drop the prepared operation and close every dialog that led to it.
fn cancel_operation(app: &mut AppState) {
    app.clear_pending_operation();
    app.close_modals();
}

/// Tick `name`, or untick it when it is ticked already.
fn toggle(ticked: &mut Vec<String>, name: &str) {
    if let Some(position) = ticked.iter().position(|value| value == name) {
        ticked.remove(position);
    } else {
        ticked.push(name.to_owned());
    }
}

/// Tick every name in `list`, or untick them all when none is missing.
fn toggle_all(ticked: &mut Vec<String>, list: &[String]) {
    if list.iter().all(|name| ticked.contains(name)) {
        ticked.retain(|name| !list.contains(name));
    } else {
        for name in list {
            if !ticked.contains(name) {
                ticked.push(name.clone());
            }
        }
    }
}

fn selected_user_group(app: &AppState) -> Option<crate::sys::SystemGroup> {
    app.selected_user_groups()
        .get(app.selected_user_group_index)
        .map(|group| (*group).clone())
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
    fn resize_event_redraws_without_being_treated_as_a_key() {
        let mut app = AppState::new();
        let before_tab = app.active_tab;
        let before_mode = app.input_mode;
        assert!(!handle_event(&mut app, Event::Resize(80, 24)));
        assert!(!handle_event(&mut app, Event::Resize(20, 5)));
        assert_eq!(app.terminal_size, (20, 5));
        assert_eq!(app.active_tab, before_tab);
        assert_eq!(app.input_mode, before_mode);
        assert!(app.modal().is_none());
    }

    #[test]
    fn quit_key_press_event_requests_exit() {
        let mut app = AppState::new();
        let quit = KeyEvent::new(KeyCode::Char('q'), crossterm::event::KeyModifiers::NONE);
        assert_eq!(app.keymap.resolve(&quit), Some(KeyAction::Quit));
        assert!(handle_event(&mut app, Event::Key(quit)));
    }

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
        let mut app = isolated_app(adapter, snapshot);
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
        assert!(matches!(app.modal(), Some(ModalState::SudoPrompt { .. })));
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

    #[derive(Clone, Default)]
    struct SharedLog(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for SharedLog {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl SharedLog {
        fn text(&self) -> String {
            String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
        }
    }

    struct FailingLog;
    impl std::io::Write for FailingLog {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("disk full"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Root identity: without dry run the adapter would call `NoRun::run`
    /// and panic, so reaching the Info modal proves it was never consulted.
    fn root_app() -> AppState {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        isolated_app(adapter, snapshot)
    }

    #[test]
    fn dry_run_shows_preview_then_executes_nothing() {
        let mut app = root_app();
        app.dry_run = true;
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action.starts_with("Dry run") && preview == &["usermod -s /bin/bash alice"]
        ));
        execute_pending_plan(&mut app);
        assert!(matches!(
            app.modal(),
            Some(ModalState::Info { message, .. }) if message.contains("Dry run: nothing was executed")
        ));
        assert!(app.pending_operation.is_none());
        assert!(app.last_report.is_none());
    }

    #[test]
    fn log_file_records_prepared_dry_run_and_errors_without_passwords() {
        let log = SharedLog::default();
        let mut app = root_app();
        app.dry_run = true;
        app.set_logger(crate::app::Logger::from_writer(Box::new(log.clone())));
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        execute_pending_plan(&mut app);
        let record = PasswordRecord::new(
            UserName::new("alice").unwrap(),
            SecretString::new("fixture-password"),
        )
        .unwrap();
        start_password_action(
            &mut app,
            PendingAction::SetPassword {
                username: "alice".into(),
                must_change: false,
            },
            record,
            &mut None,
        );
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "not-absolute".to_owned(),
            },
        );
        let text = log.text();
        let lines: Vec<&str> = text.lines().collect();
        assert!(lines[0].ends_with("prepared: usermod -s /bin/bash alice"));
        assert!(lines[1].ends_with("dry-run: usermod -s /bin/bash alice"));
        assert!(lines[2].contains("prepared: ") && lines[2].contains("chpasswd"));
        assert!(lines[3].contains("error: E-VALIDATION"));
        assert_eq!(lines.len(), 4);
        assert!(!text.contains("fixture-password"));
    }

    /// Runs every command as a failure, so the adapter returns a report
    /// with a failed step instead of an error.
    struct FailRun;
    impl CommandRunner for FailRun {
        fn authenticate(&self, _: SecretString) -> CoreResult<ElevationGrant> {
            unreachable!("root identity never authenticates")
        }
        fn run(&self, _: ElevationGrant, _: &crate::sys::CommandSpec) -> CoreResult<CommandResult> {
            Err(CoreError::MissingExecutable { program: "usermod" })
        }
    }

    #[test]
    fn log_file_records_failed_step_error_on_its_own_line() {
        let log = SharedLog::default();
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(FailRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.set_logger(crate::app::Logger::from_writer(Box::new(log.clone())));
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        execute_pending_plan(&mut app);
        assert!(
            app.last_report
                .as_ref()
                .is_some_and(|report| report.failed.is_some())
        );
        let expected =
            classified_error_message(&CoreError::MissingExecutable { program: "usermod" });
        let text = log.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].ends_with("prepared: usermod -s /bin/bash alice"));
        assert!(lines[1].contains("executed: usermod -s /bin/bash alice => "));
        assert!(lines[2].ends_with(&format!(" error: {expected}")));
    }

    #[test]
    fn failed_step_error_keeps_its_own_line_when_preview_exceeds_line_cap() {
        let log = SharedLog::default();
        let mut snapshot = fixture_snapshot();
        let groupnames: Vec<String> = (0..128)
            .map(|index| format!("group-with-a-long-name-{index:09}"))
            .collect();
        for (index, name) in groupnames.iter().enumerate() {
            assert_eq!(name.len(), 32);
            snapshot.groups.push(AccountGroup {
                gid: Gid(2000 + u32::try_from(index).unwrap()),
                name: GroupName::new(name).unwrap(),
                members: vec![],
            });
        }
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(FailRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.set_logger(crate::app::Logger::from_writer(Box::new(log.clone())));
        start_pending_action(
            &mut app,
            PendingAction::AddUserToGroups {
                username: "alice".to_owned(),
                groupnames,
            },
        );
        assert!(app.pending_operation.is_some());
        assert!(app.last_preview.join("; ").len() > 4096);
        execute_pending_plan(&mut app);
        assert!(
            app.last_report
                .as_ref()
                .is_some_and(|report| report.failed.is_some())
        );
        let expected =
            classified_error_message(&CoreError::MissingExecutable { program: "usermod" });
        let text = log.text();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].starts_with("1700000000 prepared: "));
        // The executed line is cut at the cap before the report text.
        assert!(lines[1].starts_with("1700000000 executed: "));
        assert!(!lines[1].contains(" => "));
        assert_eq!(lines[2], format!("1700000000 error: {expected}"));
    }

    #[test]
    fn log_write_failure_disables_log_and_records_reason() {
        let mut app = AppState::new();
        app.set_logger(crate::app::Logger::from_writer(Box::new(FailingLog)));
        app.log_event("error", "first");
        app.log_event("error", "second");
        assert_eq!(app.diagnostics.config_messages.len(), 1);
        assert!(app.diagnostics.config_messages[0].contains("log file disabled"));
    }

    fn press(app: &mut AppState, code: KeyCode) {
        handle_key(
            app,
            KeyEvent::new(code, crossterm::event::KeyModifiers::NONE),
        );
    }

    fn user_app() -> AppState {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    /// Actions > Modify > Modify details > Shell > Enter: the confirm dialog
    /// for a shell change sits on top of four menus.
    fn open_shell_change_confirm(app: &mut AppState) {
        press(app, KeyCode::Enter);
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::ModifyShell { .. })));
        press(app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { .. })
        ));
        assert_eq!(app.modal_depth(), 5);
    }

    #[test]
    fn esc_pops_one_modal_level_at_a_time() {
        let mut app = user_app();
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::Actions { .. })));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::ModifyMenu { .. })));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.modal(), Some(ModalState::Actions { .. })));
        assert_eq!(app.input_mode, InputMode::Modal);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn backspace_pops_menus_but_edits_text_inputs() {
        let mut app = user_app();
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyDetailsMenu { .. })
        ));
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char('a'));
        press(&mut app, KeyCode::Char('b'));
        press(&mut app, KeyCode::Backspace);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyTextInput { value, .. }) if value == "a"
        ));
        assert_eq!(app.modal_depth(), 4);
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyDetailsMenu { .. })
        ));
        press(&mut app, KeyCode::Backspace);
        // The parent menu keeps the row it was left on.
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyMenu { selected: 2 })
        ));
        press(&mut app, KeyCode::Backspace);
        assert!(matches!(app.modal(), Some(ModalState::Actions { .. })));
        press(&mut app, KeyCode::Backspace);
        assert!(app.modal().is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn esc_on_operation_confirm_cancels_and_closes_every_modal() {
        let mut app = user_app();
        open_shell_change_confirm(&mut app);
        assert!(app.pending_operation.is_some());
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
        assert!(app.pending_operation.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn esc_on_sudo_prompt_cancels_and_closes_every_modal() {
        let mut app = user_app();
        press(&mut app, KeyCode::Enter);
        // The fixture's only shell is already alice's, so pick one that
        // needs a real change and therefore authentication.
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::SudoPrompt { .. })));
        assert_eq!(app.modal_depth(), 3);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
        assert!(app.pending_operation.is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn operation_result_replaces_the_stack_so_esc_reaches_main_view() {
        let mut app = root_app();
        app.dry_run = true;
        open_shell_change_confirm(&mut app);
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::Info { .. })));
        assert_eq!(app.modal_depth(), 1);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
    }

    #[test]
    fn group_name_validation_stays_inline_in_the_form() {
        let mut app = user_app();
        open_modal(
            &mut app,
            ModalState::GroupAddInput {
                name: String::new(),
                error: None,
            },
        );
        press(&mut app, KeyCode::Char('!'));
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some("must start with an ASCII letter or underscore")
        );
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.modal_depth(), 1);
        assert!(app.pending_operation.is_none());
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupAddInput { name, error: Some(error) })
                if name == "!" && error.contains("E-VALIDATION")
        ));
        // Fixing the name clears the message while typing.
        press(&mut app, KeyCode::Backspace);
        press(&mut app, KeyCode::Char('d'));
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    fn open_user_add(app: &mut AppState) {
        open_modal(
            app,
            ModalState::UserAddInput {
                selected: 0,
                name: String::new(),
                password: SecretInput::default(),
                confirm: SecretInput::default(),
                create_home: true,
                add_to_wheel: false,
                password_reentry_required: false,
                name_touched: false,
                error: None,
            },
        );
    }

    fn type_text(app: &mut AppState, text: &str) {
        for character in text.chars() {
            press(app, KeyCode::Char(character));
        }
    }

    #[test]
    fn create_user_password_mismatch_keeps_the_form_open() {
        let mut app = root_app();
        open_user_add(&mut app);
        type_text(&mut app, "bob");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "first-secret");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "other-secret");
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.modal_depth(), 1);
        assert!(app.pending_operation.is_none());
        assert!(matches!(
            app.modal(),
            Some(ModalState::UserAddInput { name, error: Some(_), .. }) if name == "bob"
        ));
        let screen = rendered(&app);
        assert!(screen.contains("Username is required and passwords must match."));
    }

    #[test]
    fn create_user_username_is_validated_while_typing() {
        let mut app = root_app();
        open_user_add(&mut app);
        type_text(&mut app, "1bob");
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some("must start with an ASCII letter or underscore")
        );
        for _ in 0..4 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "bob");
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    #[test]
    fn create_user_keeps_empty_username_error_while_editing_passwords() {
        let mut app = root_app();
        open_user_add(&mut app);
        // A fresh form, and a password typed before the name, show nothing.
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
        type_text(&mut app, "a");
        press(&mut app, KeyCode::Backspace);
        let empty_name = Some("must contain 1 to 32 bytes");
        assert_eq!(app.modal().and_then(ModalState::form_error), empty_name);
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "x");
        assert_eq!(app.modal().and_then(ModalState::form_error), empty_name);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.modal().and_then(ModalState::form_error), empty_name);
    }

    #[test]
    fn create_user_keeps_username_error_after_an_empty_submission() {
        let mut app = root_app();
        open_user_add(&mut app);
        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.modal().and_then(ModalState::form_error).is_some());
        for _ in 0..4 {
            press(&mut app, KeyCode::Up);
        }
        type_text(&mut app, "x");
        assert!(app.modal().and_then(ModalState::form_error).is_some());
    }

    #[test]
    fn create_user_untouched_empty_username_is_not_reported_while_typing() {
        let mut app = root_app();
        open_user_add(&mut app);
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "x");
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    #[test]
    fn set_password_mismatch_keeps_the_form_open() {
        let mut app = root_app();
        open_modal(
            &mut app,
            ModalState::ChangePassword {
                selected: 0,
                password: SecretInput::default(),
                confirm: SecretInput::default(),
                must_change: false,
                error: None,
            },
        );
        type_text(&mut app, "first-secret");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "other-secret");
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.modal_depth(), 1);
        assert!(app.pending_operation.is_none());
        assert!(matches!(
            app.modal(),
            Some(ModalState::ChangePassword { error: Some(_), .. })
        ));
        assert!(rendered(&app).contains("Passwords do not match or are empty."));
        // Editing either field clears the message.
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    #[test]
    fn failed_create_user_submission_requires_password_reentry() {
        let mut snapshot = fixture_snapshot();
        snapshot.groups.push(AccountGroup {
            gid: Gid(998),
            name: GroupName::new(crate::app::sudo_group_name()).unwrap(),
            members: vec![],
        });
        // Root identity: no sudo prompt. Nothing here executes a plan, so
        // `NoRun` is never reached.
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        open_user_add(&mut app);
        type_text(&mut app, "bob");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "fixture-password");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "fixture-password");
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Down);
        // The sudo group is blocked by policy, so preparation fails and the
        // reason is shown inside the form.
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_none());
        assert_eq!(app.modal_depth(), 1);
        assert!(matches!(
            app.modal(),
            Some(ModalState::UserAddInput {
                password_reentry_required: true,
                error: Some(_),
                ..
            })
        ));

        // Untick the sudo box and resubmit without touching either secret field.
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_none());
        assert!(matches!(
            app.modal(),
            Some(ModalState::UserAddInput { error: Some(message), .. })
                if message.contains("Re-enter the password")
        ));

        // Typing the password again lets the submission through with it set.
        for _ in 0..4 {
            press(&mut app, KeyCode::Up);
        }
        type_text(&mut app, "fixture-password");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "fixture-password");
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_some());
        assert!(
            app.last_preview
                .iter()
                .any(|step| step.contains("chpasswd"))
        );
    }

    #[test]
    fn yes_no_dialogs_answer_y_and_n_directly() {
        let mut app = root_app();
        app.dry_run = true;
        press(&mut app, KeyCode::Delete);
        assert!(matches!(
            app.modal(),
            Some(ModalState::DeleteConfirm {
                selected: DELETE_ROW_NO,
                ..
            })
        ));
        press(&mut app, KeyCode::Char('n'));
        assert!(app.modal().is_none());

        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { selected: 1, .. })
        ));
        assert!(app.pending_operation.is_some());
        press(&mut app, KeyCode::Char('n'));
        assert!(app.modal().is_none());
        assert!(app.pending_operation.is_none());

        press(&mut app, KeyCode::Delete);
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::Info { message, .. }) if message.contains("Dry run: nothing was executed")
        ));
    }

    #[test]
    fn enter_on_no_closes_delete_dialogs() {
        let mut app = root_app();
        press(&mut app, KeyCode::Delete);
        assert!(matches!(
            app.modal(),
            Some(ModalState::DeleteConfirm {
                selected: DELETE_ROW_NO,
                ..
            })
        ));
        press(&mut app, KeyCode::Enter);
        assert!(app.modal().is_none());
        assert!(app.pending_operation.is_none());

        for modal in [
            ModalState::ConfirmRemoveUserFromGroup {
                selected: 1,
                group_name: "dev:alice".to_owned(),
            },
            ModalState::GroupDeleteConfirm {
                selected: 1,
                target_gid: Some(1000),
            },
        ] {
            open_modal(&mut app, modal);
            press(&mut app, KeyCode::Enter);
            assert!(app.modal().is_none());
            assert!(app.pending_operation.is_none());
        }

        // Only the dialog closes; the menu under it stays.
        open_modal(&mut app, ModalState::Actions { selected: 0 });
        open_modal(&mut app, delete_dialog(true));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::Actions { .. })));
        assert_eq!(app.modal_depth(), 1);
    }

    #[test]
    fn protected_user_delete_explains_only_on_yes() {
        let mut app = root_app();
        let protected = || delete_dialog(false);
        open_modal(&mut app, protected());
        press(&mut app, KeyCode::Enter);
        assert!(app.modal().is_none());

        open_modal(&mut app, protected());
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::Info { message, .. }) if message.contains("Protected identities")
        ));
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn yes_no_dialogs_move_with_arrows_h_l_and_tab() {
        let mut app = root_app();
        press(&mut app, KeyCode::Delete);
        let selected = |app: &AppState| match app.modal() {
            Some(ModalState::DeleteConfirm { selected, .. }) => *selected,
            _ => panic!("expected the delete dialog"),
        };
        assert_eq!(selected(&app), DELETE_ROW_NO);
        for (code, expected) in [
            (KeyCode::Char('h'), DELETE_ROW_YES),
            (KeyCode::Char('l'), DELETE_ROW_NO),
            (KeyCode::Tab, DELETE_ROW_YES),
            (KeyCode::Right, DELETE_ROW_NO),
            (KeyCode::Left, DELETE_ROW_YES),
        ] {
            press(&mut app, code);
            assert_eq!(selected(&app), expected);
        }
        // In a plain yes/no dialog, Up/Down and j/k are list keys, not
        // yes/no keys.
        app.close_modals();
        open_modal(
            &mut app,
            ModalState::GroupDeleteConfirm {
                selected: 0,
                target_gid: Some(1000),
            },
        );
        press(&mut app, KeyCode::Char('j'));
        press(&mut app, KeyCode::Down);
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupDeleteConfirm { selected: 0, .. })
        ));
    }

    /// A user delete dialog for alice with the cursor on No.
    fn delete_dialog(allowed: bool) -> ModalState {
        ModalState::DeleteConfirm {
            selected: DELETE_ROW_NO,
            allowed,
            delete_home: false,
            username: "alice".to_owned(),
            uid: 1000,
            home: "/home/alice".to_owned(),
            is_self: false,
            process_count: None,
        }
    }

    #[test]
    fn delete_dialog_cursor_reaches_the_home_checkbox() {
        let mut app = root_app();
        app.dry_run = true;
        press(&mut app, KeyCode::Delete);
        let state = |app: &AppState| match app.modal() {
            Some(ModalState::DeleteConfirm {
                selected,
                delete_home,
                ..
            }) => (*selected, *delete_home),
            _ => panic!("expected the delete dialog"),
        };
        // Space does nothing on the buttons.
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(state(&app), (DELETE_ROW_NO, false));
        press(&mut app, KeyCode::Left);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(state(&app), (DELETE_ROW_YES, false));

        press(&mut app, KeyCode::Up);
        assert_eq!(state(&app), (DELETE_ROW_HOME, false));
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(state(&app), (DELETE_ROW_HOME, true));
        assert!(rendered(&app).contains("Home: /home/alice will be removed"));
        // Enter on the checkbox neither toggles nor answers.
        press(&mut app, KeyCode::Enter);
        assert_eq!(state(&app), (DELETE_ROW_HOME, true));

        // Moving back to the buttons lands on No, both vertically and
        // horizontally.
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(state(&app), (DELETE_ROW_NO, true));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Tab);
        assert_eq!(state(&app), (DELETE_ROW_NO, true));

        // `y` answers from any row and keeps the checkbox choice.
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { .. })
        ));
        assert!(
            app.last_preview
                .iter()
                .any(|step| step.contains("userdel") && step.contains("-r"))
        );
    }

    /// Diagnostics reporting a fixed number of processes for every user.
    struct ProcessDiagnostics(usize);
    impl crate::app::DiagnosticProvider for ProcessDiagnostics {
        fn refresh(
            &self,
            snapshot: &AccountSnapshot,
            _: std::time::SystemTime,
        ) -> crate::app::CachedDiagnostics {
            let mut diagnostics = crate::app::CachedDiagnostics::default();
            for user in &snapshot.users {
                diagnostics.homes.insert(
                    user.name.as_str().to_owned(),
                    crate::app::HomeDiagnostics {
                        process_count: Some(self.0),
                        ..crate::app::HomeDiagnostics::default()
                    },
                );
            }
            diagnostics
        }
    }

    #[test]
    fn delete_dialog_names_the_user_and_warns() {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = AppState::with_dependencies(
            adapter,
            snapshot,
            Arc::new(FixedClock),
            Arc::new(ProcessDiagnostics(3)),
            Arc::new(NoConfigRoots),
        );
        app.refresh_accounts();
        app.current_username = "alice".to_owned();
        press(&mut app, KeyCode::Delete);
        assert!(matches!(
            app.modal(),
            Some(ModalState::DeleteConfirm {
                is_self: true,
                process_count: Some(3),
                ..
            })
        ));
        let screen = rendered(&app);
        assert!(screen.contains("Delete user alice (UID 1000)?"));
        assert!(screen.contains("Home: /home/alice"));
        assert!(!screen.contains("will be removed"));
        assert!(screen.contains("This is your own account."));
        assert!(screen.contains("3 running processes belong to this user."));
        assert!(screen.contains("▶ No"));

        // Another user's account with no processes carries no warnings.
        app.close_modals();
        open_modal(&mut app, delete_dialog(true));
        let screen = rendered(&app);
        assert!(screen.contains("Delete user alice (UID 1000)?"));
        assert!(!screen.contains("your own account"));
        assert!(!screen.contains("running process"));

        // Protected accounts show why in place of Yes.
        app.close_modals();
        open_modal(&mut app, delete_dialog(false));
        let screen = rendered(&app);
        assert!(screen.contains("Cannot delete: root and system accounts"));
        assert!(!screen.contains(" Yes "));
    }

    #[test]
    fn menus_move_with_j_and_k() {
        let mut app = user_app();
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::Actions { selected: 0 })
        ));
        press(&mut app, KeyCode::Char('j'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::Actions { selected: 1 })
        ));
        press(&mut app, KeyCode::Char('k'));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyMenu { selected: 0 })
        ));
        press(&mut app, KeyCode::Char('k'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyMenu { selected: 3 })
        ));
        press(&mut app, KeyCode::Char('j'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyMenu { selected: 0 })
        ));
    }

    #[test]
    fn text_fields_take_letters_used_as_keys_elsewhere() {
        let mut app = user_app();
        open_modal(
            &mut app,
            ModalState::GroupAddInput {
                name: String::new(),
                error: None,
            },
        );
        for character in ['j', 'k', 'h', 'l', 'y', 'n', '\u{7}'] {
            press(&mut app, KeyCode::Char(character));
        }
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupAddInput { name, .. }) if name == "jkhlyn"
        ));
    }

    #[test]
    fn main_view_h_l_and_left_right_switch_panes() {
        let mut app = user_app();
        assert_eq!(app.users_focus, UsersFocus::UsersList);
        press(&mut app, KeyCode::Char('l'));
        assert_eq!(app.users_focus, UsersFocus::MemberOf);
        press(&mut app, KeyCode::Char('h'));
        assert_eq!(app.users_focus, UsersFocus::UsersList);
        press(&mut app, KeyCode::Right);
        assert_eq!(app.users_focus, UsersFocus::MemberOf);
        press(&mut app, KeyCode::Left);
        assert_eq!(app.users_focus, UsersFocus::UsersList);
        assert_eq!(app.selected_user_index, 0);
    }

    fn rendered(app: &AppState) -> String {
        rendered_at(app, 100, 30)
    }

    fn rendered_at(app: &AppState, width: u16, height: u16) -> String {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn filter_menu_keeps_last_entry_visible_above_footer_at_80x16() {
        let mut app = user_app();
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('k'));
        let screen = rendered_at(&app, 80, 16);
        assert!(
            screen
                .contains("▶ [-] Expired (shadow unavailable: shadow data has not been refreshed)")
        );
        assert!(screen.contains("Backspace back  Esc close"));
    }

    #[test]
    fn create_user_form_keeps_submit_row_at_80x14() {
        let mut app = user_app();
        press(&mut app, KeyCode::Char('n'));
        assert!(matches!(app.modal(), Some(ModalState::UserAddInput { .. })));
        let rows = rendered_rows(&app, 80, 14);
        assert!(rows.iter().any(|row| row.contains("│▶ Username:")));
        assert!(rows.iter().any(|row| row.contains("│  Submit")));
    }

    /// Screen rows of a TestBackend render, one string per row.
    fn rendered_rows(app: &AppState, width: u16, height: u16) -> Vec<String> {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, app)).unwrap();
        let buffer = terminal.backend().buffer();
        (0..height)
            .map(|y| (0..width).map(|x| buffer[(x, y)].symbol()).collect())
            .collect()
    }

    #[test]
    fn narrow_dialog_shows_every_character_of_a_wrapped_value_at_30x24() {
        let mut app = user_app();
        let value = "aaaaaaaaaaa bbbbbbbbbbb ccccccccccc";
        open_modal(
            &mut app,
            ModalState::ModifyTextInput {
                field: ModifyField::Fullname,
                value: value.to_owned(),
                error: None,
            },
        );
        let rows = rendered_rows(&app, 30, 24);
        // The dialog is 22 columns wide at x = 4, so its body is x = 5..25.
        let body: Vec<String> = rows
            .iter()
            .map(|row| row.chars().skip(5).take(20).collect())
            .collect();
        let start = body
            .iter()
            .position(|row| row.starts_with("aaaaaaaaaaa"))
            .expect("first row of the value");
        assert_eq!(body[start], "aaaaaaaaaaa bbbbbbbb");
        assert_eq!(body[start + 1].trim_end(), "bbb ccccccccccc");
        assert!(rows[start + 2].contains("type to edit"));
    }

    #[test]
    fn cursor_row_below_a_wrapped_line_stays_visible() {
        let app = user_app();
        let body = vec![
            ratatui::text::Line::from("aaaaaaaaaaa bbbbbbbbbbb ccccccccccc"),
            ratatui::text::Line::from("  one"),
            ratatui::text::Line::from("▶ two"),
        ];
        // 30x6: the dialog has four interior rows, so three body rows for
        // four wrapped rows; the body scrolls by one to show the cursor.
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(30, 6)).unwrap();
        terminal
            .draw(|frame| {
                crate::ui::components::render_modal_frame(
                    frame,
                    frame.area(),
                    &app,
                    "Pick",
                    body,
                    "footer",
                )
            })
            .unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("▶ two"));
        assert!(screen.contains("  one"));
        assert!(screen.contains("footer"));
    }

    #[test]
    fn short_terminal_scrolls_to_the_cursor_and_keeps_the_footer_at_80x9() {
        let mut app = user_app();
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Char('k'));
        let screen = rendered_at(&app, 80, 9);
        assert!(screen.contains("▶ [-] Expired (shadow unavailable"));
        assert!(screen.contains("Backspace back  Esc close"));

        app.close_modals();
        press(&mut app, KeyCode::Char('n'));
        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
        }
        let screen = rendered_at(&app, 80, 9);
        assert!(screen.contains("▶ Submit"));
        assert!(screen.contains("Enter on Submit  Esc back"));
    }

    #[test]
    fn search_ignores_control_characters() {
        let mut app = user_app();
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.input_mode, InputMode::SearchUsers);
        press(&mut app, KeyCode::Char('\u{7}'));
        press(&mut app, KeyCode::Char('\u{1b}'));
        assert_eq!(app.users_query, "");
        press(&mut app, KeyCode::Char('a'));
        assert_eq!(app.users_query, "a");
    }

    fn user_names(app: &AppState) -> Vec<&str> {
        app.users.iter().map(|user| user.name.as_str()).collect()
    }

    #[test]
    fn search_survives_enter_and_filter_toggle_until_esc_clears_it() {
        let config = tempfile::tempdir().unwrap();
        let mut app = membership_app();
        // The filter menu saves filter.conf; keep it out of the working tree.
        app.config_root_provider = Arc::new(crate::app::FixedConfigRootProvider(
            config.path().to_path_buf(),
        ));
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "bob");
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(user_names(&app), ["bob"]);

        // Toggle "Human users" in the filter menu, then close it.
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Esc);
        assert!(app.users_filter_chips.human_only);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.users_query, "bob");
        assert_eq!(user_names(&app), ["bob"]);

        press(&mut app, KeyCode::Esc);
        assert_eq!(app.users_query, "");
        assert_eq!(user_names(&app), ["alice", "bob", "carol"]);
    }

    /// `membership_app` with filter.conf saved under a tempdir, never the repo.
    fn filter_app(config: &tempfile::TempDir) -> AppState {
        let mut app = membership_app();
        app.config_root_provider = Arc::new(crate::app::FixedConfigRootProvider(
            config.path().to_path_buf(),
        ));
        app
    }

    /// Open the filter menu and press Enter on row `row`.
    fn toggle_filter_row(app: &mut AppState, row: usize) {
        press(app, KeyCode::Char('f'));
        for _ in 0..row {
            press(app, KeyCode::Down);
        }
        press(app, KeyCode::Enter);
        press(app, KeyCode::Esc);
    }

    #[test]
    fn human_and_system_filters_exclude_each_other() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        toggle_filter_row(&mut app, 1);
        toggle_filter_row(&mut app, 2);
        assert!(!app.users_filter_chips.human_only);
        assert!(app.users_filter_chips.system_only);
        assert_eq!(app.active_user_filter_labels(), ["system"]);
        let saved = std::fs::read_to_string(config.path().join("filter.conf")).unwrap();
        assert!(saved.contains("human_only = false"));
        assert!(saved.contains("system_only = true"));
    }

    #[test]
    fn failed_filter_save_opens_a_dialog_over_the_filter_menu() {
        let config = tempfile::tempdir().unwrap();
        // A regular file where the config directory should be: every write fails.
        let blocker = config.path().join("not-a-dir");
        std::fs::write(&blocker, "").unwrap();
        let mut app = membership_app();
        app.config_root_provider = Arc::new(crate::app::FixedConfigRootProvider(
            blocker.join("UsrGrpManager"),
        ));
        press(&mut app, KeyCode::Char('f'));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(app.users_filter_chips.human_only);
        assert_eq!(app.diagnostics.config_messages.len(), 1);
        match app.modal() {
            Some(ModalState::Info { title, message }) => {
                assert_eq!(title.as_deref(), Some(crate::app::CONFIG_PROBLEMS_TITLE));
                assert!(message.contains("not saved"));
            }
            _ => panic!("expected the save failure dialog"),
        }
        press(&mut app, KeyCode::Esc);
        assert!(matches!(app.modal(), Some(ModalState::FilterMenu { .. })));
    }

    #[test]
    fn show_all_clears_chips_and_hand_edited_users_filter() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        app.users_filter = Some(crate::app::UsersFilter::OnlySystemIds);
        app.users_filter_chips.inactive = true;
        app.sort_and_filter();
        assert!(app.users.is_empty());
        toggle_filter_row(&mut app, 0);
        assert_eq!(app.users_filter, None);
        assert!(!app.users_filter_chips.inactive);
        assert_eq!(user_names(&app), ["alice", "bob", "carol"]);

        app.active_tab = ActiveTab::Groups;
        app.groups_filter = Some(crate::app::GroupsFilter::OnlySystemGids);
        toggle_filter_row(&mut app, 0);
        assert_eq!(app.groups_filter, None);
    }

    #[test]
    fn hand_edited_users_filter_shows_as_the_human_chip() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        app.users_filter = Some(crate::app::UsersFilter::OnlyUserIds);
        assert_eq!(app.active_user_filter_labels(), ["human"]);
        // Turning "System users" on folds the hand-edited filter away.
        toggle_filter_row(&mut app, 2);
        assert_eq!(app.users_filter, None);
        assert!(!app.human_filter_on());
        assert!(app.users_filter_chips.system_only);
    }

    #[test]
    fn shadow_chips_are_disabled_while_shadow_is_unavailable() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        app.diagnostics.shadow = crate::search::ShadowState::Unavailable {
            reason: "permission denied".to_owned(),
        };
        press(&mut app, KeyCode::Char('f'));
        let rows = rendered_rows(&app, 100, 30);
        assert!(
            rows.iter()
                .any(|row| row.contains("[-] Locked (shadow unavailable: permission denied)"))
        );
        assert!(rows.iter().any(|row| row.contains("[ ] Human users")));
        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char(' '));
        assert!(!app.users_filter_chips.locked);
        assert!(!config.path().join("filter.conf").exists());
    }

    #[test]
    fn shadow_filter_skipped_is_counted_and_shown_in_titles() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        let locked = crate::search::ShadowStatus {
            locked: true,
            ..Default::default()
        };
        app.diagnostics.shadow =
            crate::search::ShadowState::Known([("alice".to_owned(), locked)].into_iter().collect());
        toggle_filter_row(&mut app, 5);
        assert!(app.users_filter_chips.locked);
        assert_eq!(app.shadow_filter_skipped, Some(2));
        assert_eq!(user_names(&app), ["alice", "bob", "carol"]);
        let title = ui::users::users_table_title(&app);
        assert!(
            title.ends_with("3 of 3  [locked]  (shadow filter skipped: 2 users without record)"),
            "{title}"
        );
        press(&mut app, KeyCode::Char('f'));
        let rows = rendered_rows(&app, 100, 30);
        assert!(
            rows.iter()
                .any(|row| row.contains("shadow filter skipped: 2 users"))
        );
        assert!(rows.iter().any(|row| row.contains("[x] Locked (shadow)")));
    }

    #[test]
    fn table_titles_list_active_filters_after_the_count() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        toggle_filter_row(&mut app, 1);
        toggle_filter_row(&mut app, 3);
        assert_eq!(
            ui::users::users_table_title(&app),
            "[Users]  0 of 3  [human, inactive]"
        );
        app.active_tab = ActiveTab::Groups;
        toggle_filter_row(&mut app, 2);
        assert_eq!(
            ui::groups::groups_table_title(&app),
            "[Groups]  0 of 3  [system]"
        );
        press(&mut app, KeyCode::Char('f'));
        let rows = rendered_rows(&app, 100, 30);
        assert!(rows.iter().any(|row| row.contains("(•) System GIDs")));
        assert!(rows.iter().any(|row| row.contains("( ) User GIDs")));
    }

    #[test]
    fn slash_edits_the_query_in_place_and_esc_restores_it() {
        let mut app = membership_app();
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "bo");
        press(&mut app, KeyCode::Enter);

        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.users_query, "bo");
        type_text(&mut app, "x");
        assert!(app.users.is_empty());
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.users_query, "bo");
        assert_eq!(user_names(&app), ["bob"]);
    }

    #[test]
    fn each_tab_keeps_its_own_query() {
        let mut app = membership_app();
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "bob");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.input_mode, InputMode::SearchGroups);
        assert_eq!(app.groups_query, "");
        type_text(&mut app, "ops");
        press(&mut app, KeyCode::Enter);
        let groups: Vec<_> = app.groups.iter().map(|group| group.name.as_str()).collect();
        assert_eq!(groups, ["ops"]);
        assert_eq!(user_names(&app), ["bob"]);

        // Esc on the groups tab clears only the groups query.
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.groups.len(), 3);
        assert_eq!(app.users_query, "bob");
    }

    #[test]
    fn table_title_and_status_bar_show_the_search() {
        let mut app = membership_app();
        assert!(rendered_at(&app, 100, 30).contains("[Users]  3 of 3"));
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "bob");
        let screen = rendered_at(&app, 100, 30);
        assert!(screen.contains("[Users]  /bob_  1 of 3"));
        assert!(screen.contains("SEARCH(users)"));

        press(&mut app, KeyCode::Enter);
        let screen = rendered_at(&app, 100, 30);
        assert!(screen.contains("[Users]  /bob  1 of 3"));
        assert!(!screen.contains("SEARCH("));

        press(&mut app, KeyCode::Tab);
        press(&mut app, KeyCode::Char('/'));
        let screen = rendered_at(&app, 100, 30);
        assert!(screen.contains("[Groups]  /_  3 of 3"));
        assert!(screen.contains("SEARCH(groups)"));
    }

    #[test]
    fn modal_footers_name_their_keys() {
        let mut app = user_app();
        let cases = [
            (ModalState::Actions { selected: 0 }, "Backspace back"),
            (delete_dialog(true), "y/n"),
            (
                ModalState::GroupDeleteConfirm {
                    selected: 1,
                    target_gid: Some(1000),
                },
                "←→ choose  y/n  Enter confirm  Esc cancel",
            ),
            (
                ModalState::ModifyGroupsAdd {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                },
                "↑↓ move  Space tick  * all  / filter  Enter apply row  Esc close",
            ),
            (
                ModalState::GroupAddInput {
                    name: String::new(),
                    error: None,
                },
                "Enter submit  Esc back",
            ),
            (ModalState::FilterMenu { selected: 0 }, "Backspace back"),
            (
                ModalState::Info {
                    title: None,
                    message: "done".to_owned(),
                },
                "Enter/Esc close",
            ),
        ];
        for (modal, footer) in cases {
            app.close_modals();
            open_modal(&mut app, modal);
            let screen = rendered(&app);
            assert!(screen.contains(footer), "missing footer {footer:?}");
        }
    }

    /// Fixed wall clock so log timestamps and diagnostics do not vary.
    struct FixedClock;
    impl crate::app::Clock for FixedClock {
        fn now(&self) -> std::time::SystemTime {
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_700_000_000)
        }
    }

    /// Diagnostics that never read /etc/shadow, /etc/group, or home directories.
    struct NoDiagnostics;
    impl crate::app::DiagnosticProvider for NoDiagnostics {
        fn refresh(
            &self,
            _: &AccountSnapshot,
            _: std::time::SystemTime,
        ) -> crate::app::CachedDiagnostics {
            crate::app::CachedDiagnostics::default()
        }
    }

    /// No config roots, so nothing resolves against HOME or XDG_CONFIG_HOME.
    struct NoConfigRoots;
    impl crate::app::ConfigRootProvider for NoConfigRoots {
        fn roots(&self) -> Vec<std::path::PathBuf> {
            Vec::new()
        }
    }

    /// Like `AppState::with_adapter`, but every host-reading dependency is a
    /// fake, so `refresh_accounts` after an executed plan stays off the host.
    fn isolated_app(adapter: Arc<SystemAdapter>, snapshot: AccountSnapshot) -> AppState {
        AppState::with_dependencies(
            adapter,
            snapshot,
            Arc::new(FixedClock),
            Arc::new(NoDiagnostics),
            Arc::new(NoConfigRoots),
        )
    }

    /// Account source whose next result a test can swap: `None` fails the
    /// refresh the way an unreadable /etc/passwd would.
    struct SwappableSource(std::sync::Mutex<Option<AccountSnapshot>>);
    impl AccountDataSource for SwappableSource {
        fn refresh(&self) -> CoreResult<AccountSnapshot> {
            self.0
                .lock()
                .unwrap()
                .clone()
                .ok_or(crate::error::CoreError::Refresh {
                    operation: "passwd",
                    kind: std::io::ErrorKind::PermissionDenied,
                })
        }
    }

    /// Wall clock a test can move forward, in seconds since the UNIX epoch.
    struct SteppingClock(std::sync::atomic::AtomicU64);
    impl crate::app::Clock for SteppingClock {
        fn now(&self) -> std::time::SystemTime {
            std::time::UNIX_EPOCH
                + std::time::Duration::from_secs(self.0.load(std::sync::atomic::Ordering::SeqCst))
        }
    }

    #[test]
    fn utc_time_label_formats_the_time_of_day() {
        use crate::app::utc_time_label;
        use std::time::{Duration, UNIX_EPOCH};
        assert_eq!(utc_time_label(UNIX_EPOCH), "00:00:00 UTC");
        assert_eq!(
            utc_time_label(UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
            "22:13:20 UTC"
        );
        assert_eq!(
            utc_time_label(UNIX_EPOCH + Duration::from_secs(86_399)),
            "23:59:59 UTC"
        );
        assert_eq!(
            utc_time_label(UNIX_EPOCH - Duration::from_secs(5)),
            "00:00:00 UTC"
        );
    }

    #[test]
    fn r_refreshes_accounts_and_the_status_bar_shows_when() {
        use std::sync::atomic::{AtomicU64, Ordering};
        let user = |uid: u32, name: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(100),
            full_name: None,
            home_dir: format!("/home/{name}").into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        };
        let snapshot = |users: Vec<AccountUser>| AccountSnapshot {
            users,
            groups: vec![AccountGroup {
                gid: Gid(100),
                name: GroupName::new("staff").unwrap(),
                members: vec![],
            }],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        };
        let started = snapshot(vec![user(1000, "alice")]);
        // bob was added in another terminal after the app started.
        let current = snapshot(vec![user(1000, "alice"), user(1001, "bob")]);
        let source = Arc::new(SwappableSource(std::sync::Mutex::new(Some(
            current.clone(),
        ))));
        let clock = Arc::new(SteppingClock(AtomicU64::new(1_700_000_000)));
        let adapter = Arc::new(SystemAdapter::from_components(
            source.clone(),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = AppState::with_dependencies(
            adapter,
            started,
            clock.clone(),
            Arc::new(NoDiagnostics),
            Arc::new(NoConfigRoots),
        );
        assert_eq!(app.users.len(), 1);
        assert_eq!(ui::components::refresh_label(&app), "not refreshed");

        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.users.len(), 2);
        assert_eq!(app.last_refresh_label, "22:13:20 UTC");
        assert!(rendered(&app).contains("refreshed 22:13:20 UTC"));

        // A failed refresh keeps the data and says since when it is stale.
        *source.0.lock().unwrap() = None;
        clock.0.store(1_700_003_661, Ordering::SeqCst);
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(app.users.len(), 2);
        assert_eq!(app.last_refresh_label, "22:13:20 UTC");
        assert_eq!(
            ui::components::refresh_label(&app),
            "STALE since 22:13:20 UTC: could not refresh passwd: permission denied"
        );

        *source.0.lock().unwrap() = Some(current);
        press(&mut app, KeyCode::Char('r'));
        assert!(app.diagnostics.stale_reason.is_none());
        assert_eq!(
            ui::components::refresh_label(&app),
            "refreshed 23:14:21 UTC"
        );
    }

    /// alice (primary group `alice`) is already in `dev`; bob and carol are not.
    fn membership_app() -> AppState {
        let user = |uid: u32, name: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(uid),
            full_name: None,
            home_dir: format!("/home/{name}").into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        };
        let group = |gid: u32, name: &str, members: &[&str]| AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: members
                .iter()
                .map(|member| UserName::new(*member).unwrap())
                .collect(),
        };
        let snapshot = AccountSnapshot {
            users: vec![user(1000, "alice"), user(1001, "bob"), user(1002, "carol")],
            groups: vec![
                group(1000, "alice", &[]),
                group(2000, "dev", &["alice"]),
                group(2001, "ops", &[]),
            ],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    /// alice is in three groups (primary `alice`, `dev`, `ops`); bob is only
    /// in his primary group. Group `dev` has three members, `ops` one.
    fn stale_child_app() -> AppState {
        let user = |uid: u32, name: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(uid),
            full_name: None,
            home_dir: format!("/home/{name}").into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        };
        let group = |gid: u32, name: &str, members: &[&str]| AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: members
                .iter()
                .map(|member| UserName::new(*member).unwrap())
                .collect(),
        };
        let snapshot = AccountSnapshot {
            users: vec![
                user(1000, "alice"),
                user(1001, "bob"),
                user(1002, "carol"),
                user(1003, "dave"),
            ],
            groups: vec![
                group(1000, "alice", &[]),
                group(1001, "bob", &[]),
                group(2000, "dev", &["alice", "carol", "dave"]),
                group(2001, "ops", &["alice"]),
            ],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    /// 50 users `u000`..`u049` in one group; `u000` is also in 40 extra groups.
    fn paging_app() -> AppState {
        let users: Vec<AccountUser> = (0..50)
            .map(|index| AccountUser {
                uid: Uid(1000 + index),
                name: UserName::new(format!("u{index:03}")).unwrap(),
                primary_gid: Gid(100),
                full_name: None,
                home_dir: format!("/home/u{index:03}").into(),
                shell: ShellPath::new("/bin/sh").unwrap(),
            })
            .collect();
        let mut groups = vec![AccountGroup {
            gid: Gid(100),
            name: GroupName::new("staff").unwrap(),
            members: vec![],
        }];
        groups.extend((0..40).map(|index| AccountGroup {
            gid: Gid(3000 + index),
            name: GroupName::new(format!("g{index:03}")).unwrap(),
            members: vec![UserName::new("u000").unwrap()],
        }));
        let snapshot = AccountSnapshot {
            users,
            groups,
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    fn rendered_text(app: &AppState, width: u16, height: u16) -> String {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|cell| cell.symbol())
            .collect()
    }

    #[test]
    fn page_down_moves_by_the_rows_the_table_renders() {
        let mut app = paging_app();
        app.terminal_size = (100, 40);
        // 40 rows - 3 header - 1 status bar - 2 borders - 1 column header.
        let page = 33;
        let before = rendered_text(&app, 100, 40);
        assert!(before.contains(&format!("u{:03}", page - 1)));
        assert!(!before.contains(&format!("u{page:03}")));

        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected_user_index, page);
        let after = rendered_text(&app, 100, 40);
        assert!(after.contains(&format!("u{page:03}")));
        assert!(!after.contains(&format!("u{:03}", page - 1)));

        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.selected_user_index, 0);
    }

    #[test]
    fn page_size_follows_resize_and_the_focused_pane() {
        let mut app = paging_app();
        handle_event(&mut app, Event::Resize(100, 20));
        press(&mut app, KeyCode::PageDown);
        // 20 rows - 3 header - 1 status bar - 3 table chrome.
        assert_eq!(app.selected_user_index, 13);

        press(&mut app, KeyCode::PageUp);
        app.users_focus = UsersFocus::MemberOf;
        let area = Rect::new(0, 0, 100, 20);
        let members = ui::layout::main_layout(area, app.show_keybinds).members;
        press(&mut app, KeyCode::PageDown);
        assert_eq!(
            app.selected_user_group_index,
            ui::layout::table_rows(members)
        );
    }

    /// One user `alice` in one group `staff`; shadow data unavailable.
    fn single_account_app() -> AppState {
        let snapshot = AccountSnapshot {
            users: vec![AccountUser {
                uid: Uid(1000),
                name: UserName::new("alice").unwrap(),
                primary_gid: Gid(100),
                full_name: None,
                home_dir: "/home/alice".into(),
                shell: ShellPath::new("/bin/sh").unwrap(),
            }],
            groups: vec![AccountGroup {
                gid: Gid(100),
                name: GroupName::new("staff").unwrap(),
                members: vec![],
            }],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    fn status_row(app: &AppState, width: u16, height: u16) -> String {
        rendered_rows(app, width, height).pop().unwrap()
    }

    #[test]
    fn status_bar_shows_the_stale_reason_and_the_header_does_not() {
        let mut app = single_account_app();
        app.diagnostics.stale_reason = Some("boom".to_owned());
        let rows = rendered_rows(&app, 100, 30);
        assert!(rows[29].contains("STALE: boom"), "{}", rows[29]);
        assert!(rows[..3].iter().all(|row| !row.contains("STALE")));
    }

    #[test]
    fn status_bar_keeps_the_refresh_label_when_narrow() {
        for width in [30, 80] {
            let mut app = single_account_app();
            app.diagnostics
                .config_messages
                .push("theme configuration line 3: unknown color".to_owned());
            app.diagnostics.stale_reason = Some("boom".to_owned());
            let row = status_row(&app, width, 12);
            assert!(row.contains("STALE: boom"), "{width}: {row}");

            app.diagnostics.stale_reason = None;
            app.last_refresh_label = "22:13:20 UTC".to_owned();
            let row = status_row(&app, width, 12);
            assert!(row.contains("refreshed 22:13:20 UTC"), "{width}: {row}");
        }
        // At 80 columns the config error still fits after the counts.
        let mut app = single_account_app();
        app.diagnostics
            .config_messages
            .push("theme configuration line 3: unknown color".to_owned());
        app.last_refresh_label = "22:13:20 UTC".to_owned();
        assert_eq!(
            status_row(&app, 80, 12).trim_end(),
            "refreshed 22:13:20 UTC · users 1/1 · groups 1 · config: 1 error (see ? help)"
        );
    }

    #[test]
    fn long_stale_reason_is_shortened_to_the_status_bar() {
        let mut app = single_account_app();
        app.diagnostics.stale_reason = Some("x".repeat(100));
        let row = status_row(&app, 30, 12);
        assert!(row.starts_with("STALE: xxx"), "{row}");
        assert!(row.contains("… · users 1/1"), "{row}");
        assert_eq!(row.chars().count(), 30);

        // With room to spare, every other field stays and the reason takes
        // the width that is left.
        let row = status_row(&app, 100, 12);
        assert!(row.starts_with("STALE: xxx"), "{row}");
        assert!(
            row.ends_with("… · users 1/1 · groups 1 · shadow unavailable"),
            "{row}"
        );
        assert!(!row.contains(&"x".repeat(100)), "{row}");
    }

    /// Screen position of the first cell of `text`, if rendered.
    fn find_text(rows: &[String], text: &str) -> Option<(u16, u16)> {
        rows.iter().enumerate().find_map(|(y, row)| {
            let byte = row.find(text)?;
            let x = row[..byte].chars().count();
            Some((u16::try_from(x).unwrap(), u16::try_from(y).unwrap()))
        })
    }

    #[test]
    fn empty_users_table_names_the_query_and_filters_in_muted_text() {
        let mut app = single_account_app();
        app.users_query = "zzz".to_owned();
        app.sort_and_filter();
        assert!(app.users.is_empty());
        let rows = rendered_rows(&app, 100, 30);
        let (x, y) = find_text(&rows, "No users match /zzz").expect("empty state shown");
        assert!(
            x < 50 && (3..29).contains(&y),
            "inside the users table: {x},{y}"
        );
        assert!(find_text(&rows, "No user selected").is_some());

        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(100, 30)).unwrap();
        terminal.draw(|frame| ui::render(frame, &app)).unwrap();
        assert_eq!(
            terminal.backend().buffer()[(x, y)].fg,
            app.theme._muted,
            "empty state uses the muted color"
        );

        app.set_uid_range_filters(false, true);
        app.sort_and_filter();
        let rows = rendered_rows(&app, 100, 30);
        assert!(find_text(&rows, "No users match /zzz, filters: system").is_some());

        app.users_query.clear();
        app.sort_and_filter();
        let rows = rendered_rows(&app, 100, 30);
        assert!(find_text(&rows, "No users match filters: system").is_some());
    }

    #[test]
    fn empty_snapshot_shows_nothing_loaded_with_the_stale_reason() {
        let snapshot = AccountSnapshot {
            users: vec![],
            groups: vec![],
            shells: vec![],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.diagnostics.stale_reason = Some("boom".to_owned());
        let rows = rendered_rows(&app, 100, 30);
        let (_, loaded) = find_text(&rows, "No users loaded").expect("users empty state");
        let (_, stale) = find_text(&rows, "STALE: boom").expect("stale reason");
        assert_eq!(stale, loaded + 1, "stale reason under the empty state");

        app.active_tab = ActiveTab::Groups;
        let rows = rendered_rows(&app, 100, 30);
        assert!(find_text(&rows, "No groups loaded").is_some());
        assert!(find_text(&rows, "No group selected").is_some());

        app.diagnostics.stale_reason = None;
        let rows = rendered_rows(&app, 100, 30);
        assert!(find_text(&rows, "No groups loaded").is_some());
        assert!(rows.iter().all(|row| !row.contains("STALE")));
    }

    #[test]
    fn empty_groups_table_and_members_pane_say_why() {
        let mut app = single_account_app();
        app.active_tab = ActiveTab::Groups;
        let rows = rendered_rows(&app, 100, 30);
        assert!(
            find_text(&rows, "No members").is_some(),
            "staff has no members"
        );

        app.groups_query = "zzz".to_owned();
        app.groups_filter = Some(crate::app::GroupsFilter::OnlySystemGids);
        app.sort_and_filter();
        assert!(app.groups.is_empty());
        let rows = rendered_rows(&app, 100, 30);
        assert!(find_text(&rows, "No groups match /zzz, filters: system").is_some());
    }

    #[test]
    fn status_bar_position_follows_the_focused_pane() {
        let mut app = stale_child_app();
        assert!(status_row(&app, 100, 30).contains("users 1/4 · groups 4"));
        app.selected_user_index = 0; // alice, in three groups
        app.users_focus = UsersFocus::MemberOf;
        app.selected_user_group_index = 1;
        app.capture_selection_identities();
        assert!(status_row(&app, 100, 30).contains("member of 2/3 · users 4 · groups 4"));

        app.active_tab = ActiveTab::Groups;
        app.selected_group_index = 2; // dev
        app.capture_selection_identities();
        assert!(status_row(&app, 100, 30).contains("groups 3/4 · users 4"));
        app.groups_focus = GroupsFocus::Members;
        app.selected_group_member_index = 2;
        assert!(status_row(&app, 100, 30).contains("members 3/3 · users 4 · groups 4"));
    }

    #[test]
    fn header_is_one_line_with_tabs_and_key_hints() {
        let app = single_account_app();
        let rows = rendered_rows(&app, 100, 30);
        let header = &rows[1];
        assert!(header.contains("usrgrp-manager (unknown)   [Users]  Groups"));
        assert!(header.contains("? help  / search  f filter  Shift+K keys"));
        assert!(!rows[..3].iter().any(|row| row.contains("users:")));
        // Hints that do not fit are dropped from the end.
        let rows = rendered_rows(&app, 64, 20);
        assert!(rows[1].contains("? help  / search"), "{}", rows[1]);
        assert!(!rows[1].contains("filter"));
    }

    #[test]
    fn moving_to_another_user_resets_the_member_of_cursor() {
        let mut app = stale_child_app();
        app.active_tab = ActiveTab::Users;
        app.users_focus = UsersFocus::UsersList;
        app.selected_user_index = app
            .users
            .iter()
            .position(|user| user.name == "alice")
            .unwrap();
        app.capture_selection_identities();
        assert_eq!(app.selected_user_groups().len(), 3);
        app.selected_user_group_index = 2;
        app.capture_selection_identities();

        let bob = app
            .users
            .iter()
            .position(|user| user.name == "bob")
            .unwrap();
        let delta = bob as isize - app.selected_user_index as isize;
        move_selection(&mut app, delta);

        assert_eq!(app.users[app.selected_user_index].name, "bob");
        assert_eq!(app.selected_user_groups().len(), 1);
        assert_eq!(app.selected_user_group_index, 0);
        let group = app.selected_user_groups().first().map(|group| group.gid);
        assert!(group.is_some());
        assert_eq!(app.selected_user_group_gid, group);
    }

    #[test]
    fn moving_to_another_group_resets_the_members_cursor() {
        let mut app = stale_child_app();
        app.active_tab = ActiveTab::Groups;
        app.groups_focus = GroupsFocus::GroupsList;
        app.selected_group_index = app
            .groups
            .iter()
            .position(|group| group.name == "dev")
            .unwrap();
        app.selected_group_member_index = 2;
        app.capture_selection_identities();

        let ops = app
            .groups
            .iter()
            .position(|group| group.name == "ops")
            .unwrap();
        let delta = ops as isize - app.selected_group_index as isize;
        move_selection(&mut app, delta);

        assert_eq!(app.groups[app.selected_group_index].name, "ops");
        assert_eq!(app.selected_group_member_index, 0);
        assert_eq!(app.selected_group_member_name.as_deref(), Some("alice"));
    }

    /// Names listed as `[ ] name` / `[x] name` rows in the rendered dialog.
    fn rendered_candidates(app: &AppState) -> Vec<String> {
        rendered_rows(app, 80, 24)
            .iter()
            .filter_map(|row| {
                let start = row.find("[ ] ").or_else(|| row.find("[x] "))? + 4;
                row[start..].split_whitespace().next().map(str::to_owned)
            })
            .collect()
    }

    #[test]
    fn add_members_ticks_the_listed_user() {
        let mut app = membership_app();
        app.active_tab = ActiveTab::Groups;
        open_modal(
            &mut app,
            ModalState::GroupModifyAddMembers {
                selected: 0,
                offset: 0,
                target_gid: Some(2000),
                selected_multi: vec![],
                filter: String::new(),
                filtering: false,
            },
        );
        assert_eq!(rendered_candidates(&app), ["bob", "carol"]);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(rendered_candidates(&app), ["bob", "carol"]);
        assert!(rendered(&app).contains("[x] bob"));
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_some());
        assert_eq!(app.last_preview, ["gpasswd -a bob dev"]);
    }

    #[test]
    fn add_groups_lists_what_the_handler_submits() {
        let mut app = membership_app();
        assert_eq!(app.user_group_candidates(1000, true), ["ops"]);
        assert_eq!(app.user_group_candidates(1000, false), ["dev"]);
        open_modal(
            &mut app,
            ModalState::ModifyGroupsAdd {
                selected: 0,
                offset: 0,
                selected_multi: vec![],
                filter: String::new(),
                filtering: false,
            },
        );
        assert_eq!(rendered_candidates(&app), ["ops"]);
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_some());
        assert_eq!(app.last_preview, ["gpasswd -a alice ops"]);
    }

    fn add_members_dialog(target_gid: u32) -> ModalState {
        ModalState::GroupModifyAddMembers {
            selected: 0,
            offset: 0,
            target_gid: Some(target_gid),
            selected_multi: vec![],
            filter: String::new(),
            filtering: false,
        }
    }

    fn ticked(app: &AppState) -> Vec<String> {
        match app.modal() {
            Some(ModalState::GroupModifyAddMembers { selected_multi, .. }) => {
                selected_multi.clone()
            }
            _ => panic!("add members dialog is not open"),
        }
    }

    #[test]
    fn ticks_survive_filtering() {
        let mut app = membership_app();
        app.active_tab = ActiveTab::Groups;
        open_modal(&mut app, add_members_dialog(2000));
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(ticked(&app), ["bob"]);
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "car");
        assert_eq!(rendered_candidates(&app), ["carol"]);
        assert!(rendered(&app).contains("Filter: car_"));
        assert!(rendered(&app).contains("1 selected"));
        for _ in 0..3 {
            press(&mut app, KeyCode::Backspace);
        }
        press(&mut app, KeyCode::Enter);
        assert_eq!(rendered_candidates(&app), ["bob", "carol"]);
        assert_eq!(ticked(&app), ["bob"]);
        assert!(rendered(&app).contains("[x] bob"));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.last_preview, ["gpasswd -a bob dev"]);
    }

    #[test]
    fn hidden_ticks_are_still_applied() {
        let mut app = membership_app();
        app.active_tab = ActiveTab::Groups;
        open_modal(&mut app, add_members_dialog(2000));
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "carol");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(ticked(&app), ["bob", "carol"]);
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            app.last_preview,
            ["gpasswd -a bob dev", "gpasswd -a carol dev"]
        );
    }

    #[test]
    fn star_ticks_every_visible_entry() {
        let mut app = membership_app();
        app.active_tab = ActiveTab::Groups;
        open_modal(&mut app, add_members_dialog(2001));
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "o");
        press(&mut app, KeyCode::Enter);
        assert_eq!(rendered_candidates(&app), ["bob", "carol"]);
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(ticked(&app), ["bob", "carol"]);
        assert!(rendered(&app).contains("Add members  page 1/1  2 selected"));
        press(&mut app, KeyCode::Char('*'));
        assert!(ticked(&app).is_empty());
    }

    #[test]
    fn esc_while_filtering_only_leaves_filter_typing() {
        let mut app = membership_app();
        app.active_tab = ActiveTab::Groups;
        open_modal(&mut app, add_members_dialog(2000));
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "j*");
        assert!(ticked(&app).is_empty());
        assert!(rendered(&app).contains("No matches"));
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupModifyAddMembers {
                filtering: false,
                ..
            })
        ));
        assert!(rendered(&app).contains("Filter: j*"));
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
    }

    #[test]
    fn footer_says_enter_applies_highlighted_until_something_is_ticked() {
        let mut app = membership_app();
        app.active_tab = ActiveTab::Groups;
        open_modal(&mut app, add_members_dialog(2000));
        assert!(rendered(&app).contains("Enter apply row"));
        press(&mut app, KeyCode::Char(' '));
        let screen = rendered(&app);
        assert!(!screen.contains("Enter apply row"));
        assert!(screen.contains("* all  / filter  Enter apply  Esc close"));
        press(&mut app, KeyCode::Char('/'));
        assert!(rendered(&app).contains("Enter/Esc done"));
    }

    #[test]
    fn add_groups_footer_fits_an_80_column_dialog() {
        let mut app = membership_app();
        open_modal(
            &mut app,
            ModalState::ModifyGroupsAdd {
                selected: 0,
                offset: 0,
                selected_multi: vec![],
                filter: String::new(),
                filtering: false,
            },
        );
        let rows = rendered_rows(&app, 80, 24);
        let footer = rows
            .iter()
            .find(|row| row.contains("Space tick"))
            .expect("footer row");
        // Drop the dialog border, the table border behind it (a side or the
        // bottom edge, depending on the header height), and padding.
        let footer = footer.trim_end_matches([' ', '│', '─', '┘']);
        assert!(footer.ends_with("Enter apply row  Esc close"), "{footer:?}");
    }

    #[test]
    fn capped_list_says_so_and_filter_reaches_past_the_cap() {
        let users = (0..1030u32)
            .map(|index| AccountUser {
                uid: Uid(2000 + index),
                name: UserName::new(format!("user{index}")).unwrap(),
                primary_gid: Gid(100),
                full_name: None,
                home_dir: format!("/home/user{index}").into(),
                shell: ShellPath::new("/bin/sh").unwrap(),
            })
            .collect();
        let snapshot = AccountSnapshot {
            users,
            groups: vec![AccountGroup {
                gid: Gid(3000),
                name: GroupName::new("dev").unwrap(),
                members: vec![],
            }],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(2000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.active_tab = ActiveTab::Groups;
        let list = app.group_member_candidate_list(3000, true, "");
        assert_eq!((list.names.len(), list.total), (1024, 1030));
        open_modal(&mut app, add_members_dialog(3000));
        assert!(rendered(&app).contains("page 1/86  0 selected  showing first 1024 of 1030"));
        press(&mut app, KeyCode::Char('/'));
        type_text(&mut app, "user1029");
        press(&mut app, KeyCode::Enter);
        assert_eq!(rendered_candidates(&app), ["user1029"]);
        assert!(!rendered(&app).contains("showing first"));
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.last_preview, ["gpasswd -a user1029 dev"]);
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

    /// Rejects the sudo password a set number of times, then accepts it.
    /// Commands then fail without spawning anything.
    struct DenyingRun {
        denials_left: std::sync::atomic::AtomicU8,
    }
    impl DenyingRun {
        fn denying(times: u8) -> Self {
            Self {
                denials_left: std::sync::atomic::AtomicU8::new(times),
            }
        }
    }
    impl CommandRunner for DenyingRun {
        fn authenticate(&self, _: SecretString) -> CoreResult<ElevationGrant> {
            use std::sync::atomic::Ordering;
            match self
                .denials_left
                .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                    left.checked_sub(1)
                }) {
                Ok(_) => Err(CoreError::AuthenticationDenied),
                Err(_) => Ok(ElevationGrant::SudoTimestamp),
            }
        }
        fn run(&self, _: ElevationGrant, _: &crate::sys::CommandSpec) -> CoreResult<CommandResult> {
            Err(CoreError::MissingExecutable { program: "usermod" })
        }
    }

    /// A pending shell change waiting in the sudo prompt, with a runner
    /// that rejects the first `denials` passwords.
    fn app_at_sudo_prompt(denials: u8, log: &SharedLog) -> AppState {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(DenyingRun::denying(denials)),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.set_logger(crate::app::Logger::from_writer(Box::new(log.clone())));
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        execute_pending_plan(&mut app);
        assert!(matches!(
            app.modal(),
            Some(ModalState::SudoPrompt {
                attempts: 0,
                error: None,
                ..
            })
        ));
        app
    }

    fn submit_sudo_password(app: &mut AppState) {
        for character in "wrong".chars() {
            press(app, KeyCode::Char(character));
        }
        press(app, KeyCode::Enter);
    }

    #[test]
    fn rejected_sudo_password_keeps_operation_and_reopens_prompt() {
        let log = SharedLog::default();
        let mut app = app_at_sudo_prompt(2, &log);
        let depth = app.modal_depth();
        let screen = rendered(&app);
        assert!(screen.contains("Authenticating to run: usermod -s /bin/bash alice"));
        assert!(screen.contains("Enter submit  Esc cancel operation"));
        for attempt in 1..=2u8 {
            submit_sudo_password(&mut app);
            let expected = format!("Password rejected (attempt {attempt} of 3)");
            match app.modal() {
                Some(ModalState::SudoPrompt {
                    password,
                    error,
                    attempts,
                }) => {
                    assert_eq!(*attempts, attempt);
                    assert_eq!(error.as_deref(), Some(expected.as_str()));
                    assert!(password.is_empty());
                }
                _ => panic!("expected the sudo prompt"),
            }
            assert!(app.pending_operation.is_some());
            assert_eq!(app.modal_depth(), depth);
            assert!(rendered(&app).contains(&expected));
        }
        submit_sudo_password(&mut app);
        assert!(matches!(app.modal(), Some(ModalState::Info { .. })));
        assert_eq!(app.modal_depth(), 1);
        assert!(app.pending_operation.is_none());
        assert!(app.last_report.is_some());
        let denied = classified_error_message(&CoreError::AuthenticationDenied);
        let text = log.text();
        assert_eq!(text.matches(&denied).count(), 2);
        assert!(!text.contains("wrong"));
    }

    #[test]
    fn third_rejected_sudo_password_drops_the_operation() {
        let log = SharedLog::default();
        let mut app = app_at_sudo_prompt(u8::MAX, &log);
        for _ in 0..3 {
            submit_sudo_password(&mut app);
        }
        match app.modal() {
            Some(ModalState::Info { message, .. }) => assert_eq!(
                message,
                "Authentication failed three times. The operation was not run."
            ),
            _ => panic!("expected the result dialog"),
        }
        assert_eq!(app.modal_depth(), 1);
        assert!(app.pending_operation.is_none());
        assert!(app.last_report.is_none());
        let denied = classified_error_message(&CoreError::AuthenticationDenied);
        assert_eq!(log.text().matches(&denied).count(), 3);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
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
