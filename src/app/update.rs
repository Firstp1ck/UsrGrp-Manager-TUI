//! Event handling and explicit application effects.
//!
//! All mutations pass through the adapter-owned `OperationRequest` bridge.  The
//! application only displays redacted plans/reports; it never chooses commands,
//! receives runners, or retains sudo credentials.

use crate::ui::components::PreviewWindow;
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{Terminal, backend::CrosstermBackend, layout::Rect};

use crate::{
    app::{
        ActiveTab, AppState, BULK_ACTIONS, BULK_ROW_ADD, BULK_ROW_LOCK, BULK_ROW_REMOVE,
        BULK_ROW_UNLOCK, CandidateList, DELETE_ROW_HOME, DELETE_ROW_NO, DELETE_ROW_YES,
        GroupsFocus, InputMode, MAX_EXPIRY_CHARS, MAX_HOME_CHARS, MAX_LENGTH_REACHED,
        MAX_NAME_CHARS, MAX_PASSWORD_CHARS, MEMBER_ACTIONS, MEMBER_ROW_GO_TO, ModalState,
        ModifyField, ModifyMenuItem, PendingAction, RunOptions, SecretInput, UsersFocus,
        civil_date_label, filterconf::FiltersConfig, keymap::KeyAction,
    },
    error::{CoreError, CoreResult, Result},
    sys::{
        ExpiryDate, GroupName, HomePath, OperationRequest, PasswordRecord, SecretString, UserName,
    },
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
    // Loading reads every account, home directory and authorized_keys file,
    // which can take seconds on network homes: show that something is
    // happening first.
    terminal.draw(ui::render_loading)?;
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
/// for page keys and the preview review, and moves focus back to the list
/// when the member pane no longer fits; the next loop iteration redraws at
/// that size. Mouse capture
/// is not enabled, so mouse, focus, and paste events are ignored.
///
/// Ctrl+C quits from every mode whatever keybinds.conf says, because raw
/// mode turns off the terminal's own interrupt. A prepared operation has
/// not run at that point, so it is dropped.
fn handle_event(app: &mut AppState, event: Event) -> bool {
    match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => {
            // A notice such as `copied "alice"` lasts until the next key.
            app.notice = None;
            if is_interrupt(key) {
                app.clear_pending_operation();
                return true;
            }
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
            if !ui::layout::members_visible(width) {
                app.users_focus = UsersFocus::UsersList;
                app.groups_focus = GroupsFocus::GroupsList;
            }
            review_preview_after_resize(app);
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
        // With users marked, the first Esc on the Users tab clears the marks
        // and the next one the search.
        Some(KeyAction::ClearSearch)
            if app.active_tab == ActiveTab::Users && !app.marked_users.is_empty() =>
        {
            app.clear_marks()
        }
        Some(KeyAction::ClearSearch) => app.clear_search(),
        Some(KeyAction::ToggleMark) => app.toggle_mark(),
        Some(KeyAction::Refresh) => app.refresh_accounts(),
        Some(KeyAction::CycleSort) => app.cycle_sort(),
        Some(KeyAction::CopySelection) => app.copy_selection(),
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
                    reveal: false,
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
        // The stacked layout of narrow terminals has no member pane to focus.
        Some(KeyAction::ToggleUsersFocus | KeyAction::ToggleGroupsFocus)
            if !ui::layout::members_visible(app.terminal_size.0) => {}
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
            if is_typed(key, character) && app.query(app.search_tab()).len() < 256 =>
        {
            app.edit_search(|query| query.push(character))
        }
        _ => {}
    }
}

fn open_actions(app: &mut AppState) {
    match app.active_tab {
        ActiveTab::Users if matches!(app.users_focus, UsersFocus::UsersList) => {
            if !app.marked_users.is_empty() {
                open_modal(
                    app,
                    ModalState::BulkActions {
                        selected: 0,
                        error: None,
                    },
                );
            } else if !app.users.is_empty() {
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
                if let Some((username, groupname)) = selected_group_member(app) {
                    open_modal(
                        app,
                        ModalState::MemberActions {
                            selected: 0,
                            username,
                            groupname,
                            error: None,
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
                        username: user.name.clone(),
                        groupname: group.name,
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
        ActiveTab::Groups => open_member_removal(app),
    }
}

/// The highlighted member of the Members pane and its group's name.
fn selected_group_member(app: &AppState) -> Option<(String, String)> {
    let group = app.groups.get(app.selected_group_index)?;
    let member = group.members.get(app.selected_group_member_index)?;
    Some((member.clone(), group.name.clone()))
}

/// Ask to remove the highlighted member from the selected group.
fn open_member_removal(app: &mut AppState) {
    if let Some((username, groupname)) = selected_group_member(app) {
        open_modal(
            app,
            ModalState::ConfirmRemoveUserFromGroup {
                selected: 1,
                username,
                groupname,
            },
        );
    }
}

/// Rows the focused pane shows at the current terminal size, taken from the
/// same layout the renderer uses.
fn page_size(app: &AppState) -> usize {
    let (width, height) = app.terminal_size;
    let area = Rect::new(0, 0, width, height);
    // Same two passes as the renderer: the member pane is what is left
    // under the details wrapped at the pane's inner width.
    let details_width = ui::layout::main_layout(
        area,
        app.active_tab,
        app.show_keybinds,
        ui::layout::details_base_rows(app.active_tab),
    )
    .details
    .width
    .saturating_sub(2);
    let layout = ui::layout::main_layout(
        area,
        app.active_tab,
        app.show_keybinds,
        ui::details_rows(app, details_width),
    );
    let pane = match (app.active_tab, app.users_focus, app.groups_focus) {
        (ActiveTab::Users, UsersFocus::UsersList, _)
        | (ActiveTab::Groups, _, GroupsFocus::GroupsList) => layout.table,
        (ActiveTab::Users, UsersFocus::MemberOf, _)
        | (ActiveTab::Groups, _, GroupsFocus::Members) => layout.members.unwrap_or(layout.table),
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
    if app.settings.wrap_selection {
        move_index(selected, len, delta);
    } else {
        clamp_index(selected, len, delta);
    }
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

/// Move `index` by `delta` within `len` rows, wrapping past either end.
/// Dialog lists always wrap; the main tables do unless settings.conf turns
/// `wrap_selection` off (see [`clamp_index`]).
fn move_index(index: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *index = 0;
        return;
    }
    let next = (*index as isize + delta).rem_euclid(len as isize);
    *index = next as usize;
}

/// Move `index` by `delta` within `len` rows, stopping at the first and last
/// row: the main tables with `wrap_selection = false` in settings.conf.
fn clamp_index(index: &mut usize, len: usize, delta: isize) {
    if len == 0 {
        *index = 0;
        return;
    }
    let next = (*index as isize)
        .saturating_add(delta)
        .clamp(0, len as isize - 1);
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
        ModalState::BulkActions { selected, error } => {
            handle_bulk_actions(app, selected, error, key)
        }
        ModalState::BulkGroupPicker {
            adding,
            selected,
            groups,
            error,
        } => handle_bulk_group_picker(app, *adding, selected, groups, error, key),
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
        ModalState::ModifyPrimaryGroup {
            selected,
            groups,
            current,
            error,
            ..
        } => handle_primary_group_modal(app, selected, groups, *current, error, key),
        ModalState::ModifyHome {
            selected,
            value,
            move_contents,
            current,
            error,
        } => handle_home_input(
            app,
            HomeForm {
                selected,
                value,
                move_contents,
                current,
                error,
            },
            key,
        ),
        ModalState::ModifyGroupsAdd {
            selected,
            selected_multi,
            filter,
            filtering,
            error,
            ..
        } => handle_user_membership(
            app,
            true,
            CandidateDialog {
                selected,
                selected_multi,
                filter,
                filtering,
                error,
            },
            key,
        ),
        ModalState::ModifyGroupsRemove {
            selected,
            selected_multi,
            filter,
            filtering,
            error,
            ..
        } => handle_user_membership(
            app,
            false,
            CandidateDialog {
                selected,
                selected_multi,
                filter,
                filtering,
                error,
            },
            key,
        ),
        ModalState::ModifyPasswordMenu { selected } => handle_password_menu(app, selected, key),
        ModalState::ModifyExpiry {
            value,
            current,
            error,
        } => handle_expiry_input(app, value, current.as_ref(), error, key),
        ModalState::ChangePassword {
            selected,
            password,
            confirm,
            must_change,
            reveal,
            error,
        } => handle_password_input(
            app,
            selected,
            PasswordFields {
                password,
                confirm,
                reveal,
            },
            must_change,
            error,
            key,
        ),
        ModalState::DeleteConfirm {
            selected,
            allowed,
            delete_home,
            username,
            ..
        } => handle_user_delete(app, selected, *allowed, username, delete_home, key),
        ModalState::ConfirmRemoveUserFromGroup {
            selected,
            username,
            groupname,
        } => handle_member_delete(app, selected, username, groupname, key),
        ModalState::MemberActions {
            selected,
            username,
            groupname,
            error,
        } => handle_member_actions(app, selected, username, groupname, error, key),
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
            error,
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
                error,
            },
            key,
        ),
        ModalState::GroupModifyRemoveMembers {
            selected,
            target_gid,
            selected_multi,
            filter,
            filtering,
            error,
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
                error,
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
            reveal,
            password_reentry_required,
            name_touched,
            error,
        } => handle_user_add(
            app,
            selected,
            name,
            PasswordFields {
                password,
                confirm,
                reveal,
            },
            create_home,
            add_to_wheel,
            password_reentry_required,
            name_touched,
            error,
            key,
        ),
        ModalState::OperationConfirm {
            selected,
            preview,
            scroll,
            seen_all,
            ..
        } => {
            let intro = ui::components::confirm_intro(app.dry_run);
            handle_operation_confirm(
                app,
                selected,
                PreviewReview {
                    intro,
                    preview,
                    scroll,
                    seen_all,
                },
                key,
            )
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
        ModalState::Help { scroll } => handle_help_key(app, scroll, key),
    }
    app.reattach_modal(modal, detached);
}

/// Esc and Enter close the help; arrows, j/k, and PgUp/PgDn scroll it,
/// never past the page that shows the last line.
fn handle_help_key(app: &mut AppState, scroll: &mut u16, key: KeyEvent) {
    let current = usize::from(*scroll);
    let next = match key.code {
        KeyCode::Esc | KeyCode::Enter => return back(app),
        KeyCode::Up | KeyCode::Char('k') => current.saturating_sub(1),
        KeyCode::Down | KeyCode::Char('j') => current + 1,
        KeyCode::PageUp => current.saturating_sub(10),
        KeyCode::PageDown => current + 10,
        _ => return,
    };
    let maximum = ui::components::help_max_scroll(app, app.terminal_size);
    *scroll = u16::try_from(next.min(maximum)).unwrap_or(u16::MAX);
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

/// Up/Down/j/k move; Enter opens the group picker or prepares the lock or
/// unlock of every marked user. A refusal, such as a protected marked user,
/// stays in the menu as its inline error.
fn handle_bulk_actions(
    app: &mut AppState,
    selected: &mut usize,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    *error = None;
    if let Some(step) = list_step(key) {
        move_index(selected, BULK_ACTIONS.len(), step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match *selected {
            row @ (BULK_ROW_ADD | BULK_ROW_REMOVE) => {
                let adding = row == BULK_ROW_ADD;
                let groups = app.bulk_group_candidate_list(adding);
                if groups.names.is_empty() {
                    *error = Some(
                        if adding {
                            "No groups are loaded."
                        } else {
                            "No marked user is a member of a group."
                        }
                        .to_owned(),
                    );
                    return;
                }
                open_modal(
                    app,
                    ModalState::BulkGroupPicker {
                        adding,
                        selected: 0,
                        groups,
                        error: None,
                    },
                );
            }
            row @ (BULK_ROW_LOCK | BULK_ROW_UNLOCK) => {
                let usernames = app.marked_usernames();
                let request = |username: &String| {
                    if row == BULK_ROW_LOCK {
                        OperationRequest::LockUser {
                            username: username.clone(),
                        }
                    } else {
                        OperationRequest::UnlockUser {
                            username: username.clone(),
                        }
                    }
                };
                let singles: Vec<_> = usernames
                    .iter()
                    .map(|username| (username.clone(), request(username)))
                    .collect();
                let action = if row == BULK_ROW_LOCK {
                    PendingAction::LockUsers { usernames }
                } else {
                    PendingAction::UnlockUsers { usernames }
                };
                if let Some(message) = start_bulk_action(app, action, singles) {
                    *error = Some(message);
                }
            }
            _ => {}
        },
        _ => {}
    }
}

/// Up/Down/j/k move; Enter adds every marked user to the highlighted group,
/// or removes them from it. Blocked groups and a group with no marked user
/// to change are refused inline. When the bridge refuses the operation, the
/// picker closes and the bulk actions menu shows why.
fn handle_bulk_group_picker(
    app: &mut AppState,
    adding: bool,
    selected: &mut usize,
    groups: &CandidateList,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    *error = None;
    if let Some(step) = list_step(key) {
        move_index(selected, groups.names.len(), step);
        return;
    }
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Enter => {
            let Some(groupname) = groups.names.get(*selected).cloned() else {
                return;
            };
            if let Some(reason) = groups.blocked_reason(*selected) {
                *error = Some(reason.to_owned());
                return;
            }
            let usernames = app.bulk_group_targets(&groupname, adding);
            if usernames.is_empty() {
                *error = Some(if adding {
                    format!("Every marked user is already in {groupname}.")
                } else {
                    format!("No marked user is a member of {groupname}.")
                });
                return;
            }
            let singles = usernames
                .iter()
                .map(|username| {
                    let (username, groupname) = (username.clone(), groupname.clone());
                    let request = if adding {
                        OperationRequest::AddUserToGroup {
                            username: username.clone(),
                            groupname,
                        }
                    } else {
                        OperationRequest::RemoveUserFromGroup {
                            username: username.clone(),
                            groupname,
                        }
                    };
                    (username, request)
                })
                .collect();
            let action = if adding {
                PendingAction::AddMembersToGroup {
                    groupname,
                    usernames,
                }
            } else {
                PendingAction::RemoveMembersFromGroup {
                    groupname,
                    usernames,
                }
            };
            if let Some(message) = start_bulk_action(app, action, singles) {
                back(app);
                if let Some(ModalState::BulkActions { error, .. }) = app.modal_mut() {
                    *error = Some(message);
                }
            }
        }
        _ => {}
    }
}

/// Prepare a bulk action and open its confirmation. On refusal nothing is
/// pending and the inline message is returned: when one user's own request
/// (`singles`, by username) is refused as invalid, the message names that
/// user, so a protected account among the marks can be found and unmarked.
fn start_bulk_action(
    app: &mut AppState,
    action: PendingAction,
    singles: Vec<(String, OperationRequest)>,
) -> Option<String> {
    let error = try_start_pending_action(app, action).err()?;
    let mut culprit = None;
    for (username, request) in singles {
        match app.adapter.prepare_operation(request) {
            Ok(_) => {}
            Err(problem @ CoreError::Validation { .. }) => {
                culprit = Some((username, problem));
                break;
            }
            // Not about one user (for example the account files could not
            // be read): report the operation's own error.
            Err(_) => break,
        }
    }
    Some(match culprit {
        Some((username, problem)) => {
            format!("Refused for {username}: {}", log_error(app, &problem))
        }
        None => log_error(app, &error),
    })
}

fn handle_modify_menu(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    let items = app.modify_menu_items();
    if let Some(step) = list_step(key) {
        move_index(selected, items.len(), step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match items.get(*selected).copied() {
            Some(ModifyMenuItem::AddGroup) => open_modal(
                app,
                ModalState::ModifyGroupsAdd {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                    error: None,
                },
            ),
            Some(ModifyMenuItem::RemoveGroup) => open_modal(
                app,
                ModalState::ModifyGroupsRemove {
                    selected: 0,
                    offset: 0,
                    selected_multi: vec![],
                    filter: String::new(),
                    filtering: false,
                    error: None,
                },
            ),
            Some(ModifyMenuItem::Details) => {
                open_modal(app, ModalState::ModifyDetailsMenu { selected: 0 })
            }
            Some(ModifyMenuItem::Password) => {
                open_modal(app, ModalState::ModifyPasswordMenu { selected: 0 })
            }
            Some(item @ (ModifyMenuItem::Lock | ModifyMenuItem::Unlock)) => {
                if let Some(user) = app.users.get(app.selected_user_index) {
                    let username = user.name.clone();
                    start_pending_action(
                        app,
                        if item == ModifyMenuItem::Lock {
                            PendingAction::LockUser { username }
                        } else {
                            PendingAction::UnlockUser { username }
                        },
                    );
                }
            }
            None => {}
        },
        _ => {}
    }
}

/// Rows of the "Modify details" menu: Username, Full name, Shell, Primary
/// group, Home directory.
const DETAILS_MENU_ROWS: usize = 5;

fn handle_modify_details(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    if let Some(step) = list_step(key) {
        move_index(selected, DETAILS_MENU_ROWS, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => {
            let user = app.users.get(app.selected_user_index);
            match *selected {
                0 => {
                    let value = user.map(|user| user.name.clone()).unwrap_or_default();
                    open_modal(
                        app,
                        ModalState::ModifyTextInput {
                            field: ModifyField::Username,
                            value,
                            error: None,
                        },
                    )
                }
                1 => {
                    let value = user
                        .and_then(|user| user.full_name.clone())
                        .unwrap_or_default();
                    open_modal(
                        app,
                        ModalState::ModifyTextInput {
                            field: ModifyField::Fullname,
                            value,
                            error: None,
                        },
                    )
                }
                3 => {
                    let primary_gid = user.map(|user| user.primary_gid);
                    let groups = app.primary_group_candidate_list();
                    let current = primary_gid.and_then(|gid| {
                        groups.names.iter().position(|name| {
                            app.groups_all
                                .iter()
                                .any(|group| group.gid == gid && group.name == *name)
                        })
                    });
                    open_modal(
                        app,
                        ModalState::ModifyPrimaryGroup {
                            selected: current.unwrap_or(0),
                            groups,
                            current,
                            current_gid: primary_gid.filter(|_| current.is_none()),
                            error: None,
                        },
                    );
                }
                4 => {
                    let current = user.map(|user| user.home_dir.clone()).unwrap_or_default();
                    open_modal(
                        app,
                        ModalState::ModifyHome {
                            selected: 0,
                            value: current.clone(),
                            move_contents: false,
                            current,
                            error: None,
                        },
                    );
                }
                _ => {
                    let mut shells: Vec<String> =
                        app.account_snapshot
                            .as_ref()
                            .map_or_else(Vec::new, |snapshot| {
                                snapshot
                                    .shells
                                    .iter()
                                    .map(|shell| shell.as_str().to_owned())
                                    .collect()
                            });
                    let mut current =
                        user.and_then(|user| shells.iter().position(|shell| *shell == user.shell));
                    // A shell missing from /etc/shells is still the current
                    // one: list it first so the picker starts on it. Only a
                    // path can be chosen; a label such as the empty passwd
                    // shell's `(default /bin/sh)` is shown as a note instead.
                    let unlisted = user
                        .filter(|user| current.is_none() && !user.shell.is_empty())
                        .map(|user| user.shell.clone());
                    let (current_unlisted, current_label) = match unlisted {
                        Some(shell) if shell.starts_with('/') => {
                            shells.insert(0, shell);
                            current = Some(0);
                            (true, None)
                        }
                        label => (false, label),
                    };
                    open_modal(
                        app,
                        ModalState::ModifyShell {
                            selected: current.unwrap_or(0),
                            offset: 0,
                            shells,
                            current,
                            current_unlisted,
                            current_label,
                        },
                    );
                }
            }
        }
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
        KeyCode::Char(character) if is_typed(key, character) => {
            if has_room(value.chars().count(), field.max_chars(), error) {
                value.push(character);
                *error = live_error(value);
            }
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

/// Up/Down/j/k move; Enter changes the primary group to the highlighted
/// one. The current group and entries the policy blocks are refused with an
/// inline message instead.
fn handle_primary_group_modal(
    app: &mut AppState,
    selected: &mut usize,
    groups: &CandidateList,
    current: Option<usize>,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    *error = None;
    if let Some(step) = list_step(key) {
        move_index(selected, groups.names.len(), step);
        return;
    }
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Enter => {
            let (Some(user), Some(groupname)) = (
                app.users.get(app.selected_user_index),
                groups.names.get(*selected),
            ) else {
                return;
            };
            if current == Some(*selected) {
                *error = Some(format!("{groupname} is already the primary group."));
            } else if let Some(reason) = groups.blocked_reason(*selected) {
                *error = Some(reason.to_owned());
            } else {
                let action = PendingAction::ChangePrimaryGroup {
                    username: user.name.clone(),
                    groupname: groupname.clone(),
                };
                start_form_action(app, action, error);
            }
        }
        _ => {}
    }
}

/// Rows of the home directory form: the path field and the checkbox.
const HOME_ROW_PATH: usize = 0;
const HOME_ROW_MOVE: usize = 1;

/// The fields of the home directory form, borrowed from the modal.
struct HomeForm<'a> {
    selected: &'a mut usize,
    value: &'a mut String,
    move_contents: &'a mut bool,
    /// The home when the form opened.
    current: &'a str,
    error: &'a mut Option<String>,
}

/// Up/Down and Tab switch between the path and the checkbox; the path takes
/// typed characters and Backspace, Space ticks the checkbox, and Enter
/// submits from either row. Edits validate the path as it is typed.
fn handle_home_input(app: &mut AppState, form: HomeForm<'_>, key: KeyEvent) {
    let HomeForm {
        selected,
        value,
        move_contents,
        current,
        error,
    } = form;
    let live_error = |value: &str| live_name_error(HomePath::new(value));
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Up | KeyCode::Down | KeyCode::Tab | KeyCode::BackTab => {
            *selected = if *selected == HOME_ROW_PATH {
                HOME_ROW_MOVE
            } else {
                HOME_ROW_PATH
            };
        }
        KeyCode::Char(' ') if *selected == HOME_ROW_MOVE => *move_contents = !*move_contents,
        KeyCode::Backspace if *selected == HOME_ROW_PATH => {
            value.pop();
            *error = live_error(value);
        }
        KeyCode::Char(character) if is_typed(key, character) && *selected == HOME_ROW_PATH => {
            if has_room(value.chars().count(), MAX_HOME_CHARS, error) {
                value.push(character);
                *error = live_error(value);
            }
        }
        KeyCode::Enter => {
            let Some(user) = app.users.get(app.selected_user_index) else {
                return;
            };
            if std::path::Path::new(value.as_str()) == std::path::Path::new(current) {
                *error = Some("This is already the home directory.".to_owned());
                return;
            }
            let action = PendingAction::ChangeHome {
                username: user.name.clone(),
                home: value.clone(),
                move_contents: *move_contents,
            };
            start_form_action(app, action, error);
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
    let list = app.user_group_candidate_list(user.uid, adding, dialog.filter);
    let groups = &list.names;
    match candidate_key(&mut dialog, &list, key) {
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
    /// Why the last tick or apply was refused.
    error: &'a mut Option<String>,
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

/// Keys shared by the four multi-select dialogs. `candidates` is the
/// filtered candidate list the dialog shows.
///
/// `/` starts filter typing: printable characters and Backspace edit the
/// filter, Up/Down still move, and Enter or Esc stop typing. Otherwise
/// Up/Down/j/k move, Space ticks the highlighted row, and `*` ticks every
/// listed row (or clears them when all are ticked already).
///
/// Entries the policy blocks are never ticked: Space on one, or Enter on
/// one with nothing ticked, shows the reason instead, and `*` skips them.
/// Space still clears a tick set before the block appeared. Any other key
/// clears the message.
fn candidate_key(
    dialog: &mut CandidateDialog<'_>,
    candidates: &CandidateList,
    key: KeyEvent,
) -> CandidateKey {
    let list = candidates.names.as_slice();
    *dialog.error = None;
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
                if is_typed(key, character)
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
    let blocked = candidates.blocked_reason(*dialog.selected);
    let highlighted_ticked = list
        .get(*dialog.selected)
        .is_some_and(|name| dialog.selected_multi.contains(name));
    match key.code {
        KeyCode::Esc => return CandidateKey::Close,
        KeyCode::Enter if dialog.selected_multi.is_empty() && blocked.is_some() => {
            *dialog.error = blocked.map(str::to_owned);
        }
        KeyCode::Enter => return CandidateKey::Apply,
        KeyCode::Char('/') => *dialog.filtering = true,
        KeyCode::Char(' ') if blocked.is_some() && !highlighted_ticked => {
            *dialog.error = blocked.map(str::to_owned);
        }
        KeyCode::Char(' ') => {
            if let Some(name) = list.get(*dialog.selected) {
                toggle(dialog.selected_multi, name);
            }
        }
        KeyCode::Char('*') => {
            let allowed: Vec<String> = list
                .iter()
                .enumerate()
                .filter(|(index, _)| candidates.blocked_reason(*index).is_none())
                .map(|(_, name)| name.clone())
                .collect();
            toggle_all(dialog.selected_multi, &allowed);
        }
        _ => {}
    }
    CandidateKey::Handled
}

/// Rows of the "Password" menu: Set or change password, Force change at
/// next login, Account expiry date.
const PASSWORD_MENU_ROWS: usize = 3;

fn handle_password_menu(app: &mut AppState, selected: &mut usize, key: KeyEvent) {
    if let Some(step) = list_step(key) {
        move_index(selected, PASSWORD_MENU_ROWS, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => match *selected {
            0 => open_modal(
                app,
                ModalState::ChangePassword {
                    selected: 0,
                    password: SecretInput::default(),
                    confirm: SecretInput::default(),
                    must_change: false,
                    reveal: false,
                    error: None,
                },
            ),
            1 => {
                if let Some(user) = app.users.get(app.selected_user_index) {
                    start_pending_action(
                        app,
                        PendingAction::ResetPassword {
                            username: user.name.clone(),
                        },
                    )
                }
            }
            _ => {
                if let Some(user) = app.users.get(app.selected_user_index) {
                    // The cached shadow expiry as a date; None when shadow
                    // has no readable record for the user.
                    let current = app
                        .diagnostics
                        .shadow
                        .status(&user.name)
                        .map(|status| status.expire_abs_days.map(civil_date_label));
                    let value = current.clone().flatten().unwrap_or_default();
                    let error = live_expiry_error(&value);
                    open_modal(
                        app,
                        ModalState::ModifyExpiry {
                            value,
                            current,
                            error,
                        },
                    );
                }
            }
        },
        _ => {}
    }
}

/// Inline message for an expiry date typed so far. Empty is valid: it
/// means the account never expires.
fn live_expiry_error(value: &str) -> Option<String> {
    if value.is_empty() {
        None
    } else {
        live_name_error(ExpiryDate::new(value))
    }
}

/// The expiry date form takes typed characters and Backspace, validating
/// as it goes; Enter sets the typed date, or removes the expiry when the
/// field is empty. `current` is the expiry when the form opened, None when
/// it is unknown.
fn handle_expiry_input(
    app: &mut AppState,
    value: &mut String,
    current: Option<&Option<String>>,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Backspace => {
            value.pop();
            *error = live_expiry_error(value);
        }
        KeyCode::Char(character) if is_typed(key, character) => {
            if has_room(value.chars().count(), MAX_EXPIRY_CHARS, error) {
                value.push(character);
                *error = live_expiry_error(value);
            }
        }
        KeyCode::Enter => {
            let Some(user) = app.users.get(app.selected_user_index) else {
                return;
            };
            let date = (!value.is_empty()).then(|| value.clone());
            if current == Some(&date) {
                *error = Some(if date.is_some() {
                    "This is already the expiry date.".to_owned()
                } else {
                    "The account already never expires.".to_owned()
                });
                return;
            }
            let action = PendingAction::SetAccountExpiry {
                username: user.name.clone(),
                date,
            };
            start_form_action(app, action, error);
        }
        _ => {}
    }
}

fn handle_password_input(
    app: &mut AppState,
    selected: &mut usize,
    fields: PasswordFields<'_>,
    must_change: &mut bool,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    let PasswordFields {
        password,
        confirm,
        reveal,
    } = fields;
    if is_reveal_toggle(key) {
        *reveal = !*reveal;
        return;
    }
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
        KeyCode::Char(character) if is_typed(key, character) && *selected < 2 => {
            let field = if *selected == 0 { password } else { confirm };
            if has_room(field.char_count(), MAX_PASSWORD_CHARS, error) {
                field.push(character);
                *error = None;
            }
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
                message: crate::app::PROTECTED_USER_MESSAGE.to_owned(),
            },
        ),
        Answer::Other => {}
    }
}

fn handle_member_delete(
    app: &mut AppState,
    selected: &mut usize,
    username: &str,
    groupname: &str,
    key: KeyEvent,
) {
    match yes_no_answer(selected, key) {
        Answer::Cancel | Answer::Decline => back(app),
        Answer::Confirm if *selected == 0 => start_pending_action(
            app,
            PendingAction::RemoveUserFromGroup {
                username: username.to_owned(),
                groupname: groupname.to_owned(),
            },
        ),
        // Enter on No closes the dialog, like `n`.
        Answer::Confirm => back(app),
        Answer::Other => {}
    }
}

/// Rows of the group actions menu: Modify, then Delete. New groups are
/// created with the NewUser key, not from this per-group menu.
fn handle_group_actions(
    app: &mut AppState,
    selected: &mut usize,
    target_gid: &mut Option<u32>,
    key: KeyEvent,
) {
    if let Some(step) = list_step(key) {
        move_index(selected, 2, step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter => {
            let target_gid = target_gid.or_else(|| {
                app.groups
                    .get(app.selected_group_index)
                    .map(|group| group.gid)
            });
            open_modal(
                app,
                if *selected == 0 {
                    ModalState::GroupModifyMenu {
                        selected: 0,
                        target_gid,
                    }
                } else {
                    ModalState::GroupDeleteConfirm {
                        selected: 1,
                        target_gid,
                    }
                },
            );
        }
        _ => {}
    }
}

/// Up/Down/j/k move; Enter on `Go to user` shows the member on the Users
/// tab, on `Remove from group` opens the removal confirmation above this
/// menu, so Esc there comes back here.
fn handle_member_actions(
    app: &mut AppState,
    selected: &mut usize,
    username: &str,
    groupname: &str,
    error: &mut Option<String>,
    key: KeyEvent,
) {
    *error = None;
    if let Some(step) = list_step(key) {
        move_index(selected, MEMBER_ACTIONS.len(), step);
        return;
    }
    match key.code {
        KeyCode::Esc | KeyCode::Backspace => back(app),
        KeyCode::Enter if *selected == MEMBER_ROW_GO_TO => {
            if let Err(message) = go_to_user(app, username) {
                *error = Some(message);
            }
        }
        KeyCode::Enter => open_modal(
            app,
            ModalState::ConfirmRemoveUserFromGroup {
                selected: 1,
                username: username.to_owned(),
                groupname: groupname.to_owned(),
            },
        ),
        _ => {}
    }
}

/// Close the dialogs, switch to the Users tab with the users list focused,
/// and select `username` by its (UID, name) identity. When the users search
/// or filters hide that user, both are cleared first and the status bar says
/// so. A member without a local user record is an error and nothing moves.
fn go_to_user(app: &mut AppState, username: &str) -> std::result::Result<(), String> {
    let identity = |user: &crate::sys::SystemUser| (user.uid, user.name.clone());
    let visible = app
        .users
        .iter()
        .find(|user| user.name == username)
        .map(identity);
    let Some((uid, name)) = visible.clone().or_else(|| {
        app.users_all
            .iter()
            .find(|user| user.name == username)
            .map(identity)
    }) else {
        return Err(format!("{username} has no local user record"));
    };
    app.close_modals();
    app.active_tab = ActiveTab::Users;
    app.users_focus = UsersFocus::UsersList;
    if visible.is_none() {
        let chips_on = app.users_filter.is_some() || app.users_filter_chips != Default::default();
        app.users_query.clear();
        app.users_filter_chips = Default::default();
        app.users_filter = None;
        app.sort_and_filter();
        app.notice = Some(format!("cleared search and filters to show {username}"));
        // filter.conf keeps matching the chips in effect, as after the
        // filter menu.
        if chips_on {
            save_filters(app);
        }
    }
    // A new user has its own Member of list; start its cursor at the top.
    if app.selected_user_uid != Some(uid) || app.selected_user_name.as_deref() != Some(&name) {
        app.selected_user_group_index = 0;
        app.selected_user_group_gid = None;
        app.selected_user_group_name = None;
    }
    app.selected_user_uid = Some(uid);
    app.selected_user_name = Some(name);
    app.normalize_selections();
    Ok(())
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
        KeyCode::Char(character) if is_typed(key, character) => {
            if has_room(name.chars().count(), MAX_NAME_CHARS, error) {
                name.push(character);
                *error = live_name_error(GroupName::new(&*name));
            }
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
                            message: crate::app::PROTECTED_GROUP_MESSAGE.to_owned(),
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
                    error: None,
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
                    error: None,
                },
            ),
            _ => {
                let name = target_gid
                    .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                    .or_else(|| app.groups.get(app.selected_group_index))
                    .map(|group| group.name.clone())
                    .unwrap_or_default();
                open_modal(
                    app,
                    ModalState::GroupRenameInput {
                        name,
                        target_gid: *target_gid,
                        error: None,
                    },
                )
            }
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
        KeyCode::Char(character) if is_typed(key, character) => {
            if has_room(name.chars().count(), MAX_NAME_CHARS, error) {
                name.push(character);
                *error = live_name_error(GroupName::new(&*name));
            }
        }
        KeyCode::Enter => {
            let group = target_gid
                .and_then(|gid| app.groups.iter().find(|group| group.gid == gid))
                .or_else(|| app.groups.get(app.selected_group_index));
            if let Some(group) = group {
                if crate::app::is_default_protected_group(group) {
                    *error = Some(crate::app::PROTECTED_GROUP_MESSAGE.to_owned());
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
    let list = app.group_member_candidate_list(group.gid, adding, dialog.filter);
    let users = &list.names;
    match candidate_key(&mut dialog, &list, key) {
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
    fields: PasswordFields<'_>,
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
    let PasswordFields {
        password,
        confirm,
        reveal,
    } = fields;
    if is_reveal_toggle(key) {
        *reveal = !*reveal;
        return;
    }
    match key.code {
        KeyCode::Esc => back(app),
        KeyCode::Up => *selected = selected.saturating_sub(1),
        KeyCode::Down => *selected = (*selected + 1).min(5),
        KeyCode::Char(' ') if *selected == 3 => *create_home = !*create_home,
        // A group the policy refuses cannot be ticked; the row says why.
        // Clearing an existing tick is always allowed.
        KeyCode::Char(' ') if *selected == 4 => {
            if *add_to_wheel || app.membership_block(&app.sudo_group).is_none() {
                *add_to_wheel = !*add_to_wheel;
            }
        }
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
        KeyCode::Char(character) if is_typed(key, character) && *selected == 0 => {
            if has_room(name.chars().count(), MAX_NAME_CHARS, error) {
                name.push(character);
                *name_touched = true;
                *error = live_error(name, *name_touched);
            }
        }
        KeyCode::Char(character)
            if is_typed(key, character) && (*selected == 1 || *selected == 2) =>
        {
            let field = if *selected == 1 {
                &mut *password
            } else {
                &mut *confirm
            };
            if has_room(field.char_count(), MAX_PASSWORD_CHARS, error) {
                field.push(character);
                if *selected == 1 {
                    *password_reentry_required = false;
                }
                *error = live_error(name, *name_touched);
            }
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
                    sudo_group: add_to_wheel.then(|| app.sudo_group.clone()),
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

/// The scrollable preview of an open operation confirmation.
struct PreviewReview<'a> {
    /// First body line, from [`ui::components::confirm_intro`].
    intro: &'static str,
    preview: &'a [String],
    scroll: &'a mut usize,
    seen_all: &'a mut bool,
}

impl PreviewReview<'_> {
    /// What the renderer shows at `scroll` on `terminal`, from its own math.
    fn window(&self, terminal: (u16, u16), scroll: usize) -> PreviewWindow {
        ui::components::operation_preview_window(terminal, self.intro, self.preview, scroll)
    }

    /// Show the lines from `scroll` on and remember when the last step has
    /// been on screen. When no complete step is drawn (terminal too small or
    /// below the minimum), neither the position nor the review moves.
    fn show(&mut self, terminal: (u16, u16), scroll: usize) {
        if let PreviewWindow::Steps(shown) = self.window(terminal, scroll) {
            *self.scroll = shown.start;
            *self.seen_all |= shown.end >= self.preview.len();
        }
    }

    /// Apply needs every step reviewed and at least one step on screen now.
    /// A dialog that shows no command (terminal too small, or below the
    /// minimum size) never applies, however short the preview; its
    /// "Terminal too small" line says why.
    fn allows_apply(&self, terminal: (u16, u16)) -> bool {
        *self.seen_all && matches!(self.window(terminal, *self.scroll), PreviewWindow::Steps(_))
    }
}

/// A resize can bring the last preview step on screen; latch the review then,
/// since the renderer already reports every step shown.
fn review_preview_after_resize(app: &mut AppState) {
    let terminal = app.terminal_size;
    let intro = ui::components::confirm_intro(app.dry_run);
    if let Some(ModalState::OperationConfirm {
        preview,
        scroll,
        seen_all,
        ..
    }) = app.modal_mut()
    {
        let current = *scroll;
        PreviewReview {
            intro,
            preview,
            scroll,
            seen_all,
        }
        .show(terminal, current);
    }
}

/// Up/Down (k/j) and PgUp/PgDn scroll the preview; Left/Right, y/n, Enter,
/// and Esc answer. Apply is refused until every step has been on screen,
/// and whenever the terminal is too small to show any step.
fn handle_operation_confirm(
    app: &mut AppState,
    selected: &mut usize,
    mut review: PreviewReview<'_>,
    key: KeyEvent,
) {
    let terminal = app.terminal_size;
    let scroll = *review.scroll;
    let page = match review.window(terminal, scroll) {
        PreviewWindow::Steps(shown) => shown.len().max(1),
        PreviewWindow::Hidden | PreviewWindow::TooSmall { .. } => 1,
    };
    let target = match key.code {
        KeyCode::Up | KeyCode::Char('k') => Some(scroll.saturating_sub(1)),
        KeyCode::Down | KeyCode::Char('j') => Some(scroll.saturating_add(1)),
        KeyCode::PageUp => Some(scroll.saturating_sub(page)),
        KeyCode::PageDown => Some(scroll.saturating_add(page)),
        _ => None,
    };
    // Without a scroll key, re-check the current view.
    review.show(terminal, target.unwrap_or(scroll));
    if target.is_some() {
        return;
    }
    match yes_no_answer(selected, key) {
        Answer::Cancel | Answer::Decline => cancel_operation(app),
        // Refused: the dialog stays open and its review line says why.
        Answer::Confirm if *selected == 0 && !review.allows_apply(terminal) => {}
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
        KeyCode::Char(character) if is_typed(key, character) && password.len() < 1024 => {
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
    let description = action.describe();
    let result = operation_request(app, action)
        .and_then(|request| prepare_request(app, request, description));
    if result.is_err() {
        app.clear_pending_operation();
    }
    result
}

/// Prepare `request` and open its confirmation, titled with `description`
/// (what the action does, from [`PendingAction::describe`]).
fn prepare_request(
    app: &mut AppState,
    request: OperationRequest,
    description: String,
) -> CoreResult<()> {
    match app.adapter.prepare_operation(request) {
        Ok(plan) => {
            let prepared = crate::app::PreparedOperation {
                plan,
                description: description.clone(),
            };
            let preview = prepared.preview();
            app.log_event("prepared", &preview.join("; "));
            app.last_preview = preview.clone();
            app.pending_operation = Some(prepared);
            let intro = ui::components::confirm_intro(app.dry_run);
            let seen_all = preview.len() <= ui::components::PREVIEW_SEEN_IMMEDIATELY
                || ui::components::operation_preview_window(app.terminal_size, intro, &preview, 0)
                    .shows_last(preview.len());
            open_modal(
                app,
                ModalState::OperationConfirm {
                    selected: 1,
                    action: description,
                    preview,
                    scroll: 0,
                    seen_all,
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
        show_outcome(app, None, "No prepared operation is available.".to_owned());
        return;
    };
    let steps = prepared.preview().join("; ");
    let intent = prepared.description.clone();
    if app.dry_run {
        // The adapter is never consulted, so no sudo prompt and no command.
        app.clear_pending_operation();
        app.log_event("dry-run", &steps);
        show_outcome(
            app,
            Some("Dry run"),
            format!("{intent}\nDry run: nothing was executed."),
        );
        return;
    }
    match app.adapter.execute_prepared_operation(&prepared.plan) {
        Ok(report) => {
            let message = report_message(&report);
            // The log keeps the internal step ids; the dialog shows plain words.
            let logged = format_report(&report, |id| id);
            app.log_event("executed", &format!("{steps} => {logged}"));
            if let Some(failed) = &report.failed {
                // A long preview can push the failure past the line cap, so
                // the classified error also gets a line of its own.
                app.log_event("error", &classified_error_message(&failed.error));
            }
            let title = report_title(&report);
            app.last_report = Some(report);
            app.pending_operation = None;
            app.refresh_accounts();
            show_outcome(app, Some(title), format!("{intent}\n{message}"));
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
                    Some(FAILED_TITLE),
                    format!(
                        "{intent}\nAuthentication failed three times. The operation was not run."
                    ),
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
            show_outcome(app, Some(FAILED_TITLE), format!("{intent}\n{message}"));
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
            sudo_group,
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
            if let Some(groupname) = sudo_group {
                requests.push(OperationRequest::AddUserToGroup {
                    username,
                    groupname,
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
        PendingAction::LockUser { username } => vec![OperationRequest::LockUser { username }],
        PendingAction::UnlockUser { username } => {
            vec![OperationRequest::UnlockUser { username }]
        }
        PendingAction::LockUsers { usernames } => usernames
            .into_iter()
            .map(|username| OperationRequest::LockUser { username })
            .collect(),
        PendingAction::UnlockUsers { usernames } => usernames
            .into_iter()
            .map(|username| OperationRequest::UnlockUser { username })
            .collect(),
        PendingAction::ChangePrimaryGroup {
            username,
            groupname,
        } => vec![OperationRequest::ChangeUserPrimaryGroup {
            username,
            groupname,
        }],
        PendingAction::ChangeHome {
            username,
            home,
            move_contents,
        } => vec![OperationRequest::ChangeUserHome {
            username,
            home,
            move_contents,
        }],
        PendingAction::SetAccountExpiry { username, date } => {
            vec![OperationRequest::SetAccountExpiry { username, date }]
        }
    };
    Ok(if requests.len() == 1 {
        requests.pop().expect("one request remains")
    } else {
        OperationRequest::Composite { requests }
    })
}

/// Title of the result dialog. A failure before any step completed is a
/// failure; a failure after some steps, or a run whose requested state the
/// account files do not show, is partial; anything else is done.
fn report_title(report: &crate::sys::OperationReport) -> &'static str {
    let partial = matches!(
        report.reconciliation,
        crate::sys::ReconciliationStatus::Partial { .. }
    );
    match &report.failed {
        Some(_) if report.completed.is_empty() => FAILED_TITLE,
        Some(_) => "Partially done",
        None if partial => "Partially done",
        None => "Done",
    }
}

/// Plain-word name of a step id from src/sys. Step ids are internal (tests
/// and the log key on them), so the result dialog shows this label instead.
/// Unknown ids fall back to the id itself.
fn step_label(id: &str) -> &str {
    plain_step_label(id).unwrap_or(id)
}

fn plain_step_label(id: &str) -> Option<&'static str> {
    Some(match id {
        "change user GECOS" => "set full name",
        "change user shell" => "change shell",
        "rename user" => "rename user",
        "add user to group" => "add to group",
        "remove user from group" => "remove from group",
        "set user password" => "set password",
        "expire user password" => "force password change",
        "lock user" => "lock account",
        "unlock user" => "unlock account",
        "change user primary group" => "change primary group",
        "change user home" => "change home directory",
        "set account expiry" => "set account expiry",
        "create user" => "create user",
        "delete user" => "delete user",
        "create group" => "create group",
        "delete group" => "delete group",
        "rename group" => "rename group",
        _ => return None,
    })
}

/// Result lines after the description: step counts, the failed step, and
/// whether the account files confirm the outcome.
fn report_message(report: &crate::sys::OperationReport) -> String {
    format_report(report, step_label)
}

/// `report_message` with the failed step named by `label`.
fn format_report(report: &crate::sys::OperationReport, label: fn(&str) -> &str) -> String {
    let completed = report.completed.len();
    let mut message = format!(
        "{completed} {} completed, {} skipped",
        if completed == 1 { "step" } else { "steps" },
        report.skipped.len()
    );
    if let Some(failed) = &report.failed {
        message.push_str(&format!(
            "\nFailed at {}: {}",
            label(&failed.id),
            classified_error_message(&failed.error)
        ));
    }
    match &report.reconciliation {
        crate::sys::ReconciliationStatus::Verified => {
            message.push_str("\nVerified against the account files.")
        }
        crate::sys::ReconciliationStatus::Partial { detail } => {
            // `detail` names the requested state that was not observed.
            message.push_str(&format!("\nPartial: not confirmed that {detail}"))
        }
        crate::sys::ReconciliationStatus::Unavailable { detail } => {
            message.push_str(&format!("\nCould not verify: {detail}"))
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
    format!("{error} [{code}]")
}

/// Show a classified error above the current dialog and append it to the
/// operation log. Esc returns to the dialog so the input can be corrected.
fn show_error(app: &mut AppState, error: &CoreError) {
    let message = log_error(app, error);
    open_modal(
        app,
        ModalState::Info {
            title: Some(FAILED_TITLE.to_owned()),
            message,
        },
    );
}

/// The two password fields of a form and whether they are shown in clear.
struct PasswordFields<'a> {
    password: &'a mut SecretInput,
    confirm: &'a mut SecretInput,
    reveal: &'a mut bool,
}

/// Ctrl+C, the fixed quit key.
fn is_interrupt(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C'))
}

/// Whether `character` is text to insert: printable and typed without Ctrl
/// or Alt, so chords such as Ctrl+X never add their letter to a field.
fn is_typed(key: KeyEvent, character: char) -> bool {
    !character.is_control()
        && !key
            .modifiers
            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
}

/// Ctrl+R, which shows or hides the typed passwords of a form.
fn is_reveal_toggle(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('r' | 'R'))
}

/// Whether a field holding `count` characters takes one more. At the limit
/// the form shows [`MAX_LENGTH_REACHED`] instead.
fn has_room(count: usize, maximum: usize, error: &mut Option<String>) -> bool {
    if count < maximum {
        true
    } else {
        *error = Some(MAX_LENGTH_REACHED.to_owned());
        false
    }
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
fn show_outcome(app: &mut AppState, title: Option<&str>, message: String) {
    app.close_modals();
    open_modal(
        app,
        ModalState::Info {
            title: title.map(str::to_owned),
            message,
        },
    );
}

/// Result dialog title when nothing, or nothing useful, was done.
const FAILED_TITLE: &str = "Failed";

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
        return;
    }
    // The saved file now exists and is the one the next start reads.
    if let Some(paths) = app
        .config_paths
        .iter_mut()
        .find(|paths| paths.name == "filter.conf")
    {
        paths.state = crate::app::ConfigFileState::Loaded(path);
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
        assert_eq!(
            report_message(&report),
            "0 steps completed, 0 skipped\nCould not verify: shadow unavailable"
        );
    }

    #[test]
    fn pending_actions_describe_their_intent() {
        let names = |list: &[&str]| list.iter().map(|name| (*name).to_owned()).collect();
        let cases = [
            (
                PendingAction::AddUserToGroups {
                    username: "alice".into(),
                    groupnames: names(&["wheel", "docker"]),
                },
                "Add alice to wheel, docker",
            ),
            (
                PendingAction::RemoveUserFromGroup {
                    username: "alice".into(),
                    groupname: "wheel".into(),
                },
                "Remove alice from wheel",
            ),
            (
                PendingAction::AddMembersToGroup {
                    groupname: "dev".into(),
                    usernames: names(&["a", "b", "c", "d", "e", "f", "g"]),
                },
                "Add a, b, c, d, e and 2 more to dev",
            ),
            (
                PendingAction::ChangeShell {
                    username: "alice".into(),
                    new_shell: "/bin/zsh".into(),
                },
                "Change shell of alice to /bin/zsh",
            ),
            (
                PendingAction::ChangeUsername {
                    old_username: "alice".into(),
                    new_username: "alicia".into(),
                },
                "Rename user alice to alicia",
            ),
            (
                PendingAction::ChangeFullname {
                    username: "alice".into(),
                    new_fullname: "Alice L".into(),
                },
                "Set full name of alice",
            ),
            (
                PendingAction::CreateUserWithOptions {
                    username: "alice".into(),
                    set_password: true,
                    create_home: true,
                    sudo_group: Some("wheel".into()),
                },
                "Create user alice (home, password, sudo group wheel)",
            ),
            (
                PendingAction::CreateUserWithOptions {
                    username: "alice".into(),
                    set_password: false,
                    create_home: false,
                    sudo_group: None,
                },
                "Create user alice",
            ),
            (
                PendingAction::DeleteUser {
                    username: "alice".into(),
                    delete_home: true,
                },
                "Delete user alice and its home",
            ),
            (
                PendingAction::CreateGroup {
                    groupname: "dev".into(),
                },
                "Create group dev",
            ),
            (
                PendingAction::DeleteGroup {
                    groupname: "dev".into(),
                },
                "Delete group dev",
            ),
            (
                PendingAction::RenameGroup {
                    old_name: "dev".into(),
                    new_name: "devs".into(),
                },
                "Rename group dev to devs",
            ),
            (
                PendingAction::SetPassword {
                    username: "alice".into(),
                    must_change: true,
                },
                "Set password for alice (must change at next login)",
            ),
            (
                PendingAction::ResetPassword {
                    username: "alice".into(),
                },
                "Force alice to change password at next login",
            ),
        ];
        for (action, expected) in cases {
            assert_eq!(action.describe(), expected);
        }
    }

    fn report(
        completed: usize,
        failed: bool,
        reconciliation: crate::sys::ReconciliationStatus,
    ) -> crate::sys::OperationReport {
        let preview = crate::sys::CommandSpec::new(crate::sys::KnownProgram::GroupAdd)
            .group_name(&GroupName::new("dev").unwrap())
            .redacted_preview();
        crate::sys::OperationReport {
            completed: (0..completed)
                .map(|_| crate::sys::CompletedStep {
                    id: "create group".to_owned(),
                    preview: preview.clone(),
                    verification: crate::sys::StepVerification::Verified,
                })
                .collect(),
            skipped: vec![],
            compensated: vec![],
            failed: failed.then(|| crate::sys::FailedStep {
                id: "add member".to_owned(),
                error: CoreError::MissingExecutable { program: "gpasswd" },
            }),
            reconciliation,
        }
    }

    #[test]
    fn result_titles_and_lines_are_plain_words() {
        use crate::sys::ReconciliationStatus::{Partial, Unavailable, Verified};
        let partial = || Partial {
            detail: "alice is in dev".to_owned(),
        };
        let done = report(2, false, Verified);
        assert_eq!(report_title(&done), "Done");
        assert_eq!(
            report_message(&done),
            "2 steps completed, 0 skipped\nVerified against the account files."
        );
        let unverified = report(
            1,
            false,
            Unavailable {
                detail: "shadow unavailable".to_owned(),
            },
        );
        assert_eq!(report_title(&unverified), "Done");
        assert!(report_message(&unverified).starts_with("1 step completed, 0 skipped\n"));

        let failed = report(0, true, Verified);
        assert_eq!(report_title(&failed), "Failed");
        assert_eq!(report_title(&report(0, true, partial())), "Failed");
        let missing =
            classified_error_message(&CoreError::MissingExecutable { program: "gpasswd" });
        assert!(missing.ends_with(" [E-EXEC-MISSING]"));
        assert!(report_message(&failed).contains(&format!("\nFailed at add member: {missing}\n")));

        let halfway = report(1, true, partial());
        assert_eq!(report_title(&halfway), "Partially done");
        assert!(
            report_message(&halfway).ends_with("\nPartial: not confirmed that alice is in dev")
        );
        assert_eq!(report_title(&report(0, false, partial())), "Partially done");
    }

    #[test]
    fn error_codes_trail_the_message_in_brackets() {
        let error = CoreError::Validation {
            field: "user name",
            reason: "must contain 1 to 32 bytes",
        };
        assert_eq!(
            classified_error_message(&error),
            "invalid user name: must contain 1 to 32 bytes [E-VALIDATION]"
        );
    }

    #[test]
    fn confirmation_is_titled_with_the_intent() {
        let mut app = user_app();
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, .. })
                if action == "Change shell of alice to /bin/bash"
        ));
        let rows = rendered_rows(&app, 100, 30);
        assert!(rows.iter().any(|row| {
            row.contains("Confirm: Change shell of alice to /bin/bash (steps 1-1 of 1)")
        }));
        let intro = rows
            .iter()
            .position(|row| row.contains(ui::components::CONFIRM_INTRO))
            .expect("intro line");
        assert!(rows[intro + 2].contains("usermod -s /bin/bash alice"));
    }

    #[test]
    fn failed_run_reports_the_intent_under_a_failed_title() {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(FailRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        execute_pending_plan(&mut app);
        match app.modal() {
            Some(ModalState::Info { title, message }) => {
                assert_eq!(title.as_deref(), Some("Failed"), "{message}");
                let lines: Vec<&str> = message.lines().collect();
                assert_eq!(lines[0], "Change shell of alice to /bin/bash");
                assert_eq!(lines[1], "0 steps completed, 0 skipped");
                assert!(lines[2].starts_with("Failed at "));
                assert!(lines[2].ends_with("[E-EXEC-MISSING]"));
            }
            _ => panic!("expected the result dialog"),
        }
        assert!(rendered(&app).contains("Failed"));
    }

    #[test]
    fn failed_full_name_change_names_the_step_in_plain_words() {
        let snapshot = fixture_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(FailRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        start_pending_action(
            &mut app,
            PendingAction::ChangeFullname {
                username: "alice".to_owned(),
                new_fullname: "Alice Liddell".to_owned(),
            },
        );
        execute_pending_plan(&mut app);
        match app.modal() {
            Some(ModalState::Info { title, message }) => {
                assert_eq!(title.as_deref(), Some("Failed"), "{message}");
                let failure = message
                    .lines()
                    .find(|line| line.starts_with("Failed at "))
                    .expect("failure line");
                assert!(
                    failure.starts_with("Failed at set full name: "),
                    "{failure}"
                );
                assert!(message.contains("full name"));
                assert!(!message.contains("GECOS"), "{message}");
            }
            _ => panic!("expected the result dialog"),
        }
        let text = rendered(&app);
        assert!(text.contains("full name"));
        assert!(!text.contains("GECOS"));
    }

    #[test]
    fn every_planned_step_id_has_a_plain_label() {
        let source = include_str!("../sys/mod.rs");
        let mut ids = Vec::new();
        for (index, _) in source.match_indices("PlannedStep::new(") {
            let rest = &source[index..];
            // The id is the first string literal of the call; a conditional
            // id (`if .. { "a" } else { "b" }`) has one literal per branch
            // before the first `CommandSpec`/variable argument.
            let args_end = rest
                .find("CommandSpec::")
                .into_iter()
                .chain(rest.find(",\n"))
                .chain(rest.find(", "))
                .min()
                .unwrap_or(rest.len());
            let mut args = &rest[..args_end];
            while let Some(start) = args.find('"') {
                let tail = &args[start + 1..];
                let end = tail.find('"').expect("closed string literal");
                ids.push(&tail[..end]);
                args = &tail[end + 1..];
            }
        }
        assert!(ids.len() >= 11, "found {ids:?}");
        for id in [
            "change user GECOS",
            "add user to group",
            "remove user from group",
        ] {
            assert!(ids.contains(&id), "{id} missing from {ids:?}");
        }
        for id in ids {
            let label = plain_step_label(id);
            assert!(label.is_some(), "step id {id:?} has no plain-word label");
            assert!(!label.unwrap().contains("GECOS"), "{id}");
        }
        assert_eq!(step_label("some future step"), "some future step");
    }

    #[test]
    fn member_removal_names_the_user_and_the_group() {
        let mut app = wheel_app();
        app.active_tab = ActiveTab::Groups;
        app.groups_focus = GroupsFocus::Members;
        app.selected_group_index = app
            .groups
            .iter()
            .position(|group| group.name == "dev")
            .unwrap();
        app.selected_group_member_index = 0;
        open_actions(&mut app);
        assert!(matches!(
            app.modal(),
            Some(ModalState::MemberActions { username, groupname, .. })
                if username == "alice" && groupname == "dev"
        ));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ConfirmRemoveUserFromGroup { username, groupname, .. })
                if username == "alice" && groupname == "dev"
        ));
        assert!(rendered(&app).contains("Remove alice from dev?"));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, .. }) if action == "Remove alice from dev"
        ));
    }

    /// `membership_app` on the Groups tab with the Members pane of `dev`
    /// focused and `member` highlighted.
    fn dev_members_app(app: &mut AppState, member: &str) {
        app.active_tab = ActiveTab::Groups;
        app.groups_focus = GroupsFocus::Members;
        app.selected_group_index = app
            .groups
            .iter()
            .position(|group| group.name == "dev")
            .unwrap();
        app.selected_group_member_index = app.groups[app.selected_group_index]
            .members
            .iter()
            .position(|name| name == member)
            .unwrap();
        app.capture_selection_identities();
    }

    fn selected_user_name(app: &AppState) -> Option<&str> {
        app.users
            .get(app.selected_user_index)
            .map(|user| user.name.as_str())
    }

    #[test]
    fn group_actions_offer_modify_and_delete_only() {
        let mut app = wheel_app();
        app.active_tab = ActiveTab::Groups;
        let dev = app
            .groups
            .iter()
            .position(|group| group.name == "dev")
            .unwrap();
        app.selected_group_index = dev;
        app.capture_selection_identities();
        press(&mut app, KeyCode::Enter);
        let screen = rendered(&app);
        assert!(screen.contains("Group actions"));
        assert!(screen.contains("Modify"));
        assert!(screen.contains("Delete"));
        assert!(!screen.contains("Create"), "{screen}");

        // Row 0 is Modify for the highlighted group.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupModifyMenu {
                target_gid: Some(2000),
                ..
            })
        ));
        press(&mut app, KeyCode::Esc);

        // Row 1 is Delete; the list wraps after it.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupDeleteConfirm {
                target_gid: Some(2000),
                ..
            })
        ));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Down);
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupsActions { selected: 0, .. })
        ));
    }

    #[test]
    fn enter_in_members_opens_a_menu_and_delete_still_asks_directly() {
        let mut app = membership_app();
        dev_members_app(&mut app, "alice");
        press(&mut app, KeyCode::Enter);
        let screen = rendered(&app);
        for text in [
            "Member actions",
            "alice in dev",
            "Go to user",
            "Remove from group",
        ] {
            assert!(screen.contains(text), "missing {text:?} in {screen}");
        }

        // Remove from group asks on top of the menu; Esc comes back to it.
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ConfirmRemoveUserFromGroup { username, groupname, selected: 1 })
                if username == "alice" && groupname == "dev"
        ));
        assert_eq!(app.modal_depth(), 2);
        press(&mut app, KeyCode::Esc);
        assert!(matches!(
            app.modal(),
            Some(ModalState::MemberActions { selected: 1, .. })
        ));
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());

        // Delete and d skip the menu.
        for code in [KeyCode::Delete, KeyCode::Char('d')] {
            press(&mut app, code);
            assert!(matches!(
                app.modal(),
                Some(ModalState::ConfirmRemoveUserFromGroup { username, .. }) if username == "alice"
            ));
            assert_eq!(app.modal_depth(), 1);
            press(&mut app, KeyCode::Esc);
        }
    }

    #[test]
    fn go_to_user_selects_the_member_on_the_users_tab() {
        let mut app = membership_app();
        app.selected_user_index = 2;
        app.users_focus = UsersFocus::MemberOf;
        app.capture_selection_identities();
        dev_members_app(&mut app, "alice");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(app.modal().is_none());
        assert_eq!(app.input_mode, InputMode::Normal);
        assert_eq!(app.active_tab, ActiveTab::Users);
        assert_eq!(app.users_focus, UsersFocus::UsersList);
        assert_eq!(selected_user_name(&app), Some("alice"));
        assert_eq!(app.selected_user_uid, Some(1000));
        // Nothing was hidden, so nothing was cleared.
        assert_eq!(app.notice, None);
    }

    #[test]
    fn go_to_user_clears_a_search_and_filters_that_hide_the_user() {
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        app.users_query = "bob".to_owned();
        app.users_filter_chips.inactive = true;
        app.users_filter_chips.system_only = true;
        app.sort_and_filter();
        assert!(user_names(&app).is_empty());
        dev_members_app(&mut app, "alice");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(app.modal().is_none());
        assert_eq!(app.active_tab, ActiveTab::Users);
        assert_eq!(app.users_query, "");
        assert_eq!(app.users_filter_chips, Default::default());
        assert_eq!(user_names(&app), ["alice", "bob", "carol"]);
        assert_eq!(selected_user_name(&app), Some("alice"));
        assert_eq!(
            app.notice.as_deref(),
            Some("cleared search and filters to show alice")
        );
        assert!(status_row(&app, 100, 30).contains("cleared search and filters to show alice"));
        // filter.conf follows the cleared chips, as after the filter menu.
        let saved = std::fs::read_to_string(config.path().join("filter.conf")).unwrap();
        assert!(saved.contains("system_only = false"), "{saved}");
        assert!(saved.contains("inactive = false"), "{saved}");

        // A search alone is cleared without touching filter.conf.
        let config = tempfile::tempdir().unwrap();
        let mut app = filter_app(&config);
        app.users_query = "carol".to_owned();
        app.sort_and_filter();
        dev_members_app(&mut app, "alice");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.users_query, "");
        assert_eq!(selected_user_name(&app), Some("alice"));
        assert!(app.notice.is_some());
        assert!(!config.path().join("filter.conf").exists());
    }

    #[test]
    fn go_to_user_keeps_the_menu_for_a_member_without_a_user_record() {
        let mut app = membership_app();
        app.users_query = "bob".to_owned();
        let dev = app
            .groups_all
            .iter_mut()
            .find(|group| group.name == "dev")
            .unwrap();
        dev.members.push("ghost".to_owned());
        app.sort_and_filter();
        dev_members_app(&mut app, "ghost");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::MemberActions { error: Some(error), .. })
                if error == "ghost has no local user record"
        ));
        assert!(rendered(&app).contains("ghost has no local user record"));
        assert_eq!(app.active_tab, ActiveTab::Groups);
        assert_eq!(app.users_query, "bob");
        assert_eq!(app.notice, None);
        // The next key clears the message.
        press(&mut app, KeyCode::Down);
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    #[test]
    fn protected_group_changes_explain_the_uid_range() {
        let mut app = wheel_app();
        open_modal(
            &mut app,
            ModalState::GroupDeleteConfirm {
                selected: 1,
                target_gid: Some(998),
            },
        );
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::Info { message, .. }) if message == crate::app::PROTECTED_GROUP_MESSAGE
        ));
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn password_menu_says_what_each_entry_does() {
        let mut app = user_app();
        open_modal(&mut app, ModalState::ModifyPasswordMenu { selected: 0 });
        let screen = rendered(&app);
        assert!(screen.contains("Set or change password"));
        assert!(screen.contains("Force change at next login"));
        press(&mut app, KeyCode::Enter);
        assert!(rendered(&app).contains("[ ] Must change at next login"));
        press(&mut app, KeyCode::Esc);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, .. })
                if action == "Force alice to change password at next login"
        ));
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
                if action == "Change shell of alice to /bin/bash"
                    && preview == &["usermod -s /bin/bash alice"]
        ));
        assert!(rendered(&app).contains(ui::components::DRY_RUN_INTRO));
        execute_pending_plan(&mut app);
        assert!(matches!(
            app.modal(),
            Some(ModalState::Info { title: Some(title), message })
                if title == "Dry run"
                    && message == "Change shell of alice to /bin/bash\nDry run: nothing was executed."
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
        assert!(lines[3].contains("error: ") && lines[3].ends_with("[E-VALIDATION]"));
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

    /// Alice at UID 1000 with `count` extra groups, and a confirmation open
    /// for adding her to all of them: one preview step per group.
    fn app_confirming_group_adds(count: u32) -> AppState {
        app_confirming_group_adds_at(count, (100, 30))
    }

    /// [`app_confirming_group_adds`] with the confirmation opened on a
    /// `terminal` (width, height).
    fn app_confirming_group_adds_at(count: u32, terminal: (u16, u16)) -> AppState {
        let mut snapshot = fixture_snapshot();
        let groupnames: Vec<String> = (0..count).map(|index| format!("team{index:02}")).collect();
        for (index, name) in (0u32..).zip(&groupnames) {
            snapshot.groups.push(AccountGroup {
                gid: Gid(2000 + index),
                name: GroupName::new(name).unwrap(),
                members: vec![],
            });
        }
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.terminal_size = terminal;
        start_pending_action(
            &mut app,
            PendingAction::AddUserToGroups {
                username: "alice".to_owned(),
                groupnames,
            },
        );
        app
    }

    /// (scroll, seen_all, preview length) of the open confirmation.
    fn preview_state(app: &AppState) -> (usize, bool, usize) {
        match app.modal() {
            Some(ModalState::OperationConfirm {
                scroll,
                seen_all,
                preview,
                ..
            }) => (*scroll, *seen_all, preview.len()),
            _ => panic!("expected the operation confirmation"),
        }
    }

    #[test]
    fn long_preview_refuses_apply_until_scrolled_to_the_end() {
        let mut app = app_confirming_group_adds(20);
        assert_eq!(preview_state(&app), (0, false, 20));
        let screen = rendered(&app);
        // 30 rows: an 18-row body keeps 5 rows for the intro, blanks, the
        // review line, and the choices, so 13 steps show. The title says
        // what happens, shortened so the step range still fits.
        assert!(screen.contains("Confirm: Add alice to team00, team01, team02,"));
        assert!(screen.contains("… (steps 1-13 of 20)"));
        assert!(screen.contains(ui::components::CONFIRM_INTRO));
        assert!(screen.contains("team12"));
        assert!(!screen.contains("team13"));
        assert!(screen.contains(ui::components::PREVIEW_REFUSAL));
        assert!(screen.contains("PgUp PgDn scroll"));

        // `y` and Enter on Apply are refused while steps are unseen.
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { selected: 0, .. })
        ));
        assert!(app.pending_operation.is_some());

        for down in 1..=7 {
            assert!(!preview_state(&app).1);
            press(&mut app, KeyCode::Down);
            assert_eq!(preview_state(&app).0, down);
        }
        assert_eq!(preview_state(&app), (7, true, 20));
        // Scrolling stops at the last full page.
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(preview_state(&app), (7, true, 20));
        let screen = rendered(&app);
        assert!(screen.contains("(steps 8-20 of 20)"));
        assert!(screen.contains("team19"));
        assert!(screen.contains("All 20 steps shown."));
        assert!(!screen.contains(ui::components::PREVIEW_REFUSAL));

        // Scrolling back keeps the preview reviewed.
        press(&mut app, KeyCode::Up);
        assert_eq!(preview_state(&app), (6, true, 20));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::SudoPrompt { .. })));
        assert!(app.pending_operation.is_some());
    }

    #[test]
    fn preview_pages_by_its_window_and_short_previews_are_seen_at_once() {
        let mut app = app_confirming_group_adds(20);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(preview_state(&app), (7, true, 20));
        press(&mut app, KeyCode::PageUp);
        assert_eq!(preview_state(&app), (0, true, 20));

        let mut app = app_confirming_group_adds(12);
        assert_eq!(preview_state(&app), (0, true, 12));
        let screen = rendered(&app);
        assert!(screen.contains("(steps 1-12 of 12)"));
        assert!(!screen.contains(ui::components::PREVIEW_REFUSAL));
        assert!(!screen.contains("All 12 steps shown."));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(app.modal(), Some(ModalState::SudoPrompt { .. })));
    }

    #[test]
    fn preview_window_matches_the_rendered_dialog_on_a_small_terminal() {
        let mut app = app_confirming_group_adds(20);
        app.terminal_size = (40, 12);
        press(&mut app, KeyCode::Down);
        let (scroll, seen_all, _) = preview_state(&app);
        assert_eq!(scroll, 1);
        assert!(!seen_all);
        let intro = ui::components::CONFIRM_INTRO;
        let window = match app.modal() {
            Some(ModalState::OperationConfirm { preview, .. }) => {
                ui::components::operation_preview_window((40, 12), intro, preview, scroll)
            }
            _ => unreachable!(),
        };
        let PreviewWindow::Steps(window) = window else {
            panic!("expected steps at 40x12, got {window:?}");
        };
        assert!(!window.is_empty());
        let rows = rendered_rows(&app, 40, 12);
        let title = format!("steps {}-{} of 20", window.start + 1, window.end);
        assert!(rows.iter().any(|row| row.contains(&title)), "{rows:#?}");
        // The body is not scrolled: the intro's first row is still drawn.
        assert!(rows.iter().any(|row| row.contains("The following")));
        for index in window.clone() {
            let step = format!("gpasswd -a alice team{index:02}");
            assert!(rows.iter().any(|row| row.contains(&step)), "{step} missing");
        }
        assert!(rows.iter().any(|row| row.contains("Apply   ")));
        // Paging to the end on this terminal marks the preview seen.
        for _ in 0..20 {
            press(&mut app, KeyCode::PageDown);
        }
        assert!(preview_state(&app).1);
    }

    /// Interior text of a dialog `width` columns wide centred on a screen
    /// of `rows`, row after row, so a message wrapped across rows reads as
    /// one string.
    fn dialog_text(rows: &[String], width: usize) -> String {
        let screen = rows[0].chars().count();
        let left = (screen - width) / 2 + 1;
        rows.iter()
            .flat_map(|row| row.chars().skip(left).take(width - 2))
            .collect()
    }

    #[test]
    fn preview_too_small_for_one_step_is_never_reviewed() {
        let mut app = app_confirming_group_adds(20);
        app.terminal_size = (30, 8);
        let intro = ui::components::CONFIRM_INTRO;
        let window = match app.modal() {
            Some(ModalState::OperationConfirm { preview, .. }) => {
                ui::components::operation_preview_window((30, 8), intro, preview, 0)
            }
            _ => unreachable!(),
        };
        // 30 columns leave 20 per row: intro 2 rows, blanks 2, review line
        // 3, choices 1, one step 2 rows, plus borders and footer.
        assert_eq!(window, PreviewWindow::TooSmall { need_rows: 13 });
        for _ in 0..40 {
            press(&mut app, KeyCode::Down);
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(preview_state(&app), (0, false, 20));
        press(&mut app, KeyCode::Char('y'));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm {
                selected: 0,
                seen_all: false,
                ..
            })
        ));
        assert!(app.pending_operation.is_some());
        let rows = rendered_rows(&app, 30, 8);
        // 30 columns: the dialog is 22 wide.
        let text = dialog_text(&rows, 22);
        assert!(
            text.contains("Terminal too small to review the preview (need 13 rows)"),
            "{rows:#?}"
        );
        assert!(!text.contains("gpasswd"));
        assert!(rows.iter().any(|row| row.contains("Apply")));
    }

    #[test]
    fn short_preview_on_a_too_small_terminal_refuses_apply() {
        let mut app = app_confirming_group_adds_at(1, (30, 8));
        // Short previews count as reviewed at once, but no step is drawn.
        assert_eq!(preview_state(&app), (0, true, 1));
        let window = match app.modal() {
            Some(ModalState::OperationConfirm { preview, .. }) => {
                ui::components::operation_preview_window(
                    (30, 8),
                    ui::components::CONFIRM_INTRO,
                    preview,
                    0,
                )
            }
            _ => unreachable!(),
        };
        assert!(
            matches!(window, PreviewWindow::TooSmall { .. }),
            "{window:?}"
        );

        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { selected: 0, .. })
        ));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { selected: 0, .. })
        ));
        assert!(app.pending_operation.is_some());
        let rows = rendered_rows(&app, 30, 8);
        let text = dialog_text(&rows, 22);
        assert!(
            text.contains("Terminal too small to review the preview"),
            "{rows:#?}"
        );
        assert!(!text.contains("gpasswd"));

        // Below the minimum size nothing is drawn, so Apply is refused too.
        handle_event(&mut app, Event::Resize(20, 5));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { .. })
        ));

        // Once the step fits, `y` applies and reaches the sudo prompt.
        handle_event(&mut app, Event::Resize(100, 30));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(app.modal(), Some(ModalState::SudoPrompt { .. })));
        assert!(app.pending_operation.is_some());
    }

    #[test]
    fn resize_that_shows_the_last_step_latches_the_review() {
        let mut app = app_confirming_group_adds(20);
        handle_event(&mut app, Event::Resize(100, 12));
        for _ in 0..7 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(preview_state(&app), (7, false, 20));
        handle_event(&mut app, Event::Resize(100, 30));
        assert_eq!(preview_state(&app), (7, true, 20));
        assert!(rendered_at(&app, 100, 30).contains("All 20 steps shown."));
        handle_event(&mut app, Event::Resize(100, 12));
        assert_eq!(preview_state(&app), (7, true, 20));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(app.modal(), Some(ModalState::SudoPrompt { .. })));
    }

    #[test]
    fn preview_below_the_minimum_terminal_size_is_not_reviewed() {
        let mut app = app_confirming_group_adds(20);
        handle_event(&mut app, Event::Resize(20, 5));
        assert!(rendered_at(&app, 20, 5).contains("terminal too small"));
        for _ in 0..40 {
            press(&mut app, KeyCode::PageDown);
        }
        assert_eq!(preview_state(&app), (0, false, 20));
        press(&mut app, KeyCode::Char('y'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { .. })
        ));
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
        // The field opened with the current name, "alice".
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyTextInput { value, .. }) if value == "alicea"
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
                reveal: false,
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
                reveal: false,
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
        // The cached list still shows `staff`, so the form lets it be
        // ticked, but it is gone from the accounts the adapter reads at
        // preparation time.
        let host = fixture_snapshot();
        let mut cached = host.clone();
        cached.groups.push(AccountGroup {
            gid: Gid(1500),
            name: GroupName::new("staff").unwrap(),
            members: vec![],
        });
        // Root identity: no sudo prompt. Nothing here executes a plan, so
        // `NoRun` is never reached.
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(host)),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let mut app = isolated_app(adapter, cached);
        app.sudo_group = "staff".to_owned();
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
        // The sudo group no longer exists, so preparation fails and the
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

    /// Users alice (member of `dev`), bob and carol, groups `dev` and `ops`
    /// at GID 2000+ and `wheel` at GID 998, seen by root. The sudo group
    /// is the default `wheel`.
    fn wheel_app() -> AppState {
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
                group(998, "wheel", &[]),
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
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        ));
        let app = isolated_app(adapter, snapshot);
        assert_eq!(app.sudo_group, "wheel");
        app
    }

    fn add_to_wheel(app: &AppState) -> bool {
        match app.modal() {
            Some(ModalState::UserAddInput { add_to_wheel, .. }) => *add_to_wheel,
            _ => panic!("create user form is not open"),
        }
    }

    #[test]
    fn create_user_shows_the_blocked_sudo_group_and_space_does_not_tick_it() {
        let mut app = wheel_app();
        open_user_add(&mut app);
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        // The reason fits on the row, next to the group it concerns.
        let screen = rendered(&app);
        assert!(screen.contains("▶     Add to sudo group (wheel): blocked, sudo group, protected"));
        assert!(!screen.contains("[ ] Add to sudo group"));
        press(&mut app, KeyCode::Char(' '));
        assert!(!add_to_wheel(&app));
        assert!(rendered(&app).contains("(wheel): blocked"));
    }

    #[test]
    fn create_user_blocks_a_sudo_group_that_does_not_exist() {
        let mut app = root_app();
        app.sudo_group = "sudo".to_owned();
        open_user_add(&mut app);
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(!add_to_wheel(&app));
        assert!(rendered(&app).contains("Add to sudo group (sudo): blocked, group does not exist"));
    }

    #[test]
    fn create_user_ticks_an_allowed_sudo_group_and_names_it() {
        let mut app = wheel_app();
        app.sudo_group = "ops".to_owned();
        open_user_add(&mut app);
        assert!(rendered(&app).contains("[ ] Add to sudo group (ops)"));
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(add_to_wheel(&app));
        assert!(rendered(&app).contains("[x] Add to sudo group (ops)"));
    }

    #[test]
    fn create_user_confirmation_lists_the_chosen_options() {
        let mut app = wheel_app();
        app.sudo_group = "ops".to_owned();
        open_user_add(&mut app);
        type_text(&mut app, "erin");
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Create user erin (home, sudo group ops)"
                    && preview.iter().any(|line| line.ends_with("ops"))
        ));
    }

    #[test]
    fn create_user_tick_on_a_group_that_became_blocked_can_be_cleared() {
        let mut app = wheel_app();
        open_user_add(&mut app);
        if let Some(ModalState::UserAddInput { add_to_wheel, .. }) = app.modal_mut() {
            *add_to_wheel = true;
        }
        assert!(rendered(&app).contains("[x] Add to sudo group (wheel): blocked"));
        for _ in 0..4 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char(' '));
        assert!(!add_to_wheel(&app));
        press(&mut app, KeyCode::Char(' '));
        assert!(!add_to_wheel(&app));
    }

    fn user_groups_ticked(app: &AppState) -> Vec<String> {
        match app.modal() {
            Some(ModalState::ModifyGroupsAdd { selected_multi, .. }) => selected_multi.clone(),
            _ => panic!("add groups dialog is not open"),
        }
    }

    #[test]
    fn add_groups_marks_blocked_groups_and_refuses_to_tick_them() {
        let mut app = wheel_app();
        let alice = app.users.iter().position(|user| user.name == "alice");
        app.selected_user_index = alice.unwrap();
        let list = app.user_group_candidate_list(1000, true, "");
        assert_eq!(list.names, ["wheel", "ops"]);
        assert!(list.blocked_reason(0).is_some());
        assert_eq!(list.blocked_reason(1), None);
        open_modal(
            &mut app,
            ModalState::ModifyGroupsAdd {
                selected: 0,
                offset: 0,
                selected_multi: vec![],
                filter: String::new(),
                filtering: false,
                error: None,
            },
        );
        let screen = rendered(&app);
        assert!(screen.contains("[ ] wheel (blocked: sudo group, protected)"));
        assert!(screen.contains("[ ] ops "));
        assert!(!screen.contains("ops (blocked"));

        // `*` skips the blocked entry.
        press(&mut app, KeyCode::Char('*'));
        assert_eq!(user_groups_ticked(&app), ["ops"]);
        press(&mut app, KeyCode::Char('*'));
        assert!(user_groups_ticked(&app).is_empty());

        // Space on the blocked entry shows the reason instead of ticking.
        press(&mut app, KeyCode::Char(' '));
        assert!(user_groups_ticked(&app).is_empty());
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some("sudo group, protected")
        );
        // Enter with nothing ticked on the blocked row prepares nothing.
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_none());
        assert!(app.modal().and_then(ModalState::form_error).is_some());
        // Moving clears the message; the allowed entry still ticks.
        press(&mut app, KeyCode::Down);
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(user_groups_ticked(&app), ["ops"]);
    }

    #[test]
    fn add_members_of_a_blocked_group_marks_every_user() {
        let mut app = wheel_app();
        app.active_tab = ActiveTab::Groups;
        let list = app.group_member_candidate_list(998, true, "");
        assert_eq!(list.names.len(), 3);
        assert!((0..3).all(|index| list.blocked_reason(index).is_some()));
        open_modal(&mut app, add_members_dialog(998));
        let screen = rendered(&app);
        for name in ["alice", "bob", "carol"] {
            assert!(screen.contains(&format!("{name} (blocked: sudo group, protected)")));
        }
        press(&mut app, KeyCode::Char(' '));
        press(&mut app, KeyCode::Char('*'));
        assert!(ticked(&app).is_empty());
        press(&mut app, KeyCode::Char(' '));
        assert!(app.modal().and_then(ModalState::form_error).is_some());
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_none());

        // An allowed group lists the same users without a mark.
        let list = app.group_member_candidate_list(2001, true, "");
        assert!(list.blocked.iter().all(Option::is_none));
        // Removing is not checked here.
        assert!(
            app.group_member_candidate_list(2000, false, "")
                .blocked
                .is_empty()
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
                username: "alice".to_owned(),
                groupname: "dev".to_owned(),
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
            Some(ModalState::Info { message, .. }) if message == crate::app::PROTECTED_USER_MESSAGE
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
        assert!(screen.contains("Root and system accounts (UID below 1000) cannot be deleted"));
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
        // Shadow is unknown in the fixture, so the menu offers both the
        // lock and the unlock entry after the four fixed ones.
        press(&mut app, KeyCode::Char('k'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyMenu { selected: 5 })
        ));
        press(&mut app, KeyCode::Char('j'));
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyMenu { selected: 0 })
        ));
    }

    /// `user_app` with alice's cached shadow status, or none known.
    fn app_with_lock_state(locked: Option<bool>) -> AppState {
        let mut app = user_app();
        app.diagnostics.shadow = match locked {
            Some(locked) => crate::search::ShadowState::Known(
                [(
                    "alice".to_owned(),
                    crate::search::ShadowStatus {
                        locked,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            ),
            None => crate::search::ShadowState::Unavailable {
                reason: "permission denied".to_owned(),
            },
        };
        // Actions, then Modify.
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::ModifyMenu { .. })));
        app
    }

    #[test]
    fn modify_menu_lock_entry_follows_the_cached_shadow_state() {
        let cases = [
            (Some(true), false, true),
            (Some(false), true, false),
            (None, true, true),
        ];
        for (locked, shows_lock, shows_unlock) in cases {
            let app = app_with_lock_state(locked);
            let text = rendered(&app);
            assert_eq!(text.contains("Lock account"), shows_lock, "{locked:?}");
            assert_eq!(text.contains("Unlock account"), shows_unlock, "{locked:?}");
            assert!(text.contains("Password"), "{locked:?}");
            let expected: Vec<ModifyMenuItem> = [
                (shows_lock, ModifyMenuItem::Lock),
                (shows_unlock, ModifyMenuItem::Unlock),
            ]
            .into_iter()
            .filter_map(|(shown, item)| shown.then_some(item))
            .collect();
            assert_eq!(app.modify_menu_items()[4..], expected[..], "{locked:?}");
        }
    }

    #[test]
    fn modify_menu_lock_and_unlock_open_the_matching_confirmation() {
        let cases = [
            (Some(false), 4, "Lock account alice", "usermod -L alice"),
            (Some(true), 4, "Unlock account alice", "usermod -U alice"),
            (None, 4, "Lock account alice", "usermod -L alice"),
            (None, 5, "Unlock account alice", "usermod -U alice"),
        ];
        for (locked, row, title, command) in cases {
            let mut app = app_with_lock_state(locked);
            for _ in 0..row {
                press(&mut app, KeyCode::Down);
            }
            press(&mut app, KeyCode::Enter);
            assert!(
                matches!(
                    app.modal(),
                    Some(ModalState::OperationConfirm { action, preview, .. })
                        if action == title && preview == &[command]
                ),
                "{locked:?} row {row}"
            );
        }
    }

    #[test]
    fn lock_actions_describe_the_account() {
        let username = "alice".to_owned();
        assert_eq!(
            PendingAction::LockUser {
                username: username.clone()
            }
            .describe(),
            "Lock account alice"
        );
        assert_eq!(
            PendingAction::UnlockUser { username }.describe(),
            "Unlock account alice"
        );
        assert_eq!(step_label("lock user"), "lock account");
        assert_eq!(step_label("unlock user"), "unlock account");
    }

    /// daemon (UID 2, protected), alice, bob, and carol. alice is in `dev`,
    /// bob in the system group `adm`, carol in `wheel`; `docker` is empty.
    fn bulk_snapshot() -> AccountSnapshot {
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
        AccountSnapshot {
            users: vec![
                user(2, "daemon"),
                user(1000, "alice"),
                user(1001, "bob"),
                user(1002, "carol"),
            ],
            groups: vec![
                group(4, "adm", &["bob"]),
                group(2000, "dev", &["alice"]),
                group(2001, "docker", &[]),
                group(2002, "wheel", &["carol"]),
            ],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        }
    }

    fn bulk_app() -> AppState {
        let snapshot = bulk_snapshot();
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    /// Move the users cursor to `name` and press Space on it.
    fn mark(app: &mut AppState, name: &str) {
        while app.users[app.selected_user_index].name != name {
            press(app, KeyCode::Down);
        }
        press(app, KeyCode::Char(' '));
    }

    fn marked_names(app: &AppState) -> Vec<String> {
        app.marked_usernames()
    }

    fn bulk_error(app: &AppState) -> Option<String> {
        match app.modal() {
            Some(ModalState::BulkActions { error, .. }) => error.clone(),
            _ => None,
        }
    }

    #[test]
    fn space_marks_users_and_the_table_shows_them() {
        let mut app = bulk_app();
        mark(&mut app, "alice");
        mark(&mut app, "bob");
        assert_eq!(marked_names(&app), ["alice", "bob"]);
        assert_eq!(
            ui::users::users_table_title(&app),
            "[Users]  4 of 4  2 marked"
        );
        let screen = rendered(&app);
        assert!(screen.contains("* alice"), "{screen}");
        assert!(screen.contains("* bob"), "{screen}");
        assert!(!screen.contains("* carol"), "{screen}");
        // Space again unmarks the highlighted user.
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(marked_names(&app), ["alice"]);

        // Marks belong to the users list: Space elsewhere does nothing.
        app.users_focus = UsersFocus::MemberOf;
        press(&mut app, KeyCode::Char(' '));
        app.users_focus = UsersFocus::UsersList;
        app.active_tab = ActiveTab::Groups;
        press(&mut app, KeyCode::Char(' '));
        assert_eq!(marked_names(&app), ["alice"]);
    }

    #[test]
    fn marking_two_users_and_adding_them_to_a_group_is_one_composite() {
        let mut app = bulk_app();
        mark(&mut app, "alice");
        mark(&mut app, "bob");
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::BulkActions { .. })));
        let screen = rendered(&app);
        assert!(screen.contains("Bulk actions (2 marked)"), "{screen}");
        assert!(screen.contains("Marked: alice, bob"), "{screen}");
        for label in BULK_ACTIONS {
            assert!(screen.contains(label), "missing {label:?}");
        }
        press(&mut app, KeyCode::Enter);
        let Some(ModalState::BulkGroupPicker { adding, groups, .. }) = app.modal() else {
            panic!("expected the group picker");
        };
        assert!(*adding);
        assert_eq!(groups.names, ["adm", "dev", "docker", "wheel"]);
        let screen = rendered(&app);
        assert!(
            screen.contains("adm (blocked: system group, protected)"),
            "{screen}"
        );
        assert!(
            screen.contains("wheel (blocked: sudo group, protected)"),
            "{screen}"
        );

        // A blocked group cannot be chosen.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::BulkGroupPicker { error: Some(reason), .. })
                if reason == "system group, protected"
        ));

        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        match app.modal() {
            Some(ModalState::OperationConfirm {
                action, preview, ..
            }) => {
                assert_eq!(action, "Add alice, bob to docker");
                assert_eq!(
                    preview,
                    &["gpasswd -a alice docker", "gpasswd -a bob docker"]
                );
            }
            _ => panic!("expected the confirmation"),
        }
    }

    #[test]
    fn bulk_add_skips_marked_users_already_in_the_group() {
        let mut app = bulk_app();
        mark(&mut app, "alice");
        mark(&mut app, "bob");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Add bob to dev" && preview == &["gpasswd -a bob dev"]
        ));
    }

    #[test]
    fn bulk_remove_lists_groups_with_a_marked_member_and_skips_non_members() {
        let mut app = bulk_app();
        mark(&mut app, "alice");
        mark(&mut app, "bob");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        let Some(ModalState::BulkGroupPicker { adding, groups, .. }) = app.modal() else {
            panic!("expected the group picker");
        };
        assert!(!*adding);
        // docker has no member and wheel only carol, who is not marked.
        assert_eq!(groups.names, ["adm", "dev"]);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Remove alice from dev" && preview == &["gpasswd -d alice dev"]
        ));
    }

    #[test]
    fn bulk_remove_without_marked_members_stays_in_the_menu() {
        let mut app = bulk_app();
        mark(&mut app, "daemon");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            bulk_error(&app).as_deref(),
            Some("No marked user is a member of a group.")
        );
    }

    #[test]
    fn lock_marked_is_one_composite_named_after_every_user() {
        let mut app = bulk_app();
        mark(&mut app, "alice");
        mark(&mut app, "carol");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Lock accounts alice, carol"
                    && preview == &["usermod -L alice", "usermod -L carol"]
        ));
        cancel_operation(&mut app);
        press(&mut app, KeyCode::Enter);
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Unlock accounts alice, carol"
                    && preview == &["usermod -U alice", "usermod -U carol"]
        ));
    }

    #[test]
    fn protected_marked_user_is_refused_inline_naming_the_user() {
        let mut app = bulk_app();
        mark(&mut app, "daemon");
        mark(&mut app, "alice");
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        let error = bulk_error(&app).expect("inline refusal");
        assert!(error.starts_with("Refused for daemon: "), "{error}");
        assert!(error.contains("system user, protected"), "{error}");
        assert!(app.pending_operation.is_none());
        assert_eq!(app.modal_depth(), 1);
        assert!(rendered(&app).contains("Refused for daemon"));
        // Any key clears the message.
        press(&mut app, KeyCode::Down);
        assert_eq!(bulk_error(&app), None);

        // From the group picker, the picker closes and the menu says why.
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Up);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        assert_eq!(app.modal_depth(), 1);
        let error = bulk_error(&app).expect("inline refusal");
        assert!(error.starts_with("Refused for daemon: "), "{error}");
    }

    #[test]
    fn bulk_actions_describe_their_users() {
        let names =
            |list: &[&str]| -> Vec<String> { list.iter().map(|name| (*name).to_owned()).collect() };
        assert_eq!(
            PendingAction::LockUsers {
                usernames: names(&["alice"])
            }
            .describe(),
            "Lock account alice"
        );
        assert_eq!(
            PendingAction::UnlockUsers {
                usernames: names(&["alice", "bob"])
            }
            .describe(),
            "Unlock accounts alice, bob"
        );
        assert_eq!(
            PendingAction::LockUsers {
                usernames: names(&["a", "b", "c", "d", "e", "f", "g"])
            }
            .describe(),
            "Lock accounts a, b, c, d, e and 2 more"
        );
    }

    #[test]
    fn esc_clears_the_marks_first_and_the_search_second() {
        let mut app = bulk_app();
        mark(&mut app, "alice");
        app.users_query = "a".to_owned();
        app.sort_and_filter();
        press(&mut app, KeyCode::Esc);
        assert!(app.marked_users.is_empty());
        assert_eq!(app.users_query, "a");
        press(&mut app, KeyCode::Esc);
        assert_eq!(app.users_query, "");
        // Without marks Enter opens the single-user actions again.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::Actions { .. })));
    }

    #[test]
    fn marks_survive_sort_and_search_and_drop_with_their_user_on_refresh() {
        let source = Arc::new(SwappableSource(std::sync::Mutex::new(
            Some(bulk_snapshot()),
        )));
        let adapter = Arc::new(SystemAdapter::from_components(
            source.clone(),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, bulk_snapshot());
        mark(&mut app, "alice");
        mark(&mut app, "bob");
        press(&mut app, KeyCode::Char('s'));
        app.users_query = "bob".to_owned();
        app.sort_and_filter();
        assert_eq!(app.users.len(), 1);
        assert_eq!(marked_names(&app), ["alice", "bob"]);
        assert_eq!(
            ui::users::users_table_title(&app),
            "[Users]  /bob  1 of 4  2 marked  sort: name"
        );

        let mut without_bob = bulk_snapshot();
        without_bob.users.retain(|user| user.name.as_str() != "bob");
        *source.0.lock().unwrap() = Some(without_bob);
        press(&mut app, KeyCode::Char('r'));
        assert_eq!(marked_names(&app), ["alice"]);
    }

    #[test]
    fn mark_key_is_listed_in_the_panel_and_the_help() {
        let app = bulk_app();
        let lines: Vec<String> = ui::components::keybinds_panel_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(lines.contains(&"Mark user: Space".to_owned()), "{lines:?}");
        let help: Vec<String> = ui::components::help_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(help.contains(&"Mark or unmark a user for bulk actions: Space".to_owned()));
        assert!(help.contains(&"Clear marks: Esc (the next press clears the search)".to_owned()));
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

    fn help_scroll(app: &AppState) -> u16 {
        match app.modal() {
            Some(ModalState::Help { scroll }) => *scroll,
            _ => panic!("expected the help dialog"),
        }
    }

    /// Body rows of the help dialog, between its title and footer, borders
    /// stripped, from a TestBackend render at `width` x `height`.
    fn help_body(app: &AppState, width: u16, height: u16) -> Vec<String> {
        let rows: Vec<Vec<String>> = rendered_rows(app, width, height)
            .iter()
            .map(|row| row.chars().map(String::from).collect())
            .collect();
        let top = rows
            .iter()
            .position(|row| row.concat().contains("┌Help (rows"))
            .expect("help dialog title row");
        // Panels behind the dialog have corners on the same row, so anchor
        // on the corner right before the title.
        let left = rows[top]
            .windows(2)
            .position(|pair| pair[0] == "┌" && pair[1] == "H")
            .unwrap();
        let right = left
            + rows[top][left..]
                .iter()
                .position(|cell| cell == "┐")
                .unwrap();
        // The footer is the row above the bottom border; it may be clipped
        // on a narrow terminal, so find the border instead of its text.
        let bottom = (top + 1..rows.len())
            .find(|&y| rows[y][left] == "└")
            .expect("help dialog bottom border");
        rows[top + 1..bottom - 1]
            .iter()
            .map(|row| row[left + 1..right].concat())
            .collect()
    }

    #[test]
    fn help_is_grouped_and_scrolls_by_row() {
        let mut app = user_app();
        app.terminal_size = (100, 30);
        press(&mut app, KeyCode::Char('?'));
        let top = rendered_at(&app, 100, 30);
        assert!(top.contains("Navigation"));
        assert!(top.contains("Move up: Up, k"));
        assert!(top.contains("Help (rows 1-"));
        assert!(top.contains("↑↓ PgUp PgDn scroll  Esc close"));
        let top_body = help_body(&app, 100, 30);
        assert_eq!(top_body[0].trim_end(), "Navigation");
        for _ in 0..5 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(help_scroll(&app), 5);
        let scrolled = rendered_at(&app, 100, 30);
        assert!(scrolled.contains("Help (rows 6-"));
        let scrolled_body = help_body(&app, 100, 30);
        assert_ne!(top_body, scrolled_body);
        assert_eq!(scrolled_body[0], top_body[5]);
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(help_scroll(&app), 4);
        press(&mut app, KeyCode::Esc);
        assert!(app.modal().is_none());
    }

    #[test]
    fn help_scrolls_through_a_line_longer_than_the_dialog_at_30x8() {
        let mut app = user_app();
        let message: String = (0..200)
            .map(|index| char::from(b'a' + u8::try_from(index % 26).unwrap()))
            .collect();
        let tail = &message[message.len() - 20..];
        app.diagnostics.config_messages = vec![message.clone()];
        app.terminal_size = (30, 8);
        press(&mut app, KeyCode::Char('?'));
        let mut seen_tail = false;
        for _ in 0..20 {
            if help_body(&app, 30, 8).iter().any(|row| row.contains(tail)) {
                seen_tail = true;
                break;
            }
            press(&mut app, KeyCode::Down);
        }
        assert!(seen_tail, "the message's last characters never rendered");
    }

    #[test]
    fn help_scroll_stops_at_the_page_with_the_last_line() {
        let mut app = user_app();
        app.terminal_size = (100, 30);
        press(&mut app, KeyCode::Char('?'));
        for _ in 0..20 {
            press(&mut app, KeyCode::PageDown);
        }
        let maximum = ui::components::help_max_scroll(&app, (100, 30));
        assert!(maximum > 0);
        assert_eq!(usize::from(help_scroll(&app)), maximum);
        let lines = ui::components::help_lines(&app);
        let total = ui::components::help_window(&lines, (100, 30), 0).0.len();
        let screen = rendered_at(&app, 100, 30);
        assert!(screen.contains(&format!("-{total} of {total})")));
        assert!(screen.contains("--log-file <PATH>"));
        press(&mut app, KeyCode::Down);
        assert_eq!(usize::from(help_scroll(&app)), maximum);
    }

    /// Config roots under a tempdir that count how often they are resolved.
    struct CountingConfigRoots {
        root: std::path::PathBuf,
        calls: std::sync::atomic::AtomicUsize,
    }
    impl crate::app::ConfigRootProvider for CountingConfigRoots {
        fn roots(&self) -> Vec<std::path::PathBuf> {
            self.calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            vec![self.root.clone()]
        }
    }

    #[test]
    fn help_renders_config_paths_without_resolving_config_roots() {
        let config = tempfile::tempdir().unwrap();
        let roots = Arc::new(CountingConfigRoots {
            root: config.path().to_path_buf(),
            calls: std::sync::atomic::AtomicUsize::new(0),
        });
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
            Arc::new(NoDiagnostics),
            roots.clone(),
        );
        app.load_configuration();
        app.terminal_size = (100, 30);
        press(&mut app, KeyCode::Char('?'));
        let before = roots.calls.load(std::sync::atomic::Ordering::SeqCst);
        let lines: Vec<String> = ui::components::help_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect();
        let theme = config.path().join("theme.conf");
        assert!(lines.contains(&format!("theme.conf: {} (created)", theme.display())));
        for _ in 0..3 {
            rendered_at(&app, 100, 30);
        }
        assert_eq!(
            roots.calls.load(std::sync::atomic::Ordering::SeqCst),
            before
        );
    }

    /// Two config roots: changes save under the first, files may be read
    /// from either.
    struct TwoConfigRoots(std::path::PathBuf, std::path::PathBuf);
    impl crate::app::ConfigRootProvider for TwoConfigRoots {
        fn roots(&self) -> Vec<std::path::PathBuf> {
            vec![self.0.clone(), self.1.clone()]
        }
    }

    fn help_text(app: &AppState) -> Vec<String> {
        ui::components::help_lines(app)
            .iter()
            .map(ToString::to_string)
            .collect()
    }

    #[test]
    fn help_shows_loaded_and_save_paths_until_a_filter_save_moves_filter_conf() {
        let first = tempfile::tempdir().unwrap();
        let second = tempfile::tempdir().unwrap();
        for name in [
            "theme.conf",
            "keybinds.conf",
            "filter.conf",
            "settings.conf",
        ] {
            std::fs::write(second.path().join(name), "").unwrap();
        }
        let mut app = membership_app();
        app.config_root_provider = Arc::new(TwoConfigRoots(
            first.path().to_path_buf(),
            second.path().to_path_buf(),
        ));
        app.load_configuration();
        let lines = help_text(&app);
        for name in [
            "theme.conf",
            "keybinds.conf",
            "filter.conf",
            "settings.conf",
        ] {
            let expected = format!(
                "{name}: {}  saves to {}",
                second.path().join(name).display(),
                first.path().join(name).display()
            );
            assert!(
                lines.contains(&expected),
                "missing {expected:?} in {lines:?}"
            );
        }

        toggle_filter_row(&mut app, 1);
        assert!(app.users_filter_chips.human_only);
        assert!(first.path().join("filter.conf").is_file());
        let lines = help_text(&app);
        let filter = format!(
            "filter.conf: {}",
            first.path().join("filter.conf").display()
        );
        assert!(lines.contains(&filter), "missing {filter:?} in {lines:?}");
        let theme = format!(
            "theme.conf: {}  saves to {}",
            second.path().join("theme.conf").display(),
            first.path().join("theme.conf").display()
        );
        assert!(lines.contains(&theme));
    }

    #[test]
    fn help_does_not_claim_created_when_config_files_could_not_be_loaded() {
        let dir = tempfile::tempdir().unwrap();
        let blocker = dir.path().join("not-a-directory");
        std::fs::write(&blocker, "").unwrap();
        let root = blocker.join("config");
        let mut app = membership_app();
        app.config_root_provider = Arc::new(crate::app::FixedConfigRootProvider(root.clone()));
        app.load_configuration();
        assert!(!app.diagnostics.config_messages.is_empty());
        let lines = help_text(&app);
        assert!(
            lines.iter().all(|line| !line.contains("(created)")),
            "unexpected (created) in {lines:?}"
        );
        for name in [
            "theme.conf",
            "keybinds.conf",
            "filter.conf",
            "settings.conf",
        ] {
            let expected = format!(
                "{name}: not loaded, would save to {}",
                root.join(name).display()
            );
            assert!(
                lines.contains(&expected),
                "missing {expected:?} in {lines:?}"
            );
        }
    }

    #[test]
    fn help_lists_rebound_keys() {
        let mut app = user_app();
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("keybinds.conf");
        std::fs::write(&path, "StartSearch = Ctrl+f\n").unwrap();
        app.keymap = crate::app::keymap::Keymap::from_file(path.to_str().unwrap()).unwrap();
        let text: Vec<String> = ui::components::help_lines(&app)
            .iter()
            .map(ToString::to_string)
            .collect();
        assert!(
            text.iter()
                .any(|line| line == "Search the active tab: /, Ctrl+F")
        );
        app.terminal_size = (100, 30);
        press(&mut app, KeyCode::Char('?'));
        assert!(rendered_at(&app, 100, 30).contains("Ctrl+F"));
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
        // The cursor and the character count follow the value.
        assert_eq!(body[start + 1], "bbb ccccccccccc▏  35");
        assert_eq!(body[start + 2].trim_end(), "/256");
        assert!(rows[start + 3].contains("type to edit"));
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
                    Some(2),
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
                    error: None,
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
        let mut app = AppState::with_dependencies(
            adapter,
            snapshot,
            Arc::new(FixedClock),
            Arc::new(NoDiagnostics),
            Arc::new(NoConfigRoots),
        );
        // A stray copy key never writes to the terminal running the tests.
        app.set_clipboard_sink(Arc::new(RecordingClipboard::default()));
        app
    }

    /// Clipboard sink that keeps every sequence it is given.
    #[derive(Default)]
    struct RecordingClipboard(std::sync::Mutex<Vec<Vec<u8>>>);
    impl crate::app::ClipboardSink for RecordingClipboard {
        fn copy(&self, bytes: &[u8]) -> std::io::Result<()> {
            self.0.lock().unwrap().push(bytes.to_vec());
            Ok(())
        }
    }
    impl RecordingClipboard {
        fn copies(&self) -> Vec<Vec<u8>> {
            self.0.lock().unwrap().clone()
        }
    }

    /// Clipboard sink whose writes fail, like a closed terminal.
    struct FailingClipboard;
    impl crate::app::ClipboardSink for FailingClipboard {
        fn copy(&self, _: &[u8]) -> std::io::Result<()> {
            Err(std::io::ErrorKind::BrokenPipe.into())
        }
    }

    /// `bulk_app` with a recording clipboard the test can inspect.
    fn copy_app() -> (AppState, Arc<RecordingClipboard>) {
        let mut app = bulk_app();
        let clipboard = Arc::new(RecordingClipboard::default());
        app.set_clipboard_sink(clipboard.clone());
        (app, clipboard)
    }

    fn key_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code, crossterm::event::KeyModifiers::NONE))
    }

    #[test]
    fn base64_matches_the_rfc_4648_vectors() {
        use crate::app::base64_encode;
        let vectors = [
            ("", ""),
            ("f", "Zg=="),
            ("fo", "Zm8="),
            ("foo", "Zm9v"),
            ("foob", "Zm9vYg=="),
            ("fooba", "Zm9vYmE="),
            ("foobar", "Zm9vYmFy"),
        ];
        for (input, expected) in vectors {
            assert_eq!(base64_encode(input.as_bytes()), expected, "{input:?}");
        }
        assert_eq!(base64_encode(&[0xfb, 0xff, 0xbf]), "+/+/");
    }

    #[test]
    fn osc52_sequence_wraps_the_base64_text() {
        assert_eq!(
            crate::app::osc52_sequence("alice"),
            "\x1b]52;c;YWxpY2U=\x07"
        );
        assert_eq!(crate::app::osc52_sequence(""), "\x1b]52;c;\x07");
    }

    #[test]
    fn copied_text_is_capped_at_256_bytes_on_a_character_boundary() {
        use crate::app::{MAX_COPY_BYTES, cap_copy_text};
        assert_eq!(cap_copy_text("alice"), "alice");
        let long = "a".repeat(300);
        assert_eq!(cap_copy_text(&long).len(), MAX_COPY_BYTES);
        // 255 ASCII bytes then a two-byte character that would end at 257.
        let straddling = format!("{}é", "a".repeat(255));
        assert_eq!(cap_copy_text(&straddling), "a".repeat(255));
    }

    #[test]
    fn y_copies_the_selected_user_and_shows_a_notice_until_the_next_key() {
        let (mut app, clipboard) = copy_app();
        while app.users[app.selected_user_index].name != "alice" {
            press(&mut app, KeyCode::Down);
        }
        assert!(!handle_event(&mut app, key_event(KeyCode::Char('y'))));
        assert_eq!(clipboard.copies(), [b"\x1b]52;c;YWxpY2U=\x07".to_vec()]);
        assert_eq!(app.notice.as_deref(), Some("copied \"alice\""));
        let status = ui::components::status_segments(&app);
        assert_eq!(status.segments[0], "copied \"alice\"");
        assert!(rendered(&app).contains("copied \"alice\" · "));

        // A resize is not a key press, so the notice stays.
        handle_event(&mut app, Event::Resize(100, 30));
        assert!(app.notice.is_some());
        handle_event(&mut app, key_event(KeyCode::Down));
        assert_eq!(app.notice, None);
        assert!(!rendered(&app).contains("copied"));
        assert_eq!(clipboard.copies().len(), 1);
    }

    #[test]
    fn y_copies_the_name_in_the_focused_pane() {
        let (mut app, clipboard) = copy_app();
        while app.users[app.selected_user_index].name != "alice" {
            press(&mut app, KeyCode::Down);
        }
        // Member of: alice's first group (her primary group has no entry,
        // so this is dev).
        press(&mut app, KeyCode::Right);
        let group = app.selected_user_groups()[app.selected_user_group_index]
            .name
            .clone();
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.notice, Some(format!("copied \"{group}\"")));

        // Groups tab: the highlighted group, then its highlighted member.
        press(&mut app, KeyCode::Tab);
        while app.groups[app.selected_group_index].name != "wheel" {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.notice.as_deref(), Some("copied \"wheel\""));
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.notice.as_deref(), Some("copied \"carol\""));

        let expected: Vec<Vec<u8>> = [group.as_str(), "wheel", "carol"]
            .iter()
            .map(|name| crate::app::osc52_sequence(name).into_bytes())
            .collect();
        assert_eq!(clipboard.copies(), expected);
    }

    #[test]
    fn y_in_an_empty_pane_copies_nothing() {
        let (mut app, clipboard) = copy_app();
        press(&mut app, KeyCode::Tab);
        while app.groups[app.selected_group_index].name != "docker" {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Right);
        press(&mut app, KeyCode::Char('y'));
        assert!(clipboard.copies().is_empty());
        assert_eq!(app.notice, None);
    }

    #[test]
    fn failed_copy_says_why_in_the_status_bar() {
        let mut app = bulk_app();
        app.set_clipboard_sink(Arc::new(FailingClipboard));
        press(&mut app, KeyCode::Char('y'));
        assert_eq!(app.notice.as_deref(), Some("copy failed: broken pipe"));
        assert_eq!(
            ui::components::status_segments(&app).segments[0],
            "copy failed: broken pipe"
        );
    }

    #[test]
    fn copy_key_is_listed_in_the_panel_and_the_help() {
        let app = bulk_app();
        let lines: Vec<String> = ui::components::keybinds_panel_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(lines.contains(&"Copy name: y".to_owned()), "{lines:?}");
        let help: Vec<String> = ui::components::help_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(
            help.contains(
                &"Copy the highlighted user or group name: y (the terminal must allow OSC 52)"
                    .to_owned()
            )
        );
        assert!(help.contains(&"Copy the highlighted group or member name: y".to_owned()));
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
        let details_width = ui::layout::main_layout(area, app.active_tab, app.show_keybinds, 0)
            .details
            .width
            - 2;
        let members = ui::layout::main_layout(
            area,
            app.active_tab,
            app.show_keybinds,
            ui::details_rows(&app, details_width),
        )
        .members
        .unwrap();
        press(&mut app, KeyCode::PageDown);
        assert_eq!(
            app.selected_user_group_index,
            ui::layout::table_rows(members)
        );
    }

    #[test]
    fn loading_frame_is_centered_and_drawn_without_app_state() {
        let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(40, 9)).unwrap();
        terminal.draw(ui::render_loading).unwrap();
        let buffer = terminal.backend().buffer();
        let row = |y: u16| -> String {
            (0..40)
                .map(|x| buffer.cell((x, y)).unwrap().symbol().to_owned())
                .collect()
        };
        assert_eq!(row(4).trim(), ui::LOADING_TEXT);
        // Centered: the blank columns either side differ by at most one.
        let left = row(4).find('L').unwrap();
        let right = 40 - left - ui::LOADING_TEXT.len();
        assert!(left.abs_diff(right) <= 1, "left {left}, right {right}");
        for y in (0..9).filter(|y| *y != 4) {
            assert!(row(y).trim().is_empty());
        }
    }

    #[test]
    fn narrow_terminal_stacks_the_details_under_the_table_without_members() {
        let mut app = paging_app();
        handle_event(&mut app, Event::Resize(60, 24));
        let screen = rendered_text(&app, 60, 24);
        assert!(screen.contains("u000"));
        assert!(screen.contains("Details"));
        assert!(!screen.contains("Member of"));
        // Wide enough again, the member pane is back.
        assert!(rendered_text(&app, 100, 24).contains("Member of"));

        // Paging moves by the stacked table: 60% of the 20 body rows is 12,
        // less border and column header.
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected_user_index, 9);
    }

    #[test]
    fn pane_focus_skips_the_hidden_member_pane() {
        let mut app = paging_app();
        handle_event(&mut app, Event::Resize(60, 24));
        for code in [KeyCode::Right, KeyCode::Char('l'), KeyCode::BackTab] {
            press(&mut app, code);
            assert_eq!(app.users_focus, UsersFocus::UsersList);
        }
        app.active_tab = ActiveTab::Groups;
        press(&mut app, KeyCode::Right);
        assert_eq!(app.groups_focus, GroupsFocus::GroupsList);

        handle_event(&mut app, Event::Resize(100, 24));
        press(&mut app, KeyCode::Right);
        assert_eq!(app.groups_focus, GroupsFocus::Members);
        app.active_tab = ActiveTab::Users;
        press(&mut app, KeyCode::Right);
        assert_eq!(app.users_focus, UsersFocus::MemberOf);

        // Shrinking hands focus back to the lists on both tabs.
        handle_event(&mut app, Event::Resize(69, 24));
        assert_eq!(app.users_focus, UsersFocus::UsersList);
        assert_eq!(app.groups_focus, GroupsFocus::GroupsList);
    }

    #[test]
    fn wrap_selection_off_stops_at_both_ends() {
        let mut app = paging_app();
        app.terminal_size = (100, 40);
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected_user_index, 49);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_user_index, 0);

        app.settings.wrap_selection = false;
        press(&mut app, KeyCode::Up);
        assert_eq!(app.selected_user_index, 0);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.selected_user_index, 0);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected_user_index, 33);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected_user_index, 49);
        press(&mut app, KeyCode::Down);
        assert_eq!(app.selected_user_index, 49);
        press(&mut app, KeyCode::PageUp);
        assert_eq!(app.selected_user_index, 16);
    }

    #[test]
    fn settings_conf_is_loaded_and_listed_in_help() {
        let config = tempfile::tempdir().unwrap();
        std::fs::write(
            config.path().join("settings.conf"),
            "wrap_selection = false\n",
        )
        .unwrap();
        let mut app = paging_app();
        app.config_root_provider = Arc::new(crate::app::FixedConfigRootProvider(
            config.path().to_path_buf(),
        ));
        app.load_configuration();
        assert!(!app.settings.wrap_selection);
        let expected = format!(
            "settings.conf: {}",
            config.path().join("settings.conf").display()
        );
        let lines = help_text(&app);
        assert!(
            lines.contains(&expected),
            "missing {expected:?} in {lines:?}"
        );

        std::fs::write(
            config.path().join("settings.conf"),
            "wrap_selection = sometimes\n",
        )
        .unwrap();
        let mut app = paging_app();
        app.config_root_provider = Arc::new(crate::app::FixedConfigRootProvider(
            config.path().to_path_buf(),
        ));
        app.load_configuration();
        assert!(app.settings.wrap_selection);
        assert!(
            app.diagnostics
                .config_messages
                .iter()
                .any(|message| message.starts_with("settings configuration: ")
                    && message.contains("line 1"))
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
            app.theme.muted,
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
    fn theme_colors_style_selected_rows_text_details_and_header() {
        use ratatui::style::{Color, Modifier};
        let mut app = single_account_app();
        app.theme.highlight_fg = Color::Rgb(4, 5, 6);
        app.theme.highlight_bg = Color::Rgb(1, 2, 3);
        app.theme.text = Color::Rgb(7, 8, 9);
        app.theme.muted = Color::Rgb(10, 11, 12);
        app.theme.header_fg = Color::Rgb(13, 14, 15);
        app.theme.header_bg = Color::Rgb(16, 17, 18);
        let (width, height) = (100, 30);
        let draw = |app: &AppState| {
            let mut terminal =
                Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| ui::render(frame, app)).unwrap();
            terminal.backend().buffer().clone()
        };

        let rows = rendered_rows(&app, width, height);
        let buffer = draw(&app);
        let (x, y) = find_text(&rows, "1000    alice").expect("selected user row");
        let cell = &buffer[(x, y)];
        assert_eq!(cell.bg, Color::Rgb(1, 2, 3), "selected row background");
        assert_eq!(cell.fg, Color::Rgb(4, 5, 6), "selected row foreground");
        assert!(cell.modifier.contains(Modifier::BOLD));
        let (x, y) = find_text(&rows, "100     staff").expect("selected membership row");
        assert_eq!(buffer[(x, y)].bg, Color::Rgb(1, 2, 3), "member-of row");

        let (x, y) = find_text(&rows, "User: alice").expect("details pane");
        assert_eq!(buffer[(x, y)].fg, Color::Rgb(7, 8, 9), "details text");

        let (x, y) = find_text(&rows, "usrgrp-manager").expect("header line");
        assert_eq!(buffer[(x, y)].fg, Color::Rgb(13, 14, 15), "header text");
        assert_eq!(
            buffer[(x, y)].bg,
            Color::Rgb(16, 17, 18),
            "header background"
        );
        let (x, y) = find_text(&rows, "? help").expect("header key hints");
        assert_eq!(buffer[(x, y)].fg, Color::Rgb(10, 11, 12), "muted key hints");

        app.active_tab = ActiveTab::Groups;
        // Without the keybindings column every group detail line fits.
        app.show_keybinds = false;
        let rows = rendered_rows(&app, width, height);
        let buffer = draw(&app);
        let (x, y) = find_text(&rows, "│100 ").expect("selected group row");
        assert!(x < 44, "groups table on the left: {x},{y}");
        let x = x + 1;
        assert_eq!(buffer[(x, y)].bg, Color::Rgb(1, 2, 3), "selected group row");
        let (x, y) = find_text(&rows, "unavailable").expect("unavailable value");
        assert_eq!(
            buffer[(x, y)].fg,
            Color::Rgb(10, 11, 12),
            "muted unavailable"
        );
    }

    /// alice (Alice Liddell) with primary group `alice` and a listed zsh,
    /// in `wheel`; bob logs in with nologin and is in no group.
    fn details_app() -> AppState {
        details_app_with(Vec::new())
    }

    /// [`details_app`] with `extra` groups added to the snapshot.
    fn details_app_with(extra: Vec<AccountGroup>) -> AppState {
        let user = |uid, name: &str, gid, full_name: Option<&str>, shell: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(gid),
            full_name: full_name.map(|value| Gecos::new(value).unwrap()),
            home_dir: format!("/home/{name}").into(),
            shell: ShellPath::new(shell).unwrap(),
        };
        let group = |gid, name: &str, members: &[&str]| AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: members
                .iter()
                .map(|member| UserName::new(member).unwrap())
                .collect(),
        };
        let snapshot = AccountSnapshot {
            users: vec![
                user(1000, "alice", 1000, Some("Alice Liddell"), "/bin/zsh"),
                user(1001, "bob", 1001, None, "/usr/sbin/nologin"),
            ],
            groups: [
                vec![
                    group(998, "wheel", &["alice", "ghost"]),
                    group(1000, "alice", &[]),
                ],
                extra,
            ]
            .concat(),
            shells: vec![
                ShellPath::new("/bin/sh").unwrap(),
                ShellPath::new("/bin/zsh").unwrap(),
            ],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.sudo_group = "wheel".to_owned();
        // A 50-column details pane: every line fits without wrapping.
        app.show_keybinds = false;
        app
    }

    /// Diagnostics as a refresh on 2026-09-28 would cache them.
    fn known_diagnostics() -> crate::app::CachedDiagnostics {
        let mut shadow = std::collections::HashMap::new();
        shadow.insert(
            "alice".to_owned(),
            crate::search::ShadowStatus {
                locked: false,
                no_password: false,
                expired: false,
                last_change_days: Some(20_516),
                expire_abs_days: None,
            },
        );
        shadow.insert(
            "bob".to_owned(),
            crate::search::ShadowStatus {
                locked: true,
                last_change_days: Some(0),
                expire_abs_days: Some(20_849),
                ..crate::search::ShadowStatus::default()
            },
        );
        let mut diagnostics = crate::app::CachedDiagnostics {
            shadow: crate::search::ShadowState::Known(shadow),
            group_mtime_days: Some(20_697),
            refreshed_days: 20_724,
            ..crate::app::CachedDiagnostics::default()
        };
        diagnostics.homes.insert(
            "alice".to_owned(),
            crate::app::HomeDiagnostics {
                exists: Some(true),
                permissions: Some("750".to_owned()),
                authorized_key_count: Some(3),
                process_count: Some(12),
            },
        );
        diagnostics.groups.insert(
            998,
            crate::app::GroupDiagnostics {
                primary_members: 0,
                orphan_members: 1,
                locked_members: 1,
                ..crate::app::GroupDiagnostics::default()
            },
        );
        diagnostics
    }

    #[test]
    fn user_details_show_full_name_and_primary_group_name() {
        let mut app = details_app();
        app.diagnostics = known_diagnostics();
        let screen = rendered_rows(&app, 100, 30).join("\n");
        for line in [
            "User: alice (Alice Liddell)",
            "UID 1000  GID 1000 (alice)",
            "Home: /home/alice  exists: yes  perms: 750",
            "Shell: /bin/zsh  listed: yes  interactive: yes",
            "Password: locked no  empty no  expired no",
            "Last change: 2026-03-04  Expires: never",
            "Sudo group (wheel): member",
            "SSH keys: 3",
            "Processes: 12",
        ] {
            assert!(screen.contains(line), "missing {line:?} in\n{screen}");
        }
    }

    #[test]
    fn user_details_cover_unlisted_shells_forced_changes_and_missing_data() {
        let mut app = details_app();
        app.diagnostics = known_diagnostics();
        let bob = app.users.iter().find(|user| user.name == "bob").unwrap();
        let text = ui::users::user_details_text(&app, bob);
        assert_eq!(
            text,
            [
                "User: bob",
                "UID 1001  GID 1001 (no such group)",
                "Home: /home/bob  exists: unavailable  perms: unavailable",
                "Shell: /usr/sbin/nologin  listed: no  interactive: no",
                "Password: locked yes  empty no  expired no",
                "Last change: must change at next login  Expires: 2027-01-31",
                "Sudo group (wheel): not member",
                "SSH keys: unavailable",
                "Processes: unavailable",
            ]
            .join("\n")
        );

        // Primary-group membership counts as being in the sudo group.
        app.sudo_group = "alice".to_owned();
        let alice = app.users.iter().find(|user| user.name == "alice").unwrap();
        assert!(ui::users::user_details_text(&app, alice).contains("Sudo group (alice): member"));

        // Without shadow data every password fact is unavailable.
        app.diagnostics = crate::app::CachedDiagnostics::default();
        let text = ui::users::user_details_text(&app, alice);
        assert!(text.contains("Password: unavailable\n"));
        assert!(text.contains("Last change: unavailable  Expires: unavailable"));
    }

    #[test]
    fn group_details_show_a_date_and_age_instead_of_day_counts() {
        let mut app = details_app();
        app.diagnostics = known_diagnostics();
        app.active_tab = ActiveTab::Groups;
        let screen = rendered_rows(&app, 100, 30).join("\n");
        for line in [
            "Group: wheel (GID 998, system, protected)",
            "Members: 0 primary, 2 secondary, 1 orphaned",
            "/etc/group changed: 2026-09-01 (27 days ago)",
        ] {
            assert!(screen.contains(line), "missing {line:?} in\n{screen}");
        }
        let wheel = &app.groups[0];
        assert!(
            ui::groups::group_details_text(&app, wheel).contains(
                "Password status of members: 1 locked, 0 empty, 0 expired (shadow known)"
            )
        );

        let alice = app.groups.iter().find(|group| group.gid == 1000).unwrap();
        app.diagnostics.group_mtime_days = Some(20_724);
        assert!(
            ui::groups::group_details_text(&app, alice)
                .starts_with("Group: alice (GID 1000, user)\n")
        );
        assert!(ui::groups::group_details_text(&app, alice).ends_with("2026-09-28 (today)"));

        app.diagnostics = crate::app::CachedDiagnostics::default();
        let text = ui::groups::group_details_text(&app, alice);
        assert!(text.contains("Password status of members: unavailable\n"));
        assert!(text.ends_with("/etc/group changed: unavailable"));
    }

    /// Buffer of a TestBackend render at `width` x `height`.
    fn rendered_buffer(app: &AppState, width: u16, height: u16) -> ratatui::buffer::Buffer {
        let mut terminal =
            Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
        terminal.draw(|frame| ui::render(frame, app)).unwrap();
        terminal.backend().buffer().clone()
    }

    /// Rows inside the details pane of a render at `width` x `height`,
    /// trailing blanks trimmed.
    fn details_pane_rows(app: &AppState, width: u16, height: u16) -> Vec<String> {
        let area = Rect::new(0, 0, width, height);
        let pane = ui::layout::main_layout(area, app.active_tab, app.show_keybinds, 0).details;
        let (left, inner) = (usize::from(pane.x) + 1, usize::from(pane.width - 2));
        rendered_rows(app, width, height)
            .iter()
            .skip(usize::from(pane.y) + 1)
            .map(|row| row.chars().collect::<Vec<_>>())
            .take_while(|cells| cells[left - 1] != '└')
            .map(|cells| {
                let row: String = cells[left..left + inner].iter().collect();
                row.trim_end().to_owned()
            })
            .collect()
    }

    /// `text` without whitespace, so a fact wrapped by character at any
    /// column still matches the pane rows joined together.
    fn compact(text: &str) -> String {
        text.split_whitespace().collect()
    }

    const USER_FACTS: [&str; 9] = [
        "User: alice (Alice Liddell)",
        "UID 1000  GID 1000 (alice)",
        "Home: /home/alice  exists: yes  perms: 750",
        "Shell: /bin/zsh  listed: yes  interactive: yes",
        "Password: locked no  empty no  expired no",
        "Last change: 2026-03-04  Expires: never",
        "Sudo group (wheel): member",
        "SSH keys: 3",
        "Processes: 12",
    ];

    #[test]
    fn details_pane_grows_to_show_every_wrapped_fact() {
        let mut app = details_app();
        app.diagnostics = known_diagnostics();
        app.show_keybinds = true;
        for (width, height) in [(100, 30), (80, 24)] {
            let rows = details_pane_rows(&app, width, height);
            let text = compact(&rows.concat());
            for fact in USER_FACTS {
                assert!(
                    text.contains(&compact(fact)),
                    "{width}x{height}: missing {fact:?} in\n{}",
                    rows.join("\n")
                );
            }
            assert!(!text.contains(&compact(ui::components::DETAILS_MORE_MARKER)));
        }

        app.active_tab = ActiveTab::Groups;
        let rows = details_pane_rows(&app, 100, 30);
        let text = compact(&rows.concat());
        for fact in [
            "Group: wheel (GID 998, system, protected)",
            "Members: 0 primary, 2 secondary, 1 orphaned",
            "Password status of members: 1 locked, 0 empty, 0 expired (shadow known)",
            "/etc/group changed: 2026-09-01 (27 days ago)",
        ] {
            assert!(
                text.contains(&compact(fact)),
                "missing {fact:?} in\n{}",
                rows.join("\n")
            );
        }
    }

    /// Data rows the member pane titled `title` shows: the non-blank rows
    /// between its column header and its bottom border.
    fn member_pane_rows(screen: &[String], title: &str) -> usize {
        let (x, top) = find_text(screen, &format!("┌{title}")).expect("member pane");
        let (x, top) = (usize::from(x), usize::from(top));
        let top_row: Vec<char> = screen[top].chars().collect();
        let right = (x + 1..top_row.len())
            .find(|&column| top_row[column] == '┐')
            .expect("member pane right border");
        let bottom = (top + 1..screen.len())
            .find(|&y| screen[y].chars().nth(x) == Some('└'))
            .expect("member pane bottom border");
        (top + 2..bottom)
            .filter(|&y| {
                let row: String = screen[y].chars().skip(x + 1).take(right - x - 1).collect();
                !row.trim().is_empty()
            })
            .count()
    }

    #[test]
    fn page_down_in_the_member_panes_moves_by_the_rendered_rows() {
        // alice is in 25 more groups; `crowd` has 25 members.
        let mut extra: Vec<AccountGroup> = (0..25)
            .map(|index| AccountGroup {
                gid: Gid(5000 + index),
                name: GroupName::new(format!("g{index:02}")).unwrap(),
                members: vec![UserName::new("alice").unwrap()],
            })
            .collect();
        extra.push(AccountGroup {
            gid: Gid(4000),
            name: GroupName::new("crowd").unwrap(),
            members: (0..25)
                .map(|index| UserName::new(format!("m{index:02}")).unwrap())
                .collect(),
        });
        let mut app = details_app_with(extra);
        app.diagnostics = known_diagnostics();
        app.show_keybinds = true;
        app.terminal_size = (100, 30);

        app.users_focus = UsersFocus::MemberOf;
        assert!(app.selected_user_groups().len() > 20);
        let shown = member_pane_rows(&rendered_rows(&app, 100, 30), "[Member of]");
        // The wrapped details leave fewer rows than the unwrapped base height.
        assert_eq!(shown, 8);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected_user_group_index, shown);

        app.active_tab = ActiveTab::Groups;
        app.groups_focus = GroupsFocus::Members;
        app.selected_group_index = app
            .groups
            .iter()
            .position(|group| group.name == "crowd")
            .expect("crowd group");
        app.selected_group_member_index = 0;
        let shown = member_pane_rows(&rendered_rows(&app, 100, 30), "[Members]");
        assert!(shown > 0);
        press(&mut app, KeyCode::PageDown);
        assert_eq!(app.selected_group_member_index, shown);
    }

    #[test]
    fn short_terminal_caps_the_details_and_keeps_three_member_rows() {
        let mut app = details_app();
        app.diagnostics = known_diagnostics();
        app.show_keybinds = true;
        let rows = details_pane_rows(&app, 100, 14);
        assert_eq!(rows[0], "User: alice (Alice Liddell)");
        // The marker fits the 33-column pane on one row.
        assert_eq!(
            rows.last().map(String::as_str),
            Some(ui::components::DETAILS_MORE_MARKER),
            "{}",
            rows.join("\n")
        );

        let screen = rendered_rows(&app, 100, 14);
        let (x, top) = find_text(&screen, "┌Member of").expect("member pane");
        let bottom = (top + 1..14)
            .find(|&y| screen[usize::from(y)].chars().nth(usize::from(x)) == Some('└'))
            .expect("member pane bottom border");
        assert!(
            bottom - top > 3,
            "member pane interior rows\n{}",
            screen.join("\n")
        );
    }

    #[test]
    fn details_mute_only_missing_diagnostics_never_account_data() {
        use ratatui::style::Color;
        let snapshot = AccountSnapshot {
            users: vec![AccountUser {
                uid: Uid(1000),
                name: UserName::new("unavailable").unwrap(),
                primary_gid: Gid(100),
                full_name: None,
                home_dir: "/home/unavailable-user".into(),
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
        let mut app = isolated_app(adapter, snapshot);
        // A 50-column details pane, so the probed values stay on one row.
        app.show_keybinds = false;
        app.theme.text = Color::Rgb(7, 8, 9);
        app.theme.muted = Color::Rgb(10, 11, 12);
        let rows = rendered_rows(&app, 100, 30);
        let buffer = rendered_buffer(&app, 100, 30);
        let fg = |text: &str, skip: u16| {
            let (x, y) = find_text(&rows, text)
                .unwrap_or_else(|| panic!("{text:?} not shown in\n{}", rows.join("\n")));
            buffer[(x + skip, y)].fg
        };
        let text = Color::Rgb(7, 8, 9);
        let muted = Color::Rgb(10, 11, 12);
        assert_eq!(fg("User: unavailable", 6), text, "user name");
        assert_eq!(fg("Home: /home/unavailable-user", 12), text, "home dir");
        assert_eq!(fg("Home: /home/unavailable-user", 0), text, "label");
        // Shadow data is unavailable in this fixture.
        assert_eq!(fg("Password: unavailable", 10), muted, "missing shadow");
        assert_eq!(fg("Password: unavailable", 0), text, "password label");
        assert_eq!(fg("exists: unav", 8), muted, "missing home check");
    }

    #[test]
    fn blocked_candidates_and_the_blocked_sudo_row_are_muted() {
        use ratatui::style::Color;
        let muted = Color::Rgb(10, 11, 12);
        let mut app = wheel_app();
        app.theme.muted = muted;
        let alice = app.users.iter().position(|user| user.name == "alice");
        app.selected_user_index = alice.unwrap();
        open_modal(
            &mut app,
            ModalState::ModifyGroupsAdd {
                selected: 0,
                offset: 0,
                selected_multi: vec![],
                filter: String::new(),
                filtering: false,
                error: None,
            },
        );
        let rows = rendered_rows(&app, 100, 30);
        let buffer = rendered_buffer(&app, 100, 30);
        let (x, y) = find_text(&rows, "wheel (blocked: sudo group, protected)").unwrap();
        assert_eq!(buffer[(x, y)].fg, muted, "blocked candidate");
        let (x, y) = find_text(&rows, "[ ] ops").unwrap();
        assert_ne!(buffer[(x, y)].fg, muted, "allowed candidate");

        let mut app = wheel_app();
        app.theme.muted = muted;
        open_user_add(&mut app);
        let rows = rendered_rows(&app, 100, 30);
        let buffer = rendered_buffer(&app, 100, 30);
        let (x, y) = find_text(&rows, "Add to sudo group (wheel): blocked").unwrap();
        assert_eq!(buffer[(x, y)].fg, muted, "blocked sudo row");
        let (x, y) = find_text(&rows, "Create home").unwrap();
        assert_ne!(buffer[(x, y)].fg, muted, "other form rows");
    }

    #[test]
    fn status_bar_mutes_the_help_hint_and_unavailable_shadow() {
        use ratatui::style::Color;
        let mut app = single_account_app();
        app.theme.status_fg = Color::Rgb(1, 2, 3);
        app.theme.status_bg = Color::Rgb(4, 5, 6);
        app.theme.muted = Color::Rgb(10, 11, 12);
        app.diagnostics
            .config_messages
            .push("theme configuration line 3: unknown color".to_owned());
        app.last_refresh_label = "22:13:20 UTC".to_owned();
        let (width, height) = (120, 12);
        let rows = rendered_rows(&app, width, height);
        let buffer = rendered_buffer(&app, width, height);
        let y = height - 1;
        let status = &rows[usize::from(y)];
        assert_eq!(
            status.trim_end(),
            "refreshed 22:13:20 UTC · users 1/1 · groups 1 · config: 1 error (see ? help) · shadow unavailable"
        );
        let x_of = |text: &str| {
            let byte = status.find(text).unwrap();
            u16::try_from(status[..byte].chars().count()).unwrap()
        };
        for (text, fg) in [
            ("refreshed", Color::Rgb(1, 2, 3)),
            ("config: 1 error", Color::Rgb(1, 2, 3)),
            ("(see ? help)", Color::Rgb(10, 11, 12)),
            ("help)", Color::Rgb(10, 11, 12)),
            ("shadow ", Color::Rgb(1, 2, 3)),
            ("unavailable", Color::Rgb(10, 11, 12)),
        ] {
            let cell = &buffer[(x_of(text), y)];
            assert_eq!(cell.fg, fg, "{text}");
            assert_eq!(cell.bg, Color::Rgb(4, 5, 6), "{text} background");
        }
    }

    #[test]
    fn dark_preset_keeps_muted_status_text_visible() {
        let mut app = single_account_app();
        app.theme = crate::app::Theme::dark();
        app.diagnostics
            .config_messages
            .push("theme configuration line 3: unknown color".to_owned());
        app.diagnostics.shadow = crate::search::ShadowState::Unavailable {
            reason: "shadow data has not been refreshed".to_owned(),
        };
        app.last_refresh_label = "22:13:20 UTC".to_owned();
        let (width, height) = (120, 12);
        let rows = rendered_rows(&app, width, height);
        let buffer = rendered_buffer(&app, width, height);
        let y = height - 1;
        assert!(rows[usize::from(y)].contains("(see ? help) · shadow unavailable"));
        let muted: Vec<_> = (0..width)
            .map(|x| &buffer[(x, y)])
            .filter(|cell| cell.fg == app.theme.muted && !cell.symbol().trim().is_empty())
            .collect();
        assert_eq!(
            muted.len(),
            "(see ? help)unavailable"
                .chars()
                .filter(|c| *c != ' ')
                .count(),
            "muted status cells"
        );
        for cell in muted {
            assert_ne!(
                cell.fg,
                cell.bg,
                "muted {:?} on its own background",
                cell.symbol()
            );
        }
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
    fn empty_state_wraps_exactly_on_a_narrow_pane() {
        let mut app = single_account_app();
        app.users_query = "aaaaaaa aaaaaaa aaaaaaa aaaaaaa aaaaaaa bbbbbbb".to_owned();
        app.sort_and_filter();
        assert!(app.users.is_empty());
        let rows = rendered_rows(&app, 30, 14);
        assert!(
            find_text(&rows, "bbbbbbb").is_some(),
            "end of the message stays visible:\n{}",
            rows.join("\n")
        );
    }

    #[test]
    fn member_of_pane_says_when_the_user_has_no_groups() {
        // alice's primary gid 500 has no group and no group lists her.
        let snapshot = AccountSnapshot {
            users: vec![AccountUser {
                uid: Uid(1000),
                name: UserName::new("alice").unwrap(),
                primary_gid: Gid(500),
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
        let app = isolated_app(adapter, snapshot);
        assert_eq!(app.selected_user_index, 0);
        assert!(app.selected_user_groups().is_empty());
        let rows = rendered_rows(&app, 100, 30);
        let (x, _) = find_text(&rows, "No supplementary groups").expect("member-of empty state");
        assert!(x >= 50, "inside the member-of pane: {x}");
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
        let mut app = single_account_app();
        let rows = rendered_rows(&app, 100, 30);
        let header = &rows[1];
        assert!(header.contains("usrgrp-manager (unknown)   [Users]  Groups"));
        assert!(header.contains("? help  / search  f filter  Shift+K keys"));
        assert!(!rows[..3].iter().any(|row| row.contains("users:")));
        // Hints that do not fit are dropped from the end. With the
        // keybindings panel off the header has no narrow-terminal notice.
        app.show_keybinds = false;
        let rows = rendered_rows(&app, 64, 20);
        assert!(rows[1].contains("? help  / search"), "{}", rows[1]);
        assert!(!rows[1].contains("filter"));
    }

    #[test]
    fn keybinds_panel_groups_bindings_under_human_labels() {
        let mut app = user_app();
        app.show_keybinds = true;
        let screen = rendered_at(&app, 120, 40);
        for text in [
            "General",
            "Navigation",
            "Actions",
            "Move up: Up, k",
            "Quit: q, Ctrl+C",
            "Clear search: Esc",
            "Delete: Delete, d",
        ] {
            assert!(screen.contains(text), "missing {text:?}");
        }
        assert!(!screen.contains("Ignore"));
        assert!(!screen.contains("MoveUp"));

        let lines: Vec<String> = ui::components::keybinds_panel_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(lines.contains(&"Toggle keybindings: Shift+K".to_owned()));
        assert!(lines.contains(&"Switch pane: Left, Right, h, l, Shift+Tab".to_owned()));
        let position = |text: &str| lines.iter().position(|line| line == text).unwrap();
        assert!(position("General") < position("Navigation"));
        assert!(position("Navigation") < position("Actions"));
        assert!(position("Quit: q, Ctrl+C") < position("Help: ?"));
    }

    #[test]
    fn header_says_when_the_keybinds_panel_is_hidden_by_width() {
        let mut app = single_account_app();
        app.show_keybinds = true;
        let rows = rendered_rows(&app, 80, 24);
        let header = &rows[1];
        assert!(header.contains("usrgrp-manager (unknown)   [Users]  Groups"));
        assert!(header.contains(" (keybinds hidden)"), "{header}");
        assert!(header.contains("? help"), "{header}");
        assert!(!header.contains("narrow terminal"));
        let header = &rendered_rows(&app, 89, 24)[1];
        assert!(
            header.contains(" (keybinds hidden: narrow terminal)"),
            "{header}"
        );
        assert!(header.contains("? help"), "{header}");
        assert!(!rendered_rows(&app, 90, 24)[1].contains("keybinds hidden"));
        assert!(!rendered_rows(&app, 120, 40)[1].contains("keybinds hidden"));
        app.show_keybinds = false;
        assert!(!rendered_rows(&app, 80, 24)[1].contains("keybinds hidden"));
    }

    #[test]
    fn header_keeps_help_and_the_notice_at_fifty_columns() {
        let mut app = single_account_app();
        app.show_keybinds = true;
        let header = &rendered_rows(&app, 50, 24)[1];
        assert!(header.contains(" (keybinds hidden)"), "{header}");
        assert!(header.contains("? help"), "{header}");
        assert!(header.contains("usrgrp-manager"), "{header}");
        assert!(header.contains('…'), "{header}");
        assert!(!header.contains("narrow terminal"), "{header}");
    }

    #[test]
    fn header_shortens_a_long_user_name_before_dropping_help_or_the_notice() {
        let mut app = single_account_app();
        app.show_keybinds = true;
        app.current_username = "abcdefghijklmnopqrstuvwxyz012345".to_owned();
        assert_eq!(app.current_username.len(), 32);
        let rows = rendered_rows(&app, 80, 24);
        let header = &rows[1];
        assert!(header.contains("? help"), "{header}");
        assert!(header.contains(" (keybinds hidden)"), "{header}");
        assert!(!header.contains(&app.current_username), "{header}");
        assert!(header.contains("usrgrp-manager (abcdefg"), "{header}");
        assert!(header.contains("…)   [Users]  Groups"), "{header}");
        // Hints come whole or not at all.
        let line = ui::components::header_line(&app, 78);
        assert!(
            line.ends_with("? help") || line.ends_with("/ search"),
            "{line}"
        );
        // Wide enough: the name is shown in full.
        assert!(rendered_rows(&app, 160, 40)[1].contains(&app.current_username));
    }

    #[test]
    fn keybinds_panel_does_not_list_keys_bound_to_ignore() {
        let mut app = user_app();
        let config = tempfile::tempdir().unwrap();
        let path = config.path().join("keybinds.conf");
        std::fs::write(&path, "Ignore = x\n").unwrap();
        app.keymap = crate::app::keymap::Keymap::from_file(path.to_str().unwrap()).unwrap();
        assert!(
            app.keymap
                .all_bindings()
                .into_iter()
                .any(
                    |((_, code), action)| code == KeyCode::Char('x') && action == KeyAction::Ignore
                )
        );
        app.show_keybinds = true;
        let screen = rendered_at(&app, 120, 40);
        assert!(screen.contains("Navigation"));
        assert!(!screen.contains("Ignore"));
        for line in ui::components::keybinds_panel_lines(&app) {
            let line = line.to_string();
            let Some((_, keys)) = line.split_once(": ") else {
                continue;
            };
            assert!(!keys.split(", ").any(|key| key == "x"), "{line}");
        }
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
                error: None,
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
                error: None,
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
            error: None,
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
                error: None,
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
            Some(ModalState::Info { title, message }) => {
                assert_eq!(title.as_deref(), Some("Failed"));
                assert!(
                    message.ends_with(
                        "\nAuthentication failed three times. The operation was not run."
                    )
                );
            }
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

    /// Actions > Modify > Modify details, cursor on row `row`, Enter.
    fn open_details_row(app: &mut AppState, row: usize) {
        press(app, KeyCode::Enter);
        press(app, KeyCode::Enter);
        press(app, KeyCode::Down);
        press(app, KeyCode::Down);
        press(app, KeyCode::Enter);
        for _ in 0..row {
            press(app, KeyCode::Down);
        }
        press(app, KeyCode::Enter);
    }

    fn text_value(app: &AppState) -> Option<&str> {
        match app.modal() {
            Some(
                ModalState::ModifyTextInput { value, .. }
                | ModalState::GroupAddInput { name: value, .. }
                | ModalState::GroupRenameInput { name: value, .. }
                | ModalState::UserAddInput { name: value, .. },
            ) => Some(value),
            _ => None,
        }
    }

    fn ctrl(app: &mut AppState, character: char) {
        handle_key(
            app,
            KeyEvent::new(KeyCode::Char(character), KeyModifiers::CONTROL),
        );
    }

    #[test]
    fn rename_dialogs_open_with_the_current_value() {
        let mut app = user_app();
        open_details_row(&mut app, 0);
        assert_eq!(text_value(&app), Some("alice"));
        let screen = rendered(&app);
        assert!(screen.contains("alice▏  5/32"));
        assert!(screen.contains("The home directory keeps its current name."));

        app.close_modals();
        open_details_row(&mut app, 1);
        assert_eq!(text_value(&app), Some("Alice"));
        assert!(rendered(&app).contains("Alice▏  5/256"));

        app.close_modals();
        open_modal(
            &mut app,
            ModalState::GroupModifyMenu {
                selected: 2,
                target_gid: Some(1000),
            },
        );
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::GroupRenameInput { name, .. }) if name == "dev"
        ));
        assert!(rendered(&app).contains("dev▏  3/32"));
    }

    #[test]
    fn name_fields_count_characters_and_say_when_full() {
        let mut app = user_app();
        open_modal(
            &mut app,
            ModalState::GroupAddInput {
                name: String::new(),
                error: None,
            },
        );
        // Two bytes each: a byte count would stop at 16.
        type_text(&mut app, &"é".repeat(32));
        assert_eq!(text_value(&app).map(|name| name.chars().count()), Some(32));
        assert_ne!(
            app.modal().and_then(ModalState::form_error),
            Some(crate::app::MAX_LENGTH_REACHED)
        );
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(text_value(&app).map(|name| name.chars().count()), Some(32));
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some(crate::app::MAX_LENGTH_REACHED)
        );
        assert!(rendered(&app).contains("Maximum length reached"));

        app.close_modals();
        open_details_row(&mut app, 1);
        type_text(&mut app, &"b".repeat(300));
        assert_eq!(text_value(&app).map(str::len), Some(256));
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some(crate::app::MAX_LENGTH_REACHED)
        );

        app.close_modals();
        open_user_add(&mut app);
        type_text(&mut app, &"b".repeat(40));
        assert_eq!(text_value(&app).map(str::len), Some(32));
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some(crate::app::MAX_LENGTH_REACHED)
        );
        // Deleting a character makes room again and clears the message.
        press(&mut app, KeyCode::Backspace);
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    fn password_counts(app: &AppState) -> (usize, usize) {
        match app.modal() {
            Some(
                ModalState::UserAddInput {
                    password, confirm, ..
                }
                | ModalState::ChangePassword {
                    password, confirm, ..
                },
            ) => (password.char_count(), confirm.char_count()),
            _ => panic!("no password form open"),
        }
    }

    #[test]
    fn password_fields_are_limited_independently() {
        let mut app = root_app();
        open_user_add(&mut app);
        press(&mut app, KeyCode::Down);
        type_text(&mut app, &"p".repeat(1030));
        assert_eq!(password_counts(&app), (1024, 0));
        assert_eq!(
            app.modal().and_then(ModalState::form_error),
            Some(crate::app::MAX_LENGTH_REACHED)
        );
        // A full password field does not stop the confirm field.
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "pp");
        assert_eq!(password_counts(&app), (1024, 2));
        assert_eq!(app.modal().and_then(ModalState::form_error), None);
    }

    #[test]
    fn create_user_refuses_a_confirm_without_a_password() {
        let mut app = root_app();
        open_user_add(&mut app);
        type_text(&mut app, "bob");
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "only-confirm");
        for _ in 0..3 {
            press(&mut app, KeyCode::Down);
        }
        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_none());
        assert!(matches!(
            app.modal(),
            Some(ModalState::UserAddInput { error: Some(message), .. })
                if message.contains("passwords must match")
        ));
    }

    #[test]
    fn shell_picker_starts_on_and_marks_the_current_shell() {
        let mut snapshot = fixture_snapshot();
        snapshot.shells = vec![
            ShellPath::new("/bin/bash").unwrap(),
            ShellPath::new("/bin/sh").unwrap(),
            ShellPath::new("/bin/zsh").unwrap(),
        ];
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        open_details_row(&mut app, 2);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyShell {
                selected: 1,
                current: Some(1),
                ..
            })
        ));
        let screen = rendered(&app);
        assert!(screen.contains("▶ /bin/sh (current)"));
        assert!(!screen.contains("/bin/bash (current)"));
    }

    #[test]
    fn shell_picker_lists_an_unlisted_current_shell_first() {
        let mut snapshot = fixture_snapshot();
        snapshot.users[0].shell = ShellPath::new("/usr/sbin/nologin").unwrap();
        snapshot.shells = vec![ShellPath::new("/bin/bash").unwrap()];
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        open_details_row(&mut app, 2);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyShell {
                selected: 0,
                current: Some(0),
                current_unlisted: true,
                shells,
                ..
            }) if shells == &["/usr/sbin/nologin", "/bin/bash"]
        ));
        let screen = rendered(&app);
        assert!(screen.contains("▶ /usr/sbin/nologin (current, not in /etc/shells)"));
        assert!(!screen.contains("/bin/bash (current"));
    }

    #[test]
    fn shell_picker_shows_the_default_shell_label_as_a_note_only() {
        let mut snapshot = fixture_snapshot();
        snapshot.users[0].shell = ShellPath::from_observed_passwd("").unwrap();
        snapshot.shells = vec![
            ShellPath::new("/bin/bash").unwrap(),
            ShellPath::new("/bin/zsh").unwrap(),
        ];
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        assert_eq!(app.users[0].shell, "(default /bin/sh)");
        open_details_row(&mut app, 2);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyShell {
                selected: 0,
                current: None,
                current_unlisted: false,
                current_label: Some(label),
                shells,
                ..
            }) if label == "(default /bin/sh)" && shells == &["/bin/bash", "/bin/zsh"]
        ));
        let screen = rendered(&app);
        assert!(screen.contains("Current shell: (default /bin/sh)"));
        assert!(screen.contains("▶ /bin/bash"));
        assert!(!screen.contains("(current"));
        assert!(!screen.contains("▶ (default"));

        press(&mut app, KeyCode::Enter);
        assert!(app.pending_operation.is_some());
        assert_eq!(app.last_preview, ["usermod -s /bin/bash alice"]);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { .. })
        ));
    }

    /// Alice (primary GID 1000, `dev`) with extra groups `users` (GID 100,
    /// system), `staff` (GID 1001), and `wheel` (GID 1010, elevation).
    fn primary_group_app() -> AppState {
        let mut snapshot = fixture_snapshot();
        for (gid, name) in [(100, "users"), (1001, "staff"), (1010, "wheel")] {
            snapshot.groups.push(AccountGroup {
                gid: Gid(gid),
                name: GroupName::new(name).unwrap(),
                members: vec![],
            });
        }
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    /// Highlighted row and names of the open primary-group picker.
    fn primary_group_picker(app: &AppState) -> (usize, Vec<String>) {
        match app.modal() {
            Some(ModalState::ModifyPrimaryGroup {
                selected, groups, ..
            }) => (*selected, groups.names.clone()),
            _ => panic!("expected the primary group picker"),
        }
    }

    #[test]
    fn details_menu_offers_primary_group_and_home_directory() {
        let mut app = user_app();
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Enter);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Enter);
        let screen = rendered(&app);
        for entry in [
            "Username",
            "Full name",
            "Shell",
            "Primary group",
            "Home directory",
        ] {
            assert!(screen.contains(entry), "{entry} missing:\n{screen}");
        }
        // Up from the first entry wraps to the last one.
        press(&mut app, KeyCode::Up);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyDetailsMenu { selected: 4 })
        ));
    }

    #[test]
    fn primary_group_picker_marks_the_current_group_and_mutes_blocked_ones() {
        use ratatui::style::Color;
        let muted = Color::Rgb(10, 11, 12);
        let mut app = primary_group_app();
        app.theme.muted = muted;
        open_details_row(&mut app, 3);
        let (selected, names) = primary_group_picker(&app);
        assert_eq!(names, ["users", "dev", "staff", "wheel"]);
        // The picker starts on the current group.
        assert_eq!(selected, 1);
        let rows = rendered_rows(&app, 100, 30);
        let screen = rows.join("\n");
        assert!(screen.contains("Select primary group"), "{screen}");
        assert!(screen.contains("▶ dev (current)"), "{screen}");
        assert!(!screen.contains("staff (current)"), "{screen}");
        assert!(screen.contains(ui::components::LIST_FOOTER), "{screen}");
        let buffer = rendered_buffer(&app, 100, 30);
        for blocked in [
            "users (blocked: system group, protected)",
            "wheel (blocked: sudo group, protected)",
        ] {
            let (x, y) = find_text(&rows, blocked)
                .unwrap_or_else(|| panic!("{blocked:?} not shown in\n{screen}"));
            assert_eq!(buffer[(x, y)].fg, muted, "{blocked}");
        }
        let (x, y) = find_text(&rows, "staff").unwrap();
        assert_ne!(buffer[(x, y)].fg, muted, "allowed group");
    }

    #[test]
    fn primary_group_picker_refuses_current_and_blocked_groups_inline() {
        let mut app = primary_group_app();
        open_details_row(&mut app, 3);
        // Enter on the current group changes nothing.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPrimaryGroup { error: Some(message), .. })
                if message == "dev is already the primary group."
        ));
        assert!(rendered(&app).contains("dev is already the primary group."));
        // A blocked group shows the policy's reason and prepares nothing.
        press(&mut app, KeyCode::Up);
        assert_eq!(primary_group_picker(&app).0, 0);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPrimaryGroup { error: Some(message), .. })
                if message == "system group, protected"
        ));
        assert!(app.pending_operation.is_none());
        // Moving clears the message.
        press(&mut app, KeyCode::Char('k'));
        assert_eq!(primary_group_picker(&app).0, 3);
        assert!(
            app.modal()
                .is_some_and(|modal| modal.form_error().is_none())
        );
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPrimaryGroup { error: Some(message), .. })
                if message == "sudo group, protected"
        ));
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn primary_group_picker_confirms_the_exact_usermod_command() {
        let mut app = primary_group_app();
        open_details_row(&mut app, 3);
        press(&mut app, KeyCode::Char('j'));
        assert_eq!(primary_group_picker(&app).0, 2);
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Change primary group of alice to staff"
                    && preview == &["usermod -g staff alice"]
        ));
    }

    #[test]
    fn primary_group_picker_notes_a_gid_without_a_group() {
        let mut snapshot = fixture_snapshot();
        snapshot.users[0].primary_gid = Gid(4242);
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        open_details_row(&mut app, 3);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPrimaryGroup {
                selected: 0,
                current: None,
                current_gid: Some(4242),
                ..
            })
        ));
        let screen = rendered(&app);
        assert!(screen.contains("Current primary group: GID 4242 (no group entry)"));
        assert!(screen.contains("▶ dev"));
        assert!(!screen.contains("(current)"));
    }

    /// `dev` plus 1,024 more groups, `g0001` (GID 2001) to `g1024` (GID
    /// 3024): one past the membership dialogs' cap. Alice's primary GID is
    /// `primary_gid`.
    fn many_groups_app(primary_gid: u32) -> AppState {
        let mut snapshot = fixture_snapshot();
        snapshot.users[0].primary_gid = Gid(primary_gid);
        for index in 1..=1024 {
            snapshot.groups.push(AccountGroup {
                gid: Gid(2000 + index),
                name: GroupName::new(format!("g{index:04}")).unwrap(),
                members: vec![],
            });
        }
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    #[test]
    fn primary_group_picker_lists_every_group_past_the_membership_cap() {
        assert_eq!(crate::app::MAX_MEMBERSHIP_CANDIDATES, 1024);

        // The last group is the current one: it is marked, not reported as
        // a GID without a group entry.
        let mut app = many_groups_app(3024);
        open_details_row(&mut app, 3);
        let (selected, names) = primary_group_picker(&app);
        assert_eq!(names.len(), 1025);
        assert_eq!(names.last().map(String::as_str), Some("g1024"));
        assert_eq!(selected, 1024);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPrimaryGroup {
                current: Some(1024),
                current_gid: None,
                ..
            })
        ));
        let screen = rendered(&app);
        assert!(screen.contains("▶ g1024 (current)"), "{screen}");
        assert!(!screen.contains("no group entry"), "{screen}");

        // From the first group, Down reaches the last one and Enter prepares
        // the change.
        let mut app = many_groups_app(1000);
        open_details_row(&mut app, 3);
        assert_eq!(primary_group_picker(&app).0, 0);
        for _ in 0..1024 {
            press(&mut app, KeyCode::Down);
        }
        assert_eq!(primary_group_picker(&app).0, 1024);
        assert!(rendered(&app).contains("▶ g1024"));
        press(&mut app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::OperationConfirm { action, preview, .. })
                if action == "Change primary group of alice to g1024"
                    && preview == &["usermod -g g1024 alice"]
        ));
    }

    /// Value, checkbox, and inline error of the open home form.
    fn home_form(app: &AppState) -> (String, bool, Option<String>) {
        match app.modal() {
            Some(ModalState::ModifyHome {
                value,
                move_contents,
                error,
                ..
            }) => (value.clone(), *move_contents, error.clone()),
            _ => panic!("expected the home directory form"),
        }
    }

    #[test]
    fn home_form_opens_with_the_current_home_and_an_unticked_checkbox() {
        let mut app = user_app();
        open_details_row(&mut app, 4);
        assert_eq!(home_form(&app), ("/home/alice".to_owned(), false, None));
        let screen = rendered(&app);
        assert!(screen.contains("Change home directory"), "{screen}");
        assert!(screen.contains("Current: /home/alice"), "{screen}");
        assert!(screen.contains("▶ Path: /home/alice▏  11/4096"), "{screen}");
        assert!(screen.contains("[ ] move contents"), "{screen}");
        assert!(screen.contains(ui::components::FORM_FOOTER), "{screen}");
        // Submitting the unchanged path is refused in the form.
        press(&mut app, KeyCode::Enter);
        assert_eq!(
            home_form(&app).2.as_deref(),
            Some("This is already the home directory.")
        );
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn home_form_validates_the_path_as_it_is_typed() {
        let mut app = user_app();
        open_details_row(&mut app, 4);
        for _ in 0.."/home/alice".len() {
            press(&mut app, KeyCode::Backspace);
        }
        assert_eq!(
            home_form(&app).2.as_deref(),
            Some("must contain 1 to 4096 bytes")
        );
        type_text(&mut app, "srv");
        assert_eq!(home_form(&app).2.as_deref(), Some("must be absolute"));
        assert!(rendered(&app).contains("must be absolute"));
        for _ in 0..3 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "/srv/a:b");
        assert_eq!(
            home_form(&app).2.as_deref(),
            Some("must not contain colons")
        );
        // Enter with the bad path keeps the form open under the error.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::ModifyHome { .. })));
        assert!(
            home_form(&app)
                .2
                .is_some_and(|error| error.ends_with("[E-VALIDATION]"))
        );
        assert!(app.pending_operation.is_none());
        for _ in 0..3 {
            press(&mut app, KeyCode::Backspace);
        }
        type_text(&mut app, "alice");
        assert_eq!(home_form(&app), ("/srv/alice".to_owned(), false, None));
    }

    #[test]
    fn home_form_confirms_with_and_without_moving_contents() {
        for (tick, action, command) in [
            (
                false,
                "Change home of alice to /srv/alice",
                "usermod -d /srv/alice alice",
            ),
            (
                true,
                "Change home of alice to /srv/alice (moving contents)",
                "usermod -d /srv/alice -m alice",
            ),
        ] {
            let mut app = user_app();
            open_details_row(&mut app, 4);
            for _ in 0.."/home/alice".len() {
                press(&mut app, KeyCode::Backspace);
            }
            type_text(&mut app, "/srv/alice");
            if tick {
                // Space on the path types a space; on the checkbox it ticks.
                press(&mut app, KeyCode::Down);
                press(&mut app, KeyCode::Char(' '));
                assert_eq!(home_form(&app), ("/srv/alice".to_owned(), true, None));
                assert!(rendered(&app).contains("▶ [x] move contents"));
            }
            press(&mut app, KeyCode::Enter);
            assert!(
                matches!(
                    app.modal(),
                    Some(ModalState::OperationConfirm { action: title, preview, .. })
                        if title == action && preview == &[command]
                ),
                "{action}"
            );
        }
    }

    #[test]
    fn primary_group_and_home_actions_describe_the_change() {
        assert_eq!(
            PendingAction::ChangePrimaryGroup {
                username: "alice".to_owned(),
                groupname: "staff".to_owned(),
            }
            .describe(),
            "Change primary group of alice to staff"
        );
        for (move_contents, expected) in [
            (true, "Change home of alice to /srv/alice (moving contents)"),
            (false, "Change home of alice to /srv/alice"),
        ] {
            assert_eq!(
                PendingAction::ChangeHome {
                    username: "alice".to_owned(),
                    home: "/srv/alice".to_owned(),
                    move_contents,
                }
                .describe(),
                expected
            );
        }
        assert_eq!(
            step_label("change user primary group"),
            "change primary group"
        );
        assert_eq!(step_label("change user home"), "change home directory");
    }

    /// `user_app` with alice's cached shadow expiry (`Some(None)` never
    /// expires, `None` shadow unreadable) and the expiry form open.
    fn expiry_app(expiry: Option<Option<i64>>) -> AppState {
        let mut app = user_app();
        app.diagnostics.shadow = match expiry {
            Some(expire_abs_days) => crate::search::ShadowState::Known(
                [(
                    "alice".to_owned(),
                    crate::search::ShadowStatus {
                        expire_abs_days,
                        ..Default::default()
                    },
                )]
                .into_iter()
                .collect(),
            ),
            None => crate::search::ShadowState::Unavailable {
                reason: "permission denied".to_owned(),
            },
        };
        open_modal(&mut app, ModalState::ModifyPasswordMenu { selected: 0 });
        // Up from the first entry wraps to "Account expiry date".
        press(&mut app, KeyCode::Up);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPasswordMenu { selected: 2 })
        ));
        press(&mut app, KeyCode::Enter);
        app
    }

    /// Value and inline error of the open expiry form.
    fn expiry_form(app: &AppState) -> (String, Option<String>) {
        match app.modal() {
            Some(ModalState::ModifyExpiry { value, error, .. }) => (value.clone(), error.clone()),
            _ => panic!("expected the expiry form"),
        }
    }

    #[test]
    fn password_menu_offers_the_account_expiry_date() {
        let mut app = user_app();
        open_modal(&mut app, ModalState::ModifyPasswordMenu { selected: 0 });
        let screen = rendered(&app);
        for entry in [
            "Set or change password",
            "Force change at next login",
            "Account expiry date",
        ] {
            assert!(screen.contains(entry), "{entry} missing:\n{screen}");
        }
        press(&mut app, KeyCode::Down);
        press(&mut app, KeyCode::Char('j'));
        assert!(rendered(&app).contains("▶ Account expiry date"));
        // Down from the last entry wraps to the first.
        press(&mut app, KeyCode::Down);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ModifyPasswordMenu { selected: 0 })
        ));
    }

    #[test]
    fn expiry_form_opens_with_the_cached_shadow_expiry() {
        for (expiry, value, current) in [
            (Some(Some(20_849)), "2027-01-31", "Current: 2027-01-31"),
            (Some(None), "", "Current: never"),
            (None, "", "Current: unknown"),
        ] {
            let app = expiry_app(expiry);
            assert_eq!(expiry_form(&app), (value.to_owned(), None), "{expiry:?}");
            let screen = rendered(&app);
            assert!(screen.contains("Account expiry date"), "{screen}");
            assert!(screen.contains(current), "{screen}");
            assert!(
                screen.contains(&format!("Expires: {value}▏  {}/10", value.len())),
                "{screen}"
            );
            assert!(
                screen.contains("Empty = never expires. Format YYYY-MM-DD."),
                "{screen}"
            );
            assert!(screen.contains(ui::components::TEXT_FOOTER), "{screen}");
        }
    }

    #[test]
    fn expiry_form_validates_the_date_as_it_is_typed() {
        let mut app = expiry_app(None);
        type_text(&mut app, "2027");
        assert_eq!(
            expiry_form(&app).1.as_deref(),
            Some("must be a date as YYYY-MM-DD")
        );
        type_text(&mut app, "-02-29");
        assert_eq!(
            expiry_form(&app).1.as_deref(),
            Some("must be a real calendar date")
        );
        assert!(rendered(&app).contains("must be a real calendar date"));
        press(&mut app, KeyCode::Backspace);
        type_text(&mut app, "8");
        assert_eq!(expiry_form(&app), ("2027-02-28".to_owned(), None));
        // An eleventh character is refused.
        type_text(&mut app, "1");
        assert_eq!(
            expiry_form(&app),
            ("2027-02-28".to_owned(), Some(MAX_LENGTH_REACHED.to_owned()))
        );
        // Empty is valid: it means the account never expires.
        for _ in 0..10 {
            press(&mut app, KeyCode::Backspace);
        }
        assert_eq!(expiry_form(&app), (String::new(), None));
        type_text(&mut app, "1969-12-31");
        assert_eq!(
            expiry_form(&app).1.as_deref(),
            Some("year must be between 1970 and 9999")
        );
        // Enter with a bad date keeps the form open under the error.
        press(&mut app, KeyCode::Enter);
        assert!(matches!(app.modal(), Some(ModalState::ModifyExpiry { .. })));
        assert!(
            expiry_form(&app)
                .1
                .is_some_and(|error| error.ends_with("[E-VALIDATION]"))
        );
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn expiry_form_confirms_setting_and_removing_the_expiry() {
        // (cached expiry, Backspaces, typed text, title, command)
        let cases = [
            (
                None,
                0,
                "2027-01-31",
                "Set account expiry of alice to 2027-01-31",
                "chage -E 2027-01-31 alice",
            ),
            (
                Some(Some(20_849)),
                2,
                "28",
                "Set account expiry of alice to 2027-01-28",
                "chage -E 2027-01-28 alice",
            ),
            (
                Some(Some(20_849)),
                10,
                "",
                "Remove account expiry of alice",
                "chage -E -1 alice",
            ),
            // Unknown shadow: an empty field still submits "never".
            (
                None,
                0,
                "",
                "Remove account expiry of alice",
                "chage -E -1 alice",
            ),
        ];
        for (expiry, erase, typed, title, command) in cases {
            let mut app = expiry_app(expiry);
            for _ in 0..erase {
                press(&mut app, KeyCode::Backspace);
            }
            type_text(&mut app, typed);
            press(&mut app, KeyCode::Enter);
            assert!(
                matches!(
                    app.modal(),
                    Some(ModalState::OperationConfirm { action, preview, .. })
                        if action == title && preview == &[command]
                ),
                "{title}"
            );
        }
    }

    #[test]
    fn expiry_form_refuses_the_unchanged_expiry_inline() {
        for (expiry, message) in [
            (Some(Some(20_849)), "This is already the expiry date."),
            (Some(None), "The account already never expires."),
        ] {
            let mut app = expiry_app(expiry);
            press(&mut app, KeyCode::Enter);
            assert_eq!(expiry_form(&app).1.as_deref(), Some(message));
            assert!(rendered(&app).contains(message));
            assert!(app.pending_operation.is_none());
        }
    }

    #[test]
    fn expiry_actions_describe_the_change() {
        let describe = |date: Option<&str>| {
            PendingAction::SetAccountExpiry {
                username: "alice".to_owned(),
                date: date.map(str::to_owned),
            }
            .describe()
        };
        assert_eq!(
            describe(Some("2027-01-31")),
            "Set account expiry of alice to 2027-01-31"
        );
        assert_eq!(describe(None), "Remove account expiry of alice");
        assert_eq!(step_label("set account expiry"), "set account expiry");
        // Validated dates and the shadow day count convert both ways.
        for text in ["1970-01-01", "2000-02-29", "2027-01-31", "9999-12-31"] {
            let days = ExpiryDate::new(text).unwrap().days_since_epoch();
            assert_eq!(civil_date_label(days), text);
        }
    }

    #[test]
    fn typed_cursor_glyph_does_not_hide_the_real_cursor_at_30x10() {
        let mut app = user_app();
        open_details_row(&mut app, 1);
        for _ in 0.."Alice".len() {
            press(&mut app, KeyCode::Backspace);
        }
        assert_eq!(text_value(&app), Some(""));
        type_text(&mut app, "▏");
        type_text(&mut app, &"a".repeat(200));
        let rows = rendered_rows(&app, 30, 10);
        // The value, cursor, and count wrap to 20-column rows; the last one
        // is `a▏  201/256`, and the first (with the typed glyph) scrolls away.
        assert!(
            rows.iter().any(|row| row.contains("a▏  201/256")),
            "trailing cursor row scrolled out: {rows:#?}"
        );
        assert!(!rows.iter().any(|row| row.contains("▏aaaa")));
        assert!(rows.iter().any(|row| row.contains("type to edit")));
    }

    #[test]
    fn text_cursor_of_a_full_length_name_stays_visible_at_30x10() {
        let mut app = user_app();
        open_details_row(&mut app, 1);
        type_text(&mut app, &"b".repeat(251));
        assert_eq!(text_value(&app).map(str::len), Some(256));
        let rows = rendered_rows(&app, 30, 10);
        assert!(
            rows.iter().any(|row| row.contains('▏')),
            "cursor row scrolled out: {rows:#?}"
        );
    }

    #[test]
    fn revealed_password_is_a_zeroizing_copy() {
        let mut input = SecretInput::default();
        "abc".chars().for_each(|character| input.push(character));
        let shown: zeroize::Zeroizing<String> = input.display(true);
        assert_eq!(shown.as_str(), "abc");
        let _: zeroize::Zeroizing<String> = input.display(false);
        assert_eq!(input.display(false).as_str(), "***");
    }

    #[test]
    fn ctrl_r_reveals_both_password_fields_and_strength_is_hinted() {
        let mut app = root_app();
        open_menu_password_form(&mut app);
        let screen = rendered(&app);
        assert!(screen.contains("Ctrl+R reveal"));
        assert!(!screen.contains("Strength:"));
        type_text(&mut app, "abc");
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "abd");
        let screen = rendered(&app);
        assert!(screen.contains("New password: ***"));
        assert!(screen.contains("Confirm: ***▏"));
        assert!(screen.contains("Strength: weak"));
        assert!(!screen.contains("abc"));

        ctrl(&mut app, 'r');
        assert_eq!(password_counts(&app), (3, 3), "Ctrl+R is not typed");
        let screen = rendered(&app);
        assert!(screen.contains("New password: abc"));
        assert!(screen.contains("Confirm: abd▏"));

        ctrl(&mut app, 'r');
        assert!(!rendered(&app).contains("abc"));

        // The create-user form toggles the same way.
        app.close_modals();
        open_user_add(&mut app);
        press(&mut app, KeyCode::Down);
        type_text(&mut app, "Tr0ub4dor&33");
        ctrl(&mut app, 'r');
        let screen = rendered(&app);
        assert!(screen.contains("Password: Tr0ub4dor&33▏"));
        assert!(screen.contains("Strength: strong"));
    }

    fn open_menu_password_form(app: &mut AppState) {
        open_modal(app, ModalState::ModifyPasswordMenu { selected: 0 });
        press(app, KeyCode::Enter);
        assert!(matches!(
            app.modal(),
            Some(ModalState::ChangePassword { reveal: false, .. })
        ));
    }

    #[test]
    fn password_strength_hint_levels() {
        let strength = |text: &str| {
            let mut input = SecretInput::default();
            text.chars().for_each(|character| input.push(character));
            input.strength()
        };
        assert_eq!(strength("abc1!"), "weak");
        assert_eq!(strength("abcdefgh"), "ok");
        assert_eq!(strength("abcdefgh1234"), "ok");
        assert_eq!(strength("abcdefgh12!"), "ok");
        assert_eq!(strength("abcdefgh12!?"), "strong");
    }

    fn ctrl_c() -> Event {
        Event::Key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL))
    }

    #[test]
    fn ctrl_c_quits_from_a_dialog_and_from_search() {
        let mut app = user_app();
        open_modal(
            &mut app,
            ModalState::GroupAddInput {
                name: String::new(),
                error: None,
            },
        );
        assert!(handle_event(&mut app, ctrl_c()));
        assert_eq!(text_value(&app), Some(""), "Ctrl+C is not typed");

        let mut app = user_app();
        press(&mut app, KeyCode::Char('/'));
        assert_eq!(app.input_mode, InputMode::SearchUsers);
        assert!(handle_event(&mut app, ctrl_c()));
        assert_eq!(app.users_query, "");
    }

    #[test]
    fn ctrl_c_drops_a_prepared_operation_before_quitting() {
        let mut app = user_app();
        start_pending_action(
            &mut app,
            PendingAction::ChangeShell {
                username: "alice".to_owned(),
                new_shell: "/bin/bash".to_owned(),
            },
        );
        assert!(app.pending_operation.is_some());
        assert!(handle_event(&mut app, ctrl_c()));
        assert!(app.pending_operation.is_none());
    }

    #[test]
    fn ctrl_and_alt_chords_are_not_typed_into_search_or_fields() {
        let mut app = user_app();
        press(&mut app, KeyCode::Char('/'));
        ctrl(&mut app, 'x');
        handle_key(
            &mut app,
            KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT),
        );
        assert_eq!(app.users_query, "");
        press(&mut app, KeyCode::Char('x'));
        assert_eq!(app.users_query, "x");

        let mut app = user_app();
        open_modal(
            &mut app,
            ModalState::GroupAddInput {
                name: String::new(),
                error: None,
            },
        );
        ctrl(&mut app, 'x');
        assert_eq!(text_value(&app), Some(""));
    }

    #[test]
    fn ctrl_r_still_reveals_passwords_and_other_chords_are_not_typed() {
        let mut app = root_app();
        open_menu_password_form(&mut app);
        ctrl(&mut app, 'x');
        assert_eq!(password_counts(&app), (0, 0));
        ctrl(&mut app, 'r');
        assert!(matches!(
            app.modal(),
            Some(ModalState::ChangePassword { reveal: true, .. })
        ));
        assert_eq!(password_counts(&app), (0, 0));
    }

    /// Users and groups whose UID/GID, name, shell, home and member-count
    /// orders all differ; names mix upper and lower case.
    fn sortable_app() -> AppState {
        let user = |uid: u32, name: &str, home: &str, shell: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(100),
            full_name: None,
            home_dir: home.into(),
            shell: ShellPath::new(shell).unwrap(),
        };
        let group = |gid: u32, name: &str, members: &[&str]| AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: members
                .iter()
                .map(|member| UserName::new(member).unwrap())
                .collect(),
        };
        let snapshot = AccountSnapshot {
            users: vec![
                user(1002, "Bob", "/srv/bob", "/bin/sh"),
                user(1000, "carol", "/home/carol", "/bin/bash"),
                user(1001, "alice", "/home/alice", "/bin/zsh"),
                user(1003, "dave", "/opt/dave", "/bin/bash"),
            ],
            groups: vec![
                group(30, "audio", &["alice"]),
                group(10, "Wheel", &["alice", "Bob"]),
                group(20, "dev", &["carol"]),
                group(40, "empty", &[]),
            ],
            shells: vec![],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    fn group_names(app: &AppState) -> Vec<&str> {
        app.groups.iter().map(|group| group.name.as_str()).collect()
    }

    fn select_user(app: &mut AppState, name: &str) {
        app.selected_user_index = app.users.iter().position(|user| user.name == name).unwrap();
        app.capture_selection_identities();
    }

    #[test]
    fn s_sorts_users_by_name_and_keeps_the_selected_user() {
        let mut app = sortable_app();
        assert_eq!(user_names(&app), ["carol", "alice", "Bob", "dave"]);
        select_user(&mut app, "Bob");
        assert!(!ui::users::users_table_title(&app).contains("sort:"));

        press(&mut app, KeyCode::Char('s'));
        assert_eq!(user_names(&app), ["alice", "Bob", "carol", "dave"]);
        assert_eq!(app.users[app.selected_user_index].name, "Bob");
        assert_eq!(app.selected_user_uid, Some(1002));
        assert_eq!(
            ui::users::users_table_title(&app),
            "[Users]  4 of 4  sort: name"
        );
        assert!(rendered_at(&app, 120, 40).contains("sort: name"));
    }

    #[test]
    fn users_sort_cycles_through_shell_home_and_back_to_uid() {
        let mut app = sortable_app();
        select_user(&mut app, "dave");
        press(&mut app, KeyCode::Char('s'));
        press(&mut app, KeyCode::Char('s'));
        // Equal shells keep UID order: carol (1000) before dave (1003).
        assert_eq!(user_names(&app), ["carol", "dave", "Bob", "alice"]);
        assert!(ui::users::users_table_title(&app).ends_with("sort: shell"));
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(user_names(&app), ["alice", "carol", "dave", "Bob"]);
        assert!(ui::users::users_table_title(&app).ends_with("sort: home"));
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(user_names(&app), ["carol", "alice", "Bob", "dave"]);
        assert!(!ui::users::users_table_title(&app).contains("sort:"));
        assert_eq!(app.users[app.selected_user_index].name, "dave");
    }

    #[test]
    fn sort_label_sits_between_the_count_and_the_filter_chips() {
        let mut app = sortable_app();
        app.users_query = "a".to_owned();
        app.set_uid_range_filters(true, false);
        app.sort_and_filter();
        press(&mut app, KeyCode::Char('s'));
        assert_eq!(
            ui::users::users_table_title(&app),
            "[Users]  /a  3 of 4  sort: name  [human]"
        );
        assert_eq!(user_names(&app), ["alice", "carol", "dave"]);
    }

    #[test]
    fn groups_sort_by_name_then_member_count_per_tab() {
        let mut app = sortable_app();
        app.active_tab = ActiveTab::Groups;
        assert_eq!(group_names(&app), ["Wheel", "dev", "audio", "empty"]);
        app.selected_group_index = 2;
        app.capture_selection_identities();

        press(&mut app, KeyCode::Char('s'));
        assert_eq!(group_names(&app), ["audio", "dev", "empty", "Wheel"]);
        assert_eq!(app.groups[app.selected_group_index].name, "audio");
        assert!(ui::groups::groups_table_title(&app).ends_with("  sort: name"));

        press(&mut app, KeyCode::Char('s'));
        // Most members first; equal counts keep GID order (dev 20, audio 30).
        assert_eq!(group_names(&app), ["Wheel", "dev", "audio", "empty"]);
        assert!(ui::groups::groups_table_title(&app).ends_with("  sort: members"));
        assert_eq!(app.groups[app.selected_group_index].name, "audio");

        // The Users tab kept its own default order.
        assert_eq!(app.users_sort, crate::app::UsersSort::Uid);
        assert_eq!(user_names(&app), ["carol", "alice", "Bob", "dave"]);

        press(&mut app, KeyCode::Char('s'));
        assert_eq!(group_names(&app), ["Wheel", "dev", "audio", "empty"]);
        assert!(!ui::groups::groups_table_title(&app).contains("sort:"));
    }

    #[test]
    fn keybinds_panel_and_help_list_the_sort_key() {
        let mut app = sortable_app();
        app.show_keybinds = true;
        let panel: Vec<String> = ui::components::keybinds_panel_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(panel.contains(&"Sort: s".to_owned()), "{panel:?}");
        let help: Vec<String> = ui::components::help_lines(&app)
            .iter()
            .map(|line| line.to_string())
            .collect();
        assert!(
            help.iter()
                .any(|line| line.starts_with("Sort the active tab: s (")),
            "{help:?}"
        );
    }

    /// carol and alice share UID 1000; staff and admins share GID 50. Rows
    /// sharing an ID are ordered by name in the default (ID) order.
    fn duplicate_id_app() -> AppState {
        let user = |uid: u32, name: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(50),
            full_name: None,
            home_dir: format!("/home/{name}").into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        };
        let group = |gid: u32, name: &str| AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: vec![],
        };
        let snapshot = AccountSnapshot {
            users: vec![user(1000, "carol"), user(1000, "alice"), user(1001, "bob")],
            groups: vec![group(50, "staff"), group(50, "admins"), group(10, "wheel")],
            shells: vec![],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        isolated_app(adapter, snapshot)
    }

    #[test]
    fn sorting_keeps_the_selected_user_when_another_user_shares_its_uid() {
        let mut app = duplicate_id_app();
        assert_eq!(user_names(&app), ["alice", "carol", "bob"]);
        select_user(&mut app, "carol");

        press(&mut app, KeyCode::Char('s'));
        assert_eq!(user_names(&app), ["alice", "bob", "carol"]);
        assert_eq!(app.users[app.selected_user_index].name, "carol");
        assert_eq!(app.selected_user_uid, Some(1000));
        assert_eq!(app.selected_user_name.as_deref(), Some("carol"));
    }

    #[test]
    fn sorting_keeps_the_selected_group_when_another_group_shares_its_gid() {
        let mut app = duplicate_id_app();
        select_user(&mut app, "carol");
        let user_group_names = |app: &AppState| -> Vec<String> {
            app.selected_user_groups()
                .iter()
                .map(|group| group.name.clone())
                .collect()
        };
        assert_eq!(user_group_names(&app), ["admins", "staff"]);
        app.selected_user_group_index = 1;
        app.active_tab = ActiveTab::Groups;
        assert_eq!(group_names(&app), ["wheel", "admins", "staff"]);
        app.selected_group_index = 2;
        app.capture_selection_identities();

        press(&mut app, KeyCode::Char('s'));
        assert_eq!(group_names(&app), ["admins", "staff", "wheel"]);
        assert_eq!(app.groups[app.selected_group_index].name, "staff");
        assert_eq!(app.selected_group_gid, Some(50));
        assert_eq!(app.selected_group_name.as_deref(), Some("staff"));
        // The Users-tab membership pane follows the groups order and keeps
        // its own selection too.
        assert_eq!(user_group_names(&app), ["admins", "staff"]);
        assert_eq!(
            app.selected_user_groups()[app.selected_user_group_index].name,
            "staff"
        );
    }

    #[test]
    fn selection_falls_back_to_the_id_when_the_name_is_gone() {
        let mut app = duplicate_id_app();
        select_user(&mut app, "alice");
        app.users_all
            .iter_mut()
            .find(|user| user.name == "alice")
            .unwrap()
            .name = "zoe".to_owned();
        app.sort_and_filter();
        // The exact pair is gone; the first row with UID 1000 is chosen.
        assert_eq!(app.users[app.selected_user_index].name, "carol");
    }

    #[test]
    fn membership_cache_is_built_in_id_order_whatever_the_sort() {
        let mut app = sortable_app();
        app.membership_cache_bounds = crate::app::MembershipCacheBounds {
            max_users: 2,
            max_groups: 2,
            max_membership_edges: 2,
        };
        app.sort_and_filter();
        let canonical = app.user_group_gids.clone();
        // UIDs 1000 and 1001 get their primary group; Wheel (GID 10) uses up
        // both membership edges.
        let expected: std::collections::HashMap<String, std::collections::HashSet<u32>> = [
            ("carol".to_owned(), [100].into()),
            ("alice".to_owned(), [100, 10].into()),
            ("Bob".to_owned(), [10].into()),
        ]
        .into();
        assert_eq!(canonical, expected);

        for tab in [ActiveTab::Users, ActiveTab::Groups] {
            app.active_tab = tab;
            for _ in 0..4 {
                press(&mut app, KeyCode::Char('s'));
                assert_eq!(
                    app.user_group_gids, canonical,
                    "users sort {:?}, groups sort {:?}",
                    app.users_sort, app.groups_sort
                );
            }
        }
    }

    /// App with the given users and groups and small membership-cache bounds.
    fn bounded_cache_app(
        users: Vec<AccountUser>,
        groups: Vec<AccountGroup>,
        bounds: crate::app::MembershipCacheBounds,
    ) -> AppState {
        let snapshot = AccountSnapshot {
            users,
            groups,
            shells: vec![],
            diagnostics: vec![],
        };
        let adapter = Arc::new(SystemAdapter::from_components(
            Arc::new(StaticSource(snapshot.clone())),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(1000))),
        ));
        let mut app = isolated_app(adapter, snapshot);
        app.membership_cache_bounds = bounds;
        app.sort_and_filter();
        app
    }

    /// Cycle the sort of both tabs through every mode and back, rebuilding
    /// the cache at each step, and run `check` after each one.
    fn cycle_every_sort(app: &mut AppState, check: impl Fn(&AppState)) {
        check(app);
        for (tab, modes) in [(ActiveTab::Users, 4), (ActiveTab::Groups, 3)] {
            app.active_tab = tab;
            for _ in 0..modes {
                press(app, KeyCode::Char('s'));
                check(app);
                app.sort_and_filter();
                check(app);
            }
        }
        assert_eq!(app.users_sort, crate::app::UsersSort::Uid);
        assert_eq!(app.groups_sort, crate::app::GroupsSort::Gid);
    }

    #[test]
    fn membership_cache_ignores_sort_history_for_users_sharing_a_uid() {
        let user = |uid: u32, name: &str, gid: u32, home: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(gid),
            full_name: None,
            home_dir: home.into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        };
        // dave fills the first slot; carol and alice share the UID of the
        // last slot, so only one of them fits. carol comes first in the
        // snapshot and in home order, alice first by name.
        let mut app = bounded_cache_app(
            vec![
                user(1000, "dave", 100, "/home/dave"),
                user(1001, "carol", 200, "/home/carol"),
                user(1001, "alice", 300, "/srv/alice"),
            ],
            vec![],
            crate::app::MembershipCacheBounds {
                max_users: 2,
                max_groups: 10,
                max_membership_edges: 10,
            },
        );
        let alice = app.user_group_gids.get("alice").cloned();
        let carol = app.user_group_gids.get("carol").cloned();
        assert_eq!(alice, Some([300].into()));
        assert_eq!(carol, None);
        cycle_every_sort(&mut app, |app| {
            let step = (app.users_sort, app.groups_sort);
            assert_eq!(app.user_group_gids.get("alice"), alice.as_ref(), "{step:?}");
            assert_eq!(app.user_group_gids.get("carol"), carol.as_ref(), "{step:?}");
        });
    }

    #[test]
    fn membership_cache_ignores_sort_history_for_groups_sharing_a_gid() {
        let user = |uid: u32, name: &str| AccountUser {
            uid: Uid(uid),
            name: UserName::new(name).unwrap(),
            primary_gid: Gid(100),
            full_name: None,
            home_dir: format!("/home/{name}").into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        };
        let group = |gid: u32, name: &str, members: &[&str]| AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: members
                .iter()
                .map(|member| UserName::new(member).unwrap())
                .collect(),
        };
        // wheel fills the first slot; staff and admins share the GID of the
        // last slot, so only one of them fits. staff comes first in the
        // snapshot and by member count, admins first by name.
        let mut app = bounded_cache_app(
            vec![user(1000, "erin"), user(1001, "frank")],
            vec![
                group(10, "wheel", &[]),
                group(50, "staff", &["erin", "frank"]),
                group(50, "admins", &["frank"]),
            ],
            crate::app::MembershipCacheBounds {
                max_users: 10,
                max_groups: 2,
                max_membership_edges: 10,
            },
        );
        let erin = app.user_group_gids.get("erin").cloned();
        let frank = app.user_group_gids.get("frank").cloned();
        // admins (GID 50, first by name) takes the last slot; staff is cut,
        // so erin keeps only her primary group.
        assert_eq!(erin, Some([100].into()));
        assert_eq!(frank, Some([100, 50].into()));
        cycle_every_sort(&mut app, |app| {
            let step = (app.users_sort, app.groups_sort);
            assert_eq!(app.user_group_gids.get("erin"), erin.as_ref(), "{step:?}");
            assert_eq!(app.user_group_gids.get("frank"), frank.as_ref(), "{step:?}");
        });
    }
}
