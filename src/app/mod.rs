//! Application state, explicit effects, and configuration entry points.
//!
//! Construction is pure by default.  Host reads, diagnostics, configuration
//! loading, and mutations are explicit effects owned by `update` rather than
//! rendering or tests.
pub mod filterconf;
pub mod keymap;
pub mod update;

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

/// Color palette for theming the TUI.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
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
    /// Dark default theme with neutral grays and cyan accents.
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
        Self {
            text: Color::Rgb(0xcd, 0xd6, 0xf4),
            _muted: Color::Rgb(0x7f, 0x84, 0x9c),
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
    pub fn from_file(path: &str) -> std::io::Result<Self> {
        let contents = crate::config::read_bounded(path)?;
        let mut theme = Self::mocha();
        let mut seen = std::collections::HashSet::new();
        for assignment in crate::config::parse_assignments(&contents)? {
            let key = assignment.key.as_str();
            if !seen.insert(key.to_owned()) {
                return config_line_error(assignment.line, "duplicate theme key");
            }
            let Some(color) = Self::parse_color(&assignment.value) else {
                return config_line_error(assignment.line, "invalid theme color");
            };
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
                _ => return config_line_error(assignment.line, "unknown theme key"),
            }
        }
        Ok(theme)
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

    /// Atomically persist every theme field in the canonical format.
    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        let mut contents = String::from(
            "# usrgrp-manager theme configuration\n# Format: key = value\n# Colors: #RRGGBB, index:N, named:color, or reset\n\n",
        );
        for (key, color) in [
            ("text", self.text),
            ("muted", self._muted),
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
        password: SecretInput,
        confirm: SecretInput,
        must_change: bool,
    },
    /// The exact redacted bridge plan is visible before it can run.
    OperationConfirm {
        selected: usize,
        action: String,
        preview: Vec<String>,
    },
    Info {
        message: String,
    },
    Help {
        scroll: u16,
    },
    SudoPrompt {
        password: SecretInput,
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
        password: SecretInput,
        confirm: SecretInput,
        create_home: bool,
        add_to_wheel: bool,
    },
}

#[derive(Clone, Debug)]
pub enum ModifyField {
    Username,
    Fullname,
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
        add_to_wheel: bool,
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
    pub fn is_empty(&self) -> bool {
        self.value.is_empty()
    }
    pub fn matches(&self, other: &Self) -> bool {
        self.value == other.value
    }
    pub fn take(&mut self) -> String {
        std::mem::take(&mut self.value)
    }
}

impl Drop for SecretInput {
    fn drop(&mut self) {
        use zeroize::Zeroize;
        self.value.zeroize();
    }
}

