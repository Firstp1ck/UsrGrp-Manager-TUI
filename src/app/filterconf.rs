//! Filters configuration: parse/write `filter.conf` and apply to AppState.

use super::{AppState, GroupsFilter, UsersFilter};

#[derive(Clone, Debug, Default)]
pub struct FiltersConfig {
    // Top-level filters (optional)
    pub users_filter: Option<UsersFilter>,
    pub groups_filter: Option<GroupsFilter>,

    // Chip filters for users
    pub human_only: bool,
    pub system_only: bool,
    pub inactive: bool,
    pub no_home: bool,
    pub locked: bool,
    pub no_password: bool,
    pub expired: bool,
}

impl FiltersConfig {
    pub fn default_all_false() -> Self {
        Self::default()
    }

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

    pub fn save_from_app(app: &AppState, path: &str) -> std::io::Result<()> {
        Self::from_app(app).write_file(path)
    }

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

    pub fn from_file(path: &str) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        let mut cfg = Self::default_all_false();
        for raw in contents.lines() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') { continue; }
            let mut parts = line.splitn(2, '=');
            let lhs = parts.next().map(|s| s.trim()).unwrap_or("");
            let rhs = parts.next().map(|s| s.trim()).unwrap_or("");
            if lhs.is_empty() || rhs.is_empty() { continue; }

            match lhs {
                // UsersFilter
                "users_filter" => {
                    cfg.users_filter = match rhs {
                        "OnlyUserIds" | "users" | "user_ids" => Some(UsersFilter::OnlyUserIds),
                        "OnlySystemIds" | "system" | "system_ids" => Some(UsersFilter::OnlySystemIds),
                        "None" | "none" | "" => None,
                        _ => cfg.users_filter,
                    };
                }
                // GroupsFilter
                "groups_filter" => {
                    cfg.groups_filter = match rhs {
                        "OnlyUserGids" | "user_gids" | "users" => Some(GroupsFilter::OnlyUserGids),
                        "OnlySystemGids" | "system_gids" | "system" => Some(GroupsFilter::OnlySystemGids),
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


