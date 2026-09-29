mod common;

use std::{
    collections::VecDeque,
    sync::{Arc, Mutex},
};
use usrgrp_manager::{
    error::{CoreError, CoreResult},
    sys::{
        AccountDataSource, AccountGroup, AccountSnapshot, AccountUser, Gecos, Gid, GroupName,
        OperationRequest, ProtectedIdentityPolicy, ReconciliationStatus, ShellPath, SnapshotState,
        SystemAdapter, Uid, UserName,
    },
};

fn snapshot() -> AccountSnapshot {
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
            members: vec![],
        }],
        shells: vec![ShellPath::new("/bin/sh").unwrap()],
        diagnostics: vec![],
    }
}

fn adapter(
    source: Arc<dyn AccountDataSource>,
    runner: Arc<common::FakeRunner>,
    uid: u32,
) -> SystemAdapter {
    SystemAdapter::from_components(source, runner, Arc::new(common::FixtureIdentity::uid(uid)))
}

#[test]
fn prepared_preview_and_execution_use_the_same_bound_identity_and_command() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        0,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        })
        .unwrap();
    let preview = plan.redacted_preview();
    let report = adapter.execute_prepared_operation(&plan).unwrap();

    assert_eq!(plan.bound_targets.len(), 1);
    assert_eq!(
        preview,
        runner
            .recorded()
            .into_iter()
            .map(|call| call.preview)
            .collect::<Vec<_>>()
    );
    // The static fixture does not apply the fake mutation, so the trusted
    // boundary reports the postcondition honestly instead of claiming success.
    assert!(report.is_partial());
    assert!(matches!(
        report.reconciliation,
        ReconciliationStatus::Partial { .. }
    ));
}

#[test]
fn authentication_required_is_returned_before_any_prepared_command_runs() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        1000,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        })
        .unwrap();

    let error = adapter.execute_prepared_operation(&plan).unwrap_err();
    assert_eq!(error, CoreError::AuthenticationRequired);
    assert!(runner.recorded().is_empty());

    adapter.set_elevation_secret(usrgrp_manager::sys::SecretString::new("fixture-elevation"));
    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert!(report.is_partial());
    assert_eq!(runner.recorded().len(), 1);
}

#[test]
fn root_and_changed_targets_are_rejected_before_runner_execution() {
    let root = AccountSnapshot {
        users: vec![AccountUser {
            uid: Uid(0),
            name: UserName::new("root").unwrap(),
            primary_gid: Gid(0),
            full_name: None,
            home_dir: "/root".into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        }],
        groups: vec![],
        shells: vec![ShellPath::new("/bin/sh").unwrap()],
        diagnostics: vec![],
    };
    let root_runner = Arc::new(common::FakeRunner::succeeds());
    let root_adapter = adapter(
        Arc::new(common::FixtureSource(Ok(root))),
        root_runner.clone(),
        0,
    );
    assert!(matches!(
        root_adapter.prepare_operation(OperationRequest::ChangeUserShell {
            username: "root".into(),
            shell: "/bin/bash".into(),
        }),
        Err(CoreError::Validation { .. })
    ));
    assert!(root_runner.recorded().is_empty());

    let mut changed = snapshot();
    changed.users[0].uid = Uid(1001);
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(SequencedSource::new([Ok(snapshot()), Ok(changed)])),
        runner.clone(),
        0,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        })
        .unwrap();
    assert!(matches!(
        adapter.execute_prepared_operation(&plan),
        Err(CoreError::Validation { .. })
    ));
    assert!(runner.recorded().is_empty());
}

#[test]
fn failed_prepared_command_returns_a_partial_report_and_reconciles() {
    let runner = Arc::new(common::FakeRunner::failing_at(
        1,
        CoreError::ExitStatus {
            program: "usermod",
            code: Some(1),
        },
    ));
    let adapter = adapter(Arc::new(common::FixtureSource(Ok(snapshot()))), runner, 0);
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        })
        .unwrap();
    let report = adapter.execute_prepared_operation(&plan).unwrap();

    assert!(report.is_partial());
    assert!(report.failed.is_some());
    assert!(matches!(
        report.reconciliation,
        ReconciliationStatus::Partial { .. }
    ));
}

