//! Application state, explicit effects, and configuration entry points.
//!
//! Construction is pure by default.  Host reads, diagnostics, configuration
//! loading, and mutations are explicit effects owned by `update` rather than
//! rendering or tests.
pub mod filterconf;
pub mod keymap;
pub mod logger;
pub mod update;

pub use logger::Logger;

use std::{
    collections::{HashMap, HashSet},
    fs::File,
    io::Read,
    path::PathBuf,
    sync::Arc,
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use ratatui::{style::Color, widgets::TableState};

use crate::sys::{self, AccountSnapshot, OperationPlan, OperationReport, PasswordRecord};

/// Top-level active tab in the UI.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum ActiveTab {
    Users,
    Groups,
}

/// Focus within the users tab.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsersFocus {
    UsersList,
    MemberOf,
}

/// Focus within the groups tab.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GroupsFocus {
    GroupsList,
    Members,
}

/// Current input mode for key handling.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum InputMode {
    Normal,
    SearchUsers,
    SearchGroups,
    Modal,
}

/// Built-in base palette a theme file can select with `preset = mocha | dark`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemePreset {
    Mocha,
    Dark,
}

impl ThemePreset {
    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "mocha" => Some(Self::Mocha),
            "dark" => Some(Self::Dark),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Mocha => "mocha",
            Self::Dark => "dark",
        }
    }

    /// The preset's colors, with the preset recorded so it is written back.
    fn theme(self) -> Theme {
        let base = match self {
            Self::Mocha => Theme::mocha(),
            Self::Dark => Theme::dark(),
        };
        Theme {
            preset: Some(self),
            ..base
        }
    }
}

/// Theme keys that take a color. `_muted` is the old spelling of `muted`,
/// still read so existing files keep loading.
const THEME_COLOR_KEYS: [&str; 11] = [
    "text",
    "muted",
    "_muted",
    "title",
    "border",
    "header_bg",
    "header_fg",
    "status_bg",
    "status_fg",
    "highlight_fg",
    "highlight_bg",
];

/// Color palette for theming the TUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Theme {
    /// Base chosen by the file's `preset` key, if it had one. Only a theme
    /// read with a preset writes the key back.
    pub preset: Option<ThemePreset>,
    pub text: Color,
    pub muted: Color,
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
    /// Dark default theme with neutral grays and cyan accents.
    pub fn dark() -> Self {
        Self {
            preset: None,
            text: Color::Gray,
            muted: Color::DarkGray,
            title: Color::Cyan,
            border: Color::Gray,
            header_bg: Color::Black,
            header_fg: Color::Cyan,
            // Black, not DarkGray: muted status spans are DarkGray.
            status_bg: Color::Black,
            status_fg: Color::Gray,
            highlight_fg: Color::Yellow,
            highlight_bg: Color::Reset,
        }
    }

    /// Catppuccin Mocha theme defaults.
    pub fn mocha() -> Self {
        Self {
            preset: None,
            text: Color::Rgb(0xcd, 0xd6, 0xf4),
            muted: Color::Rgb(0x7f, 0x84, 0x9c),
            title: Color::Rgb(0xcb, 0xa6, 0xf7),
            border: Color::Rgb(0x58, 0x5b, 0x70),
            header_bg: Color::Rgb(0x31, 0x32, 0x44),
            header_fg: Color::Rgb(0xb4, 0xbe, 0xfe),
            status_bg: Color::Rgb(0x45, 0x47, 0x5a),
            status_fg: Color::Rgb(0xcd, 0xd6, 0xf4),
            highlight_fg: Color::Rgb(0xf9, 0xe2, 0xaf),
            highlight_bg: Color::Rgb(0x45, 0x47, 0x5a),
        }
    }

    /// Load a theme with line-oriented, lossless supported-color parsing.
    /// The base is Mocha, or the `preset` key's palette wherever that key
    /// appears in the file; every color key then overrides the base.
    pub fn from_file(path: &str) -> std::io::Result<Self> {
        let contents = crate::config::read_bounded(path)?;
        let mut preset = None;
        let mut colors = Vec::new();
        let mut seen = std::collections::HashSet::new();
        for assignment in crate::config::parse_assignments(&contents)? {
            let key = assignment.key;
            if !seen.insert(key.clone()) {
                return config_line_error(assignment.line, "duplicate theme key");
            }
            if key == "preset" {
                let Some(parsed) = ThemePreset::parse(&assignment.value) else {
                    return config_line_error(assignment.line, "invalid theme preset");
                };
                preset = Some(parsed);
                continue;
            }
            if !THEME_COLOR_KEYS.contains(&key.as_str()) {
                return config_line_error(assignment.line, "unknown theme key");
            }
            let Some(color) = Self::parse_color(&assignment.value) else {
                return config_line_error(assignment.line, "invalid theme color");
            };
            colors.push((key, color));
        }
        let mut theme = preset.map_or_else(Self::mocha, ThemePreset::theme);
        for (key, color) in colors {
            theme.set_color(&key, color);
        }
        Ok(theme)
    }

    /// Set the color named by one of [`THEME_COLOR_KEYS`].
    fn set_color(&mut self, key: &str, color: Color) {
        match key {
            "text" => self.text = color,
            "muted" | "_muted" => self.muted = color,
            "title" => self.title = color,
            "border" => self.border = color,
            "header_bg" => self.header_bg = color,
            "header_fg" => self.header_fg = color,
            "status_bg" => self.status_bg = color,
            "status_fg" => self.status_fg = color,
            "highlight_fg" => self.highlight_fg = color,
            "highlight_bg" => self.highlight_bg = color,
            _ => {}
        }
    }

    fn parse_color(value: &str) -> Option<Color> {
        let value = value.trim();
        if value.eq_ignore_ascii_case("reset") {
            return Some(Color::Reset);
        }
        if let Some(name) = value.strip_prefix("named:") {
            return Some(match name.to_ascii_lowercase().as_str() {
                "black" => Color::Black,
                "red" => Color::Red,
                "green" => Color::Green,
                "yellow" => Color::Yellow,
                "blue" => Color::Blue,
                "magenta" => Color::Magenta,
                "cyan" => Color::Cyan,
                "gray" => Color::Gray,
                "darkgray" => Color::DarkGray,
                "lightred" => Color::LightRed,
                "lightgreen" => Color::LightGreen,
                "lightyellow" => Color::LightYellow,
                "lightblue" => Color::LightBlue,
                "lightmagenta" => Color::LightMagenta,
                "lightcyan" => Color::LightCyan,
                "white" => Color::White,
                _ => return None,
            });
        }
        if let Some(index) = value.strip_prefix("index:") {
            return index.parse().ok().map(Color::Indexed);
        }
        let hex = value.strip_prefix('#').unwrap_or(value);
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

    fn color_to_string(color: Color) -> String {
        match color {
            Color::Rgb(red, green, blue) => format!("#{red:02X}{green:02X}{blue:02X}"),
            Color::Indexed(index) => format!("index:{index}"),
            Color::Reset => "reset".to_owned(),
            Color::Black => "named:black".to_owned(),
            Color::Red => "named:red".to_owned(),
            Color::Green => "named:green".to_owned(),
            Color::Yellow => "named:yellow".to_owned(),
            Color::Blue => "named:blue".to_owned(),
            Color::Magenta => "named:magenta".to_owned(),
            Color::Cyan => "named:cyan".to_owned(),
            Color::Gray => "named:gray".to_owned(),
            Color::DarkGray => "named:darkgray".to_owned(),
            Color::LightRed => "named:lightred".to_owned(),
            Color::LightGreen => "named:lightgreen".to_owned(),
            Color::LightYellow => "named:lightyellow".to_owned(),
            Color::LightBlue => "named:lightblue".to_owned(),
            Color::LightMagenta => "named:lightmagenta".to_owned(),
            Color::LightCyan => "named:lightcyan".to_owned(),
            Color::White => "named:white".to_owned(),
        }
    }

    /// Atomically persist every theme field in the canonical format. The
    /// `preset` line is written only when the theme was read with one.
    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        let mut contents = String::from(
            "# usrgrp-manager theme configuration\n# Format: key = value\n# Optional: preset = mocha | dark picks the base colors; the keys below override it\n# Colors: #RRGGBB, index:N, named:color, or reset\n\n",
        );
        if let Some(preset) = self.preset {
            contents.push_str("preset = ");
            contents.push_str(preset.name());
            contents.push('\n');
        }
        for (key, color) in [
            ("text", self.text),
            ("muted", self.muted),
            ("title", self.title),
            ("border", self.border),
            ("header_bg", self.header_bg),
            ("header_fg", self.header_fg),
            ("status_bg", self.status_bg),
            ("status_fg", self.status_fg),
            ("highlight_fg", self.highlight_fg),
            ("highlight_bg", self.highlight_bg),
        ] {
            contents.push_str(key);
            contents.push_str(" = ");
            contents.push_str(&Self::color_to_string(color));
            contents.push('\n');
        }
        crate::config::atomic_write(path, contents.as_bytes())
    }

    /// Load an existing file or create a canonical default atomically.
    pub fn load_or_init(path: &str) -> std::io::Result<Self> {
        match Self::from_file(path) {
            Ok(theme) => Ok(theme),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let theme = Self::mocha();
                theme.write_file(path)?;
                Ok(theme)
            }
            Err(error) => Err(error),
        }
    }
}

fn config_line_error<T>(line: usize, reason: &str) -> std::io::Result<T> {
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("configuration line {line}: {reason}"),
    ))
}

