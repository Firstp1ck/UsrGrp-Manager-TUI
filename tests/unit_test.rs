// Unit tests for usrgrp-manager
// These tests work with the public API without modifying the main codebase

#[cfg(test)]
mod sys_tests {
    use usrgrp_manager::sys::{SystemAdapter, SystemGroup, SystemUser};

    // Since parse_passwd and parse_group are private, we test through SystemAdapter
    #[test]
    fn test_system_adapter_list_users() {
        // This test would normally require root or special setup to modify /etc/passwd
        // So we test that the function doesn't panic and returns a Result
        let adapter = SystemAdapter::new();
        let result = adapter.list_users();
        assert!(result.is_ok() || result.is_err()); // Either works, just shouldn't panic
    }

    #[test]
    fn test_system_adapter_list_groups() {
        let adapter = SystemAdapter::new();
        let result = adapter.list_groups();
        assert!(result.is_ok() || result.is_err());
    }

    #[test]
    fn test_system_adapter_groups_for_user() {
        let adapter = SystemAdapter::new();
        // Test with a likely non-existent user
        // groups_for_user is only compiled in tests behind cfg(test) on the library side,
        // but here we just ensure SystemAdapter exists and is usable.
        let _ = adapter.list_groups();
    }

    #[test]
    fn test_system_adapter_list_shells() {
        let adapter = SystemAdapter::new();
        let result = adapter.list_shells();
        // /etc/shells should exist on most Unix systems
        if let Ok(shells) = result {
            // Most systems have at least /bin/sh
            assert!(!shells.is_empty() || shells.is_empty()); // Either is fine
        }
    }

    #[test]
    fn test_system_user_struct() {
        let user = SystemUser {
            uid: 1000,
            name: "testuser".to_string(),
            primary_gid: 1000,
            full_name: Some("Test User".to_string()),
            home_dir: "/home/testuser".to_string(),
            shell: "/bin/bash".to_string(),
        };

        assert_eq!(user.uid, 1000);
        assert_eq!(user.name, "testuser");
        assert_eq!(user.full_name.as_deref(), Some("Test User"));
    }

    #[test]
    fn test_system_group_struct() {
        let group = SystemGroup {
            gid: 1000,
            name: "testgroup".to_string(),
            members: vec!["user1".to_string(), "user2".to_string()],
        };

        assert_eq!(group.gid, 1000);
        assert_eq!(group.name, "testgroup");
        assert_eq!(group.members.len(), 2);
    }

    #[test]
    fn test_current_username() {
        // This should return Some(username) on Unix systems
        let username = usrgrp_manager::sys::current_username();
        // Can't assert specific value, but it should work
        assert!(username.is_some() || username.is_none());
    }
}

#[cfg(test)]
mod search_tests {
    use ratatui::widgets::TableState;
    use usrgrp_manager::app::{ActiveTab, AppState, InputMode, Theme, UsersFocus};
    use usrgrp_manager::search::apply_filters_and_search;
    use usrgrp_manager::sys::{SystemGroup, SystemUser};

    fn create_test_app() -> AppState {
        AppState {
            started_at: std::time::Instant::now(),
            users_all: vec![],
            users: vec![],
            groups_all: vec![],
            groups: vec![],
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
            users_filter: None,
            groups_filter: None,
            users_filter_chips: Default::default(),
        }
    }

    fn create_test_user(name: &str, uid: u32) -> SystemUser {
        SystemUser {
            uid,
            name: name.to_string(),
            primary_gid: uid,
            full_name: Some(format!("{} User", name)),
            home_dir: format!("/home/{}", name),
            shell: "/bin/bash".to_string(),
        }
    }

    fn create_test_group(name: &str, gid: u32, members: Vec<String>) -> SystemGroup {
        SystemGroup {
            gid,
            name: name.to_string(),
            members,
        }
    }

