// Integration tests for usrgrp-manager

// 1) Theme config roundtrip and init
#[test]
fn theme_roundtrip_and_init() {
    use std::{fs, path::PathBuf, time::{SystemTime, UNIX_EPOCH}};
    use usrgrp_manager::app::Theme;

    // Unique temp path
    let mut path = std::env::temp_dir();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    path.push(format!("ugm_theme_{}_{}.conf", std::process::id(), nonce));
    let path_str = path.to_string_lossy().to_string();

    // Roundtrip write/read
    let t = Theme::mocha();
    t.write_file(&path_str).expect("write theme");
    let t2 = Theme::from_file(&path_str).expect("read theme");
    // Compare key fields
    assert_eq!(format!("{:?}", t.text), format!("{:?}", t2.text));
    assert_eq!(format!("{:?}", t.title), format!("{:?}", t2.title));
    assert_eq!(format!("{:?}", t.header_bg), format!("{:?}", t2.header_bg));

    // load_or_init creates file if missing
    let mut p2 = PathBuf::from(&path_str);
    p2.set_file_name(format!("{}_init.conf", p2.file_stem().unwrap().to_string_lossy()));
    let p2_str = p2.to_string_lossy().to_string();
    let _ = fs::remove_file(&p2_str);
    let _created = Theme::load_or_init(&p2_str);
    assert!(PathBuf::from(&p2_str).exists());

    // Cleanup best-effort
    let _ = fs::remove_file(&path_str);
    let _ = fs::remove_file(&p2_str);
}

// 2) Search with combined filters across users and groups
#[test]
fn search_applies_filters_across_users_and_groups() {
    use ratatui::widgets::TableState;
    use usrgrp_manager::{
        app::{AppState, ActiveTab, InputMode, Theme, UsersFocus, UsersFilter, GroupsFilter},
        search::apply_filters_and_search,
    };

    // Seed users and groups
    let users = vec![
        usrgrp_manager::sys::SystemUser { uid: 999,  name: "daemon".into(), primary_gid: 999, full_name: None, home_dir: "/".into(),         shell: "/sbin/nologin".into() },
        usrgrp_manager::sys::SystemUser { uid: 1000, name: "alice".into(),  primary_gid: 1000, full_name: Some("Alice".into()), home_dir: "/home/alice".into(), shell: "/bin/zsh".into() },
        usrgrp_manager::sys::SystemUser { uid: 1001, name: "bob".into(),    primary_gid: 1001, full_name: Some("Bobby".into()), home_dir: "/home/bob".into(),   shell: "/bin/bash".into() },
    ];
    let groups = vec![
        usrgrp_manager::sys::SystemGroup { gid: 998,  name: "wheel".into(), members: vec!["root".into()] },
        usrgrp_manager::sys::SystemGroup { gid: 1000, name: "users".into(), members: vec!["alice".into()] },
        usrgrp_manager::sys::SystemGroup { gid: 1001, name: "dev".into(),   members: vec!["bob".into()] },
    ];

    let mut app = AppState {
        started_at: std::time::Instant::now(),
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
        theme: Theme::mocha(),
        keymap: usrgrp_manager::app::keymap::Keymap::default(),
        modal: None,
        users_focus: UsersFocus::UsersList,
        sudo_password: None,
        users_filter: Some(UsersFilter::OnlyUserIds),
        groups_filter: Some(GroupsFilter::OnlyUserGids),
        users_filter_chips: Default::default(),
    };

    // Users search
    app.input_mode = InputMode::SearchUsers;
    app.search_query = "bo".into();
    apply_filters_and_search(&mut app);
    assert_eq!(app.users.len(), 1);
    assert_eq!(app.users[0].name, "bob");
    assert_eq!(app.selected_user_index, 0);

    // Groups search
    app.active_tab = ActiveTab::Groups;
    app.input_mode = InputMode::SearchGroups;
    app.search_query = "dev".into();
    apply_filters_and_search(&mut app);
    assert_eq!(app.groups.len(), 1);
    assert_eq!(app.groups[0].name, "dev");
    assert_eq!(app.selected_group_index, 0);
}

