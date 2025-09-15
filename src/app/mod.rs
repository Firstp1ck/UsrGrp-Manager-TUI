pub mod update;

use ratatui::style::Color;
use ratatui::widgets::TableState;
use std::time::Instant;

use crate::sys;

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActiveTab {
    Users,
    Groups,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsersFocus {
    UsersList,
    MemberOf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchUsers,
    SearchGroups,
    Modal,
}

#[derive(Clone, Copy, Debug)]
pub struct Theme {
    pub text: Color,
    pub _muted: Color,
    pub title: Color,
    pub border: Color,
    pub header_bg: Color,
    pub header_fg: Color,
    pub status_bg: Color,
    pub status_fg: Color,
    pub highlight_fg: Color,
    pub highlight_bg: Color,
}

impl Theme {
    pub fn dark() -> Self {
        Self {
            text: Color::Gray,
            _muted: Color::DarkGray,
            title: Color::Cyan,
            border: Color::Gray,
            header_bg: Color::Black,
            header_fg: Color::Cyan,
            status_bg: Color::DarkGray,
            status_fg: Color::Black,
            highlight_fg: Color::Yellow,
            highlight_bg: Color::Reset,
        }
    }
}

#[derive(Clone, Debug)]
pub enum ModalState {
    Actions {
        selected: usize,
    },
    ModifyMenu {
        selected: usize,
    },
    ModifyGroupsAdd {
        selected: usize,
        offset: usize,
        selected_multi: Vec<usize>,
    },
    ModifyGroupsRemove {
        selected: usize,
        offset: usize,
        selected_multi: Vec<usize>,
    },
    ModifyDetailsMenu {
        selected: usize,
    },
    ModifyShell {
        selected: usize,
        offset: usize,
        shells: Vec<String>,
    },
    ModifyTextInput {
        field: ModifyField,
        value: String,
    },
    DeleteConfirm {
        selected: usize,
        allowed: bool,
        delete_home: bool,
    },
    ModifyPasswordMenu {
        selected: usize,
    },
    ChangePassword {
        selected: usize,
        password: String,
        confirm: String,
        must_change: bool,
    },
    Info {
        message: String,
    },
    SudoPrompt {
        next: PendingAction,
        password: String,
        error: Option<String>,
    },
    GroupsActions {
        selected: usize,
        target_gid: Option<u32>,
    },
    GroupAddInput {
        name: String,
    },
    GroupDeleteConfirm {
        selected: usize,
    },
    GroupModifyMenu {
        selected: usize,
        target_gid: Option<u32>,
    },
    GroupModifyAddMembers {
        selected: usize,
        offset: usize,
        target_gid: Option<u32>,
        selected_multi: Vec<usize>,
    },
    GroupModifyRemoveMembers {
        selected: usize,
        offset: usize,
        target_gid: Option<u32>,
        selected_multi: Vec<usize>,
    },
    GroupRenameInput {
        name: String,
        target_gid: Option<u32>,
    },
    UserAddInput {
        selected: usize,
        name: String,
        password: String,
        confirm: String,
        create_home: bool,
        add_to_wheel: bool,
    },
}

#[derive(Clone, Debug)]
pub enum ModifyField {
    Username,
    Fullname,
}

#[derive(Clone, Debug)]
pub enum PendingAction {
    AddUserToGroup {
        username: String,
        groupname: String,
    },
    RemoveUserFromGroup {
        username: String,
        groupname: String,
    },
    AddUserToGroups {
        username: String,
        groupnames: Vec<String>,
    },
    RemoveUserFromGroups {
        username: String,
        groupnames: Vec<String>,
    },
    AddMembersToGroup {
        groupname: String,
        usernames: Vec<String>,
    },
    RemoveMembersFromGroup {
        groupname: String,
        usernames: Vec<String>,
    },
    ChangeShell {
        username: String,
        new_shell: String,
    },
    ChangeFullname {
        username: String,
        new_fullname: String,
    },
    ChangeUsername {
        old_username: String,
        new_username: String,
    },
    CreateGroup {
        groupname: String,
    },
    DeleteGroup {
        groupname: String,
    },
    RenameGroup {
        old_name: String,
        new_name: String,
    },

    CreateUserWithOptions {
        username: String,
        password: Option<String>,
        create_home: bool,
        add_to_wheel: bool,
    },
    DeleteUser {
        username: String,
        delete_home: bool,
    },
    SetPassword {
        username: String,
        password: String,
        must_change: bool,
    },
    ResetPassword {
        username: String,
    },
}

pub struct AppState {
    pub started_at: Instant,
    pub users_all: Vec<sys::SystemUser>,
    pub users: Vec<sys::SystemUser>,
    pub groups_all: Vec<sys::SystemGroup>,
    pub groups: Vec<sys::SystemGroup>,
    pub active_tab: ActiveTab,
    pub selected_user_index: usize,
    pub selected_group_index: usize,
    pub rows_per_page: usize,
    pub _table_state: TableState,
    pub input_mode: InputMode,
    pub search_query: String,
    pub theme: Theme,
    pub modal: Option<ModalState>,
    pub users_focus: UsersFocus,
    pub sudo_password: Option<String>,
}

impl AppState {
    pub fn new() -> Self {
        let adapter = crate::sys::SystemAdapter::new();
        let mut users_all = adapter.list_users().unwrap_or_default();
        users_all.sort_by_key(|u| u.uid);
        let mut groups_all = adapter.list_groups().unwrap_or_default();
        groups_all.sort_by_key(|g| g.gid);
        Self {
            started_at: Instant::now(),
            users: users_all.clone(),
            users_all,
            groups: groups_all.clone(),
            groups_all,
            active_tab: ActiveTab::Users,
            selected_user_index: 0,
            selected_group_index: 0,
            rows_per_page: 10,
            _table_state: TableState::default(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            theme: Theme::dark(),
            modal: None,
            users_focus: UsersFocus::UsersList,
            sudo_password: None,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

pub use update::run_app as run;