#[test]
fn lock_and_unlock_preview_the_exact_usermod_command_and_stay_unverified() {
    for (request, command, id) in [
        (
            OperationRequest::LockUser {
                username: "alice".into(),
            },
            "usermod -L alice",
            "lock user",
        ),
        (
            OperationRequest::UnlockUser {
                username: "alice".into(),
            },
            "usermod -U alice",
            "unlock user",
        ),
    ] {
        let runner = Arc::new(common::FakeRunner::succeeds());
        let adapter = adapter(
            Arc::new(common::FixtureSource(Ok(snapshot()))),
            runner.clone(),
            0,
        );
        let plan = adapter.prepare_operation(request).unwrap();
        let preview = plan.redacted_preview();
        assert_eq!(
            preview.iter().map(|line| line.render()).collect::<Vec<_>>(),
            [command]
        );
        assert_eq!(plan.bound_targets.len(), 1);

        let report = adapter.execute_prepared_operation(&plan).unwrap();
        assert_eq!(
            preview,
            runner
                .recorded()
                .into_iter()
                .map(|call| call.preview)
                .collect::<Vec<_>>()
        );
        // The lock flag lives in /etc/shadow, which the account source does
        // not read: a successful child is reported as unobserved.
        assert!(report.failed.is_none());
        assert_eq!(report.completed.len(), 1);
        assert_eq!(report.completed[0].id, id);
        assert!(matches!(
            report.completed[0].verification,
            usrgrp_manager::sys::StepVerification::Unverified { .. }
        ));
        assert!(matches!(
            report.reconciliation,
            ReconciliationStatus::Unavailable { .. }
        ));
    }
}

#[test]
fn lock_and_unlock_reject_root_and_protected_users_before_runner_execution() {
    let root = AccountSnapshot {
        users: vec![AccountUser {
            uid: Uid(0),
            name: UserName::new("root").unwrap(),
            primary_gid: Gid(0),
            full_name: None,
            home_dir: "/root".into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        }],
        groups: vec![],
        shells: vec![ShellPath::new("/bin/sh").unwrap()],
        diagnostics: vec![],
    };
    let mut service = snapshot();
    service.users[0].uid = Uid(998);
    for (accounts, username) in [(root, "root"), (service, "alice")] {
        let runner = Arc::new(common::FakeRunner::succeeds());
        let adapter = adapter(
            Arc::new(common::FixtureSource(Ok(accounts))),
            runner.clone(),
            0,
        );
        for request in [
            OperationRequest::LockUser {
                username: username.into(),
            },
            OperationRequest::UnlockUser {
                username: username.into(),
            },
        ] {
            assert!(matches!(
                adapter.prepare_operation(request),
                Err(CoreError::Validation { .. })
            ));
        }
        assert!(runner.recorded().is_empty());
    }

    // A target whose UID changed after preview is refused before running.
    let mut changed = snapshot();
    changed.users[0].uid = Uid(1001);
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(SequencedSource::new([Ok(snapshot()), Ok(changed)])),
        runner.clone(),
        0,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::LockUser {
            username: "alice".into(),
        })
        .unwrap();
    assert!(matches!(
        adapter.execute_prepared_operation(&plan),
        Err(CoreError::Validation { .. })
    ));
    assert!(runner.recorded().is_empty());
}

/// `snapshot()` plus group `staff` (GID 1001), system group `users` (GID
/// 100), and elevation group `wheel` (GID 1010).
fn snapshot_with_groups() -> AccountSnapshot {
    let mut accounts = snapshot();
    for (gid, name) in [(1001, "staff"), (100, "users"), (1010, "wheel")] {
        accounts.groups.push(AccountGroup {
            gid: Gid(gid),
            name: GroupName::new(name).unwrap(),
            members: vec![],
        });
    }
    accounts
}

/// Serves `before` until the runner has run a command, then `after`, the way
/// the account files look once a real child has changed them.
struct AppliedSource {
    before: AccountSnapshot,
    after: AccountSnapshot,
    runner: Arc<common::FakeRunner>,
}

impl AccountDataSource for AppliedSource {
    fn refresh(&self) -> CoreResult<AccountSnapshot> {
        Ok(if self.runner.recorded().is_empty() {
            self.before.clone()
        } else {
            self.after.clone()
        })
    }
}

