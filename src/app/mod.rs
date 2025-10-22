//! Application state types and entry glue.
//!
//! Defines enums and structs that model the TUI state, as well as helpers
//! to construct defaults and to run the application loop (re-exported as `run`).
//!
pub mod filterconf;
pub mod keymap;
pub mod update;

use ratatui::style::Color;
use ratatui::widgets::TableState;
use std::time::Instant;

use crate::sys;
use std::path::PathBuf;

/// Top-level active tab in the UI.
///
/// Determines which major view the user sees: either the Users tab or the Groups tab.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActiveTab {
    /// Displays the users list and related management options.
    Users,
    /// Displays the groups list and related management options.
    Groups,
}

/// Which subsection is focused on the Users screen.
///
/// Used to implement the two-pane Users screen: the main Users list or the Member-of pane
/// showing which groups the selected user is a member of.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsersFocus {
    /// Focus is on the main users table.
    UsersList,
    /// Focus is on the "Member of" pane showing group memberships.
    MemberOf,
}

/// Which subsection is focused on the Groups screen.
///
/// Similar to [`UsersFocus`], allows toggling between the groups table and the members list.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GroupsFocus {
    /// Focus is on the main groups table.
    GroupsList,
    /// Focus is on the members list for the selected group.
    Members,
}

/// Current input mode for key handling.
///
/// Determines which keyboard shortcuts are active and how input is interpreted.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    /// Normal navigation mode; keybindings for movement and actions are active.
    Normal,
    /// User is typing in the search box for the Users tab.
    SearchUsers,
    /// User is typing in the search box for the Groups tab.
    SearchGroups,
    /// A modal dialog is open; only modal-specific keybindings are active.
    Modal,
}

/// Color palette for theming the TUI.
///
/// Defines the visual appearance of the TUI, including text, borders, headers, and highlights.
/// Can be loaded from a config file or use built-in defaults.
#[derive(Clone, Copy, Debug)]
pub struct Theme {
    /// Primary text color.
    pub text: Color,
    /// Secondary/muted text color.
    pub _muted: Color,
    /// Color for titles and headings.
    pub title: Color,
    /// Color for borders and separators.
    pub border: Color,
    /// Background color for headers.
    pub header_bg: Color,
    /// Foreground (text) color for headers.
    pub header_fg: Color,
    /// Background color for the status bar.
    pub status_bg: Color,
    /// Foreground (text) color for the status bar.
    pub status_fg: Color,
    /// Foreground color for highlighted/selected items.
    pub highlight_fg: Color,
    /// Background color for highlighted/selected items.
    pub highlight_bg: Color,
}

impl Theme {
    /// Dark default theme with neutral grays and cyan accents.
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