/// Modal dialog states.  Sudo input exists only while its prompt is visible;
/// no credential is retained in `AppState` after Enter/Esc.
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
    /// `selected` indexes the filtered list; ticks are kept by name so they
    /// survive filter changes.
    ModifyGroupsAdd {
        selected: usize,
        offset: usize,
        selected_multi: Vec<String>,
        filter: String,
        /// Keys edit `filter` until Enter or Esc.
        filtering: bool,
        /// Why the last tick was refused, shown under the list.
        error: Option<String>,
    },
    /// Same fields as `ModifyGroupsAdd`.
    ModifyGroupsRemove {
        selected: usize,
        offset: usize,
        selected_multi: Vec<String>,
        filter: String,
        filtering: bool,
        error: Option<String>,
    },
    ModifyDetailsMenu {
        selected: usize,
    },
    ModifyShell {
        selected: usize,
        offset: usize,
        /// Shells from `/etc/shells`, led by the user's current shell when
        /// that one is not listed there.
        shells: Vec<String>,
        /// Index of the user's current shell in `shells`.
        current: Option<usize>,
        /// The current shell was prepended because `/etc/shells` lacks it.
        current_unlisted: bool,
        /// The user's current shell when it is a label rather than a path,
        /// such as `(default /bin/sh)` for an empty passwd shell. Shown as a
        /// note above the list and never offered as a choice.
        current_label: Option<String>,
    },
    /// Single-select list of every group for the selected user's primary
    /// group, captured when the dialog opens. Blocked entries cannot be
    /// chosen; Enter on one shows the reason in `error`.
    ModifyPrimaryGroup {
        selected: usize,
        /// `groups_all` names with the policy's reason per blocked entry.
        groups: CandidateList,
        /// Index of the user's current primary group in `groups`.
        current: Option<usize>,
        /// The current primary GID when no group has it, shown as a note.
        current_gid: Option<u32>,
        /// Why the last Enter was refused.
        error: Option<String>,
    },
    /// Opens with the current home filled in. Row 0 is the path, row 1 the
    /// "move contents" checkbox.
    ModifyHome {
        selected: usize,
        value: String,
        move_contents: bool,
        /// The home when the dialog opened, shown above the field.
        current: String,
        /// Validation problem shown inline under the fields.
        error: Option<String>,
    },
    /// Opens with the current username or full name filled in.
    ModifyTextInput {
        field: ModifyField,
        value: String,
        /// Validation problem shown inline under the fields.
        error: Option<String>,
    },
    /// Rows: [`DELETE_ROW_HOME`], [`DELETE_ROW_YES`], [`DELETE_ROW_NO`].
    /// Everything shown is captured when the dialog opens.
    DeleteConfirm {
        selected: usize,
        allowed: bool,
        delete_home: bool,
        username: String,
        uid: u32,
        home: String,
        /// The target is the account running this program.
        is_self: bool,
        /// Running processes owned by the target, from the last refresh.
        process_count: Option<usize>,
    },
    ModifyPasswordMenu {
        selected: usize,
    },
    ChangePassword {
        selected: usize,
        password: SecretInput,
        confirm: SecretInput,
        must_change: bool,
        /// Ctrl+R shows both password fields in clear text.
        reveal: bool,
        /// Validation problem shown inline under the fields.
        error: Option<String>,
    },
    /// The exact redacted bridge plan is visible before it can run.
    /// Long previews scroll; Apply is refused until the last step has been
    /// on screen.
    OperationConfirm {
        selected: usize,
        /// What the operation does, from [`PendingAction::describe`]; the
        /// dialog is titled `Confirm: <action>`.
        action: String,
        preview: Vec<String>,
        /// Index of the first preview line shown.
        scroll: usize,
        /// Every preview line has been displayed at least once.
        seen_all: bool,
    },
    Info {
        /// Dialog title; None shows the default "Result".
        title: Option<String>,
        message: String,
    },
    Help {
        scroll: u16,
    },
    SudoPrompt {
        password: SecretInput,
        error: Option<String>,
        /// Passwords already rejected for the pending operation.
        attempts: u8,
    },
    GroupsActions {
        selected: usize,
        target_gid: Option<u32>,
    },
    GroupAddInput {
        name: String,
        /// Validation problem shown inline under the fields.
        error: Option<String>,
    },
    GroupDeleteConfirm {
        selected: usize,
        target_gid: Option<u32>,
    },
    ConfirmRemoveUserFromGroup {
        selected: usize,
        username: String,
        groupname: String,
    },
    GroupModifyMenu {
        selected: usize,
        target_gid: Option<u32>,
    },
    /// Same list fields as `ModifyGroupsAdd`.
    GroupModifyAddMembers {
        selected: usize,
        offset: usize,
        target_gid: Option<u32>,
        selected_multi: Vec<String>,
        filter: String,
        filtering: bool,
        error: Option<String>,
    },
    /// Same list fields as `ModifyGroupsAdd`.
    GroupModifyRemoveMembers {
        selected: usize,
        offset: usize,
        target_gid: Option<u32>,
        selected_multi: Vec<String>,
        filter: String,
        filtering: bool,
        error: Option<String>,
    },
    /// Opens with the current group name filled in.
    GroupRenameInput {
        name: String,
        target_gid: Option<u32>,
        /// Validation problem shown inline under the fields.
        error: Option<String>,
    },
    UserAddInput {
        selected: usize,
        name: String,
        password: SecretInput,
        confirm: SecretInput,
        create_home: bool,
        add_to_wheel: bool,
        /// Ctrl+R shows both password fields in clear text.
        reveal: bool,
        /// Set once a submission has consumed the typed password. Until a new
        /// password is typed, submitting is refused, so a failed attempt
        /// cannot be retried as an account without a password.
        password_reentry_required: bool,
        /// Set by the first edit of the username field. Until then an empty
        /// name is not reported, so a fresh form opens without an error.
        name_touched: bool,
        /// Validation problem shown inline under the fields.
        error: Option<String>,
    },
}

impl ModalState {
    /// The inline validation message of a form or multi-select dialog, if
    /// it has one.
    pub fn form_error(&self) -> Option<&str> {
        match self {
            Self::ModifyTextInput { error, .. }
            | Self::ModifyPrimaryGroup { error, .. }
            | Self::ModifyHome { error, .. }
            | Self::ChangePassword { error, .. }
            | Self::GroupAddInput { error, .. }
            | Self::GroupRenameInput { error, .. }
            | Self::UserAddInput { error, .. }
            | Self::ModifyGroupsAdd { error, .. }
            | Self::ModifyGroupsRemove { error, .. }
            | Self::GroupModifyAddMembers { error, .. }
            | Self::GroupModifyRemoveMembers { error, .. } => error.as_deref(),
            _ => None,
        }
    }
}

/// Longest username or group name a text field accepts, in characters.
pub const MAX_NAME_CHARS: usize = 32;
/// Longest full name (GECOS) a text field accepts, in characters.
pub const MAX_GECOS_CHARS: usize = 256;
/// Longest home directory path the home field accepts, in characters.
pub const MAX_HOME_CHARS: usize = 4096;
/// Longest password each password field accepts, in characters.
pub const MAX_PASSWORD_CHARS: usize = 1024;
/// Form error shown when a keystroke would pass a field's limit.
pub const MAX_LENGTH_REACHED: &str = "Maximum length reached";

/// Why root and system accounts cannot be deleted: replaces Yes in their
/// delete dialog and answers Enter on it.
pub const PROTECTED_USER_MESSAGE: &str =
    "Root and system accounts (UID below 1000) cannot be deleted from this tool.";
/// Why root and system groups cannot be deleted or renamed.
pub const PROTECTED_GROUP_MESSAGE: &str =
    "Root and system groups (GID below 1000) cannot be changed from this tool.";

/// "Also delete home" checkbox row of the user delete dialog.
pub const DELETE_ROW_HOME: usize = 0;
/// Yes button of the user delete dialog.
pub const DELETE_ROW_YES: usize = 1;
/// No button of the user delete dialog, where the cursor starts.
pub const DELETE_ROW_NO: usize = 2;

#[derive(Clone, Debug)]
pub enum ModifyField {
    Username,
    Fullname,
}

impl ModifyField {
    /// Characters the field accepts before [`MAX_LENGTH_REACHED`].
    pub fn max_chars(&self) -> usize {
        match self {
            Self::Username => MAX_NAME_CHARS,
            Self::Fullname => MAX_GECOS_CHARS,
        }
    }
}

/// One entry of the "Modify user" menu. The renderer and the key handler both
/// index [`AppState::modify_menu_items`], so a highlighted row is always the
/// entry that opens.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ModifyMenuItem {
    AddGroup,
    RemoveGroup,
    Details,
    Password,
    Lock,
    Unlock,
}

impl ModifyMenuItem {
    pub fn label(self) -> &'static str {
        match self {
            Self::AddGroup => "Add group",
            Self::RemoveGroup => "Remove group",
            Self::Details => "Modify details",
            Self::Password => "Password",
            Self::Lock => "Lock account",
            Self::Unlock => "Unlock account",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct UsersFilterChips {
    pub human_only: bool,
    pub system_only: bool,
    pub inactive: bool,
    pub no_home: bool,
    pub locked: bool,
    pub no_password: bool,
    pub expired: bool,
}

impl UsersFilterChips {
    /// Whether any chip backed by shadow data (locked, no password, expired) is on.
    pub fn any_shadow(&self) -> bool {
        self.locked || self.no_password || self.expired
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum UsersFilter {
    OnlyUserIds,
    OnlySystemIds,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum GroupsFilter {
    OnlyUserGids,
    OnlySystemGids,
}

/// Row order of the Users table, cycled with `CycleSort`. Not persisted.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum UsersSort {
    #[default]
    Uid,
    Name,
    Shell,
    Home,
}

impl UsersSort {
    /// The order after this one; the last wraps to UID.
    pub fn next(self) -> Self {
        match self {
            Self::Uid => Self::Name,
            Self::Name => Self::Shell,
            Self::Shell => Self::Home,
            Self::Home => Self::Uid,
        }
    }

    /// Title label (`sort: name`), or `None` for the default UID order.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Uid => None,
            Self::Name => Some("name"),
            Self::Shell => Some("shell"),
            Self::Home => Some("home"),
        }
    }
}

/// Row order of the Groups table, cycled with `CycleSort`. Not persisted.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum GroupsSort {
    #[default]
    Gid,
    Name,
    /// Most secondary members first.
    Members,
}

impl GroupsSort {
    /// The order after this one; the last wraps to GID.
    pub fn next(self) -> Self {
        match self {
            Self::Gid => Self::Name,
            Self::Name => Self::Members,
            Self::Members => Self::Gid,
        }
    }