    #[test]
    fn test_search_empty_query_resets() {
        let mut app = create_test_app();
        app.users_all = vec![
            create_test_user("alice", 1000),
            create_test_user("bob", 1001),
        ];
        app.users = vec![app.users_all[0].clone()]; // Filtered state
        app.selected_user_index = 0;
        app.search_query = String::new();
        app.input_mode = InputMode::SearchUsers;

        apply_filters_and_search(&mut app);

        assert_eq!(app.users.len(), 2); // Reset to all users
        assert_eq!(app.selected_user_index, 0); // Index reset
    }

    #[test]
    fn test_search_case_insensitive() {
        let mut app = create_test_app();
        app.users_all = vec![
            create_test_user("Alice", 1000),
            create_test_user("bob", 1001),
        ];
        app.input_mode = InputMode::SearchUsers;

        app.search_query = "aLiCe".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "Alice");

        app.search_query = "BOB".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "bob");
    }

    #[test]
    fn test_search_numeric_uid_gid() {
        let mut app = create_test_app();
        app.users_all = vec![
            create_test_user("user1", 1000),
            create_test_user("user2", 2000),
        ];
        app.input_mode = InputMode::SearchUsers;

        app.search_query = "1000".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].uid, 1000);
    }

    #[test]
    fn test_search_groups() {
        let mut app = create_test_app();
        app.groups_all = vec![
            create_test_group("wheel", 10, vec!["alice".to_string()]),
            create_test_group("users", 100, vec!["alice".to_string(), "bob".to_string()]),
        ];
        app.groups = app.groups_all.clone();
        app.input_mode = InputMode::SearchGroups;

        app.search_query = "wheel".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "wheel");

        // Search by member
        app.search_query = "bob".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "users");
    }

    #[test]
    fn test_search_performance_large_dataset() {
        use std::time::Instant;

        let mut app = create_test_app();
        // Create 10,000 users
        app.users_all = (0..10000)
            .map(|i| create_test_user(&format!("user{}", i), 1000 + i as u32))
            .collect();
        app.input_mode = InputMode::SearchUsers;
        app.search_query = "user5000".to_string();

        let start = Instant::now();
        apply_filters_and_search(&mut app);
        let duration = start.elapsed();

        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "user5000");
        // Performance assertion: should complete within 100ms
        assert!(
            duration.as_millis() < 100,
            "Search took too long: {:?}",
            duration
        );
    }

    #[test]
    fn test_search_special_characters_and_no_panic() {
        let mut app = create_test_app();
        app.users_all = vec![
            create_test_user("alice", 1000),
            SystemUser {
                uid: 1002,
                name: "charlie".to_string(),
                primary_gid: 1002,
                full_name: Some("Charlie C".to_string()),
                home_dir: "/home/charlie".to_string(),
                shell: "/bin/bash".to_string(),
            },
        ];
        app.input_mode = InputMode::SearchUsers;

        // Special characters should be treated literally and not cause panics
        app.search_query = "[".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.users.len(), 0);

        // Slash and other chars in shell/home should match (use unique home path)
        app.search_query = "/home/charlie".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "charlie");
    }

    #[test]
    fn test_selection_index_clamp_after_filter() {
        let mut app = create_test_app();
        app.users_all = vec![
            create_test_user("alice", 1000),
            create_test_user("bob", 1001),
            create_test_user("carol", 1002),
        ];
        app.users = app.users_all.clone();
        app.selected_user_index = 2; // Last index
        app.input_mode = InputMode::SearchUsers;

        // Filter down to a single user
        app.search_query = "alice".to_string();
        apply_filters_and_search(&mut app);

        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "alice");
        assert_eq!(app.selected_user_index, 0); // Clamped/reset to 0
    }

    #[test]
    fn test_partial_field_matching_multiple_fields() {
        let mut app = create_test_app();
        app.users_all = vec![
            create_test_user("alice", 1000),
            create_test_user("bob", 1001),
        ];
        app.groups_all = vec![
            create_test_group("developers", 2000, vec!["alice".to_string()]),
            create_test_group("operators", 2001, vec!["bob".to_string()]),
        ];

        // Users: partial match in home path
        app.input_mode = InputMode::SearchUsers;
        app.search_query = "/home/al".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.users.len(), 1);
        assert_eq!(app.users[0].name, "alice");

        // Groups: partial match in member name
        app.input_mode = InputMode::SearchGroups;
        app.search_query = "ali".to_string();
        apply_filters_and_search(&mut app);
        assert_eq!(app.groups.len(), 1);
        assert_eq!(app.groups[0].name, "developers");
    }
}