#[test]
fn primary_group_and_home_changes_preview_exact_usermod_commands_and_verify() {
    // Each request with its preview, step id, and alice's primary GID and
    // home once the command has run.
    let cases = [
        (
            OperationRequest::ChangeUserPrimaryGroup {
                username: "alice".into(),
                groupname: "staff".into(),
            },
            "usermod -g staff alice",
            "change user primary group",
            (Gid(1001), "/home/alice"),
        ),
        (
            OperationRequest::ChangeUserHome {
                username: "alice".into(),
                home: "/srv/alice".into(),
                move_contents: false,
            },
            "usermod -d /srv/alice alice",
            "change user home",
            (Gid(1000), "/srv/alice"),
        ),
        (
            OperationRequest::ChangeUserHome {
                username: "alice".into(),
                home: "/srv/alice".into(),
                move_contents: true,
            },
            "usermod -d /srv/alice -m alice",
            "change user home",
            (Gid(1000), "/srv/alice"),
        ),
    ];
    for (request, command, id, (gid, home)) in cases {
        let runner = Arc::new(common::FakeRunner::succeeds());
        let mut after = snapshot_with_groups();
        after.users[0].primary_gid = gid;
        after.users[0].home_dir = home.into();
        let adapter = adapter(
            Arc::new(AppliedSource {
                before: snapshot_with_groups(),
                after,
                runner: runner.clone(),
            }),
            runner.clone(),
            0,
        );
        let plan = adapter.prepare_operation(request).unwrap();
        let preview = plan.redacted_preview();
        assert_eq!(
            preview.iter().map(|line| line.render()).collect::<Vec<_>>(),
            [command]
        );
        // The primary group binds the target group too.
        let targets = if id == "change user primary group" {
            2
        } else {
            1
        };
        assert_eq!(plan.bound_targets.len(), targets, "{command}");

        let report = adapter.execute_prepared_operation(&plan).unwrap();
        assert_eq!(
            preview,
            runner
                .recorded()
                .into_iter()
                .map(|call| call.preview)
                .collect::<Vec<_>>()
        );
        // passwd holds both fields, so the result is checked, not assumed.
        assert!(report.failed.is_none(), "{command}");
        assert_eq!(report.completed.len(), 1);
        assert_eq!(report.completed[0].id, id);
        assert!(
            matches!(report.reconciliation, ReconciliationStatus::Verified),
            "{command}"
        );
    }

    // A child that exits 0 without changing passwd is reported as partial.
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot_with_groups()))),
        runner.clone(),
        0,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserHome {
            username: "alice".into(),
            home: "/srv/alice".into(),
            move_contents: true,
        })
        .unwrap();
    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert!(matches!(
        report.reconciliation,
        ReconciliationStatus::Partial { .. }
    ));
}

#[test]
fn primary_group_and_home_already_in_place_run_nothing() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot_with_groups()))),
        runner.clone(),
        0,
    );
    for request in [
        OperationRequest::ChangeUserPrimaryGroup {
            username: "alice".into(),
            groupname: "dev".into(),
        },
        OperationRequest::ChangeUserHome {
            username: "alice".into(),
            home: "/home/alice".into(),
            move_contents: true,
        },
    ] {
        let plan = adapter.prepare_operation(request).unwrap();
        let report = adapter.execute_prepared_operation(&plan).unwrap();
        assert!(report.completed.is_empty());
        assert_eq!(report.skipped.len(), 1);
    }
    assert!(runner.recorded().is_empty());
}

/// `snapshot_with_groups()` plus bob, who shares alice's UID 1000 but has
/// primary group `staff` (GID 1001) and home `/home/bob`. alice comes first,
/// so a lookup by UID alone finds her.
fn shared_uid_snapshot() -> AccountSnapshot {
    let mut accounts = snapshot_with_groups();
    accounts.users.push(AccountUser {
        uid: Uid(1000),
        name: UserName::new("bob").unwrap(),
        primary_gid: Gid(1001),
        full_name: None,
        home_dir: "/home/bob".into(),
        shell: ShellPath::new("/bin/sh").unwrap(),
    });
    accounts
}

