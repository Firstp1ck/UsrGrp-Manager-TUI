mod common;

use std::sync::Arc;

use usrgrp_manager::sys::{
    AccountGroup, AccountSnapshot, AccountUser, Gecos, Gid, GroupName, OperationRequest, ShellPath,
    SystemAdapter, Uid, UserName,
};

fn snapshot(member_present: bool) -> AccountSnapshot {
    AccountSnapshot {
        users: vec![AccountUser {
            uid: Uid(1000),
            name: UserName::new("alice").unwrap(),
            primary_gid: Gid(1000),
            full_name: Some(Gecos::new("Alice").unwrap()),
            home_dir: "/home/alice".into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        }],
        groups: vec![AccountGroup {
            gid: Gid(1000),
            name: GroupName::new("dev").unwrap(),
            members: if member_present {
                vec![UserName::new("alice").unwrap()]
            } else {
                Vec::new()
            },
        }],
        shells: vec![ShellPath::new("/bin/sh").unwrap()],
        diagnostics: vec![],
    }
}

#[test]
fn observed_membership_retry_matrix_skips_each_already_satisfied_direction_before_elevation() {
    let cases = [
        (
            "add already present",
            true,
            OperationRequest::AddUserToGroup {
                username: "alice".into(),
                groupname: "dev".into(),
            },
        ),
        (
            "remove already absent",
            false,
            OperationRequest::RemoveUserFromGroup {
                username: "alice".into(),
                groupname: "dev".into(),
            },
        ),
    ];
    for (name, member_present, request) in cases {
        let runner = Arc::new(common::FakeRunner::succeeds());
        let adapter = SystemAdapter::from_components(
            Arc::new(common::FixtureSource(Ok(snapshot(member_present)))),
            runner.clone(),
            Arc::new(common::FixtureIdentity::uid(1000)),
        );
        let plan = adapter.prepare_operation(request).unwrap();
        let report = adapter.execute_prepared_operation(&plan).unwrap();
        assert_eq!(report.skipped.len(), 1, "{name}");
        assert!(runner.recorded().is_empty(), "{name}");
    }
}