fn is_root() -> bool {
    if let Ok(s) = std::fs::read_to_string("/proc/self/status") {
        for line in s.lines() {
            if let Some(rest) = line.strip_prefix("Uid:") {
                if let Some(first) = rest.split_whitespace().next() {
                    return first == "0";
                }
            }
        }
    }
    false
}

// 3) Privileged ops require authentication when not root
#[test]
fn privileged_ops_require_auth_without_sudo_password() {
    use usrgrp_manager::sys::SystemAdapter;

    // If running as root, skip (root won't require sudo)
    if is_root() {
        eprintln!("Skipping on root");
        return;
    }

    let adapter = SystemAdapter::with_sudo_password(None);

    // create_group should fail with auth required
    let err = adapter.create_group("ugm_test_should_not_exist").unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));

    // add_user_to_group should fail with auth required
    let err = adapter.add_user_to_group("root", "root").unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));

    // set_user_password should fail early with auth required
    let err = adapter.set_user_password("root", "dummy").unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));
}

// 4) delete_group is idempotent when group is already missing
#[test]
fn delete_group_is_idempotent_without_sudo_when_missing() {
    use usrgrp_manager::sys::SystemAdapter;

    let adapter = SystemAdapter::with_sudo_password(None);
    // Should return Ok if group is already absent (no sudo needed)
    adapter.delete_group("ugm_definitely_missing_group_name").unwrap();
}


// 5) Search mode-gating: when not in Search* modes, search_query should not filter
#[test]
fn search_mode_gating_leaves_lists_unchanged() {
    use ratatui::widgets::TableState;
    use usrgrp_manager::{
        app::{AppState, ActiveTab, InputMode, Theme, UsersFocus},
        search::apply_filters_and_search,
    };

    let users = vec![
        usrgrp_manager::sys::SystemUser { uid: 1000, name: "alice".into(), primary_gid: 1000, full_name: None, home_dir: "/home/alice".into(), shell: "/bin/zsh".into() },
        usrgrp_manager::sys::SystemUser { uid: 1001, name: "bob".into(),   primary_gid: 1001, full_name: None, home_dir: "/home/bob".into(),   shell: "/bin/bash".into() },
    ];
    let groups = vec![
        usrgrp_manager::sys::SystemGroup { gid: 1000, name: "users".into(), members: vec!["alice".into()] },
        usrgrp_manager::sys::SystemGroup { gid: 1001, name: "dev".into(),   members: vec!["bob".into()] },
    ];

    let mut app = AppState {
        started_at: std::time::Instant::now(),
        users_all: users.clone(),
        users: users.clone(),
        groups_all: groups.clone(),
        groups: groups.clone(),
        active_tab: ActiveTab::Users,
        selected_user_index: 0,
        selected_group_index: 0,
        rows_per_page: 10,
        _table_state: TableState::default(),
        input_mode: InputMode::Normal,
        search_query: "alice".into(),
        theme: Theme::mocha(),
        keymap: usrgrp_manager::app::keymap::Keymap::default(),
        modal: None,
        users_focus: UsersFocus::UsersList,
        sudo_password: None,
        users_filter: None,
        groups_filter: None,
        users_filter_chips: Default::default(),
    };

    apply_filters_and_search(&mut app);
    assert_eq!(app.users.len(), 2);
    assert_eq!(app.groups.len(), 2);
}