#[test]
fn primary_group_and_home_checks_bind_to_the_named_account_when_uids_are_shared() {
    // Each request for bob sets the value alice already has, with the
    // command it must run and how to apply the change to one account.
    type Request = fn() -> OperationRequest;
    type Apply = fn(&mut AccountUser);
    let cases: [(Request, &str, Apply); 2] = [
        (
            || OperationRequest::ChangeUserHome {
                username: "bob".into(),
                home: "/home/alice".into(),
                move_contents: false,
            },
            "usermod -d /home/alice bob",
            |user| user.home_dir = "/home/alice".into(),
        ),
        (
            || OperationRequest::ChangeUserPrimaryGroup {
                username: "bob".into(),
                groupname: "dev".into(),
            },
            "usermod -g dev bob",
            |user| user.primary_gid = Gid(1000),
        ),
    ];
    for (request, command, apply) in cases {
        // Afterwards, the change is observed on `changed` only (index 0 is
        // alice, 1 is bob); reconciliation must look at bob.
        for (changed, verified) in [(1, true), (0, false)] {
            let runner = Arc::new(common::FakeRunner::succeeds());
            let mut after = shared_uid_snapshot();
            apply(&mut after.users[changed]);
            let adapter = adapter(
                Arc::new(AppliedSource {
                    before: shared_uid_snapshot(),
                    after,
                    runner: runner.clone(),
                }),
                runner.clone(),
                0,
            );
            let plan = adapter.prepare_operation(request()).unwrap();
            let report = adapter.execute_prepared_operation(&plan).unwrap();
            // alice already holding the value does not skip bob's change.
            assert!(report.skipped.is_empty(), "{command}");
            assert_eq!(report.completed.len(), 1, "{command}");
            assert_eq!(
                runner
                    .recorded()
                    .into_iter()
                    .map(|call| call.preview.render())
                    .collect::<Vec<_>>(),
                [command]
            );
            if verified {
                assert!(
                    matches!(report.reconciliation, ReconciliationStatus::Verified),
                    "{command}: {:?}",
                    report.reconciliation
                );
            } else {
                assert!(
                    matches!(report.reconciliation, ReconciliationStatus::Partial { .. }),
                    "{command} changed on alice: {:?}",
                    report.reconciliation
                );
            }
        }
    }
}

#[test]
fn home_change_rejects_paths_the_home_validation_refuses() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        0,
    );
    for home in ["", "srv/alice", "/srv/a:b", "/srv/a\nb"] {
        assert!(
            matches!(
                adapter.prepare_operation(OperationRequest::ChangeUserHome {
                    username: "alice".into(),
                    home: home.into(),
                    move_contents: false,
                }),
                Err(CoreError::Validation {
                    field: "home directory",
                    ..
                })
            ),
            "{home:?}"
        );
    }
    assert!(runner.recorded().is_empty());
}

#[test]
fn primary_group_target_must_pass_the_membership_policy() {
    let mut accounts = snapshot_with_groups();
    accounts.groups.push(AccountGroup {
        gid: Gid(0),
        name: GroupName::new("root").unwrap(),
        members: vec![],
    });
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(accounts))),
        runner.clone(),
        0,
    );
    for (group, gid) in [("users", 100), ("wheel", 1010), ("root", 0)] {
        assert!(adapter.membership_allowed(group, gid).is_err(), "{group}");
        assert!(
            matches!(
                adapter.prepare_operation(OperationRequest::ChangeUserPrimaryGroup {
                    username: "alice".into(),
                    groupname: group.into(),
                }),
                Err(CoreError::Validation { .. })
            ),
            "{group}"
        );
    }
    assert!(adapter.membership_allowed("staff", 1001).is_ok());
    assert!(
        adapter
            .prepare_operation(OperationRequest::ChangeUserPrimaryGroup {
                username: "alice".into(),
                groupname: "staff".into(),
            })
            .is_ok()
    );
    // A group missing from the snapshot is refused as well.
    assert!(
        adapter
            .prepare_operation(OperationRequest::ChangeUserPrimaryGroup {
                username: "alice".into(),
                groupname: "nosuch".into(),
            })
            .is_err()
    );
    assert!(runner.recorded().is_empty());

    // An allowlisted elevation group is accepted, as it is for membership.
    let allowing = SystemAdapter::from_components_with_policy(
        Arc::new(common::FixtureSource(Ok(snapshot_with_groups()))),
        runner.clone(),
        Arc::new(common::FixtureIdentity::uid(0)),
        ProtectedIdentityPolicy::fail_closed()
            .allow_elevation_membership_group(GroupName::new("wheel").unwrap()),
    );
    assert!(
        allowing
            .prepare_operation(OperationRequest::ChangeUserPrimaryGroup {
                username: "alice".into(),
                groupname: "wheel".into(),
            })
            .is_ok()
    );
}