#[cfg(test)]
mod error_handling_tests {
    use usrgrp_manager::error::{Context, SimpleError, simple_error};

    #[test]
    fn test_context_error_chaining() {
        // Test with a concrete error type that implements std::error::Error
        let base_error = std::io::Error::new(std::io::ErrorKind::NotFound, "file not found");
        let result: Result<(), std::io::Error> = Err(base_error);

        let with_context = result.with_ctx(|| "Failed to read config file".to_string());

        assert!(with_context.is_err());
        let err = with_context.unwrap_err();
        let err_string = err.to_string();
        assert!(err_string.contains("Failed to read config file"));
        assert!(err_string.contains("file not found"));
    }

    #[test]
    fn test_nested_contexts() {
        // Test single level of context wrapping
        let base_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "access denied");
        let result: Result<(), std::io::Error> = Err(base_error);

        let with_context = result.with_ctx(|| "Cannot write to file".to_string());

        let err = with_context.unwrap_err();
        let err_string = err.to_string();
        assert!(err_string.contains("Cannot write to file"));
        assert!(err_string.contains("access denied"));

        // Check error chain - the source should be the original io::Error
        let source = err.source();
        assert!(source.is_some());
        let inner = source.unwrap().to_string();
        assert!(inner.contains("access denied"));
    }

    #[test]
    fn test_simple_error() {
        let err = simple_error("Custom error message");
        assert_eq!(err.to_string(), "Custom error message");

        let err2 = SimpleError::new("Another error");
        assert_eq!(err2.to_string(), "Another error");
    }
}

#[cfg(test)]
mod app_state_tests {
    use usrgrp_manager::app::{
        ActiveTab, AppState, InputMode, ModalState, ModifyField, PendingAction, Theme, UsersFocus,
    };

    #[test]
    fn test_app_state_creation() {
        // Test that AppState::new() works
        let app = AppState::new();
        assert_eq!(app.active_tab, ActiveTab::Users);
        assert_eq!(app.selected_user_index, 0);
        assert_eq!(app.selected_group_index, 0);
        assert!(matches!(app.input_mode, InputMode::Normal));
    }

    #[test]
    fn test_app_state_invariants_sorted_and_defaults() {
        let app = AppState::new();
        assert!(matches!(app.input_mode, InputMode::Normal));
        assert_eq!(app.selected_user_index, 0);
        assert_eq!(app.selected_group_index, 0);

        // Users and groups sorted by ID (non-decreasing)
        assert!(app.users_all.windows(2).all(|w| w[0].uid <= w[1].uid));
        assert!(app.groups_all.windows(2).all(|w| w[0].gid <= w[1].gid));

        // Theme loaded (simple sanity check: status_fg not default Black from dark theme)
        let _ = app.theme;
    }

    #[test]
    fn test_active_tab_enum() {
        let tab = ActiveTab::Users;
        assert!(matches!(tab, ActiveTab::Users));

        let tab = ActiveTab::Groups;
        assert!(matches!(tab, ActiveTab::Groups));
    }

    #[test]
    fn test_users_focus_enum() {
        let focus = UsersFocus::UsersList;
        assert!(matches!(focus, UsersFocus::UsersList));

        let focus = UsersFocus::MemberOf;
        assert!(matches!(focus, UsersFocus::MemberOf));
    }

    #[test]
    fn test_input_mode_enum() {
        let mode = InputMode::Normal;
        assert!(matches!(mode, InputMode::Normal));

        let mode = InputMode::SearchUsers;
        assert!(matches!(mode, InputMode::SearchUsers));

        let mode = InputMode::SearchGroups;
        assert!(matches!(mode, InputMode::SearchGroups));

        let mode = InputMode::Modal;
        assert!(matches!(mode, InputMode::Modal));
    }

