//! Pure search/filter reducers plus an explicit shadow-refresh effect.

use std::{
    collections::HashMap,
    fs::File,
    io::Read,
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_SHADOW_BYTES: usize = 1024 * 1024;
const MAX_SHADOW_RECORD_BYTES: usize = 8192;
const MAX_QUERY_BYTES: usize = 256;

use crate::app::{AppState, GroupsFilter, InputMode, UsersFilter};

/// Password status cached during an explicit refresh.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct ShadowStatus {
    pub locked: bool,
    pub no_password: bool,
    pub expired: bool,
    pub last_change_days: Option<i64>,
    pub expire_abs_days: Option<i64>,
}

/// Shadow data is never guessed from metadata.  Unavailable is distinct from
/// known false and causes shadow-dependent filters to remain visibly inactive.
#[derive(Clone, Debug)]
pub enum ShadowState {
    Known(HashMap<String, ShadowStatus>),
    Unavailable { reason: String },
}

impl Default for ShadowState {
    fn default() -> Self {
        Self::Unavailable {
            reason: "shadow data has not been refreshed".to_owned(),
        }
    }
}

/// Per-account shadow status. A readable source can still have no usable
/// record for a passwd account, which is explicitly `Unknown` rather than a
/// false status or a source-wide availability claim.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AccountShadowState {
    Known(ShadowStatus),
    Unknown { reason: &'static str },
    Unavailable { reason: String },
}

impl ShadowState {
    /// Compatibility lookup for renderers that only need a known value.
    pub fn status(&self, username: &str) -> Option<&ShadowStatus> {
        match self {
            Self::Known(statuses) => statuses.get(username),
            Self::Unavailable { .. } => None,
        }
    }

    /// Return the honest account-level state required by filters and reports.
    pub fn account_status(&self, username: &str) -> AccountShadowState {
        match self {
            Self::Known(statuses) => statuses
                .get(username)
                .cloned()
                .map(AccountShadowState::Known)
                .unwrap_or(AccountShadowState::Unknown {
                    reason: "no usable shadow record for local passwd account",
                }),
            Self::Unavailable { reason } => AccountShadowState::Unavailable {
                reason: reason.clone(),
            },
        }
    }

    pub fn availability_label(&self) -> &'static str {
        match self {
            Self::Known(_) => "known",
            Self::Unavailable { .. } => "unavailable",
        }
    }
}

/// Explicitly read `/etc/shadow` once.  This is an application effect, never a
/// renderer or filter side effect.  Actual open/read results determine state.
pub fn read_shadow_state() -> ShadowState {
    #[cfg(target_os = "linux")]
    {
        match read_shadow_file() {
            Ok(contents) => ShadowState::Known(parse_shadow_records(&contents, today_days())),
            Err(error) => ShadowState::Unavailable {
                reason: format!("shadow refresh failed: {}", error.kind()),
            },
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        ShadowState::Unavailable {
            reason: "shadow data is supported only on Linux".to_owned(),
        }
    }
}

fn today_days() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| (duration.as_secs() / 86_400) as i64)
        .unwrap_or_default()
}

fn read_shadow_file() -> std::io::Result<String> {
    let mut file = File::open("/etc/shadow")?;
    let mut bytes = Vec::with_capacity(MAX_SHADOW_BYTES.min(8192));
    file.by_ref()
        .take((MAX_SHADOW_BYTES + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.len() > MAX_SHADOW_BYTES {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "shadow file exceeds configured byte limit",
        ));
    }
    String::from_utf8(bytes)
        .map_err(|_| std::io::Error::new(std::io::ErrorKind::InvalidData, "shadow is not UTF-8"))
}

/// Truncate a query by bytes while preserving a valid UTF-8 boundary.
pub fn truncate_query_bytes(query: &str) -> String {
    if query.len() <= MAX_QUERY_BYTES {
        return query.to_owned();
    }
    let mut end = MAX_QUERY_BYTES;
    while !query.is_char_boundary(end) {
        end -= 1;
    }
    query[..end].to_owned()
}

pub fn parse_shadow_records(contents: &str, today: i64) -> HashMap<String, ShadowStatus> {
    const MAX_RECORDS: usize = 100_000;
    let mut statuses = HashMap::new();
    for line in contents
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .take(MAX_RECORDS)
    {
        if line.len() > MAX_SHADOW_RECORD_BYTES {
            continue;
        }
        let fields: Vec<_> = line.split(':').collect();
        if fields.len() < 2 || fields[0].is_empty() {
            continue;
        }
        let password = fields[1];
        let last_change = fields.get(2).and_then(|value| value.parse::<i64>().ok());
        let maximum_age = fields.get(4).and_then(|value| value.parse::<i64>().ok());
        let absolute_expiry = fields.get(7).and_then(|value| value.parse::<i64>().ok());
        // `chage -d 0` records day zero to force a password change. Preserve
        // that distinct must-change state rather than treating it as unknown.
        let must_change = last_change == Some(0);
        let expired_by_age = matches!((last_change, maximum_age), (Some(last), Some(maximum)) if last > 0 && maximum >= 0 && last + maximum <= today);
        let expired_by_date = absolute_expiry.is_some_and(|expiry| expiry >= 0 && expiry <= today);
        statuses.insert(
            fields[0].to_owned(),
            ShadowStatus {
                locked: password.starts_with('!') || password == "*",
                no_password: password.is_empty(),
                expired: must_change || expired_by_age || expired_by_date,
                last_change_days: last_change.filter(|value| *value >= 0),
                expire_abs_days: absolute_expiry.filter(|value| *value >= 0),
            },
        );
    }
    statuses
}