#[test]
fn primary_group_and_home_reject_root_protected_and_changed_users_before_runner_execution() {
    let root = AccountSnapshot {
        users: vec![AccountUser {
            uid: Uid(0),
            name: UserName::new("root").unwrap(),
            primary_gid: Gid(0),
            full_name: None,
            home_dir: "/root".into(),
            shell: ShellPath::new("/bin/sh").unwrap(),
        }],
        groups: snapshot_with_groups().groups,
        shells: vec![ShellPath::new("/bin/sh").unwrap()],
        diagnostics: vec![],
    };
    let mut service = snapshot_with_groups();
    service.users[0].uid = Uid(998);
    for (accounts, username) in [(root, "root"), (service, "alice")] {
        let runner = Arc::new(common::FakeRunner::succeeds());
        let adapter = adapter(
            Arc::new(common::FixtureSource(Ok(accounts))),
            runner.clone(),
            0,
        );
        for request in [
            OperationRequest::ChangeUserPrimaryGroup {
                username: username.into(),
                groupname: "staff".into(),
            },
            OperationRequest::ChangeUserHome {
                username: username.into(),
                home: "/srv/elsewhere".into(),
                move_contents: true,
            },
        ] {
            assert!(matches!(
                adapter.prepare_operation(request),
                Err(CoreError::Validation { .. })
            ));
        }
        assert!(runner.recorded().is_empty());
    }

    // A target whose UID changed after preview is refused before running.
    let mut changed = snapshot_with_groups();
    changed.users[0].uid = Uid(1001);
    for request in [
        OperationRequest::ChangeUserPrimaryGroup {
            username: "alice".into(),
            groupname: "staff".into(),
        },
        OperationRequest::ChangeUserHome {
            username: "alice".into(),
            home: "/srv/alice".into(),
            move_contents: false,
        },
    ] {
        let runner = Arc::new(common::FakeRunner::succeeds());
        let adapter = adapter(
            Arc::new(SequencedSource::new([
                Ok(snapshot_with_groups()),
                Ok(changed.clone()),
            ])),
            runner.clone(),
            0,
        );
        let plan = adapter.prepare_operation(request).unwrap();
        assert!(matches!(
            adapter.execute_prepared_operation(&plan),
            Err(CoreError::Validation { .. })
        ));
        assert!(runner.recorded().is_empty());
    }
}

#[test]
fn composite_password_and_expiry_runs_all_required_commands_when_shadow_is_unobservable() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        0,
    );
    let record = usrgrp_manager::sys::PasswordRecord::new(
        UserName::new("alice").unwrap(),
        usrgrp_manager::sys::SecretString::new("fixture-password"),
    )
    .unwrap();
    let plan = adapter
        .prepare_operation(OperationRequest::Composite {
            requests: vec![
                OperationRequest::SetUserPassword { record },
                OperationRequest::ExpireUserPassword {
                    username: "alice".into(),
                },
            ],
        })
        .unwrap();

    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert_eq!(plan.redacted_preview().len(), 2);
    assert_eq!(runner.recorded().len(), 2);
    assert_eq!(report.completed.len(), 2);
    assert!(report.completed.iter().all(|step| matches!(
        step.verification,
        usrgrp_manager::sys::StepVerification::Unverified { .. }
    )));
    assert!(report.is_partial());
}