    /// Title label (`sort: members`), or `None` for the default GID order.
    pub fn label(self) -> Option<&'static str> {
        match self {
            Self::Gid => None,
            Self::Name => Some("name"),
            Self::Members => Some("members"),
        }
    }
}

#[derive(Clone, Debug)]
pub enum ActionsContext {
    GroupMemberRemoval { group_name: String },
}

/// Legacy UI action vocabulary.  `update` translates it only to the closed
/// `sys::OperationRequest` bridge; it never selects commands directly.
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
        set_password: bool,
        create_home: bool,
        /// The sudo group to join, when the box was ticked.
        sudo_group: Option<String>,
    },
    DeleteUser {
        username: String,
        delete_home: bool,
    },
    SetPassword {
        username: String,
        must_change: bool,
    },
    ResetPassword {
        username: String,
    },
    LockUser {
        username: String,
    },
    UnlockUser {
        username: String,
    },
    ChangePrimaryGroup {
        username: String,
        groupname: String,
    },
    ChangeHome {
        username: String,
        home: String,
        move_contents: bool,
    },
}

/// Most names an action description lists before summarizing the rest.
const DESCRIBE_MAX_NAMES: usize = 5;

/// `wheel, docker`; past [`DESCRIBE_MAX_NAMES`] the rest become `and 3 more`.
fn describe_names(names: &[String]) -> String {
    let mut text = names
        .iter()
        .take(DESCRIBE_MAX_NAMES)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > DESCRIBE_MAX_NAMES {
        text.push_str(&format!(" and {} more", names.len() - DESCRIBE_MAX_NAMES));
    }
    text
}

impl PendingAction {
    /// What the action does, as one sentence for the confirmation title and
    /// the result dialog: `Change shell of alice to /bin/zsh`.
    pub fn describe(&self) -> String {
        match self {
            Self::AddUserToGroup {
                username,
                groupname,
            } => format!("Add {username} to {groupname}"),
            Self::RemoveUserFromGroup {
                username,
                groupname,
            } => format!("Remove {username} from {groupname}"),
            Self::AddUserToGroups {
                username,
                groupnames,
            } => format!("Add {username} to {}", describe_names(groupnames)),
            Self::RemoveUserFromGroups {
                username,
                groupnames,
            } => format!("Remove {username} from {}", describe_names(groupnames)),
            Self::AddMembersToGroup {
                groupname,
                usernames,
            } => format!("Add {} to {groupname}", describe_names(usernames)),
            Self::RemoveMembersFromGroup {
                groupname,
                usernames,
            } => format!("Remove {} from {groupname}", describe_names(usernames)),
            Self::ChangeShell {
                username,
                new_shell,
            } => format!("Change shell of {username} to {new_shell}"),
            Self::ChangeFullname { username, .. } => format!("Set full name of {username}"),
            Self::ChangeUsername {
                old_username,
                new_username,
            } => format!("Rename user {old_username} to {new_username}"),
            Self::CreateGroup { groupname } => format!("Create group {groupname}"),
            Self::DeleteGroup { groupname } => format!("Delete group {groupname}"),
            Self::RenameGroup { old_name, new_name } => {
                format!("Rename group {old_name} to {new_name}")
            }
            Self::CreateUserWithOptions {
                username,
                set_password,
                create_home,
                sudo_group,
            } => {
                let mut options = Vec::new();
                if *create_home {
                    options.push("home".to_owned());
                }
                if *set_password {
                    options.push("password".to_owned());
                }
                if let Some(group) = sudo_group {
                    options.push(format!("sudo group {group}"));
                }
                if options.is_empty() {
                    format!("Create user {username}")
                } else {
                    format!("Create user {username} ({})", options.join(", "))
                }
            }
            Self::DeleteUser {
                username,
                delete_home,
            } => {
                if *delete_home {
                    format!("Delete user {username} and its home")
                } else {
                    format!("Delete user {username}")
                }
            }
            Self::SetPassword {
                username,
                must_change,
            } => {
                if *must_change {
                    format!("Set password for {username} (must change at next login)")
                } else {
                    format!("Set password for {username}")
                }
            }
            Self::ResetPassword { username } => {
                format!("Force {username} to change password at next login")
            }
            Self::LockUser { username } => format!("Lock account {username}"),
            Self::UnlockUser { username } => format!("Unlock account {username}"),
            Self::ChangePrimaryGroup {
                username,
                groupname,
            } => format!("Change primary group of {username} to {groupname}"),
            Self::ChangeHome {
                username,
                home,
                move_contents,
            } => {
                if *move_contents {
                    format!("Change home of {username} to {home} (moving contents)")
                } else {
                    format!("Change home of {username} to {home}")
                }
            }
        }
    }
}

/// Password input which cannot be cloned or formatted and is zeroized when
/// cancelled or moved into the one-shot trusted boundary.
#[derive(Default)]
pub struct SecretInput {
    value: String,
}

impl SecretInput {
    pub fn push(&mut self, character: char) {
        self.value.push(character);
    }
    pub fn pop(&mut self) {
        self.value.pop();
    }
    pub fn len(&self) -> usize {
        self.value.len()
    }
    /// Characters typed, the unit form limits count in.
    pub fn char_count(&self) -> usize {
        self.value.chars().count()
    }
    /// One `*` per character, or the typed text when `reveal` is set. The
    /// copy is zeroized on drop; borrow it for the render, never re-format it.
    pub fn display(&self, reveal: bool) -> zeroize::Zeroizing<String> {
        zeroize::Zeroizing::new(if reveal {
            self.value.clone()
        } else {
            "*".repeat(self.char_count())
        })
    }
    /// Rough strength hint: `weak` under 8 characters, `strong` from 12
    /// characters with letters, digits, and symbols, otherwise `ok`.
    pub fn strength(&self) -> &'static str {
        let has = |test: fn(&char) -> bool| self.value.chars().any(|c| test(&c));
        let count = self.char_count();
        if count < 8 {
            "weak"
        } else if count >= 12
            && has(|c| c.is_alphabetic())
            && has(|c| c.is_numeric())
            && has(|c| !c.is_alphanumeric())
        {
            "strong"
        } else {
            "ok"
        }
    }
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
    pub fn matches(&self, other: &Self) -> bool {
        self.value == other.value
    }
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.value)
    }
    /// Zeroize and empty the field.
    pub fn clear(&mut self) {
        use zeroize::Zeroize;
        self.value.zeroize();
    }
}

impl Drop for SecretInput {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.value.zeroize();
    }
}

/// Title of the dialog and help section that list configuration problems.
pub const CONFIG_PROBLEMS_TITLE: &str = "Configuration problems";

/// Cached, bounded diagnostics populated only by explicit refresh effects.
#[derive(Clone, Debug, Default)]
pub struct CachedDiagnostics {
    pub shadow: crate::search::ShadowState,
    pub homes: HashMap<String, HomeDiagnostics>,
    /// Group summaries are computed during refresh, never by a frame render.
    pub groups: HashMap<u32, GroupDiagnostics>,
    pub group_mtime_days: Option<u64>,
    /// Days since the UNIX epoch when this refresh ran, so the details pane
    /// can say how long ago /etc/group changed without reading the clock.
    pub refreshed_days: u64,
    pub stale_reason: Option<String>,
    /// At most three source/line diagnostics are retained for display.
    pub config_messages: Vec<String>,
}

/// Diagnostics for one account home, never populated by rendering.
#[derive(Clone, Debug, Default)]
pub struct HomeDiagnostics {
    pub exists: Option<bool>,
    pub permissions: Option<String>,
    pub authorized_key_count: Option<usize>,
    /// Processes whose real UID is this account's. None when /proc could
    /// not be read completely.
    pub process_count: Option<usize>,
}

/// Cached group facts used by the details pane. Counts are bounded by the
/// refresh provider rather than re-scanning users/groups every frame.
#[derive(Clone, Debug, Default)]
pub struct GroupDiagnostics {
    pub primary_members: usize,
    pub orphan_members: usize,
    pub locked_members: usize,
    pub empty_password_members: usize,
    pub expired_members: usize,
    pub members_truncated: bool,
}

/// Format the time of day of `time` as `HH:MM:SS UTC`. Times before the
/// UNIX epoch show as midnight.
pub fn utc_time_label(time: SystemTime) -> String {
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_secs())
        % 86_400;
    format!(
        "{:02}:{:02}:{:02} UTC",
        seconds / 3_600,
        seconds / 60 % 60,
        seconds % 60
    )
}

/// Format a day count since the UNIX epoch as a `YYYY-MM-DD` calendar date
/// (proleptic Gregorian, UTC). Negative counts are dates before 1970.
pub fn civil_date_label(days: i64) -> String {
    // Howard Hinnant's days-to-civil algorithm, with eras of 400 years
    // counted from 0000-03-01 so leap days fall at the end of each year.
    let z = days.saturating_add(719_468);
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

/// Injectable clock for deterministic diagnostics and config-root tests.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

pub struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Injectable candidate config roots. Tests never need HOME/XDG state.
pub trait ConfigRootProvider: Send + Sync {
    fn roots(&self) -> Vec<PathBuf>;
}

pub struct SystemConfigRootProvider;
impl ConfigRootProvider for SystemConfigRootProvider {
    fn roots(&self) -> Vec<PathBuf> {
        let mut roots = Vec::new();
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
            && !xdg.trim().is_empty()
        {
            roots.push(PathBuf::from(xdg).join("UsrGrpManager"));
        }
        if let Some(home) = dirs_next::home_dir() {
            roots.push(home.join(".config").join("UsrGrpManager"));
            roots.push(home.join("UsrGrpManager"));
        }
        roots
    }
}

/// What `load_configuration` did with one configuration file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfigFileState {
    /// The file at this path was read.
    Loaded(String),
    /// No file existed, so defaults were written to the save path.
    Created,
    /// Reading or creating the file failed; defaults are in use and the
    /// error is in the recorded configuration messages.
    Failed,
}

