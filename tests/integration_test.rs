//! Deterministic application integration tests.  No test reads host accounts,
//! HOME, shadow, procfs, or invokes account tools.

use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend};
use usrgrp_manager::{
    app::{AppState, InputMode},
    search::apply_filters_and_search,
    sys::{
        AccountGroup, AccountSnapshot, AccountUser, Gecos, Gid, GroupName, ShellPath,
        SystemAdapter, Uid, UserName,
    },
    ui,
};

fn snapshot() -> AccountSnapshot {
    AccountSnapshot {
        users: vec![
            AccountUser {
                uid: Uid(1000),
                name: UserName::new("alice").unwrap(),
                primary_gid: Gid(1000),
                full_name: Some(Gecos::new("Alice").unwrap()),
                home_dir: "/home/alice".into(),
                shell: ShellPath::new("/bin/sh").unwrap(),
            },
            AccountUser {
                uid: Uid(1001),
                name: UserName::new("bob").unwrap(),
                primary_gid: Gid(1001),
                full_name: Some(Gecos::new("Bob").unwrap()),
                home_dir: "/home/bob".into(),
                shell: ShellPath::new("/bin/bash").unwrap(),
            },
        ],
        groups: vec![
            AccountGroup {
                gid: Gid(1000),
                name: GroupName::new("dev").unwrap(),
                members: vec![UserName::new("alice").unwrap()],
            },
            AccountGroup {
                gid: Gid(1001),
                name: GroupName::new("ops").unwrap(),
                members: vec![UserName::new("bob").unwrap()],
            },
        ],
        shells: vec![
            ShellPath::new("/bin/sh").unwrap(),
            ShellPath::new("/bin/bash").unwrap(),
        ],
        diagnostics: vec![],
    }
}

fn app() -> AppState {
    AppState::with_adapter(Arc::new(SystemAdapter::new()), snapshot())
}

#[test]
fn pure_construction_and_render_do_not_require_host_data() {
    let app = app();
    let mut terminal = Terminal::new(TestBackend::new(100, 30)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    assert_eq!(app.users.len(), 2);
}

#[test]
fn search_preserves_selected_stable_identity() {
    let mut app = app();
    app.selected_user_index = 1;
    app.input_mode = InputMode::SearchUsers;
    app.search_query = "bob".to_owned();
    apply_filters_and_search(&mut app);
    assert_eq!(app.users[app.selected_user_index].name, "bob");
}