#[test]
fn composite_create_password_and_membership_prepares_one_redacted_ordered_plan() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(Arc::new(common::FixtureSource(Ok(snapshot()))), runner, 0);
    let record = usrgrp_manager::sys::PasswordRecord::new(
        UserName::new("newuser").unwrap(),
        usrgrp_manager::sys::SecretString::new("fixture-password"),
    )
    .unwrap();
    let plan = adapter
        .prepare_operation(OperationRequest::Composite {
            requests: vec![
                OperationRequest::CreateUser {
                    username: "newuser".into(),
                    create_home: true,
                },
                OperationRequest::SetUserPassword { record },
                OperationRequest::AddUserToGroup {
                    username: "newuser".into(),
                    groupname: "dev".into(),
                },
            ],
        })
        .unwrap();

    let preview = plan.redacted_preview();
    assert_eq!(preview.len(), 3);
    assert_eq!(preview[0].program.executable(), "useradd");
    assert_eq!(preview[1].program.executable(), "chpasswd");
    assert_eq!(preview[2].program.executable(), "gpasswd");
    assert!(preview[1].render().contains("redacted password record"));
}

#[test]
fn elevation_grant_is_scoped_to_one_execution_and_allows_later_reauthentication() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        1000,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        })
        .unwrap();

    adapter.set_elevation_secret(usrgrp_manager::sys::SecretString::new("first-secret"));
    let first = adapter.execute_prepared_operation(&plan).unwrap();
    assert!(first.is_partial());
    assert_eq!(runner.recorded().len(), 1);

    assert_eq!(
        adapter.execute_prepared_operation(&plan).unwrap_err(),
        CoreError::AuthenticationRequired
    );
    adapter.set_elevation_secret(usrgrp_manager::sys::SecretString::new("second-secret"));
    assert!(adapter.execute_prepared_operation(&plan).is_ok());
    assert_eq!(runner.recorded().len(), 2);
}

#[test]
fn observed_membership_retry_skips_before_elevation_or_command_execution() {
    let mut accounts = snapshot();
    accounts.groups[0]
        .members
        .push(UserName::new("alice").unwrap());
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(accounts))),
        runner.clone(),
        1000,
    );
    let plan = adapter
        .prepare_operation(OperationRequest::AddUserToGroup {
            username: "alice".into(),
            groupname: "dev".into(),
        })
        .unwrap();

    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert_eq!(report.skipped.len(), 1);
    assert!(matches!(
        report.skipped[0].kind,
        usrgrp_manager::sys::SkipKind::AlreadySatisfied { .. }
    ));
    assert!(runner.recorded().is_empty());
}

#[test]
fn service_and_elevation_membership_require_explicit_injected_policy() {
    let mut accounts = snapshot();
    accounts.groups[0].gid = Gid(10);
    accounts.groups[0].name = GroupName::new("wheel").unwrap();
    let request = || OperationRequest::AddUserToGroup {
        username: "alice".into(),
        groupname: "wheel".into(),
    };
    let denied = adapter(
        Arc::new(common::FixtureSource(Ok(accounts.clone()))),
        Arc::new(common::FakeRunner::succeeds()),
        0,
    );
    assert!(matches!(
        denied.prepare_operation(request()),
        Err(CoreError::Validation { .. })
    ));
    // The read-only query agrees with preparation and names the elevation
    // rule first.
    assert_eq!(
        denied.membership_allowed("wheel", 10),
        Err("sudo group, protected")
    );
    assert_eq!(
        denied.membership_allowed("audio", 10),
        Err("system group, protected")
    );
    assert_eq!(
        denied.membership_allowed("root", 0),
        Err("root group is immutable")
    );
    assert_eq!(denied.membership_allowed("dev", 1000), Ok(()));

    let elevation_only = SystemAdapter::from_components_with_policy(
        Arc::new(common::FixtureSource(Ok(accounts.clone()))),
        Arc::new(common::FakeRunner::succeeds()),
        Arc::new(common::FixtureIdentity::uid(0)),
        ProtectedIdentityPolicy::fail_closed()
            .allow_elevation_membership_group(GroupName::new("wheel").unwrap()),
    );
    assert_eq!(
        elevation_only.membership_allowed("wheel", 10),
        Err("system group, protected")
    );
    assert!(elevation_only.prepare_operation(request()).is_err());

    let allowed = SystemAdapter::from_components_with_policy(
        Arc::new(common::FixtureSource(Ok(accounts))),
        Arc::new(common::FakeRunner::succeeds()),
        Arc::new(common::FixtureIdentity::uid(0)),
        ProtectedIdentityPolicy::fail_closed()
            .allow_service_group(Gid(10))
            .allow_elevation_membership_group(GroupName::new("wheel").unwrap()),
    );
    assert_eq!(allowed.membership_allowed("wheel", 10), Ok(()));
    assert!(allowed.prepare_operation(request()).is_ok());
}