/// Cached, bounded diagnostics populated only by explicit refresh effects.
#[derive(Clone, Debug, Default)]
pub struct CachedDiagnostics {
    pub shadow: crate::search::ShadowState,
    pub homes: HashMap<String, HomeDiagnostics>,
    /// Group summaries are computed during refresh, never by a frame render.
    pub groups: HashMap<u32, GroupDiagnostics>,
    pub group_mtime_days: Option<u64>,
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
            ..CachedDiagnostics::default()
        };
        let mut remaining_bytes = MAX_TOTAL_DIAGNOSTIC_BYTES;
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
    pub selected_user_uid: Option<u32>,
    pub selected_group_gid: Option<u32>,
    pub selected_user_group_gid: Option<u32>,
    pub selected_group_member_name: Option<String>,
    /// Bounded, precomputed primary and supplementary memberships by user.
    /// Renderers consume this cache instead of scanning every group's members.
    pub user_group_gids: HashMap<String, HashSet<u32>>,
    pub rows_per_page: usize,
    pub _table_state: TableState,
    pub input_mode: InputMode,
    pub search_query: String,
    pub theme: Theme,
    pub keymap: keymap::Keymap,
    pub modal: Option<ModalState>,
    pub users_focus: UsersFocus,
    pub groups_focus: GroupsFocus,
    pub users_filter: Option<UsersFilter>,
    pub groups_filter: Option<GroupsFilter>,
    pub users_filter_chips: UsersFilterChips,
    pub actions_context: Option<ActionsContext>,
    pub show_keybinds: bool,
    /// Adapter-owned authority for refresh, preparation, elevation and execution.
    pub adapter: Arc<sys::SystemAdapter>,
    pub account_snapshot: Option<AccountSnapshot>,
    pub diagnostics: CachedDiagnostics,
    clock: Arc<dyn Clock>,
    diagnostic_provider: Arc<dyn DiagnosticProvider>,
    config_root_provider: Arc<dyn ConfigRootProvider>,
    pub current_username: String,
    pub last_preview: Vec<String>,
    pub last_report: Option<OperationReport>,
    /// The only application-held operation capability. It contains the exact
    /// trusted plan shown to the user and drops on cancel/error.
    pub pending_operation: Option<PreparedOperation>,
    /// Password material is never queued or reported. This dedicated,
    /// non-cloneable capability exists only while converting a password modal
    /// into its one exact trusted operation.
    pending_password: Option<OneShotPassword>,
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
            selected_group_gid: None,
            selected_user_group_gid: None,
            selected_group_member_name: None,
            user_group_gids: HashMap::new(),
            rows_per_page: 10,
            _table_state: TableState::default(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            theme: Theme::mocha(),
            keymap: keymap::Keymap::default(),
            modal: None,
            users_focus: UsersFocus::UsersList,
            groups_focus: GroupsFocus::GroupsList,
            users_filter: None,
            groups_filter: None,
            users_filter_chips: UsersFilterChips::default(),
            actions_context: None,
            show_keybinds: true,
            adapter,
            account_snapshot: Some(snapshot),
            diagnostics: CachedDiagnostics::default(),
            clock,
            diagnostic_provider,
            config_root_provider,
            current_username: "unknown".to_owned(),
            last_preview: Vec::new(),
            last_report: None,
            pending_operation: None,
            pending_password: None,
        };
        app.sort_and_filter();
        app
    }

    /// Production construction with explicit account/config effects.
    pub fn load_system() -> Self {
        let mut app = Self::new();
        app.refresh_accounts();
        app.current_username = sys::current_username().unwrap_or_else(|| "unknown".to_owned());
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
        let theme_path = self
            .config_file_read_path("theme.conf")
            .unwrap_or_else(|| self.configuration_write_path("theme.conf"));
        match Theme::load_or_init(&theme_path) {
            Ok(theme) => self.theme = theme,
            Err(error) => self.record_config_message("theme", &error),
        }
        let keymap_path = self
            .config_file_read_path("keybinds.conf")
            .unwrap_or_else(|| self.configuration_write_path("keybinds.conf"));
        match keymap::Keymap::load_or_init(&keymap_path) {
            Ok(keymap) => self.keymap = keymap,
            Err(error) => self.record_config_message("keymap", &error),
        }
        let filters_path = self
            .config_file_read_path("filter.conf")
            .unwrap_or_else(|| self.configuration_write_path("filter.conf"));
        match filterconf::FiltersConfig::load_or_init(&filters_path) {
            Ok(filters) => filters.apply_to(self),
            Err(error) => self.record_config_message("filter", &error),
        }
        self.sort_and_filter();
    }

    /// Retain only a small amount of non-secret configuration diagnostics.
    pub fn record_config_message(&mut self, source: &str, error: &std::io::Error) {
        const MAX_CONFIG_MESSAGES: usize = 3;
        const MAX_CONFIG_MESSAGE_BYTES: usize = 256;
        if self.diagnostics.config_messages.len() >= MAX_CONFIG_MESSAGES {
            return;
        }
        let mut message = format!("{source} configuration: {error}");
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
        self.users_all.sort_by_key(|user| user.uid);
        self.groups_all.sort_by_key(|group| group.gid);
        self.rebuild_user_group_cache();
        crate::search::apply_filters_and_search(self);
        self.normalize_selections();
    }

    fn rebuild_user_group_cache(&mut self) {
        const MAX_USERS: usize = 10_000;
        const MAX_GROUPS: usize = 10_000;
        const MAX_MEMBERSHIP_EDGES: usize = 100_000;

        self.user_group_gids.clear();
        for user in self.users_all.iter().take(MAX_USERS) {
            self.user_group_gids
                .entry(user.name.clone())
                .or_default()
                .insert(user.primary_gid);
        }
        let mut remaining_edges = MAX_MEMBERSHIP_EDGES;
        for group in self.groups_all.iter().take(MAX_GROUPS) {
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
        self.selected_user_index = self
            .selected_user_uid
            .and_then(|uid| self.users.iter().position(|user| user.uid == uid))
            .unwrap_or(self.selected_user_index)
            .min(self.users.len().saturating_sub(1));
        self.selected_group_index = self
            .selected_group_gid
            .and_then(|gid| self.groups.iter().position(|group| group.gid == gid))
            .unwrap_or(self.selected_group_index)
            .min(self.groups.len().saturating_sub(1));
        let user_groups = self.selected_user_groups();
        self.selected_user_group_index = self
            .selected_user_group_gid
            .and_then(|gid| user_groups.iter().position(|group| group.gid == gid))
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
        self.selected_user_uid = self
            .users
            .get(self.selected_user_index)
            .map(|user| user.uid);
        self.selected_group_gid = self
            .groups
            .get(self.selected_group_index)
            .map(|group| group.gid);
        self.selected_user_group_gid = self
            .selected_user_groups()
            .get(self.selected_user_group_index)
            .map(|group| group.gid);
        self.selected_group_member_name = self
            .groups
            .get(self.selected_group_index)
            .and_then(|group| group.members.get(self.selected_group_member_index))
            .cloned();
    }

    pub fn selected_user_groups(&self) -> Vec<&sys::SystemGroup> {
        self.users
            .get(self.selected_user_index)
            .and_then(|user| self.user_group_gids.get(&user.name))
            .map_or_else(Vec::new, |group_gids| {
                self.groups
                    .iter()
                    .filter(|group| group_gids.contains(&group.gid))
                    .collect()
            })
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

    pub fn clear_pending_operation(&mut self) {
        self.pending_operation = None;
        self.pending_password = None;
        self.last_preview.clear();
    }
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