/// Paths of one configuration file, cached for the help dialog.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigFilePaths {
    /// File name, such as `theme.conf`.
    pub name: String,
    /// Whether the file was read, created, or could not be loaded.
    pub state: ConfigFileState,
    /// Where changes are written: the file in the first config root.
    pub save: String,
}

impl ConfigFilePaths {
    /// Load `name` with `load` from the file found in the config roots of
    /// `app`, or from the save path when none exists (`load` then writes
    /// defaults there), and record which of those happened.
    fn load<T>(
        app: &AppState,
        name: &str,
        load: impl FnOnce(&str) -> std::io::Result<T>,
    ) -> (Self, std::io::Result<T>) {
        let save = app.configuration_write_path(name);
        let found = app.config_file_read_path(name);
        let result = load(found.as_deref().unwrap_or(&save));
        let state = match (&result, found) {
            (Err(_), _) => ConfigFileState::Failed,
            (Ok(_), Some(path)) => ConfigFileState::Loaded(path),
            (Ok(_), None) => ConfigFileState::Created,
        };
        let paths = Self {
            name: name.to_owned(),
            state,
            save,
        };
        (paths, result)
    }
}

/// Single config root chosen with `--config-dir`. The default search order
/// is not consulted, so reads and writes both use this directory.
pub struct FixedConfigRootProvider(pub PathBuf);
impl ConfigRootProvider for FixedConfigRootProvider {
    fn roots(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
}

/// Startup options from the command line. The default is the plain TUI.
#[derive(Default)]
pub struct RunOptions {
    /// Replace the default config search roots with this one directory.
    pub config_dir: Option<PathBuf>,
    /// Show every prepared preview but never execute it.
    pub dry_run: bool,
    /// Already-opened operation log.
    pub logger: Option<Logger>,
}

/// Injectable bounded diagnostics effect. Renderers only consume its result.
pub trait DiagnosticProvider: Send + Sync {
    fn refresh(&self, snapshot: &AccountSnapshot, now: SystemTime) -> CachedDiagnostics;
}

pub struct SystemDiagnosticProvider;
impl DiagnosticProvider for SystemDiagnosticProvider {
    fn refresh(&self, snapshot: &AccountSnapshot, now: SystemTime) -> CachedDiagnostics {
        const MAX_USERS: usize = 10_000;
        const MAX_AUTHORIZED_KEYS_BYTES_PER_USER: usize = 64 * 1024;
        const MAX_TOTAL_DIAGNOSTIC_BYTES: usize = 1024 * 1024;
        let mut diagnostics = CachedDiagnostics {
            shadow: crate::search::read_shadow_state(),
            refreshed_days: now
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_secs() / 86_400),
            ..CachedDiagnostics::default()
        };
        let mut remaining_bytes = MAX_TOTAL_DIAGNOSTIC_BYTES;
        let process_counts = read_process_counts(std::path::Path::new("/proc"));
        for user in snapshot.users.iter().take(MAX_USERS) {
            let metadata = std::fs::metadata(&user.home_dir);
            let (exists, permissions) = match metadata {
                Ok(metadata) => {
                    #[cfg(unix)]
                    let permissions = {
                        use std::os::unix::fs::PermissionsExt;
                        Some(format!("{:03o}", metadata.permissions().mode() & 0o777))
                    };
                    #[cfg(not(unix))]
                    let permissions = None;
                    (Some(true), permissions)
                }
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (Some(false), None),
                Err(_) => (None, None),
            };
            let key_path = user.home_dir.join(".ssh/authorized_keys");
            let limit = remaining_bytes.min(MAX_AUTHORIZED_KEYS_BYTES_PER_USER);
            let authorized_key_count =
                read_authorized_key_count(&key_path, limit).map(|(count, consumed)| {
                    remaining_bytes = remaining_bytes.saturating_sub(consumed);
                    count
                });
            diagnostics.homes.insert(
                user.name.as_str().to_owned(),
                HomeDiagnostics {
                    exists,
                    permissions,
                    authorized_key_count,
                    process_count: process_counts
                        .as_ref()
                        .map(|counts| counts.get(&user.uid.0).copied().unwrap_or(0)),
                },
            );
            if remaining_bytes == 0 {
                break;
            }
        }
        let primary_counts = snapshot.users.iter().take(MAX_USERS).fold(
            HashMap::<u32, usize>::new(),
            |mut counts, user| {
                *counts.entry(user.primary_gid.0).or_default() += 1;
                counts
            },
        );
        let known_users = snapshot
            .users
            .iter()
            .take(MAX_USERS)
            .map(|user| user.name.as_str())
            .collect::<std::collections::HashSet<_>>();
        let shadow = match &diagnostics.shadow {
            crate::search::ShadowState::Known(statuses) => Some(statuses),
            crate::search::ShadowState::Unavailable { .. } => None,
        };
        const MAX_GROUP_MEMBERS: usize = 100_000;
        for group in snapshot.groups.iter().take(MAX_USERS) {
            let mut group_diagnostics = GroupDiagnostics {
                primary_members: primary_counts
                    .get(&group.gid.0)
                    .copied()
                    .unwrap_or_default(),
                ..GroupDiagnostics::default()
            };
            for member in group.members.iter().take(MAX_GROUP_MEMBERS) {
                if !known_users.contains(member.as_str()) {
                    group_diagnostics.orphan_members += 1;
                }
                if let Some(status) = shadow.and_then(|statuses| statuses.get(member.as_str())) {
                    group_diagnostics.locked_members += usize::from(status.locked);
                    group_diagnostics.empty_password_members += usize::from(status.no_password);
                    group_diagnostics.expired_members += usize::from(status.expired);
                }
            }
            group_diagnostics.members_truncated = group.members.len() > MAX_GROUP_MEMBERS;
            diagnostics.groups.insert(group.gid.0, group_diagnostics);
        }
        diagnostics.group_mtime_days = std::fs::metadata("/etc/group")
            .and_then(|metadata| metadata.modified())
            .ok()
            .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
            .or_else(|| now.duration_since(UNIX_EPOCH).ok())
            .map(|duration| duration.as_secs() / 86_400);
        diagnostics
    }
}

/// Most `/proc` entries scanned for process counts.
const MAX_PROC_ENTRIES: usize = 65_536;
/// Most bytes read from one `/proc/<pid>/status` file.
const MAX_PROC_STATUS_BYTES: u64 = 4096;

/// Count processes per real UID from `<proc_root>/<pid>/status`. Fails
/// closed: returns None when the directory cannot be listed, has more than
/// [`MAX_PROC_ENTRIES`] entries, a status file cannot be opened or read for
/// any reason other than NotFound (its process exited mid-scan), or a status
/// file has no parseable `Uid:` line. A partial count is never reported as
/// complete.
fn read_process_counts(proc_root: &std::path::Path) -> Option<HashMap<u32, usize>> {
    let mut counts = HashMap::new();
    let mut entries = std::fs::read_dir(proc_root).ok()?;
    for entry in entries.by_ref().take(MAX_PROC_ENTRIES) {
        let entry = entry.ok()?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name.is_empty() || !name.bytes().all(|byte| byte.is_ascii_digit()) {
            continue;
        }
        let Some(bytes) = read_proc_status(&entry.path().join("status")).ok()? else {
            continue;
        };
        let uid = status_real_uid(&String::from_utf8_lossy(&bytes))?;
        *counts.entry(uid).or_default() += 1;
    }
    if entries.next().is_some() {
        return None;
    }
    Some(counts)
}

/// Read at most [`MAX_PROC_STATUS_BYTES`] of one status file. `Ok(None)`
/// means the process exited (NotFound) and is skipped; any other open or
/// read error is returned and fails the whole scan.
fn read_proc_status(path: &std::path::Path) -> std::io::Result<Option<Vec<u8>>> {
    let read = || -> std::io::Result<Vec<u8>> {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(MAX_PROC_STATUS_BYTES)
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    };
    match read() {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Real UID from the `Uid:` line of a `/proc/<pid>/status` file.
fn status_real_uid(status: &str) -> Option<u32> {
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|ids| ids.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

fn read_authorized_key_count(path: &std::path::Path, limit: usize) -> Option<(usize, usize)> {
    if limit == 0 {
        return None;
    }
    let mut file = File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(limit.min(8192));
    file.by_ref()
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)
        .ok()?;
    if bytes.len() > limit {
        return None;
    }
    let contents = std::str::from_utf8(&bytes).ok()?;
    let count = contents
        .lines()
        .filter(|line| {
            let line = line.trim();
            !line.is_empty() && !line.starts_with('#')
        })
        .count();
    Some((count, bytes.len()))
}

/// Dedicated one-shot password capability. It is neither cloneable nor
/// printable and is consumed exactly once while preparing a trusted plan.
pub struct OneShotPassword {
    record: PasswordRecord,
}
impl OneShotPassword {
    pub fn new(record: PasswordRecord) -> Self {
        Self { record }
    }
    fn take(self) -> PasswordRecord {
        self.record
    }
}

/// The exact bridge plan awaiting a confirmation. This keeps plan capability
/// separate from generic UI requests/reports and drops it on cancellation.
pub struct PreparedOperation {
    pub(crate) plan: OperationPlan,
    /// What the plan does, from [`PendingAction::describe`].
    pub description: String,
}
impl PreparedOperation {
    pub fn preview(&self) -> Vec<String> {
        const MAX_PREVIEW_STEPS: usize = 1024;
        self.plan
            .redacted_preview()
            .into_iter()
            .take(MAX_PREVIEW_STEPS)
            .map(|preview| preview.render())
            .collect()
    }
}

/// Limits on the membership cache (`AppState::user_group_gids`): how many
/// users and groups are read and how many supplementary membership edges are
/// recorded. The cache is built in UID/GID order, so the limits cut the same
/// entries whatever the display order is.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MembershipCacheBounds {
    pub max_users: usize,
    pub max_groups: usize,
    pub max_membership_edges: usize,
}

impl Default for MembershipCacheBounds {
    fn default() -> Self {
        Self {
            max_users: 10_000,
            max_groups: 10_000,
            max_membership_edges: 100_000,
        }
    }
}

/// Row of a selection identity: the exact (id, name) pair first, then the
/// first row with the id alone (for example after a rename).
pub(crate) fn identity_position<T>(
    rows: &[T],
    id: Option<u32>,
    name: Option<&str>,
    key: impl Fn(&T) -> (u32, &str),
) -> Option<usize> {
    let id = id?;
    name.and_then(|name| rows.iter().position(|row| key(row) == (id, name)))
        .or_else(|| rows.iter().position(|row| key(row).0 == id))
}

/// Most entries an add/remove membership dialog offers.
pub const MAX_MEMBERSHIP_CANDIDATES: usize = 1024;

/// The entries a membership dialog lists after its filter, capped at
/// [`MAX_MEMBERSHIP_CANDIDATES`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CandidateList {
    /// Listed names, in display order.
    pub names: Vec<String>,
    /// How many candidates matched the filter before the cap.
    pub total: usize,
    /// Per listed name, why the policy refuses the membership change, or
    /// `None` when it is allowed. Empty when nothing was checked.
    pub blocked: Vec<Option<&'static str>>,
}