// 6) Numeric matching: searching numbers hits UID/GID columns
#[test]
fn search_numeric_matching_users_and_groups() {
    use ratatui::widgets::TableState;
    use usrgrp_manager::{
        app::{AppState, ActiveTab, InputMode, Theme, UsersFocus},
        search::apply_filters_and_search,
    };

    let users = vec![
        usrgrp_manager::sys::SystemUser { uid: 999,  name: "daemon".into(), primary_gid: 999,  full_name: None, home_dir: "/".into(),           shell: "/sbin/nologin".into() },
        usrgrp_manager::sys::SystemUser { uid: 1000, name: "alice".into(),  primary_gid: 1000, full_name: None, home_dir: "/home/alice".into(), shell: "/bin/zsh".into() },
    ];
    let groups = vec![
        usrgrp_manager::sys::SystemGroup { gid: 1001, name: "dev".into(), members: vec!["alice".into()] },
        usrgrp_manager::sys::SystemGroup { gid: 2000, name: "staff".into(), members: vec![] },
    ];

    let mut app = AppState {
        started_at: std::time::Instant::now(),
        users_all: users.clone(),
        users: users,
        groups_all: groups.clone(),
        groups: groups,
        active_tab: ActiveTab::Users,
        selected_user_index: 0,
        selected_group_index: 0,
        rows_per_page: 10,
        _table_state: TableState::default(),
        input_mode: InputMode::SearchUsers,
        search_query: "1000".into(),
        theme: Theme::mocha(),
        keymap: usrgrp_manager::app::keymap::Keymap::default(),
        modal: None,
        users_focus: UsersFocus::UsersList,
        sudo_password: None,
        users_filter: None,
        groups_filter: None,
        users_filter_chips: Default::default(),
    };

    apply_filters_and_search(&mut app);
    assert_eq!(app.users.len(), 1);
    assert_eq!(app.users[0].uid, 1000);

    app.active_tab = ActiveTab::Groups;
    app.input_mode = InputMode::SearchGroups;
    app.search_query = "1001".into();
    apply_filters_and_search(&mut app);
    assert_eq!(app.groups.len(), 1);
    assert_eq!(app.groups[0].gid, 1001);
}

// 7) Empty-query filters: filters alone narrow results and reset indices
#[test]
fn filters_apply_with_empty_query() {
    use ratatui::widgets::TableState;
    use usrgrp_manager::{
        app::{AppState, ActiveTab, InputMode, Theme, UsersFocus, UsersFilter, GroupsFilter},
        search::apply_filters_and_search,
    };

    let users = vec![
        usrgrp_manager::sys::SystemUser { uid: 500,  name: "sys".into(),   primary_gid: 500,  full_name: None, home_dir: "/".into(),           shell: "/sbin/nologin".into() },
        usrgrp_manager::sys::SystemUser { uid: 1000, name: "alice".into(), primary_gid: 1000, full_name: None, home_dir: "/home/alice".into(), shell: "/bin/zsh".into() },
    ];
    let groups = vec![
        usrgrp_manager::sys::SystemGroup { gid: 99,  name: "system".into(), members: vec![] },
        usrgrp_manager::sys::SystemGroup { gid: 1000, name: "users".into(), members: vec!["alice".into()] },
    ];

    let mut app = AppState {
        started_at: std::time::Instant::now(),
        users_all: users.clone(),
        users: users.clone(),
        groups_all: groups.clone(),
        groups: groups.clone(),
        active_tab: ActiveTab::Users,
        selected_user_index: 1,
        selected_group_index: 1,
        rows_per_page: 10,
        _table_state: TableState::default(),
        input_mode: InputMode::SearchUsers,
        search_query: String::new(),
        theme: Theme::mocha(),
        keymap: usrgrp_manager::app::keymap::Keymap::default(),
        modal: None,
        users_focus: UsersFocus::UsersList,
        sudo_password: None,
        users_filter: Some(UsersFilter::OnlySystemIds),
        groups_filter: Some(GroupsFilter::OnlySystemGids),
        users_filter_chips: Default::default(),
    };

    apply_filters_and_search(&mut app);
    assert_eq!(app.users.len(), 1);
    assert_eq!(app.users[0].uid, 500);
    assert_eq!(app.selected_user_index, 0);

    app.active_tab = ActiveTab::Groups;
    app.input_mode = InputMode::SearchGroups;
    app.search_query.clear();
    apply_filters_and_search(&mut app);
    assert_eq!(app.groups.len(), 1);
    assert_eq!(app.groups[0].gid, 99);
    assert_eq!(app.selected_group_index, 0);
}

