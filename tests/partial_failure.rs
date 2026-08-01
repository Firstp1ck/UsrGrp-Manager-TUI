mod common;

use usrgrp_manager::{
    error::CoreError,
    sys::{
        CheckStatus, CommandSpec, ElevationGrant, FixedReconciler, Gid, GroupName, GroupTarget,
        KnownProgram, OperationPlan, OperationTarget, PlannedStep, ReconciliationStatus,
        execute_plan, require_complete,
    },
};

#[test]
fn failed_first_step_has_no_false_completed_or_compensated_work() {
    let plan = OperationPlan::new(OperationTarget::Group(GroupTarget {
        gid: Gid(1000),
        name: GroupName::new("dev").unwrap(),
        generation: 1,
    }))
    .then(PlannedStep::new(
        "create group",
        CommandSpec::new(KnownProgram::GroupAdd).group_name(&GroupName::new("dev").unwrap()),
    ));
    let runner = common::FakeRunner::failing_at(
        1,
        CoreError::ExitStatus {
            program: "groupadd",
            code: Some(9),
        },
    );
    let report = execute_plan(
        &runner,
        ElevationGrant::Direct,
        &plan,
        &FixedReconciler {
            check_status: CheckStatus::Unsatisfied,
            reconciliation: ReconciliationStatus::Unavailable {
                detail: "fixture refresh unavailable".into(),
            },
        },
    );

    assert!(report.completed.is_empty());
    assert!(report.compensated.is_empty());
    assert!(report.failed.is_some());
    assert!(require_complete(report).is_err());
}

#[test]
fn already_satisfied_precondition_skips_execution_for_safe_retry() {
    let plan = OperationPlan::new(OperationTarget::Group(GroupTarget {
        gid: Gid(1000),
        name: GroupName::new("dev").unwrap(),
        generation: 1,
    }))
    .require(usrgrp_manager::sys::OperationCheck::new(
        "group already exists",
    ))
    .then(PlannedStep::new(
        "create group",
        CommandSpec::new(KnownProgram::GroupAdd).group_name(&GroupName::new("dev").unwrap()),
    ));
    let runner = common::FakeRunner::succeeds();
    let report = execute_plan(
        &runner,
        ElevationGrant::Direct,
        &plan,
        &FixedReconciler {
            check_status: CheckStatus::Satisfied,
            reconciliation: ReconciliationStatus::Verified,
        },
    );

    assert!(report.failed.is_none());
    assert_eq!(report.skipped.len(), 1);
    assert!(runner.recorded().is_empty());
    assert!(report.is_complete());
}