impl CandidateList {
    /// Keep the names in `pool` that contain `filter` (ignoring case), up to
    /// the cap, and count every match.
    fn from_pool<'a>(pool: impl IntoIterator<Item = &'a str>, filter: &str) -> Self {
        let filter = filter.to_lowercase();
        let mut list = Self::default();
        for name in pool {
            if !filter.is_empty() && !name.to_lowercase().contains(&filter) {
                continue;
            }
            list.total += 1;
            if list.names.len() < MAX_MEMBERSHIP_CANDIDATES {
                list.names.push(name.to_owned());
            }
        }
        list
    }

    /// Every name in `pool`, uncapped, for single-select pickers such as the
    /// primary group, where a missing entry could not be chosen at all.
    fn every<'a>(pool: impl IntoIterator<Item = &'a str>) -> Self {
        let names: Vec<String> = pool.into_iter().map(str::to_owned).collect();
        Self {
            total: names.len(),
            names,
            blocked: Vec::new(),
        }
    }

    /// True when matches were left out because of the cap.
    pub fn is_capped(&self) -> bool {
        self.total > self.names.len()
    }

    /// Record, for every listed name, why `reason` refuses it.
    fn with_blocks(mut self, reason: impl Fn(&str) -> Option<&'static str>) -> Self {
        self.blocked = self.names.iter().map(|name| reason(name)).collect();
        self
    }

    /// Why the entry at `index` cannot be ticked, if it cannot.
    pub fn blocked_reason(&self, index: usize) -> Option<&'static str> {
        self.blocked.get(index).copied().flatten()
    }
}

/// `ticked` names that are in `pool`, in tick order.
fn offered(pool: &[&str], ticked: &[String]) -> Vec<String> {
    ticked
        .iter()
        .filter(|name| pool.contains(&name.as_str()))
        .cloned()
        .collect()
}

/// In-memory application state.  It intentionally stores no sudo credential.
pub struct AppState {
    pub started_at: Instant,
    pub users_all: Vec<sys::SystemUser>,
    pub users: Vec<sys::SystemUser>,
    pub groups_all: Vec<sys::SystemGroup>,
    pub groups: Vec<sys::SystemGroup>,
    pub active_tab: ActiveTab,
    /// Render index derived from the stable user identity below.
    pub selected_user_index: usize,
    /// Render index derived from the stable group identity below.
    pub selected_group_index: usize,
    /// Render index derived from the stable users-tab membership group identity.
    pub selected_user_group_index: usize,
    /// Render index derived from the stable groups-tab member identity.
    pub selected_group_member_index: usize,
    /// Stable selection source of truth for every pane. Indices are only
    /// normalized render coordinates and never define an action target.
    /// Users and groups are identified by id and name together, because
    /// account files may give several entries the same UID or GID.
    pub selected_user_uid: Option<u32>,
    pub selected_user_name: Option<String>,
    pub selected_group_gid: Option<u32>,
    pub selected_group_name: Option<String>,
    pub selected_user_group_gid: Option<u32>,
    pub selected_user_group_name: Option<String>,
    pub selected_group_member_name: Option<String>,
    /// Bounded, precomputed primary and supplementary memberships by user.
    /// Renderers consume this cache instead of scanning every group's members.
    pub user_group_gids: HashMap<String, HashSet<u32>>,
    /// Bounds applied while building `user_group_gids`.
    pub membership_cache_bounds: MembershipCacheBounds,
    /// Last known terminal size (columns, rows). Page keys derive the page
    /// size from the layout at this size.
    pub terminal_size: (u16, u16),
    pub _table_state: TableState,
    pub input_mode: InputMode,
    /// Search text for the Users table. It stays applied in every input
    /// mode until the user clears it.
    pub users_query: String,
    /// Search text for the Groups table, independent of `users_query`.
    pub groups_query: String,
    /// The query as it was when `/` started editing, restored by Esc.
    search_before_edit: String,
    pub theme: Theme,
    pub keymap: keymap::Keymap,
    /// Open dialogs, innermost last. Only the top is rendered and receives keys.
    modals: Vec<ModalState>,
    /// Counts pops and clears so key handling can tell whether the handled
    /// dialog was dismissed (see `detach_top_modal`).
    modal_pops: u64,
    pub users_focus: UsersFocus,
    pub groups_focus: GroupsFocus,
    pub users_filter: Option<UsersFilter>,
    pub groups_filter: Option<GroupsFilter>,
    pub users_filter_chips: UsersFilterChips,
    /// Row order of each table, applied by `sort_and_filter`.
    pub users_sort: UsersSort,
    pub groups_sort: GroupsSort,
    /// Set by the last filter pass when shadow is known, a shadow chip is on,
    /// and this many listed users had no shadow record, so the shadow chips
    /// were not applied.
    pub shadow_filter_skipped: Option<usize>,
    pub actions_context: Option<ActionsContext>,
    pub show_keybinds: bool,
    /// Adapter-owned authority for refresh, preparation, elevation and execution.
    pub adapter: Arc<sys::SystemAdapter>,
    pub account_snapshot: Option<AccountSnapshot>,
    pub diagnostics: CachedDiagnostics,
    /// Time of the last successful account refresh as `HH:MM:SS UTC`, empty
    /// until one succeeds. A failed refresh keeps the previous value, so the
    /// status bar can say since when the shown data is stale.
    pub last_refresh_label: String,
    clock: Arc<dyn Clock>,
    diagnostic_provider: Arc<dyn DiagnosticProvider>,
    config_root_provider: Arc<dyn ConfigRootProvider>,
    /// Where theme.conf, keybinds.conf and filter.conf were loaded from and
    /// where they save, as resolved by the last `load_configuration`. Empty
    /// until then. The help dialog shows these, so rendering never resolves
    /// config roots.
    pub config_paths: Vec<ConfigFilePaths>,
    pub current_username: String,
    /// Group the create-user form offers to add the new account to. Set
    /// from `UGM_SUDO_GROUP` by `load_system_with`; `wheel` otherwise.
    pub sudo_group: String,
    pub last_preview: Vec<String>,
    pub last_report: Option<OperationReport>,
    /// The only application-held operation capability. It contains the exact
    /// trusted plan shown to the user and drops on cancel/error.
    pub pending_operation: Option<PreparedOperation>,
    /// Password material is never queued or reported. This dedicated,
    /// non-cloneable capability exists only while converting a password modal
    /// into its one exact trusted operation.
    pending_password: Option<OneShotPassword>,
    /// `--dry-run`: confirmed plans are reported but never handed to the adapter.
    pub dry_run: bool,
    /// `--log-file`: redacted operation and error log. Dropped after a write failure.
    logger: Option<Logger>,
}

impl AppState {
    /// Pure deterministic construction.  It neither reads host accounts nor
    /// creates user configuration; tests should start here or use `with_adapter`.
    pub fn new() -> Self {
        Self::with_adapter(
            Arc::new(sys::SystemAdapter::new()),
            AccountSnapshot::empty(),
        )
    }

    /// Pure construction with explicit trusted dependencies and account data.
    pub fn with_adapter(adapter: Arc<sys::SystemAdapter>, snapshot: AccountSnapshot) -> Self {
        Self::with_dependencies(
            adapter,
            snapshot,
            Arc::new(SystemClock),
            Arc::new(SystemDiagnosticProvider),
            Arc::new(SystemConfigRootProvider),
        )
    }

    /// Fully injected construction for deterministic clocks, diagnostics, and
    /// configuration roots. No test needs host HOME, `/etc`, or wall clock.
    pub fn with_dependencies(
        adapter: Arc<sys::SystemAdapter>,
        snapshot: AccountSnapshot,
        clock: Arc<dyn Clock>,
        diagnostic_provider: Arc<dyn DiagnosticProvider>,
        config_root_provider: Arc<dyn ConfigRootProvider>,
    ) -> Self {
        let mut app = Self {
            started_at: Instant::now(),
            users_all: snapshot.users.iter().cloned().map(Into::into).collect(),
            users: Vec::new(),
            groups_all: snapshot.groups.iter().cloned().map(Into::into).collect(),
            groups: Vec::new(),
            active_tab: ActiveTab::Users,
            selected_user_index: 0,
            selected_group_index: 0,
            selected_user_group_index: 0,
            selected_group_member_index: 0,
            selected_user_uid: None,
            selected_user_name: None,
            selected_group_gid: None,
            selected_group_name: None,
            selected_user_group_gid: None,
            selected_user_group_name: None,
            selected_group_member_name: None,
            user_group_gids: HashMap::new(),
            membership_cache_bounds: MembershipCacheBounds::default(),
            terminal_size: (80, 24),
            _table_state: TableState::default(),
            input_mode: InputMode::Normal,
            users_query: String::new(),
            groups_query: String::new(),
            search_before_edit: String::new(),
            theme: Theme::mocha(),
            keymap: keymap::Keymap::default(),
            modals: Vec::new(),
            modal_pops: 0,
            users_focus: UsersFocus::UsersList,
            groups_focus: GroupsFocus::GroupsList,
            users_filter: None,
            groups_filter: None,
            users_filter_chips: UsersFilterChips::default(),
            users_sort: UsersSort::default(),
            groups_sort: GroupsSort::default(),
            shadow_filter_skipped: None,
            actions_context: None,
            show_keybinds: true,
            adapter,
            account_snapshot: Some(snapshot),
            diagnostics: CachedDiagnostics::default(),
            last_refresh_label: String::new(),
            clock,
            diagnostic_provider,
            config_root_provider,
            config_paths: Vec::new(),
            current_username: "unknown".to_owned(),
            sudo_group: "wheel".to_owned(),
            last_preview: Vec::new(),
            last_report: None,
            pending_operation: None,
            pending_password: None,
            dry_run: false,
            logger: None,
        };
        app.sort_and_filter();
        app
    }