// 8) Theme config robustness: unknown keys ignored, invalid values ignored, valid parsed
#[test]
fn theme_from_file_robustness() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    use usrgrp_manager::app::Theme;

    let mut path = std::env::temp_dir();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    path.push(format!("ugm_theme_rb_{}_{}.conf", std::process::id(), nonce));
    let p = path.to_string_lossy().to_string();

    // Craft a config with a mix of valid/invalid/unknown keys
    let contents = r#"
text = #112233
title = not-a-color
header_bg = reset
unknown_key = #abcdef
"#;
    fs::write(&p, contents).expect("write theme file");

    let t = Theme::from_file(&p).expect("load theme");
    let mocha = Theme::mocha();

    // text parsed as hex
    assert_eq!(format!("{:?}", t.text), format!("{:?}", ratatui::style::Color::Rgb(0x11, 0x22, 0x33)));
    // header_bg parsed as reset
    assert_eq!(format!("{:?}", t.header_bg), format!("{:?}", ratatui::style::Color::Reset));
    // title invalid -> should remain default (mocha)
    assert_eq!(format!("{:?}", t.title), format!("{:?}", mocha.title));

    let _ = std::fs::remove_file(&p);
}

// 9) Extended auth coverage for more ops (non-root)
#[test]
fn privileged_ops_auth_required_extended_when_not_root() {
    use usrgrp_manager::sys::SystemAdapter;
    if is_root() {
        eprintln!("Skipping on root");
        return;
    }
    let adapter = SystemAdapter::with_sudo_password(None);

    let err = adapter.change_user_shell("root", "/bin/bash").unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));

    let err = adapter.change_user_fullname("root", "Root User").unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));

    let err = adapter.rename_group("root", "root2").unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));

    let err = adapter.delete_user("unlikely_user_xyz", false).unwrap_err();
    assert!(format!("{err}").contains("Authentication required"));
}

// 10) Idempotent delete_group for multiple distinct missing names
#[test]
fn delete_group_is_idempotent_for_multiple_missing_names() {
    use usrgrp_manager::sys::SystemAdapter;
    let adapter = SystemAdapter::with_sudo_password(None);
    adapter.delete_group("ugm_missing_one").unwrap();
    adapter.delete_group("ugm_missing_two").unwrap();
}

// 11) Theme write header/content: header lines present and all keys exactly once
#[test]
fn theme_write_includes_header_and_all_keys_once() {
    use std::{fs, time::{SystemTime, UNIX_EPOCH}};
    use usrgrp_manager::app::Theme;

    let mut path = std::env::temp_dir();
    let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
    path.push(format!("ugm_theme_hdr_{}_{}.conf", std::process::id(), nonce));
    let p = path.to_string_lossy().to_string();

    let t = Theme::mocha();
    t.write_file(&p).expect("write theme file");
    let contents = fs::read_to_string(&p).expect("read back theme file");

    assert!(contents.contains("# usrgrp-manager theme configuration"));
    assert!(contents.contains("# Colors: hex as #RRGGBB or RRGGBB, or 'reset'"));

    // Each key appears exactly once with '='
    let keys = [
        "text = ",
        "muted = ",
        "title = ",
        "border = ",
        "header_bg = ",
        "header_fg = ",
        "status_bg = ",
        "status_fg = ",
        "highlight_fg = ",
        "highlight_bg = ",
    ];
    for k in keys {
        let count = contents.matches(k).count();
        assert_eq!(count, 1, "key '{}' should appear exactly once", k);
    }

    let _ = std::fs::remove_file(&p);
}


