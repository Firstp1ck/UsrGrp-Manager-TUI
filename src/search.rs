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