//! Application state types and entry glue.
//!
//! Defines enums and structs that model the TUI state, as well as helpers
//! to construct defaults and to run the application loop (re-exported as `run`).
//!
pub mod update;

use ratatui::style::Color;
use ratatui::widgets::TableState;
use std::time::Instant;

use crate::sys;

/// Top-level active tab in the UI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActiveTab {
    Users,
    Groups,
}

/// Which subsection is focused on the Users screen.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsersFocus {
    UsersList,
    MemberOf,
}

/// Current input mode for key handling.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchUsers,
    SearchGroups,
    Modal,
}

/// Color palette for theming the TUI.
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
    /// Dark default theme.
    #[allow(dead_code)]
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

    /// Catppuccin Mocha theme defaults.
    pub fn mocha() -> Self {
        // Palette reference: https://github.com/catppuccin/catppuccin
        Self {
            // text & neutrals
            text: Color::Rgb(0xcd, 0xd6, 0xf4),      // text
            _muted: Color::Rgb(0x7f, 0x84, 0x9c),    // overlay1
            // accents and chrome
            title: Color::Rgb(0xcb, 0xa6, 0xf7),     // mauve
            border: Color::Rgb(0x58, 0x5b, 0x70),    // surface2
            header_bg: Color::Rgb(0x31, 0x32, 0x44), // surface0
            header_fg: Color::Rgb(0xb4, 0xbe, 0xfe), // lavender
            status_bg: Color::Rgb(0x45, 0x47, 0x5a), // surface1
            status_fg: Color::Rgb(0xcd, 0xd6, 0xf4), // text
            highlight_fg: Color::Rgb(0xf9, 0xe2, 0xaf), // yellow
            highlight_bg: Color::Rgb(0x45, 0x47, 0x5a), // surface1
        }
    }

    /// Load theme from a simple key=value file. Unknown or missing keys fall back to `mocha`.
    pub fn from_file(path: &str) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        let mut theme = Self::mocha();

        for raw_line in contents.lines() {
            let line = raw_line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(2, '=');
            let key = parts.next().map(|s| s.trim()).unwrap_or("");
            let val = parts.next().map(|s| s.trim()).unwrap_or("");
            if key.is_empty() || val.is_empty() {
                continue;
            }
            if let Some(color) = Self::parse_color(val) {
                match key {
                    "text" => theme.text = color,
                    "muted" | "_muted" => theme._muted = color,
                    "title" => theme.title = color,
                    "border" => theme.border = color,
                    "header_bg" => theme.header_bg = color,
                    "header_fg" => theme.header_fg = color,
                    "status_bg" => theme.status_bg = color,
                    "status_fg" => theme.status_fg = color,
                    "highlight_fg" => theme.highlight_fg = color,
                    "highlight_bg" => theme.highlight_bg = color,
                    _ => {}
                }
            }
        }

        Some(theme)
    }

    /// Parse a color from hex ("#RRGGBB" or "RRGGBB") or special names: "reset".
    fn parse_color(s: &str) -> Option<Color> {
        let t = s.trim();
        let lower = t.to_ascii_lowercase();
        if lower == "reset" {
            return Some(Color::Reset);
        }
        let hex = if let Some(h) = lower.strip_prefix('#') { h } else { lower.as_str() };
        if hex.len() == 6 {
            if let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            ) {
                return Some(Color::Rgb(r, g, b));
            }
        }
        None
    }

    /// Persist the theme to a config file in key=value format.
    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        use std::fmt::Write as _;
        let mut buf = String::new();
        // Minimal header
        buf.push_str("# usrgrp-manager theme configuration\n");
        buf.push_str("# Colors: hex as #RRGGBB or RRGGBB, or 'reset'\n\n");

        fn color_to_str(c: Color) -> String {
            match c {
                Color::Rgb(r, g, b) => format!("#{:02X}{:02X}{:02X}", r, g, b),
                Color::Reset => "reset".to_string(),
                // For named colors, emit a best-effort hex approximation
                Color::Black => "#000000".to_string(),
                Color::Red => "#FF0000".to_string(),
                Color::Green => "#00FF00".to_string(),
                Color::Yellow => "#FFFF00".to_string(),
                Color::Blue => "#0000FF".to_string(),
                Color::Magenta => "#FF00FF".to_string(),
                Color::Cyan => "#00FFFF".to_string(),
                Color::Gray => "#B3B3B3".to_string(),
                Color::DarkGray => "#4D4D4D".to_string(),
                Color::LightRed => "#FF6666".to_string(),
                Color::LightGreen => "#66FF66".to_string(),
                Color::LightYellow => "#FFFF66".to_string(),
                Color::LightBlue => "#6666FF".to_string(),
                Color::LightMagenta => "#FF66FF".to_string(),
                Color::LightCyan => "#66FFFF".to_string(),
                Color::White => "#FFFFFF".to_string(),
                Color::Indexed(i) => format!("index:{}", i),
            }
        }

        let mut kv = |k: &str, v: Color| {
            let _ = writeln!(&mut buf, "{} = {}", k, color_to_str(v));
        };

        kv("text", self.text);
        kv("muted", self._muted);
        kv("title", self.title);
        kv("border", self.border);
        kv("header_bg", self.header_bg);
        kv("header_fg", self.header_fg);
        kv("status_bg", self.status_bg);
        kv("status_fg", self.status_fg);
        kv("highlight_fg", self.highlight_fg);
        kv("highlight_bg", self.highlight_bg);

        std::fs::write(path, buf)
    }

    /// Ensure a config file exists; if missing, write one with the current default theme and return it.
    /// If present, load from it; on parse errors, return `mocha`.
    pub fn load_or_init(path: &str) -> Self {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Self::from_file(path).unwrap_or_else(Self::mocha);
        }
        let t = Self::mocha();
        let _ = t.write_file(path);
        t
    }
}

/// Modal dialog states for user and group actions.
#[derive(Clone, Debug)]
pub enum ModalState {
    Actions {
        selected: usize,
    },
    FilterMenu {
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

/// Field selectors for text input dialogs.
#[derive(Clone, Debug)]
pub enum ModifyField {
    Username,
    Fullname,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UsersFilter {
    OnlyUserIds,   // uid >= 1000
    OnlySystemIds, // uid < 1000
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GroupsFilter {
    OnlyUserGids,   // gid >= 1000
    OnlySystemGids, // gid < 1000
}

/// Actions that require privileged changes, executed via `sys::SystemAdapter`.
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
    pub users_filter: Option<UsersFilter>,
    pub groups_filter: Option<GroupsFilter>,
}

impl AppState {
    /// Create a new `AppState` by reading users/groups from the system.
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
            theme: Theme::load_or_init("theme.conf"),
            modal: None,
            users_focus: UsersFocus::UsersList,
            sudo_password: None,
            users_filter: None,
            groups_filter: None,
        }
    }
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Re-export the application event loop entry function.
pub use update::run_app as run;
