mod common;

use std::sync::Arc;
use usrgrp_manager::{
    error::CoreError,
    sys::{
        AccountDataSource, AccountPaths, AccountSnapshot, LocalFileAccountDataSource, ShellPath,
        SnapshotState, Uid, parse_group_records, parse_passwd_records, parse_shell_records,
        refresh_retaining,
    },
};

const PASSWD: &str = include_str!("fixtures/passwd");
const GROUP: &str = include_str!("fixtures/group");
const SHELLS: &str = include_str!("fixtures/shells");

#[test]
fn malformed_ids_are_diagnosed_and_never_coerced_to_root() {
    let users = parse_passwd_records(PASSWD, "fixtures/passwd");
    assert_eq!(users.records.len(), 3);
    assert_eq!(users.records[0].uid, Uid(0));
    assert_eq!(users.records[1].uid, Uid(1000));
    assert_eq!(users.diagnostics.len(), 1);
    assert_eq!(users.diagnostics[0].line, 5);

    let groups = parse_group_records(GROUP, "fixtures/group");
    assert_eq!(groups.records.len(), 2);
    assert_eq!(groups.records[0].gid.0, 0);
    assert_eq!(groups.diagnostics.len(), 1);
    assert_eq!(groups.diagnostics[0].line, 3);
}

#[test]
fn local_file_records_are_typed_and_shells_are_validated() {
    let users = parse_passwd_records(PASSWD, "fixtures/passwd");
    assert_eq!(users.records[1].name.as_str(), "alice");
    assert_eq!(users.records[1].shell.as_str(), "/bin/bash");
    assert_eq!(users.records[2].name.as_str(), "defaultsh");
    assert!(users.records[2].shell.is_observed_default());
    assert_eq!(users.records[2].shell.display_label(), "(default /bin/sh)");
    assert!(ShellPath::new("").is_err());

    let shells = parse_shell_records(SHELLS, "fixtures/shells");
    assert_eq!(shells.records.len(), 2);
    assert_eq!(shells.diagnostics.len(), 1);
}

struct FailingSource;

impl AccountDataSource for FailingSource {
    fn refresh(&self) -> Result<AccountSnapshot, CoreError> {
        Err(CoreError::Refresh {
            operation: "fixture accounts",
            kind: std::io::ErrorKind::PermissionDenied,
        })
    }
}

#[test]
fn refresh_failure_retains_the_known_good_snapshot_as_stale() {
    let prior = AccountSnapshot::empty();
    let state = refresh_retaining(&FailingSource, Some(prior.clone()));
    assert_eq!(
        state,
        SnapshotState::Stale {
            prior,
            error: CoreError::Refresh {
                operation: "fixture accounts",
                kind: std::io::ErrorKind::PermissionDenied,
            },
        }
    );
}

#[test]
fn local_file_source_rejects_an_oversized_account_file_while_reading() {
    let root = tempfile::tempdir().unwrap();
    let passwd = root.path().join("passwd");
    let group = root.path().join("group");
    let shells = root.path().join("shells");
    std::fs::write(&passwd, vec![b'x'; 1024 * 1024 + 1]).unwrap();
    std::fs::write(&group, "dev:x:1000:\n").unwrap();
    std::fs::write(&shells, "/bin/sh\n").unwrap();
    let source = LocalFileAccountDataSource::with_paths(AccountPaths {
        passwd,
        group,
        shells,
    });

    assert!(matches!(
        source.refresh(),
        Err(CoreError::Validation { .. })
    ));
}

#[test]
fn fixture_source_is_injectable_without_host_account_reads() {
    let source = common::FixtureSource(Ok(AccountSnapshot::empty()));
    let source: Arc<dyn AccountDataSource> = Arc::new(source);
    assert!(matches!(source.refresh(), Ok(snapshot) if snapshot.users.is_empty()));
}
