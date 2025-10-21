//! Search utilities for filtering users and groups.
//!
//! Currently provides [`apply_filters_and_search`] which filters the `AppState` in-place
//! based on the current input mode and query string.
//!
use crate::app::{AppState, GroupsFilter, InputMode, UsersFilter};
use std::collections::HashMap;

type ShadowMap = HashMap<String, ShadowStatus>;
type ShadowMapResult = std::io::Result<ShadowMap>;
type ShadowProviderFn = dyn Fn() -> ShadowMapResult;

/// Filter the visible users or groups of `app` according to the lowercase query.
///
/// - In `SearchUsers`, filters by username, full name, home directory, shell, UID, or GID.
/// - In `SearchGroups`, filters by group name, GID, or any member name.
/// - For empty queries, restores the full lists.
pub fn apply_filters_and_search(app: &mut AppState) {
    let q = app.search_query.to_lowercase();

    // Users view
    let mut users_view = app.users_all.clone();
    if let Some(f) = app.users_filter {
        match f {
            UsersFilter::OnlyUserIds => users_view.retain(|u| u.uid >= 1000),
            UsersFilter::OnlySystemIds => users_view.retain(|u| u.uid < 1000),
        }
    }

    // Apply chip filters (combinable)
    {
        let chips = &app.users_filter_chips;
        if chips.human_only {
            users_view.retain(|u| u.uid >= 1000);
        }
        if chips.system_only {
            users_view.retain(|u| u.uid < 1000);
        }
        if chips.inactive {
            users_view.retain(|u| {
                let sh = u.shell.to_ascii_lowercase();
                sh.contains("nologin") || sh.ends_with("/false")
            });
        }
        if chips.no_home {
            users_view.retain(|u| !std::path::Path::new(&u.home_dir).exists());
        }
        // System-backed filters via /etc/shadow (best-effort; ignored if unreadable)
        if (chips.locked || chips.no_password || chips.expired)
            && let Ok(shadow) = get_shadow_status()
        {
            if chips.locked {
                users_view.retain(|u| shadow.get(&u.name).map(|s| s.locked).unwrap_or(false));
            }
            if chips.no_password {
                users_view.retain(|u| shadow.get(&u.name).map(|s| s.no_password).unwrap_or(false));
            }
            if chips.expired {
                users_view.retain(|u| shadow.get(&u.name).map(|s| s.expired).unwrap_or(false));
            }
        }
    }
    if matches!(app.input_mode, InputMode::SearchUsers) && !q.is_empty() {
        users_view.retain(|u| {
            u.name.to_lowercase().contains(&q)
                || u.full_name
                    .as_deref()
                    .unwrap_or("")
                    .to_lowercase()
                    .contains(&q)
                || u.home_dir.to_lowercase().contains(&q)
                || u.shell.to_lowercase().contains(&q)
                || u.uid.to_string().contains(&q)
                || u.primary_gid.to_string().contains(&q)
        });
    }
    app.users = users_view;
    app.selected_user_index = 0.min(app.users.len().saturating_sub(1));

    // Groups view
    let mut groups_view = app.groups_all.clone();
    if let Some(f) = app.groups_filter {
        match f {
            GroupsFilter::OnlyUserGids => groups_view.retain(|g| g.gid >= 1000),
            GroupsFilter::OnlySystemGids => groups_view.retain(|g| g.gid < 1000),
        }
    }
    if matches!(app.input_mode, InputMode::SearchGroups) && !q.is_empty() {
        groups_view.retain(|g| {
            g.name.to_lowercase().contains(&q)
                || g.gid.to_string().contains(&q)
                || g.members.iter().any(|m| m.to_lowercase().contains(&q))
        });
    }
    app.groups = groups_view;
    app.selected_group_index = 0.min(app.groups.len().saturating_sub(1));
}

// Lightweight shadow status used for filters
#[derive(Clone, Debug)]
pub struct ShadowStatus {
    pub locked: bool,
    pub no_password: bool,
    pub expired: bool,
}

fn read_shadow_status() -> ShadowMapResult {
    use std::fs;
    use std::os::unix::fs::MetadataExt;
    use std::time::{SystemTime, UNIX_EPOCH};

    // Quick permission check: if not root and cannot read, bail fast
    if fs::metadata("/etc/shadow")
        .map(|m| m.mode() & 0o004 == 0)
        .unwrap_or(true)
    {
        // Likely unreadable, return an error to signal caller to skip filters
        return Err(std::io::Error::new(
            std::io::ErrorKind::PermissionDenied,
            "shadow unreadable",
        ));
    }

    let contents = fs::read_to_string("/etc/shadow")?;
    let today_days: i64 = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| (d.as_secs() / 86_400) as i64)
        .unwrap_or(0);
    let mut map: ShadowMap = HashMap::new();
    for line in contents.lines() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.split(':').collect();
        if parts.len() < 2 {
            continue;
        }
        let name = parts[0].to_string();
        let pw = parts[1];
        let lastchg: i64 = parts.get(2).and_then(|s| s.parse().ok()).unwrap_or(0);
        let max: i64 = parts.get(4).and_then(|s| s.parse().ok()).unwrap_or(-1);
        let expire_abs: i64 = parts.get(7).and_then(|s| s.parse().ok()).unwrap_or(-1);

        let locked = pw.starts_with('!') || pw == "*" || pw == "!!";
        let no_password = pw.is_empty();
        let expired_by_max = max >= 0 && lastchg > 0 && (lastchg + max) <= today_days;
        let expired_by_abs = expire_abs >= 0 && expire_abs <= today_days;
        let expired = expired_by_max || expired_by_abs;

        map.insert(
            name,
            ShadowStatus {
                locked,
                no_password,
                expired,
            },
        );
    }
    Ok(map)
}

