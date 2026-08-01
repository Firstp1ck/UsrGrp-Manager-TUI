//! Durable filters configuration.

use super::{AppState, GroupsFilter, UsersFilter};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FiltersConfig {
    pub users_filter: Option<UsersFilter>,
    pub groups_filter: Option<GroupsFilter>,
    pub human_only: bool,
    pub system_only: bool,
    pub inactive: bool,
    pub no_home: bool,
    pub locked: bool,
    pub no_password: bool,
    pub expired: bool,
}

impl FiltersConfig {
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

    pub fn load_or_init(path: &str) -> std::io::Result<Self> {
        match Self::from_file(path) {
            Ok(config) => Ok(config),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                let config = Self::default();
                config.write_file(path)?;
                Ok(config)
            }
            Err(error) => Err(error),
        }
    }

    pub fn from_file(path: &str) -> std::io::Result<Self> {
        let contents = crate::config::read_bounded(path)?;
        let mut config = Self::default();
        let mut seen = std::collections::HashSet::new();
        for assignment in crate::config::parse_assignments(&contents)? {
            let key = assignment.key.as_str();
            let value = assignment.value.as_str();
            if !seen.insert(key.to_owned()) {
                return invalid_line(assignment.line, "duplicate filter key");
            }
            match key {
                "users_filter" => {
                    config.users_filter = match value {
                        "None" | "none" => None,
                        "OnlyUserIds" => Some(UsersFilter::OnlyUserIds),
                        "OnlySystemIds" => Some(UsersFilter::OnlySystemIds),
                        _ => return invalid_line(assignment.line, "invalid users_filter value"),
                    };
                }
                "groups_filter" => {
                    config.groups_filter = match value {
                        "None" | "none" => None,
                        "OnlyUserGids" => Some(GroupsFilter::OnlyUserGids),
                        "OnlySystemGids" => Some(GroupsFilter::OnlySystemGids),
                        _ => return invalid_line(assignment.line, "invalid groups_filter value"),
                    };
                }
                "human_only" => config.human_only = parse_bool(assignment.line, value)?,
                "system_only" => config.system_only = parse_bool(assignment.line, value)?,
                "inactive" => config.inactive = parse_bool(assignment.line, value)?,
                "no_home" => config.no_home = parse_bool(assignment.line, value)?,
                "locked" => config.locked = parse_bool(assignment.line, value)?,
                "no_password" => config.no_password = parse_bool(assignment.line, value)?,
                "expired" => config.expired = parse_bool(assignment.line, value)?,
                _ => return invalid_line(assignment.line, "unknown filter key"),
            }
        }
        Ok(config)
    }

    pub fn write_file(&self, path: &str) -> std::io::Result<()> {
        let users_filter = match self.users_filter {
            None => "None",
            Some(UsersFilter::OnlyUserIds) => "OnlyUserIds",
            Some(UsersFilter::OnlySystemIds) => "OnlySystemIds",
        };
        let groups_filter = match self.groups_filter {
            None => "None",
            Some(GroupsFilter::OnlyUserGids) => "OnlyUserGids",
            Some(GroupsFilter::OnlySystemGids) => "OnlySystemGids",
        };
        let contents = format!(
            "# usrgrp-manager filters\n# Canonical values are lossless and atomically saved.\nusers_filter = {users_filter}\ngroups_filter = {groups_filter}\nhuman_only = {}\nsystem_only = {}\ninactive = {}\nno_home = {}\nlocked = {}\nno_password = {}\nexpired = {}\n",
            self.human_only,
            self.system_only,
            self.inactive,
            self.no_home,
            self.locked,
            self.no_password,
            self.expired,
        );
        crate::config::atomic_write(path, contents.as_bytes())
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

fn parse_bool(line: usize, value: &str) -> std::io::Result<bool> {
    match value {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => invalid_line(line, "expected true or false"),
    }
}

fn invalid_line<T>(line: usize, reason: &str) -> std::io::Result<T> {
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidData,
        format!("configuration line {line}: {reason}"),
    ))
}
