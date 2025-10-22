//! Filters configuration: parse/write `filter.conf` and apply to AppState.
//!
//! This module manages filter settings that can be persisted to a configuration file.
//! It supports:
//! - Loading filter preferences from `filter.conf`
//! - Saving current filter state back to the file
//! - Applying filters to the application state

use super::{AppState, GroupsFilter, UsersFilter};

/// Represents filter settings that can be loaded from or saved to a configuration file.
///
/// Filters control which users and groups are visible in the UI. They can be either
/// top-level (mutually exclusive) or combinable "chips" (multiple can be active at once).
#[derive(Clone, Debug, Default)]
pub struct FiltersConfig {
    /// Top-level filter for users (optional): show only user or system accounts.
    pub users_filter: Option<UsersFilter>,
    /// Top-level filter for groups (optional): show only user or system groups.
    pub groups_filter: Option<GroupsFilter>,

    /// Show only users with UID >= 1000 (human/regular accounts).
    pub human_only: bool,
    /// Show only users with UID < 1000 (system accounts).
    pub system_only: bool,
    /// Show only inactive users (those with nologin or false shells).
    pub inactive: bool,
    /// Show only users whose home directory does not exist.
    pub no_home: bool,
    /// Show only users with locked passwords (from `/etc/shadow`).
    pub locked: bool,
    /// Show only users with no password set (empty password field).
    pub no_password: bool,
    /// Show only users whose password has expired.
    pub expired: bool,
}

impl FiltersConfig {
    /// Create a filters configuration with all options disabled/empty.
    ///
    /// This is equivalent to `Default::default()`.
    pub fn default_all_false() -> Self {
        Self::default()
    }

    /// Extract the current filter state from an [`AppState`].
    ///
    /// This method reads the current filter settings from the application state
    /// and converts them into a [`FiltersConfig`] for saving or exporting.
    ///
    /// # Arguments
    ///
    /// * `app` - The application state to extract filters from.
    pub fn from_app(app: &AppState) -> Self {
        Self {
            users_filter: app.users_filter,
            groups_filter: app.groups_filter,
            human_only: app.users_filter_chips.human_only,
            system_only: app.users_filter_chips.system_only,
            inactive: app.users_filter_chips.inactive,
            no_home: app.users_filter_chips.no_home,
            locked: app.users_filter_chips.locked,
            no_password: app.users_filter_chips.no_password,
            expired: app.users_filter_chips.expired,
        }
    }

    /// Save the current filter state from an [`AppState`] to a file.
    ///
    /// This is a convenience method that combines [`from_app`](Self::from_app) and
    /// [`write_file`](Self::write_file).
    ///
    /// # Arguments
    ///
    /// * `app` - The application state containing the current filters.
    /// * `path` - The path where the configuration will be written.
    pub fn save_from_app(app: &AppState, path: &str) -> std::io::Result<()> {
        Self::from_app(app).write_file(path)
    }

    /// Load filters from a file, or create defaults if the file doesn't exist.
    ///
    /// This is the main entry point for loading filter configuration. It first checks
    /// if the specified path exists; if not, it searches standard config locations.
    /// If still not found, it creates a default (all filters off) and writes it to
    /// the specified path for future customization.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the filters configuration file.
    pub fn load_or_init(path: &str) -> Self {
        let p = std::path::Path::new(path);
        if p.exists() {
            return Self::from_file(path).unwrap_or_else(Self::default_all_false);
        }
        // try read path resolution in case caller passed a write path but an existing file is elsewhere
        if let Some(existing) = crate::app::config_file_read_path("filter.conf") {
            return Self::from_file(&existing).unwrap_or_else(Self::default_all_false);
        }
        let cfg = Self::default_all_false();
        let _ = cfg.write_file(path);
        cfg
    }