fn get_shadow_status() -> ShadowMapResult {
    if let Some(res) = SHADOW_PROVIDER.with(|p| p.borrow().as_ref().map(|f| f())) {
        return res;
    }
    read_shadow_status()
}

thread_local! {
    static SHADOW_PROVIDER: std::cell::RefCell<Option<Box<ShadowProviderFn>>> = std::cell::RefCell::new(None);
}

#[allow(dead_code)]
pub fn set_shadow_provider<F>(f: F)
where
    F: Fn() -> ShadowMapResult + 'static,
{
    SHADOW_PROVIDER.with(|p| *p.borrow_mut() = Some(Box::new(f)));
}

#[allow(dead_code)]
pub fn clear_shadow_provider() {
    SHADOW_PROVIDER.with(|p| *p.borrow_mut() = None);
}

#[allow(dead_code)]
pub fn make_shadow_status(locked: bool, no_password: bool, expired: bool) -> ShadowStatus {
    ShadowStatus {
        locked,
        no_password,
        expired,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::{ActiveTab, Theme, UsersFocus};
    use ratatui::widgets::TableState;
    use std::time::Instant;

    fn mk_user(
        uid: u32,
        name: &str,
        gid: u32,
        full: Option<&str>,
        home: &str,
        shell: &str,
    ) -> crate::sys::SystemUser {
        crate::sys::SystemUser {
            uid,
            name: name.to_string(),
            primary_gid: gid,
            full_name: full.map(|s| s.to_string()),
            home_dir: home.to_string(),
            shell: shell.to_string(),
        }
    }

    fn mk_group(gid: u32, name: &str, members: &[&str]) -> crate::sys::SystemGroup {
        crate::sys::SystemGroup {
            gid,
            name: name.to_string(),
            members: members.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn mk_app(
        users: Vec<crate::sys::SystemUser>,
        groups: Vec<crate::sys::SystemGroup>,
    ) -> crate::app::AppState {
        crate::app::AppState {
            started_at: Instant::now(),
            users_all: users.clone(),
            users,
            groups_all: groups.clone(),
            groups,
            active_tab: ActiveTab::Users,
            selected_user_index: 0,
            selected_group_index: 0,
            rows_per_page: 10,
            _table_state: TableState::default(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            theme: Theme::dark(),
            keymap: crate::app::keymap::Keymap::default(),
            modal: None,
            users_focus: UsersFocus::UsersList,
            sudo_password: None,
            users_filter: None,
            groups_filter: None,
            users_filter_chips: Default::default(),
        }
    }

    #[test]
    fn search_users_filters_by_multiple_fields() {
        let users = vec![
            mk_user(
                1000,
                "alice",
                1000,
                Some("Alice A"),
                "/home/alice",
                "/bin/zsh",
            ),
            mk_user(
                1001,
                "bob",
                1001,
                Some("Bobby Tables"),
                "/home/bob",
                "/bin/bash",
            ),
        ];
        let mut app = mk_app(users, vec![]);
        app.input_mode = InputMode::SearchUsers;
        app.search_query = "bOb".to_string();
        app.input_mode = InputMode::SearchUsers;
        apply_filters_and_search(&mut app);

        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "bob");
    }

    #[test]
    fn search_groups_filters_by_name_gid_or_members() {
        let users = vec![
            mk_user(1000, "alice", 1000, None, "/home/alice", "/bin/zsh"),
            mk_user(1001, "bob", 1001, None, "/home/bob", "/bin/bash"),
        ];
        let groups = vec![
            mk_group(1000, "users", &["alice"]),
            mk_group(1001, "wheel", &["root", "bob"]),
        ];
        let mut app = mk_app(users, groups);
        app.input_mode = InputMode::SearchGroups;
        app.search_query = "wh".to_string();
        app.input_mode = InputMode::SearchGroups;
        apply_filters_and_search(&mut app);
        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "wheel");

        app.search_query = "bob".to_string();
        app.input_mode = InputMode::SearchGroups;
        apply_filters_and_search(&mut app);
        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "wheel");
    }
}