    /// Production construction with explicit account/config effects.
    pub fn load_system() -> Self {
        Self::load_system_with(RunOptions::default())
    }

    /// Production construction honoring command-line options.
    pub fn load_system_with(options: RunOptions) -> Self {
        let mut app = match options.config_dir {
            Some(dir) => Self::with_dependencies(
                Arc::new(sys::SystemAdapter::new()),
                AccountSnapshot::empty(),
                Arc::new(SystemClock),
                Arc::new(SystemDiagnosticProvider),
                Arc::new(FixedConfigRootProvider(dir)),
            ),
            None => Self::new(),
        };
        app.dry_run = options.dry_run;
        app.logger = options.logger;
        app.refresh_accounts();
        app.current_username = sys::current_username().unwrap_or_else(|| "unknown".to_owned());
        app.sudo_group = sudo_group_name();
        app.load_configuration();
        app
    }

    /// Explicit refresh retaining the last known-good snapshot as stale.
    pub fn refresh_accounts(&mut self) {
        match self.adapter.refresh_state(self.account_snapshot.clone()) {
            sys::SnapshotState::Fresh(snapshot) => {
                self.refresh_diagnostics(&snapshot);
                self.apply_snapshot(snapshot);
                self.diagnostics.stale_reason = None;
                self.last_refresh_label = utc_time_label(self.clock.now());
            }
            sys::SnapshotState::Stale { prior, error } => {
                self.refresh_diagnostics(&prior);
                self.apply_snapshot(prior);
                self.diagnostics.stale_reason = Some(error.to_string());
            }
            sys::SnapshotState::Unavailable { error } => {
                self.diagnostics.stale_reason = Some(error.to_string());
            }
        }
    }

    fn refresh_diagnostics(&mut self, snapshot: &AccountSnapshot) {
        let config_messages = std::mem::take(&mut self.diagnostics.config_messages);
        self.diagnostics = self.diagnostic_provider.refresh(snapshot, self.clock.now());
        self.diagnostics.config_messages = config_messages;
    }

    fn apply_snapshot(&mut self, snapshot: AccountSnapshot) {
        self.users_all = snapshot.users.iter().cloned().map(Into::into).collect();
        self.groups_all = snapshot.groups.iter().cloned().map(Into::into).collect();
        self.account_snapshot = Some(snapshot);
        self.sort_and_filter();
    }

    /// Load durable settings, retaining defaults and surfacing any error in UI state.
    pub fn load_configuration(&mut self) {
        let (theme, result) = ConfigFilePaths::load(self, "theme.conf", Theme::load_or_init);
        match result {
            Ok(loaded) => self.theme = loaded,
            Err(error) => self.record_config_message("theme", &error),
        }
        let (keys, result) =
            ConfigFilePaths::load(self, "keybinds.conf", keymap::Keymap::load_or_init);
        match result {
            Ok(keymap) => self.keymap = keymap,
            Err(error) => self.record_config_message("keymap", &error),
        }
        let (filters, result) =
            ConfigFilePaths::load(self, "filter.conf", filterconf::FiltersConfig::load_or_init);
        match result {
            Ok(loaded) => loaded.apply_to(self),
            Err(error) => self.record_config_message("filter", &error),
        }
        self.config_paths = vec![theme, keys, filters];
        self.sort_and_filter();
    }

    /// Open a dialog listing the recorded configuration problems, if any.
    /// Called once after startup so the user learns why defaults are in use.
    pub fn show_config_problems(&mut self) {
        if self.diagnostics.config_messages.is_empty() {
            return;
        }
        let mut message = self.diagnostics.config_messages.join("\n");
        message.push_str("\n\nDefaults are in use for the affected file.");
        self.push_modal(ModalState::Info {
            title: Some(CONFIG_PROBLEMS_TITLE.to_owned()),
            message,
        });
    }

    /// Retain only a small amount of non-secret configuration diagnostics.
    pub fn record_config_message(&mut self, source: &str, error: &std::io::Error) {
        self.record_diagnostic_message(format!("{source} configuration: {error}"));
    }

    fn record_diagnostic_message(&mut self, mut message: String) {
        const MAX_CONFIG_MESSAGES: usize = 3;
        const MAX_CONFIG_MESSAGE_BYTES: usize = 256;
        if self.diagnostics.config_messages.len() >= MAX_CONFIG_MESSAGES {
            return;
        }
        if message.len() > MAX_CONFIG_MESSAGE_BYTES {
            let mut end = MAX_CONFIG_MESSAGE_BYTES;
            while !message.is_char_boundary(end) {
                end -= 1;
            }
            message.truncate(end);
        }
        self.diagnostics.config_messages.push(message);
    }

    pub fn sort_and_filter(&mut self) {
        self.capture_selection_identities();
        // Build the membership cache from the canonical UID/GID order so its
        // bounds never depend on the display order.
        self.users_all.sort_by_key(|user| user.uid);
        self.groups_all.sort_by_key(|group| group.gid);
        self.rebuild_user_group_cache(self.membership_cache_bounds);
        // Stable sorts by the chosen key, so rows with equal keys stay in
        // UID/GID order.
        match self.users_sort {
            UsersSort::Uid => {}
            UsersSort::Name => self
                .users_all
                .sort_by_cached_key(|user| user.name.to_lowercase()),
            UsersSort::Shell => self.users_all.sort_by(|a, b| a.shell.cmp(&b.shell)),
            UsersSort::Home => self.users_all.sort_by(|a, b| a.home_dir.cmp(&b.home_dir)),
        }
        match self.groups_sort {
            GroupsSort::Gid => {}
            GroupsSort::Name => self
                .groups_all
                .sort_by_cached_key(|group| group.name.to_lowercase()),
            GroupsSort::Members => self
                .groups_all
                .sort_by_key(|group| std::cmp::Reverse(group.members.len())),
        }
        crate::search::apply_filters_and_search(self);
        self.normalize_selections();
    }

    /// "Human users" is on through its chip or a hand-edited
    /// `users_filter = OnlyUserIds` in filter.conf.
    pub fn human_filter_on(&self) -> bool {
        self.users_filter_chips.human_only || self.users_filter == Some(UsersFilter::OnlyUserIds)
    }

    /// "System users" is on through its chip or a hand-edited
    /// `users_filter = OnlySystemIds` in filter.conf.
    pub fn system_filter_on(&self) -> bool {
        self.users_filter_chips.system_only || self.users_filter == Some(UsersFilter::OnlySystemIds)
    }

    /// Set the human/system filters. The chips carry the state from here on,
    /// so a hand-edited `users_filter` is folded into them and cleared.
    pub fn set_uid_range_filters(&mut self, human: bool, system: bool) {
        self.users_filter_chips.human_only = human;
        self.users_filter_chips.system_only = system;
        self.users_filter = None;
    }

