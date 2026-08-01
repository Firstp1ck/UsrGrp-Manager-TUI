use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend};
use usrgrp_manager::{
    app::AppState,
    sys::{
        AccountGroup, AccountSnapshot, AccountUser, Gecos, Gid, GroupName, ShellPath,
        SystemAdapter, Uid, UserName,
    },
    ui,
};

fn app() -> AppState {
    let users = (0..25)
        .map(|index| AccountUser {
            uid: Uid(1000 + index),
            name: UserName::new(format!("user{index}")).unwrap(),
            primary_gid: Gid(1000),
            full_name: Some(Gecos::new("Fixture").unwrap()),
            home_dir: format!("/home/user{index}").into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        })
        .collect();
    AppState::with_adapter(
        Arc::new(SystemAdapter::new()),
        AccountSnapshot {
            users,
            groups: vec![AccountGroup {
                gid: Gid(1000),
                name: GroupName::new("dev").unwrap(),
                members: vec![],
            }],
            shells: vec![ShellPath::new("/bin/sh").unwrap()],
            diagnostics: vec![],
        },
    )
}

#[test]
fn rendering_does_not_mutate_selection_or_pagination_state() {
    let app = app();
    let before = (
        app.selected_user_index,
        app.selected_group_index,
        app.selected_user_group_index,
        app.selected_group_member_index,
        app.rows_per_page,
    );
    let mut terminal = Terminal::new(TestBackend::new(32, 8)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    assert_eq!(
        before,
        (
            app.selected_user_index,
            app.selected_group_index,
            app.selected_user_group_index,
            app.selected_group_member_index,
            app.rows_per_page
        )
    );
}

#[test]
fn small_terminal_uses_stable_fallback() {
    let app = app();
    let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
}
