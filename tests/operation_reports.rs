mod common;

use usrgrp_manager::{
    error::CoreError,
    sys::{
        CheckStatus, CommandSpec, ElevationGrant, FixedReconciler, Gid, GroupName, GroupTarget,
        KnownProgram, OperationCheck, OperationPlan, OperationTarget, PlannedStep,
        ReconciliationStatus, execute_plan,
    },
};

fn target() -> OperationTarget {
    OperationTarget::Group(GroupTarget {
        gid: Gid(1000),
        name: GroupName::new("dev").unwrap(),
        generation: 7,
    })
}

fn plan() -> OperationPlan {
    OperationPlan::new(target())
        .then(PlannedStep::new(
            "create group",
            CommandSpec::new(KnownProgram::GroupAdd).group_name(&GroupName::new("dev").unwrap()),
        ))
        .then(
            PlannedStep::new(
                "add member",
                CommandSpec::new(KnownProgram::GPasswd)
                    .fixed_arg("-a")
                    .unwrap()
                    .user_name(&usrgrp_manager::sys::UserName::new("alice").unwrap())
                    .group_name(&GroupName::new("dev").unwrap()),
            )
            .with_postcondition(OperationCheck::new("alice is a dev member")),
        )
}

#[test]
fn report_records_completed_failed_and_reconciled_steps_without_claiming_success() {
    let runner = common::FakeRunner::failing_at(
        2,
        CoreError::ExitStatus {
            program: "gpasswd",
            code: Some(1),
        },
    );
    let reconciler = FixedReconciler {
        check_status: CheckStatus::Satisfied,
        reconciliation: ReconciliationStatus::Partial {
            detail: "group exists but member was not added".into(),
        },
    };
    let report = execute_plan(&runner, ElevationGrant::Direct, &plan(), &reconciler);

    assert_eq!(report.completed.len(), 1);
    assert_eq!(report.completed[0].id, "create group");
    assert!(matches!(report.failed, Some(ref failed) if failed.id == "add member"));
    assert!(matches!(
        report.reconciliation,
        ReconciliationStatus::Partial { .. }
    ));
    assert!(report.is_partial());
}

#[test]
fn successful_unobservable_step_does_not_skip_later_required_work() {
    let plan = OperationPlan::new(target())
        .then(
            PlannedStep::new(
                "set password",
                CommandSpec::new(KnownProgram::ChAge)
                    .fixed_arg("-d")
                    .unwrap()
                    .fixed_arg("0")
                    .unwrap()
                    .user_name(&usrgrp_manager::sys::UserName::new("alice").unwrap()),
            )
            .with_postcondition(OperationCheck::new("password state is observed")),
        )
        .then(
            PlannedStep::new(
                "expire password",
                CommandSpec::new(KnownProgram::ChAge)
                    .fixed_arg("-d")
                    .unwrap()
                    .fixed_arg("0")
                    .unwrap()
                    .user_name(&usrgrp_manager::sys::UserName::new("alice").unwrap()),
            )
            .with_postcondition(OperationCheck::new("expiry state is observed")),
        );
    let runner = common::FakeRunner::succeeds();
    let report = execute_plan(
        &runner,
        ElevationGrant::Direct,
        &plan,
        &FixedReconciler {
            check_status: CheckStatus::Unavailable,
            reconciliation: ReconciliationStatus::Unavailable {
                detail: "shadow fixture unavailable".into(),
            },
        },
    );

    assert_eq!(runner.recorded().len(), 2);
    assert_eq!(report.completed.len(), 2);
    assert!(report.completed.iter().all(|step| matches!(
        step.verification,
        usrgrp_manager::sys::StepVerification::Unverified { .. }
    )));
    assert!(report.skipped.is_empty());
    assert!(report.is_partial());
}

#[test]
fn failed_step_reports_each_downstream_step_as_skipped() {
    let plan = plan();
    let runner = common::FakeRunner::failing_at(
        1,
        CoreError::ExitStatus {
            program: "groupadd",
            code: Some(1),
        },
    );
    let report = execute_plan(
        &runner,
        ElevationGrant::Direct,
        &plan,
        &FixedReconciler::verified(),
    );

    assert!(matches!(report.failed, Some(ref failed) if failed.id == "create group"));
    assert_eq!(report.skipped.len(), 1);
    assert_eq!(report.skipped[0].id, "add member");
    assert!(matches!(
        report.skipped[0].kind,
        usrgrp_manager::sys::SkipKind::DownstreamFailure { .. }
    ));
}

#[test]
fn preview_is_derived_from_the_same_ordered_plan_as_execution() {
    let plan = plan();
    let preview = plan.redacted_preview();
    let runner = common::FakeRunner::succeeds();
    let report = execute_plan(
        &runner,
        ElevationGrant::Direct,
        &plan,
        &FixedReconciler::verified(),
    );

    assert!(report.is_complete());
    assert_eq!(
        preview,
        runner
            .recorded()
            .into_iter()
            .map(|call| call.preview)
            .collect::<Vec<_>>()
    );
}
