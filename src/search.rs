use crate::app::{AppState, InputMode};

pub fn apply_search(app: &mut AppState) {
    let q = app.search_query.to_lowercase();
    match app.input_mode {
        InputMode::SearchUsers => {
            if q.is_empty() {
                app.users = app.users_all.clone();
            } else {
                app.users = app
                    .users_all
                    .iter()
                    .filter(|u| {
                        u.name.to_lowercase().contains(&q)
                            || u.full_name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                            || u.home_dir.to_lowercase().contains(&q)
                            || u.shell.to_lowercase().contains(&q)
                            || u.uid.to_string().contains(&q)
                            || u.primary_gid.to_string().contains(&q)
                    })
                    .cloned()
                    .collect();
            }
            app.selected_user_index = 0.min(app.users.len().saturating_sub(1));
        }
        InputMode::SearchGroups => {
            if q.is_empty() {
                app.groups = app.groups_all.clone();
            } else {
                app.groups = app
                    .groups_all
                    .iter()
                    .filter(|g| {
                        g.name.to_lowercase().contains(&q)
                            || g.gid.to_string().contains(&q)
                            || g.members.iter().any(|m| m.to_lowercase().contains(&q))
                    })
                    .cloned()
                    .collect();
            }
            app.selected_group_index = 0.min(app.groups.len().saturating_sub(1));
        }
        InputMode::Normal | InputMode::Modal => {}
    }
}

#[cfg(test)]
mod tests {
	use super::*;
	use crate::app::{ActiveTab, UsersFocus, Theme};
	use ratatui::widgets::TableState;
	use std::time::Instant;

	fn mk_user(uid: u32, name: &str, gid: u32, full: Option<&str>, home: &str, shell: &str) -> crate::sys::SystemUser {
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

	fn mk_app(users: Vec<crate::sys::SystemUser>, groups: Vec<crate::sys::SystemGroup>) -> crate::app::AppState {
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
			modal: None,
			users_focus: UsersFocus::UsersList,
			sudo_password: None,
		}
	}

	#[test]
	fn search_users_filters_by_multiple_fields() {
		let users = vec![
			mk_user(1000, "alice", 1000, Some("Alice A"), "/home/alice", "/bin/zsh"),
			mk_user(1001, "bob", 1001, Some("Bobby Tables"), "/home/bob", "/bin/bash"),
		];
		let mut app = mk_app(users, vec![]);
		app.input_mode = InputMode::SearchUsers;
		app.search_query = "bOb".to_string();
		apply_search(&mut app);

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
		apply_search(&mut app);
		assert_eq!(app.groups.len(), 1);
		assert_eq!(app.groups[0].name, "wheel");

		app.search_query = "bob".to_string();
		apply_search(&mut app);
		assert_eq!(app.groups.len(), 1);
		assert_eq!(app.groups[0].name, "wheel");
	}
}