/// Purely filter `AppState` from cached state.  It performs no filesystem or
/// process I/O and preserves selected identities where still visible.
pub fn apply_filters_and_search(app: &mut AppState) {
    let selected_user = app.users.get(app.selected_user_index).map(|user| user.uid);
    let selected_group = app
        .groups
        .get(app.selected_group_index)
        .map(|group| group.gid);
    app.search_query = truncate_query_bytes(&app.search_query).to_ascii_lowercase();
    let query = app.search_query.as_str();

    let mut users = app.users_all.clone();
    if let Some(filter) = app.users_filter {
        match filter {
            UsersFilter::OnlyUserIds => users.retain(|user| user.uid >= 1000),
            UsersFilter::OnlySystemIds => users.retain(|user| user.uid < 1000),
        }
    }
    let chips = &app.users_filter_chips;
    if chips.human_only {
        users.retain(|user| user.uid >= 1000);
    }
    if chips.system_only {
        users.retain(|user| user.uid < 1000);
    }
    if chips.inactive {
        users.retain(|user| {
            let shell = user.shell.to_ascii_lowercase();
            shell.contains("nologin") || shell.ends_with("/false")
        });
    }
    if chips.no_home {
        users.retain(|user| {
            app.diagnostics
                .homes
                .get(&user.name)
                .and_then(|diagnostic| diagnostic.exists)
                .is_some_and(|exists| !exists)
        });
    }
    if (chips.locked || chips.no_password || chips.expired)
        && let ShadowState::Known(statuses) = &app.diagnostics.shadow
    {
        // Do not claim a complete status filter when any visible account is
        // unknown to shadow.  The UI keeps the selected filter visible and
        // reports shadow availability separately.
        if users.iter().all(|user| statuses.contains_key(&user.name)) {
            if chips.locked {
                users.retain(|user| statuses[&user.name].locked);
            }
            if chips.no_password {
                users.retain(|user| statuses[&user.name].no_password);
            }
            if chips.expired {
                users.retain(|user| statuses[&user.name].expired);
            }
        }
    }
    if matches!(app.input_mode, InputMode::SearchUsers) && !query.is_empty() {
        users.retain(|user| {
            user.name.to_ascii_lowercase().contains(query)
                || user
                    .full_name
                    .as_deref()
                    .unwrap_or_default()
                    .to_ascii_lowercase()
                    .contains(query)
                || user.home_dir.to_ascii_lowercase().contains(query)
                || user.shell.to_ascii_lowercase().contains(query)
                || user.uid.to_string().contains(query)
                || user.primary_gid.to_string().contains(query)
        });
    }
    app.users = users;
    app.selected_user_index = selected_user
        .and_then(|uid| app.users.iter().position(|user| user.uid == uid))
        .unwrap_or(0)
        .min(app.users.len().saturating_sub(1));

    let mut groups = app.groups_all.clone();
    if let Some(filter) = app.groups_filter {
        match filter {
            GroupsFilter::OnlyUserGids => groups.retain(|group| group.gid >= 1000),
            GroupsFilter::OnlySystemGids => groups.retain(|group| group.gid < 1000),
        }
    }
    if matches!(app.input_mode, InputMode::SearchGroups) && !query.is_empty() {
        groups.retain(|group| {
            group.name.to_ascii_lowercase().contains(query)
                || group.gid.to_string().contains(query)
                || group
                    .members
                    .iter()
                    .any(|member| member.to_ascii_lowercase().contains(query))
        });
    }
    app.groups = groups;
    app.selected_group_index = selected_group
        .and_then(|gid| app.groups.iter().position(|group| group.gid == gid))
        .unwrap_or(0)
        .min(app.groups.len().saturating_sub(1));
    app.selected_group_member_index = app.selected_group_member_index.min(
        app.groups
            .get(app.selected_group_index)
            .map_or(0, |group| group.members.len().saturating_sub(1)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_marks_known_shadow_status() {
        let statuses = parse_shadow_records("alice:!:1:0:30::::\nbob::1:0:30::::\n", 50);
        assert!(statuses["alice"].locked);
        assert!(statuses["bob"].no_password);
        assert!(statuses["alice"].expired);
    }
}