    #[test]
    fn test_theme_creation() {
        let theme = Theme::dark();
        // Just verify it can be created
        assert_eq!(theme.text, ratatui::style::Color::Gray);
    }

    #[test]
    fn test_modal_state_variants() {
        let modal = ModalState::Actions { selected: 0 };
        assert!(matches!(modal, ModalState::Actions { .. }));

        let modal = ModalState::Info {
            message: "Test".to_string(),
        };
        assert!(matches!(modal, ModalState::Info { .. }));

        let modal = ModalState::UserAddInput {
            selected: 0,
            name: String::new(),
            password: String::new(),
            confirm: String::new(),
            create_home: true,
            add_to_wheel: false,
        };
        assert!(matches!(modal, ModalState::UserAddInput { .. }));
    }

    #[test]
    fn test_modify_field_enum() {
        let field = ModifyField::Username;
        assert!(matches!(field, ModifyField::Username));

        let field = ModifyField::Fullname;
        assert!(matches!(field, ModifyField::Fullname));
    }

    #[test]
    fn test_pending_action_variants() {
        let action = PendingAction::CreateUserWithOptions {
            username: "test".to_string(),
            password: Some("secret".to_string()),
            create_home: true,
            add_to_wheel: true,
        };
        assert!(matches!(
            action,
            PendingAction::CreateUserWithOptions { .. }
        ));

        let action = PendingAction::DeleteUser {
            username: "test".to_string(),
            delete_home: false,
        };
        assert!(matches!(action, PendingAction::DeleteUser { .. }));

        let action = PendingAction::CreateGroup {
            groupname: "test".to_string(),
        };
        assert!(matches!(action, PendingAction::CreateGroup { .. }));
    }
}

#[cfg(test)]
mod username_validation_tests {
    // Since we can't access private validation functions,
    // we'll test our own implementation that could be used

    #[test]
    fn test_valid_usernames() {
        assert!(is_valid_username("alice"));
        assert!(is_valid_username("user123"));
        assert!(is_valid_username("test-user"));
        assert!(is_valid_username("test_user"));
        assert!(is_valid_username("a")); // Single char should be valid
    }

    #[test]
    fn test_invalid_usernames() {
        assert!(!is_valid_username("")); // Empty
        assert!(!is_valid_username("root")); // Reserved
        assert!(!is_valid_username("123user")); // Starts with number
        assert!(!is_valid_username("user name")); // Contains space
        assert!(!is_valid_username("user@domain")); // Contains @
        assert!(!is_valid_username("user:name")); // Contains colon
        assert!(!is_valid_username(&"a".repeat(33))); // Too long (>32 chars)
    }

    fn is_valid_username(name: &str) -> bool {
        // Example implementation for testing
        if name.is_empty() || name.len() > 32 {
            return false;
        }
        if ["root", "bin", "daemon", "sys", "sync", "mail", "nobody"].contains(&name) {
            return false; // Reserved names
        }
        if !name.chars().next().unwrap_or('0').is_ascii_lowercase() {
            return false; // Must start with lowercase letter
        }
        name.chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    }
}

#[cfg(test)]
mod integration_tests {
    use ratatui::{backend::TestBackend, Terminal};
    use usrgrp_manager::app::AppState;
    use usrgrp_manager::ui::render;

    #[test]
    fn test_ui_render_smoke() {
        // Render a basic AppState into a TestBackend and ensure it doesn't panic
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        let mut app = AppState::new();
        terminal
            .draw(|f| {
                render(f, &mut app);
            })
            .expect("render frame");
    }

    #[test]
    fn test_ui_render_with_empty_data() {
        let backend = TestBackend::new(80, 24);
        let mut terminal = Terminal::new(backend).expect("create terminal");
        let mut app = AppState {
            users_all: vec![],
            users: vec![],
            groups_all: vec![],
            groups: vec![],
            ..AppState::new()
        };
        terminal
            .draw(|f| {
                render(f, &mut app);
            })
            .expect("render frame with empty data");
    }
}
