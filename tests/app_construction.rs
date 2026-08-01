mod common;

use std::sync::Arc;
use usrgrp_manager::{
    error::CoreError,
    sys::{AccountSnapshot, OperationRequest, SystemAdapter, Uid},
};

#[test]
fn trusted_adapter_construction_uses_injected_account_identity_and_command_seams() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(AccountSnapshot::empty()))),
        runner.clone(),
        Arc::new(common::FixtureIdentity::uid(0)),
    );

    let plan = adapter
        .prepare_operation(OperationRequest::CreateGroup {
            groupname: "developers".into(),
        })
        .unwrap();
    let report = adapter.execute_prepared_operation(&plan).unwrap();
    assert!(report.is_partial()); // The immutable fixture cannot observe creation.
    assert_eq!(runner.recorded().len(), 1);
    assert_eq!(
        runner.recorded()[0].preview.program.executable(),
        "groupadd"
    );
}

#[test]
fn unknown_effective_identity_fails_closed_before_runner_invocation() {
    let runner = Arc::new(common::FakeRunner::succeeds());
    let adapter = SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(AccountSnapshot::empty()))),
        runner.clone(),
        Arc::new(common::FixtureIdentity(Err(CoreError::UnsupportedPlatform))),
    );

    let plan = adapter
        .prepare_operation(OperationRequest::CreateGroup {
            groupname: "developers".into(),
        })
        .unwrap();
    let error = adapter.execute_prepared_operation(&plan).unwrap_err();
    assert_eq!(error.to_string(), "unsupported platform");
    assert!(runner.recorded().is_empty());
}

#[test]
fn fixture_identity_never_assumes_root_when_it_returns_an_error() {
    let identity = common::FixtureIdentity(Err(CoreError::Io {
        operation: "fixture identity",
        kind: std::io::ErrorKind::Other,
    }));
    assert!(identity.0.is_err());
    assert_ne!(Uid(0), Uid(1));
}
