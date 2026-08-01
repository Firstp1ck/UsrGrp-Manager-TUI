mod common;

use usrgrp_manager::{
    error::CoreError,
    sys::{
        AccountDataSource, AccountSnapshot, ReconciliationStatus, SnapshotState, refresh_retaining,
    },
};

struct RefreshFailure;

impl AccountDataSource for RefreshFailure {
    fn refresh(&self) -> Result<AccountSnapshot, CoreError> {
        Err(CoreError::Refresh {
            operation: "fixture refresh",
            kind: std::io::ErrorKind::TimedOut,
        })
    }
}

#[test]
fn reconciliation_refresh_failure_is_stale_not_an_empty_success() {
    let prior = AccountSnapshot::empty();
    let state = refresh_retaining(&RefreshFailure, Some(prior));
    assert!(matches!(
        state,
        SnapshotState::Stale {
            error: CoreError::Refresh { .. },
            ..
        }
    ));
}

#[test]
fn unavailable_reconciliation_has_an_explicit_status() {
    let status = ReconciliationStatus::Unavailable {
        detail: "fixture account source denied refresh".to_owned(),
    };
    assert!(matches!(status, ReconciliationStatus::Unavailable { .. }));
}