    /// Load filters from a configuration file.
    ///
    /// The file should use the format: `<key> = <value>`. Comments (lines starting with '#')
    /// and empty lines are ignored. Unknown keys are skipped silently.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to the filters configuration file.
    ///
    /// # Returns
    ///
    /// `Some(config)` if the file exists and is readable; `None` otherwise.
    pub fn from_file(path: &str) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        let mut cfg = Self::default_all_false();
        for raw in contents.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut parts = line.splitn(2, '=');
            let lhs = parts.next().map(|s| s.trim()).unwrap_or("");
            let rhs = parts.next().map(|s| s.trim()).unwrap_or("");
            if lhs.is_empty() || rhs.is_empty() {
                continue;
            }

            match lhs {
                // UsersFilter
                "users_filter" => {
                    cfg.users_filter = match rhs {
                        "OnlyUserIds" | "users" | "user_ids" => Some(UsersFilter::OnlyUserIds),
                        "OnlySystemIds" | "system" | "system_ids" => {
                            Some(UsersFilter::OnlySystemIds)
                        }
                        "None" | "none" | "" => None,
                        _ => cfg.users_filter,
                    };
                }
                // GroupsFilter
                "groups_filter" => {
                    cfg.groups_filter = match rhs {
                        "OnlyUserGids" | "user_gids" | "users" => Some(GroupsFilter::OnlyUserGids),
                        "OnlySystemGids" | "system_gids" | "system" => {
                            Some(GroupsFilter::OnlySystemGids)
                        }
                        "None" | "none" | "" => None,
                        _ => cfg.groups_filter,
                    };
                }
                // Chips
                "human_only" => cfg.human_only = parse_bool(rhs),
                "system_only" => cfg.system_only = parse_bool(rhs),
                "inactive" => cfg.inactive = parse_bool(rhs),
                "no_home" => cfg.no_home = parse_bool(rhs),
                "locked" => cfg.locked = parse_bool(rhs),
                "no_password" => cfg.no_password = parse_bool(rhs),
                "expired" => cfg.expired = parse_bool(rhs),
                _ => {}
            }
        }
        Some(cfg)
    }

    /// Write the current filter state to a configuration file.
    ///
    /// This method writes the current filter settings to the specified path in the
    /// format: `<key> = <value>`.
    ///
    /// # Arguments
    ///
    /// * `path` - The path where the configuration will be written.
    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        use std::fmt::Write as _;
        let mut buf = String::new();
        buf.push_str("# usrgrp-manager filters\n");
        buf.push_str("# Default: all unset/false. Set to true to enable.\n");
        buf.push_str("# Users filter: None|OnlyUserIds|OnlySystemIds\n");
        buf.push_str("users_filter = None\n");
        buf.push_str("# Groups filter: None|OnlyUserGids|OnlySystemGids\n");
        buf.push_str("groups_filter = None\n\n");

        let mut kv = |k: &str, v: bool| {
            let _ = writeln!(&mut buf, "{} = {}", k, if v { "true" } else { "false" });
        };
        kv("human_only", self.human_only);
        kv("system_only", self.system_only);
        kv("inactive", self.inactive);
        kv("no_home", self.no_home);
        kv("locked", self.locked);
        kv("no_password", self.no_password);
        kv("expired", self.expired);

        std::fs::write(path, buf)
    }

    /// Apply the current filter state to an [`AppState`].
    ///
    /// This method updates the application state with the current filter settings.
    ///
    /// # Arguments
    ///
    /// * `app` - The application state to update.
    pub fn apply_to(&self, app: &mut AppState) {
        app.users_filter = self.users_filter;
        app.groups_filter = self.groups_filter;
        app.users_filter_chips.human_only = self.human_only;
        app.users_filter_chips.system_only = self.system_only;
        app.users_filter_chips.inactive = self.inactive;
        app.users_filter_chips.no_home = self.no_home;
        app.users_filter_chips.locked = self.locked;
        app.users_filter_chips.no_password = self.no_password;
        app.users_filter_chips.expired = self.expired;
    }
}

fn parse_bool(s: &str) -> bool {
    matches!(s.to_ascii_lowercase().as_str(), "1" | "true" | "yes" | "on")
}