#[test]
fn create_password_membership_failure_keeps_completed_and_downstream_skipped_evidence() {
    let base = snapshot();
    let mut after_create = base.clone();
    after_create.users.push(AccountUser {
        uid: Uid(1001),
        name: UserName::new("newuser").unwrap(),
        primary_gid: Gid(1000),
        full_name: None,
        home_dir: "/home/newuser".into(),
        shell: ShellPath::new("/bin/sh").unwrap(),
    });
    let source = SequencedSource::new([
        Ok(base.clone()),
        Ok(base.clone()),
        Ok(base.clone()),
        Ok(base.clone()),
        Ok(base),
        Ok(after_create.clone()),
        Ok(after_create),
    ]);
    let runner = Arc::new(common::FakeRunner::failing_at(
        2,
        CoreError::ExitStatus {
            program: "chpasswd",
            code: Some(1),
        },
    ));
    let adapter = adapter(Arc::new(source), runner, 0);
    let record = usrgrp_manager::sys::PasswordRecord::new(
        UserName::new("newuser").unwrap(),
        usrgrp_manager::sys::SecretString::new("fixture-password"),
    )
    .unwrap();
    let plan = adapter
        .prepare_operation(OperationRequest::Composite {
            requests: vec![
                OperationRequest::CreateUser {
                    username: "newuser".into(),
                    create_home: true,
                },
                OperationRequest::SetUserPassword { record },
                OperationRequest::AddUserToGroup {
                    username: "newuser".into(),
                    groupname: "dev".into(),
                },
            ],
        })
        .unwrap();

    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert!(matches!(report.failed, Some(ref failed) if failed.id == "set user password"));
    assert!(matches!(
        report.skipped.as_slice(),
        [skipped] if skipped.id == "add user to group"
            && matches!(skipped.kind, usrgrp_manager::sys::SkipKind::DownstreamFailure { .. })
    ));
}

#[test]
fn password_expiry_failure_reports_the_real_second_boundary() {
    let runner = Arc::new(common::FakeRunner::failing_at(
        2,
        CoreError::ExitStatus {
            program: "chage",
            code: Some(1),
        },
    ));
    let adapter = adapter(Arc::new(common::FixtureSource(Ok(snapshot()))), runner, 0);
    let record = usrgrp_manager::sys::PasswordRecord::new(
        UserName::new("alice").unwrap(),
        usrgrp_manager::sys::SecretString::new("fixture-password"),
    )
    .unwrap();
    let plan = adapter
        .prepare_operation(OperationRequest::Composite {
            requests: vec![
                OperationRequest::SetUserPassword { record },
                OperationRequest::ExpireUserPassword {
                    username: "alice".into(),
                },
            ],
        })
        .unwrap();

    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert!(matches!(
        report.completed[0].verification,
        usrgrp_manager::sys::StepVerification::Unverified { .. }
    ));
    assert!(matches!(report.failed, Some(ref failed) if failed.id == "expire user password"));
}

#[test]
fn bulk_membership_failure_reports_prior_completion_without_replay() {
    let mut base = snapshot();
    base.users.push(AccountUser {
        uid: Uid(1001),
        name: UserName::new("bob").unwrap(),
        primary_gid: Gid(1000),
        full_name: None,
        home_dir: "/home/bob".into(),
        shell: ShellPath::new("/bin/sh").unwrap(),
    });
    let mut after_first = base.clone();
    after_first.groups[0]
        .members
        .push(UserName::new("alice").unwrap());
    let source = SequencedSource::new([
        Ok(base.clone()),
        Ok(base.clone()),
        Ok(base.clone()),
        Ok(base.clone()),
        Ok(base),
        Ok(after_first.clone()),
        Ok(after_first.clone()),
        Ok(after_first),
    ]);
    let runner = Arc::new(common::FakeRunner::failing_at(
        2,
        CoreError::ExitStatus {
            program: "gpasswd",
            code: Some(1),
        },
    ));
    let adapter = adapter(Arc::new(source), runner, 0);
    let plan = adapter
        .prepare_operation(OperationRequest::Composite {
            requests: vec![
                OperationRequest::AddUserToGroup {
                    username: "alice".into(),
                    groupname: "dev".into(),
                },
                OperationRequest::AddUserToGroup {
                    username: "bob".into(),
                    groupname: "dev".into(),
                },
            ],
        })
        .unwrap();

    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert_eq!(report.completed.len(), 1);
    assert!(matches!(report.failed, Some(ref failed) if failed.id == "add user to group"));
    assert!(report.skipped.is_empty());
}