    /// Catppuccin Mocha theme defaults with warm, sophisticated colors.
    pub fn mocha() -> Self {
        // Palette reference: https://github.com/catppuccin/catppuccin
        Self {
            // text & neutrals
            text: Color::Rgb(0xcd, 0xd6, 0xf4),   // text
            _muted: Color::Rgb(0x7f, 0x84, 0x9c), // overlay1
            // accents and chrome
            title: Color::Rgb(0xcb, 0xa6, 0xf7),        // mauve
            border: Color::Rgb(0x58, 0x5b, 0x70),       // surface2
            header_bg: Color::Rgb(0x31, 0x32, 0x44),    // surface0
            header_fg: Color::Rgb(0xb4, 0xbe, 0xfe),    // lavender
            status_bg: Color::Rgb(0x45, 0x47, 0x5a),    // surface1
            status_fg: Color::Rgb(0xcd, 0xd6, 0xf4),    // text
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
        let hex = if let Some(h) = lower.strip_prefix('#') {
            h
        } else {
            lower.as_str()
        };
        if hex.len() == 6
            && let (Ok(r), Ok(g), Ok(b)) = (
                u8::from_str_radix(&hex[0..2], 16),
                u8::from_str_radix(&hex[2..4], 16),
                u8::from_str_radix(&hex[4..6], 16),
            )
        {
            return Some(Color::Rgb(r, g, b));
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
    Help {
        scroll: u16,
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
        target_gid: Option<u32>,
    },
    ConfirmRemoveUserFromGroup {
        selected: usize,
        group_name: String,
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

/// Combinable filter chips for users that refine the list further.
///
/// Unlike top-level filters, multiple chips can be enabled simultaneously.
#[derive(Clone, Debug, Default)]
pub struct UsersFilterChips {
    /// Show only users with UID >= 1000 (opposite of system_only).
    pub human_only: bool,
    /// Show only users with UID < 1000 (opposite of human_only).
    pub system_only: bool,
    /// Show only users whose shell ends with "nologin" or "/false" (inactive accounts).
    pub inactive: bool,
    /// Show only users whose home directory does not exist.
    pub no_home: bool,
    /// Show only users with locked passwords (read from `/etc/shadow`).
    pub locked: bool,
    /// Show only users with no password set (empty field in `/etc/shadow`).
    pub no_password: bool,
    /// Show only users whose password has expired.
    pub expired: bool,
}

/// Filter types for narrowing the users list.
///
/// Allows showing only system users (UID < 1000) or only regular users (UID >= 1000).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsersFilter {
    /// Show only regular (non-system) users with UID >= 1000.
    OnlyUserIds,
    /// Show only system users with UID < 1000.
    OnlySystemIds,
}

/// Filter types for narrowing the groups list.
///
/// Allows showing only system groups (GID < 1000) or only regular groups (GID >= 1000).
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GroupsFilter {
    /// Show only regular (non-system) groups with GID >= 1000.
    OnlyUserGids,
    /// Show only system groups with GID < 1000.
    OnlySystemGids,
}

#[derive(Clone, Debug)]
pub enum ActionsContext {
    GroupMemberRemoval { group_name: String },
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
    pub selected_group_member_index: usize,
    pub rows_per_page: usize,
    pub _table_state: TableState,
    pub input_mode: InputMode,
    pub search_query: String,
    pub theme: Theme,
    pub keymap: keymap::Keymap,
    pub modal: Option<ModalState>,
    pub users_focus: UsersFocus,
    pub groups_focus: GroupsFocus,
    pub sudo_password: Option<String>,
    pub users_filter: Option<UsersFilter>,
    pub groups_filter: Option<GroupsFilter>,
    pub users_filter_chips: UsersFilterChips,
    pub actions_context: Option<ActionsContext>,
    pub show_keybinds: bool,
}

impl AppState {
    /// Create a new `AppState` by reading users/groups from the system.
    pub fn new() -> Self {
        let adapter = crate::sys::SystemAdapter::new();
        let mut users_all = adapter.list_users().unwrap_or_default();
        users_all.sort_by_key(|u| u.uid);
        let mut groups_all = adapter.list_groups().unwrap_or_default();
        groups_all.sort_by_key(|g| g.gid);
        let mut app = Self {
            started_at: Instant::now(),
            users: users_all.clone(),
            users_all,
            groups: groups_all.clone(),
            groups_all,
            active_tab: ActiveTab::Users,
            selected_user_index: 0,
            selected_group_index: 0,
            selected_group_member_index: 0,
            rows_per_page: 10,
            _table_state: TableState::default(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            theme: Theme::load_or_init(
                &config_file_read_path("theme.conf")
                    .unwrap_or_else(|| config_file_write_path("theme.conf")),
            ),
            keymap: keymap::Keymap::load_or_init(
                &config_file_read_path("keybinds.conf")
                    .unwrap_or_else(|| config_file_write_path("keybinds.conf")),
            ),
            modal: None,
            users_focus: UsersFocus::UsersList,
            groups_focus: GroupsFocus::GroupsList,
            sudo_password: None,
            users_filter: None,
            groups_filter: None,
            users_filter_chips: UsersFilterChips::default(),
            actions_context: None,
            show_keybinds: true,
        };

        // Load and apply filter configuration from filter.conf (creates default if missing/empty)
        let filters_cfg = filterconf::FiltersConfig::load_or_init(
            &config_file_read_path("filter.conf")
                .unwrap_or_else(|| config_file_write_path("filter.conf")),
        );
        filters_cfg.apply_to(&mut app);

        // Apply the loaded filters to seed the initial views
        crate::search::apply_filters_and_search(&mut app);

        app
    }
}

/// Candidate roots in priority order for config files.
fn config_roots() -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        let mut p = PathBuf::from(xdg);
        p.push("UsrGrpManager");
        roots.push(p);
    }
    if let Some(home) = dirs_next::home_dir() {
        let mut p = home.clone();
        p.push(".config");
        p.push("UsrGrpManager");
        roots.push(p);
    }
    if let Some(home) = dirs_next::home_dir() {
        let mut p = home.clone();
        p.push("UsrGrpManager");
        roots.push(p);
    }
    roots
}

/// Resolve existing config file path (read) according to priority order.
pub fn config_file_read_path(name: &str) -> Option<String> {
    for root in config_roots() {
        let mut p = root.clone();
        p.push(name);
        if p.exists() {
            return Some(p.to_string_lossy().to_string());
        }
    }
    None
}

/// Resolve a path for writing a config file; ensures the directory exists.
pub fn config_file_write_path(name: &str) -> String {
    if let Some(root) = config_roots().into_iter().next() {
        let _ = std::fs::create_dir_all(&root);
        let mut p = root.clone();
        p.push(name);
        return p.to_string_lossy().to_string();
    }
    name.to_string()
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Re-export the application event loop entry function.
pub use update::run_app as run;

/// Resolve the sudo group name from environment, defaulting to "wheel".
pub fn sudo_group_name() -> String {
    std::env::var("UGM_SUDO_GROUP").unwrap_or_else(|_| "wheel".to_string())
}
