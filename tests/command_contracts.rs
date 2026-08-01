mod common;

use std::sync::Arc;
use usrgrp_manager::{
    error::CoreError,
    sys::{
        AccountGroup, AccountSnapshot, AccountUser, CommandRunner, ElevationGrant, Gecos, Gid,
        GroupName, OperationRequest, PasswordRecord, SecretString, ShellPath, SystemAdapter, Uid,
        UserName,
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

#[test]
fn fake_runner_receives_only_fixed_program_and_validated_arguments() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        Arc::new(common::FixtureIdentity::uid(1000)),
    );
    adapter.set_elevation_secret(SecretString::new("one-shot-authentication"));
    let plan = adapter
        .prepare_operation(OperationRequest::AddUserToGroup {
            username: "alice".into(),
            groupname: "dev".into(),
        })
        .unwrap();
    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert!(report.is_partial()); // Fixture state is immutable after fake command success.

    let calls = runner.recorded();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].grant, ElevationGrant::SudoTimestamp);
    assert_eq!(calls[0].preview.program.executable(), "gpasswd");
    assert_eq!(calls[0].preview.arguments, ["-a", "alice", "dev"]);
    assert!(!calls[0].preview.render().contains("bash"));
    assert!(!calls[0].preview.render().contains("-c"));
}

#[test]
fn password_is_stdin_only_and_never_in_the_recorded_preview() {
    let runner = common::FakeRunner::succeeds();
    let record = PasswordRecord::new(
        UserName::new("alice").unwrap(),
        SecretString::new("password-not-in-argv"),
    )
    .unwrap();
    let spec = usrgrp_manager::sys::CommandSpec::new(usrgrp_manager::sys::KnownProgram::ChPasswd)
        .password_record(record)
        .unwrap();
    runner.run(ElevationGrant::Direct, &spec).unwrap();

    let rendered = runner.recorded()[0].preview.render();
    assert!(rendered.starts_with("chpasswd"));
    assert!(rendered.contains("redacted password record"));
    assert!(!rendered.contains("password-not-in-argv"));
    assert!(!rendered.contains("bash"));
}

#[test]
fn authentication_required_is_distinct_from_command_execution_errors() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        Arc::new(common::FixtureIdentity::uid(1000)),
    );
    let plan = adapter
        .prepare_operation(OperationRequest::AddUserToGroup {
            username: "alice".into(),
            groupname: "dev".into(),
        })
        .unwrap();
    let error = adapter.execute_prepared_operation(&plan).unwrap_err();
    assert_eq!(
        error.to_string(),
        CoreError::AuthenticationRequired.to_string()
    );
    assert!(runner.recorded().is_empty());
}

#[test]
fn ordinary_tests_prove_privileged_programs_only_reach_the_fake_runner() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(snapshot()))),
        runner.clone(),
        Arc::new(common::FixtureIdentity::uid(0)),
    );
    let plan = adapter
        .prepare_operation(OperationRequest::ChangeUserShell {
            username: "alice".into(),
            shell: "/bin/bash".into(),
        })
        .unwrap();
    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert!(report.is_partial());
    let calls = runner.recorded();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].preview.program.executable(), "usermod");
    // `FakeRunner` implements no spawn path; seeing this call is proof the
    // command contract was inspected without executing host account tools.
}