    /// Short names of the user filters that narrow the list, in menu order.
    /// Shadow chips are left out while shadow data is unavailable, because
    /// they are disabled and not applied.
    pub fn active_user_filter_labels(&self) -> Vec<&'static str> {
        let chips = &self.users_filter_chips;
        let shadow = matches!(
            self.diagnostics.shadow,
            crate::search::ShadowState::Known(_)
        );
        [
            (self.human_filter_on(), "human"),
            (self.system_filter_on(), "system"),
            (chips.inactive, "inactive"),
            (chips.no_home, "no home"),
            (shadow && chips.locked, "locked"),
            (shadow && chips.no_password, "no password"),
            (shadow && chips.expired, "expired"),
        ]
        .into_iter()
        .filter_map(|(on, label)| on.then_some(label))
        .collect()
    }

    /// Rebuild `user_group_gids` from `users_all` and `groups_all` in their
    /// current order; the bounds apply in that order.
    fn rebuild_user_group_cache(&mut self, bounds: MembershipCacheBounds) {
        self.user_group_gids.clear();
        for user in self.users_all.iter().take(bounds.max_users) {
            self.user_group_gids
                .entry(user.name.clone())
                .or_default()
                .insert(user.primary_gid);
        }
        let mut remaining_edges = bounds.max_membership_edges;
        for group in self.groups_all.iter().take(bounds.max_groups) {
            for member in group.members.iter().take(remaining_edges) {
                self.user_group_gids
                    .entry(member.clone())
                    .or_default()
                    .insert(group.gid);
            }
            remaining_edges = remaining_edges.saturating_sub(group.members.len());
            if remaining_edges == 0 {
                break;
            }
        }
    }

    /// Normalize all render indices from stable pane identities after every
    /// refresh/filter/transition. When an identity disappears, use its
    /// bounded nearest visible index and capture that documented neighbor.
    pub fn normalize_selections(&mut self) {
        self.selected_user_index = identity_position(
            &self.users,
            self.selected_user_uid,
            self.selected_user_name.as_deref(),
            |user| (user.uid, user.name.as_str()),
        )
        .unwrap_or(self.selected_user_index)
        .min(self.users.len().saturating_sub(1));
        self.selected_group_index = identity_position(
            &self.groups,
            self.selected_group_gid,
            self.selected_group_name.as_deref(),
            |group| (group.gid, group.name.as_str()),
        )
        .unwrap_or(self.selected_group_index)
        .min(self.groups.len().saturating_sub(1));
        let user_groups = self.selected_user_groups();
        self.selected_user_group_index = identity_position(
            &user_groups,
            self.selected_user_group_gid,
            self.selected_user_group_name.as_deref(),
            |group| (group.gid, group.name.as_str()),
        )
        .unwrap_or(self.selected_user_group_index)
        .min(user_groups.len().saturating_sub(1));
        let members = self
            .groups
            .get(self.selected_group_index)
            .map_or(&[][..], |group| group.members.as_slice());
        self.selected_group_member_index = self
            .selected_group_member_name
            .as_deref()
            .and_then(|name| members.iter().position(|member| member == name))
            .unwrap_or(self.selected_group_member_index)
            .min(members.len().saturating_sub(1));
        self.capture_selection_identities();
    }

    /// Persist stable identities after a user-driven index movement.
    pub fn capture_selection_identities(&mut self) {
        let user = self.users.get(self.selected_user_index);
        self.selected_user_uid = user.map(|user| user.uid);
        self.selected_user_name = user.map(|user| user.name.clone());
        let group = self.groups.get(self.selected_group_index);
        self.selected_group_gid = group.map(|group| group.gid);
        self.selected_group_name = group.map(|group| group.name.clone());
        let user_group = self
            .selected_user_groups()
            .get(self.selected_user_group_index)
            .map(|group| (group.gid, group.name.clone()));
        self.selected_user_group_gid = user_group.as_ref().map(|(gid, _)| *gid);
        self.selected_user_group_name = user_group.map(|(_, name)| name);
        self.selected_group_member_name = self
            .groups
            .get(self.selected_group_index)
            .and_then(|group| group.members.get(self.selected_group_member_index))
            .cloned();
    }

    /// Groups the selected user belongs to (primary group and supplementary
    /// memberships). Resolved against `groups_all` so the Groups-tab filter
    /// and search never hide memberships in the Users tab.
    pub fn selected_user_groups(&self) -> Vec<&sys::SystemGroup> {
        self.users
            .get(self.selected_user_index)
            .and_then(|user| self.user_group_gids.get(&user.name))
            .map_or_else(Vec::new, |group_gids| {
                self.groups_all
                    .iter()
                    .filter(|group| group_gids.contains(&group.gid))
                    .collect()
            })
    }

    /// Entries of the "Modify user" menu for the selected user. The lock entry
    /// follows the cached shadow status: `Unlock account` for a locked
    /// account, `Lock account` otherwise. When the status is unknown (shadow
    /// unreadable or no record for the user) both are offered.
    pub fn modify_menu_items(&self) -> Vec<ModifyMenuItem> {
        let mut items = vec![
            ModifyMenuItem::AddGroup,
            ModifyMenuItem::RemoveGroup,
            ModifyMenuItem::Details,
            ModifyMenuItem::Password,
        ];
        let locked = self
            .users
            .get(self.selected_user_index)
            .and_then(|user| self.diagnostics.shadow.status(&user.name))
            .map(|status| status.locked);
        match locked {
            Some(true) => items.push(ModifyMenuItem::Unlock),
            Some(false) => items.push(ModifyMenuItem::Lock),
            None => items.extend([ModifyMenuItem::Lock, ModifyMenuItem::Unlock]),
        }
        items
    }

    /// Users the Groups-tab "Add members" (`adding`) or "Remove members"
    /// dialog offers for group `gid`, in display order, without a filter.
    pub fn group_member_candidates(&self, gid: u32, adding: bool) -> Vec<String> {
        self.group_member_candidate_list(gid, adding, "").names
    }

    /// The members dialog list for group `gid` narrowed by `filter`. The
    /// renderer and the key handler both index this list, so a highlighted
    /// row is always the name that gets submitted. Adding offers users who
    /// are not members yet; removing offers the current members.
    pub fn group_member_candidate_list(
        &self,
        gid: u32,
        adding: bool,
        filter: &str,
    ) -> CandidateList {
        let list = CandidateList::from_pool(self.group_member_pool(gid, adding), filter);
        if !adding {
            return list;
        }
        // Every user is refused for the same reason when the group is.
        let reason = self
            .groups_all
            .iter()
            .find(|group| group.gid == gid)
            .and_then(|group| self.adapter.membership_allowed(&group.name, gid).err());
        list.with_blocks(|_| reason)
    }

    /// Ticked names the members dialog for group `gid` still offers, in
    /// tick order. Ticks beyond the cap or hidden by the filter count.
    pub fn offered_group_members(&self, gid: u32, adding: bool, ticked: &[String]) -> Vec<String> {
        offered(&self.group_member_pool(gid, adding), ticked)
    }

    /// Every candidate of the members dialog for group `gid`, uncapped.
    fn group_member_pool(&self, gid: u32, adding: bool) -> Vec<&str> {
        let Some(group) = self
            .groups
            .iter()
            .chain(&self.groups_all)
            .find(|group| group.gid == gid)
        else {
            return Vec::new();
        };
        if adding {
            self.users_all
                .iter()
                .filter(|user| !group.members.contains(&user.name))
                .map(|user| user.name.as_str())
                .collect()
        } else {
            group.members.iter().map(String::as_str).collect()
        }
    }

    /// Groups the Users-tab "Add groups" (`adding`) or "Remove groups" dialog
    /// offers for user `uid`, in display order, without a filter.
    pub fn user_group_candidates(&self, uid: u32, adding: bool) -> Vec<String> {
        self.user_group_candidate_list(uid, adding, "").names
    }

    /// The groups dialog list for user `uid` narrowed by `filter`. Adding
    /// offers groups the user is not in; removing offers supplementary
    /// memberships. The primary group is never offered. Shared by renderer
    /// and key handler.
    pub fn user_group_candidate_list(&self, uid: u32, adding: bool, filter: &str) -> CandidateList {
        let list = CandidateList::from_pool(self.user_group_pool(uid, adding), filter);
        if !adding {
            return list;
        }
        // One lookup table instead of a scan of every group per entry. The
        // first group of a name wins, as in `membership_block`.
        let mut gids = HashMap::new();
        for group in &self.groups_all {
            gids.entry(group.name.as_str()).or_insert(group.gid);
        }
        list.with_blocks(|name| {
            gids.get(name)
                .and_then(|gid| self.adapter.membership_allowed(name, *gid).err())
        })
    }

    /// Every group for the primary-group picker, in `groups_all` order and
    /// without the membership-dialog cap, each with the reason the membership
    /// policy refuses it, if it does. Reads no host state.
    pub fn primary_group_candidate_list(&self) -> CandidateList {
        CandidateList::every(self.groups_all.iter().map(|group| group.name.as_str()))
            .with_blocks(|name| self.membership_block(name))
    }

    /// Why the policy refuses adding a user to group `name`, if it does.
    /// The GID comes from the cached group list; a group that is not in it
    /// is refused as missing. Reads no host state.
    pub fn membership_block(&self, name: &str) -> Option<&'static str> {
        match self.groups_all.iter().find(|group| group.name == name) {
            Some(group) => self.adapter.membership_allowed(name, group.gid).err(),
            None => Some("group does not exist"),
        }
    }

    /// Ticked names the groups dialog for user `uid` still offers, in tick
    /// order. Ticks beyond the cap or hidden by the filter count.
    pub fn offered_user_groups(&self, uid: u32, adding: bool, ticked: &[String]) -> Vec<String> {
        offered(&self.user_group_pool(uid, adding), ticked)
    }

    /// Every candidate of the groups dialog for user `uid`, uncapped.
    fn user_group_pool(&self, uid: u32, adding: bool) -> Vec<&str> {
        // Prefer the selected row so accounts sharing a UID resolve to the
        // name the user is looking at.
        let Some(user) = self
            .users
            .get(self.selected_user_index)
            .filter(|user| user.uid == uid)
            .or_else(|| {
                self.users
                    .iter()
                    .chain(&self.users_all)
                    .find(|user| user.uid == uid)
            })
        else {
            return Vec::new();
        };
        self.groups_all
            .iter()
            .filter(|group| {
                let member = group.members.iter().any(|name| name == &user.name);
                group.gid != user.primary_gid && if adding { !member } else { member }
            })
            .map(|group| group.name.as_str())
            .collect()
    }

    fn config_file_read_path(&self, name: &str) -> Option<String> {
        self.config_root_provider
            .roots()
            .into_iter()
            .map(|root| root.join(name))
            .find(|path| path.is_file())
            .map(|path| path.to_string_lossy().into_owned())
    }

    pub fn configuration_write_path(&self, name: &str) -> String {
        self.config_root_provider
            .roots()
            .into_iter()
            .next()
            .unwrap_or_else(|| PathBuf::from("."))
            .join(name)
            .to_string_lossy()
            .into_owned()
    }

    pub fn set_password_capability(&mut self, record: PasswordRecord) {
        self.pending_password = Some(OneShotPassword::new(record));
    }

    pub(crate) fn take_password_capability(&mut self) -> Option<PasswordRecord> {
        self.pending_password.take().map(OneShotPassword::take)
    }

    /// Attach an operation log (`--log-file`).
    pub fn set_logger(&mut self, logger: Logger) {
        self.logger = Some(logger);
    }

    /// Append one redacted event to the operation log, if one is open. A
    /// failed write closes the log and records why, so later events are not
    /// silently lost without notice.
    pub fn log_event(&mut self, kind: &str, message: &str) {
        let Some(logger) = self.logger.as_mut() else {
            return;
        };
        let seconds = self
            .clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_secs());
        if let Err(error) = logger.write_event(seconds, kind, message) {
            self.logger = None;
            self.record_diagnostic_message(format!("log file disabled: {error}"));
        }
    }

    /// The tab whose query is being edited, or the active tab outside search.
    pub fn search_tab(&self) -> ActiveTab {
        match self.input_mode {
            InputMode::SearchUsers => ActiveTab::Users,
            InputMode::SearchGroups => ActiveTab::Groups,
            InputMode::Normal | InputMode::Modal => self.active_tab,
        }
    }

    /// The search query that applies to `tab`.
    pub fn query(&self, tab: ActiveTab) -> &str {
        match tab {
            ActiveTab::Users => &self.users_query,
            ActiveTab::Groups => &self.groups_query,
        }
    }

    fn query_mut(&mut self, tab: ActiveTab) -> &mut String {
        match tab {
            ActiveTab::Users => &mut self.users_query,
            ActiveTab::Groups => &mut self.groups_query,
        }
    }

    /// `/`: edit the active tab's query in place, remembering it for Esc.
    pub fn start_search(&mut self) {
        let tab = self.active_tab;
        self.search_before_edit = self.query(tab).to_owned();
        self.input_mode = match tab {
            ActiveTab::Users => InputMode::SearchUsers,
            ActiveTab::Groups => InputMode::SearchGroups,
        };
    }

    /// Enter while searching: keep the query and return to the table.
    pub fn finish_search(&mut self) {
        self.search_before_edit.clear();
        self.input_mode = InputMode::Normal;
    }

    /// Esc while searching: put back the query from before `/` was pressed.
    pub fn cancel_search(&mut self) {
        let tab = self.search_tab();
        let previous = std::mem::take(&mut self.search_before_edit);
        *self.query_mut(tab) = previous;
        self.input_mode = InputMode::Normal;
        self.sort_and_filter();
    }

    /// Edit the query being searched and re-run the filters.
    pub fn edit_search(&mut self, edit: impl FnOnce(&mut String)) {
        let tab = self.search_tab();
        edit(self.query_mut(tab));
        self.sort_and_filter();
    }

    /// `s`: switch the active tab to its next row order. The selected row
    /// stays selected by its UID or GID.
    pub fn cycle_sort(&mut self) {
        match self.active_tab {
            ActiveTab::Users => self.users_sort = self.users_sort.next(),
            ActiveTab::Groups => self.groups_sort = self.groups_sort.next(),
        }
        self.sort_and_filter();
    }

    /// Clear the active tab's query (Esc in the main view).
    pub fn clear_search(&mut self) {
        let tab = self.active_tab;
        if !self.query(tab).is_empty() {
            self.query_mut(tab).clear();
            self.sort_and_filter();
        }
    }

    /// The dialog on top of the stack, the one shown and receiving keys.
    pub fn modal(&self) -> Option<&ModalState> {
        self.modals.last()
    }

    pub fn modal_mut(&mut self) -> Option<&mut ModalState> {
        self.modals.last_mut()
    }

    /// Number of open dialogs.
    pub fn modal_depth(&self) -> usize {
        self.modals.len()
    }

    /// Open a dialog on top of the current one.
    pub fn push_modal(&mut self, modal: ModalState) {
        self.modals.push(modal);
        self.input_mode = InputMode::Modal;
    }

    /// Close the top dialog, returning to the one below it, or to the main
    /// view when it was the last.
    pub fn pop_modal(&mut self) -> Option<ModalState> {
        self.modal_pops = self.modal_pops.wrapping_add(1);
        let modal = self.modals.pop();
        if self.modals.is_empty() {
            self.input_mode = InputMode::Normal;
        }
        modal
    }

    /// Close every dialog and return to the main view.
    pub fn close_modals(&mut self) {
        self.modal_pops = self.modal_pops.wrapping_add(1);
        self.modals.clear();
        self.input_mode = InputMode::Normal;
    }

    /// Take the top dialog out so a key handler can borrow it mutably while
    /// also mutating the app. Its slot keeps a placeholder, so a dialog the
    /// handler pushes lands above it and a pop or clear removes it.
    pub(crate) fn detach_top_modal(&mut self) -> Option<(ModalState, DetachedModal)> {
        let index = self.modals.len().checked_sub(1)?;
        let placeholder = ModalState::Help { scroll: 0 };
        let modal = std::mem::replace(&mut self.modals[index], placeholder);
        Some((
            modal,
            DetachedModal {
                index,
                pops: self.modal_pops,
            },
        ))
    }

    /// Put a detached dialog back unless the handler popped or cleared it.
    pub(crate) fn reattach_modal(&mut self, modal: ModalState, detached: DetachedModal) {
        if self.modal_pops == detached.pops
            && let Some(slot) = self.modals.get_mut(detached.index)
        {
            *slot = modal;
        }
    }

    pub fn clear_pending_operation(&mut self) {
        self.pending_operation = None;
        self.pending_password = None;
        self.last_preview.clear();
    }
}