#[test]
fn every_supported_request_compiles_to_a_redacted_bridge_plan() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(
        Arc::new(common::FixtureSource(Ok(snapshot_with_groups()))),
        runner,
        0,
    );
    let requests = vec![
        OperationRequest::AddUserToGroup {
            username: "alice".into(),
            groupname: "dev".into(),
        },
        OperationRequest::RemoveUserFromGroup {
            username: "alice".into(),
            groupname: "dev".into(),
        },
        OperationRequest::CreateGroup {
            groupname: "newgroup".into(),
        },
        OperationRequest::CreateUser {
            username: "newuser".into(),
            create_home: true,
        },
        OperationRequest::DeleteGroup {
            groupname: "dev".into(),
        },
        OperationRequest::RenameGroup {
            old_name: "dev".into(),
            new_name: "newgroup".into(),
        },
        OperationRequest::DeleteUser {
            username: "alice".into(),
            delete_home: false,
        },
        OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        },
        OperationRequest::ChangeUserGecos {
            username: "alice".into(),
            gecos: "Alice Example".into(),
        },
        OperationRequest::RenameUser {
            old_username: "alice".into(),
            new_username: "newuser".into(),
        },
        OperationRequest::SetUserPassword {
            record: usrgrp_manager::sys::PasswordRecord::new(
                UserName::new("alice").unwrap(),
                usrgrp_manager::sys::SecretString::new("fixture-password"),
            )
            .unwrap(),
        },
        OperationRequest::ExpireUserPassword {
            username: "alice".into(),
        },
        OperationRequest::LockUser {
            username: "alice".into(),
        },
        OperationRequest::UnlockUser {
            username: "alice".into(),
        },
        OperationRequest::ChangeUserPrimaryGroup {
            username: "alice".into(),
            groupname: "staff".into(),
        },
        OperationRequest::ChangeUserHome {
            username: "alice".into(),
            home: "/srv/alice".into(),
            move_contents: true,
        },
        OperationRequest::Composite {
            requests: vec![
                OperationRequest::AddUserToGroup {
                    username: "alice".into(),
                    groupname: "dev".into(),
                },
                OperationRequest::RemoveUserFromGroup {
                    username: "alice".into(),
                    groupname: "dev".into(),
                },
            ],
        },
    ];

    for request in requests {
        let plan = adapter.prepare_operation(request).unwrap();
        assert!(!plan.steps.is_empty());
        for preview in plan.redacted_preview() {
            assert!(!preview.render().contains("bash -c"));
            assert!(!preview.render().contains("fixture-password"));
        }
    }
}

#[test]
fn adapter_refresh_state_retains_stale_snapshot() {
    let source = common::FixtureSource(Err(CoreError::Refresh {
        operation: "fixture accounts",
        kind: std::io::ErrorKind::PermissionDenied,
    }));
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = adapter(Arc::new(source), runner, 0);

    assert!(matches!(
        adapter.refresh_state(Some(snapshot())),
        SnapshotState::Stale {
            error: CoreError::Refresh { .. },
            ..
        }
    ));
}

struct SequencedSource {
    snapshots: Mutex<VecDeque<CoreResult<AccountSnapshot>>>,
}

impl SequencedSource {
    fn new(values: impl IntoIterator<Item = CoreResult<AccountSnapshot>>) -> Self {
        Self {
            snapshots: Mutex::new(values.into_iter().collect()),
        }
    }
}

impl AccountDataSource for SequencedSource {
    fn refresh(&self) -> CoreResult<AccountSnapshot> {
        self.snapshots
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or(Err(CoreError::Refresh {
                operation: "sequenced fixture exhausted",
                kind: std::io::ErrorKind::UnexpectedEof,
            }))
    }
}
