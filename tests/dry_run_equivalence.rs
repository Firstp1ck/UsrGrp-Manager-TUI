mod common;

use usrgrp_manager::sys::{
    CommandSpec, ElevationGrant, FixedReconciler, Gid, GroupName, GroupTarget, KnownProgram,
    OperationPlan, OperationTarget, PlannedStep, execute_plan,
};

#[test]
fn dry_run_preview_matches_the_command_contract_executed_after_confirmation() {
    let plan = OperationPlan::new(OperationTarget::Group(GroupTarget {
        gid: Gid(1000),
        name: GroupName::new("dev").unwrap(),
        generation: 42,
    }))
    .then(PlannedStep::new(
        "rename group",
        CommandSpec::new(KnownProgram::GroupMod)
            .fixed_arg("-n")
            .unwrap()
            .group_name(&GroupName::new("engineering").unwrap())
            .group_name(&GroupName::new("dev").unwrap()),
    ));
    let dry_run = plan.redacted_preview();
    let runner = common::FakeRunner::succeeds();
    let report = execute_plan(
        &runner,
        ElevationGrant::Direct,
        &plan,
        &FixedReconciler::verified(),
    );

    assert!(report.is_complete());
    assert_eq!(
        dry_run,
        runner
            .recorded()
            .iter()
            .map(|call| call.preview.clone())
            .collect::<Vec<_>>()
    );
}
