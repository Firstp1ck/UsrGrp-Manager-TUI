mod common;

use std::sync::Arc;

use usrgrp_manager::{
    app::{AppState, is_default_protected_group, is_default_protected_user},
    sys::{
        AccountGroup, AccountSnapshot, AccountUser, FixedIdentityProvider, Gecos, Gid, GroupName,
        ShellPath, SystemAdapter, Uid, UserName,
    },
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
                shell: ShellPath::new("/bin/sh").unwrap(),
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
        shells: vec![ShellPath::new("/bin/sh").unwrap()],
        diagnostics: vec![],
    }
}

fn app() -> AppState {
    let snapshot = snapshot();
    AppState::with_adapter(
        Arc::new(SystemAdapter::from_components(
            Arc::new(common::FixtureSource(Ok(snapshot.clone()))),
            Arc::new(common::FakeRunner::succeeds()),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        )),
        snapshot,
    )
}

#[test]
fn every_pane_selection_is_restored_from_stable_identity_after_reorder_and_removal() {
    let mut app = app();
    app.selected_user_index = 1;
    app.selected_group_index = 1;
    app.selected_user_group_index = 0;
    app.selected_group_member_index = 0;
    app.capture_selection_identities();
    assert_eq!(app.selected_user_uid, Some(1001));
    assert_eq!(app.selected_group_gid, Some(1001));
    assert_eq!(app.selected_user_group_gid, Some(1001));
    assert_eq!(app.selected_group_member_name.as_deref(), Some("bob"));

    app.users_all.reverse();
    app.groups_all.reverse();
    app.sort_and_filter();
    assert_eq!(app.users[app.selected_user_index].uid, 1001);
    assert_eq!(app.groups[app.selected_group_index].gid, 1001);
    assert_eq!(
        app.selected_user_groups()[app.selected_user_group_index].gid,
        1001
    );
    assert_eq!(
        app.groups[app.selected_group_index].members[app.selected_group_member_index].as_str(),
        "bob"
    );

    app.selected_user_uid = Some(4040);
    app.selected_group_gid = Some(4040);
    app.selected_user_group_gid = Some(4040);
    app.selected_group_member_name = Some("missing".into());
    app.normalize_selections();
    assert!(app.selected_user_index < app.users.len());
    assert!(app.selected_group_index < app.groups.len());
}

#[test]
fn ui_default_protected_policy_matches_w3_fail_closed_presentation() {
    let mut app = app();
    assert!(!is_default_protected_user(&app.users[0]));
    assert!(!is_default_protected_group(&app.groups[0]));
    app.users[0].uid = 42;
    app.groups[0].gid = 42;
    assert!(is_default_protected_user(&app.users[0]));
    assert!(is_default_protected_group(&app.groups[0]));
}