/// Where a dialog was taken from by `AppState::detach_top_modal`.
pub(crate) struct DetachedModal {
    index: usize,
    pops: u64,
}

impl Default for AppState {
    fn default() -> Self {
        Self::new()
    }
}

/// Candidate roots in priority order for config files.
fn config_roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME")
        && !xdg.trim().is_empty()
    {
        roots.push(PathBuf::from(xdg).join("UsrGrpManager"));
    }
    if let Some(home) = dirs_next::home_dir() {
        roots.push(home.join(".config").join("UsrGrpManager"));
        roots.push(home.join("UsrGrpManager"));
    }
    roots
}

pub fn config_file_read_path(name: &str) -> Option<String> {
    config_roots()
        .into_iter()
        .map(|root| root.join(name))
        .find(|path| path.is_file())
        .map(|path| path.to_string_lossy().into_owned())
}

/// Select a config path without creating directories or discarding errors.
pub fn config_file_write_path(name: &str) -> String {
    config_roots()
        .into_iter()
        .next()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(name)
        .to_string_lossy()
        .into_owned()
}

pub use update::run_app as run;
pub use update::run_app_with as run_with;

/// Mirror the W3 default fail-closed policy in presentation only. Trusted
/// preparation remains the enforcement point and supports explicit injected
/// exceptions; UI never offers a misleading success claim for default-protected
/// identities.
pub fn is_default_protected_user(user: &sys::SystemUser) -> bool {
    user.uid == 0 || user.name == "root" || user.uid < 1000
}

pub fn is_default_protected_group(group: &sys::SystemGroup) -> bool {
    group.gid == 0 || group.name == "root" || group.gid < 1000
}

pub fn is_default_elevation_group(name: &str) -> bool {
    matches!(name, "sudo" | "wheel")
}

pub fn sudo_group_name() -> String {
    std::env::var("UGM_SUDO_GROUP").unwrap_or_else(|_| "wheel".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{civil_date_label, read_process_counts, status_real_uid};

    #[test]
    fn civil_date_label_converts_days_since_the_epoch() {
        assert_eq!(civil_date_label(0), "1970-01-01");
        assert_eq!(civil_date_label(-1), "1969-12-31");
        assert_eq!(civil_date_label(59), "1970-03-01");
        // Leap day in a year divisible by 400, and the day after.
        assert_eq!(civil_date_label(11_016), "2000-02-29");
        assert_eq!(civil_date_label(11_017), "2000-03-01");
        assert_eq!(civil_date_label(19_675), "2023-11-14");
        assert_eq!(civil_date_label(20_516), "2026-03-04");
        // 2100 is not a leap year.
        assert_eq!(civil_date_label(47_540), "2100-02-28");
        assert_eq!(civil_date_label(47_541), "2100-03-01");
    }

    fn write_status(root: &std::path::Path, pid: &str, status: &str) {
        let dir = root.join(pid);
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("status"), status).unwrap();
    }

    #[test]
    fn status_real_uid_reads_the_first_uid_field() {
        assert_eq!(
            status_real_uid("Name:\tbash\nUid:\t1000\t0\t0\t0\nGid:\t1000\n"),
            Some(1000)
        );
        assert_eq!(status_real_uid("Name:\tbash\n"), None);
        assert_eq!(status_real_uid("Uid:\tnope\n"), None);
    }

    /// A fake /proc tree with two users' processes and non-process entries.
    fn fake_proc() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        write_status(root.path(), "1", "Name:\tinit\nUid:\t0\t0\t0\t0\n");
        write_status(
            root.path(),
            "20",
            "Name:\tsh\nUid:\t1000\t1000\t1000\t1000\n",
        );
        write_status(root.path(), "21", "Name:\tvim\nUid:\t1000\t0\t0\t0\n");
        // Not process directories.
        write_status(root.path(), "self", "Name:\tx\n");
        std::fs::write(root.path().join("uptime"), "1.0 1.0\n").unwrap();
        root
    }

    /// The fake /proc lives in a temporary directory; the host /proc is never read.
    #[test]
    fn process_counts_group_pid_directories_by_real_uid() {
        let root = fake_proc();
        let counts = read_process_counts(root.path()).unwrap();
        assert_eq!(counts.get(&1000), Some(&2));
        assert_eq!(counts.get(&0), Some(&1));
        assert_eq!(counts.len(), 2);

        assert!(read_process_counts(&root.path().join("missing")).is_none());
    }

    #[test]
    fn process_counts_skip_a_process_that_exited_mid_scan() {
        let root = fake_proc();
        // A PID directory whose status file is gone opens with NotFound.
        std::fs::create_dir(root.path().join("30")).unwrap();
        let counts = read_process_counts(root.path()).unwrap();
        assert_eq!(counts.get(&1000), Some(&2));
        assert_eq!(counts.get(&0), Some(&1));
    }

    #[test]
    fn process_counts_fail_closed_on_a_status_read_error() {
        let root = fake_proc();
        // Opening a directory succeeds; reading it fails with a non-NotFound error.
        std::fs::create_dir_all(root.path().join("40").join("status")).unwrap();
        assert!(read_process_counts(root.path()).is_none());
    }

    #[test]
    fn process_counts_fail_closed_on_a_status_without_uid() {
        let root = fake_proc();
        write_status(root.path(), "50", "Name:\todd\nGid:\t0\n");
        assert!(read_process_counts(root.path()).is_none());

        let root = fake_proc();
        write_status(root.path(), "51", "Name:\todd\nUid:\tnope\t0\n");
        assert!(read_process_counts(root.path()).is_none());
    }
}
