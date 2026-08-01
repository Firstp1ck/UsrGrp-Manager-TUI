//! Typed operation plans, redacted previews, and honest partial-state reports.

use super::{
    CommandPreview, CommandRunner, CommandSpec, CoreError, CoreResult, ElevationGrant, Gecos, Gid,
    GroupName, PasswordRecord, ShellPath, Uid, UserName,
};

/// A stable user identity captured when an operation is planned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UserTarget {
    pub uid: Uid,
    pub name: UserName,
    pub generation: u64,
}

/// A stable group identity captured when an operation is planned.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GroupTarget {
    pub gid: Gid,
    pub name: GroupName,
    pub generation: u64,
}

/// A target that must still resolve to the planned name and numeric identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum OperationTarget {
    User(UserTarget),
    Group(GroupTarget),
    /// A validated identity which must be absent before creation.
    NewUser(UserName),
    /// A validated identity which must be absent before creation.
    NewGroup(GroupName),
}

/// A closed request vocabulary for every currently supported account mutation.
///
/// A [`Composite`](Self::Composite) is one user-visible operation: it is prepared,
/// confirmed, elevated, executed, and reported as one ordered plan. Password records
/// remain typed and redacted throughout that process.
pub enum OperationRequest {
    AddUserToGroup {
        username: String,
        groupname: String,
    },
    RemoveUserFromGroup {
        username: String,
        groupname: String,
    },
    CreateGroup {
        groupname: String,
    },
    CreateUser {
        username: String,
        create_home: bool,
    },
    DeleteGroup {
        groupname: String,
    },
    RenameGroup {
        old_name: String,
        new_name: String,
    },
    DeleteUser {
        username: String,
        delete_home: bool,
    },
    ChangeUserShell {
        username: String,
        shell: String,
    },
    ChangeUserGecos {
        username: String,
        gecos: String,
    },
    RenameUser {
        old_username: String,
        new_username: String,
    },
    SetUserPassword {
        record: PasswordRecord,
    },
    ExpireUserPassword {
        username: String,
    },
    /// Ordered requests compiled by the trusted adapter into one exact plan.
    Composite {
        requests: Vec<OperationRequest>,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) enum AccountCondition {
    Opaque,
    UserExists {
        name: UserName,
        exists: bool,
    },
    GroupExists {
        name: GroupName,
        exists: bool,
    },
    UserIdentity {
        uid: Uid,
        name: UserName,
    },
    GroupIdentity {
        gid: Gid,
        name: GroupName,
    },
    GroupMember {
        user: UserName,
        group: GroupName,
        present: bool,
    },
    UserShell {
        uid: Uid,
        shell: ShellPath,
    },
    UserGecos {
        uid: Uid,
        gecos: Option<Gecos>,
    },
}

/// A typed named condition checked before or after a step.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationCheck {
    pub description: String,
    condition: AccountCondition,
}

impl OperationCheck {
    /// Construct an opaque, safe, human-readable condition description.
    /// Opaque checks are deliberately reported as unverified rather than false.
    pub fn new(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            condition: AccountCondition::Opaque,
        }
    }

    pub(crate) fn account(description: impl Into<String>, condition: AccountCondition) -> Self {
        Self {
            description: description.into(),
            condition,
        }
    }

    pub(crate) fn condition(&self) -> &AccountCondition {
        &self.condition
    }
}

/// One ordered mutation step and its retry/postcondition contracts.
pub struct PlannedStep {
    pub id: String,
    command: CommandSpec,
    /// A typed observed desired state. If it is already satisfied, this one
    /// step is skipped during retry without suppressing later required steps.
    pub already_satisfied: Option<OperationCheck>,
    pub postcondition: Option<OperationCheck>,
}

impl PlannedStep {
    /// Build a named step. The command remains private so password-bearing
    /// stdin cannot be formatted accidentally.
    pub fn new(id: impl Into<String>, command: CommandSpec) -> Self {
        Self {
            id: id.into(),
            command,
            already_satisfied: None,
            postcondition: None,
        }
    }

    /// Skip this exact step if its typed desired state is already observed.
    pub fn skip_if_satisfied(mut self, check: OperationCheck) -> Self {
        self.already_satisfied = Some(check);
        self
    }

    /// Require a reconciliation-visible condition after the command succeeds.
    pub fn with_postcondition(mut self, postcondition: OperationCheck) -> Self {
        self.postcondition = Some(postcondition);
        self
    }

    /// Redacted command preview for reviews and dry-runs.
    pub fn redacted_preview(&self) -> CommandPreview {
        self.command.redacted_preview()
    }
}

/// A single validated operation used for both preview and execution.
pub struct OperationPlan {
    /// Primary target shown in previews and reports.
    pub target: OperationTarget,
    /// Every identity that must remain stable until execution.
    pub bound_targets: Vec<OperationTarget>,
    /// Legacy plan-wide retry checks. New bridge plans use per-step checks.
    pub preconditions: Vec<OperationCheck>,
    pub steps: Vec<PlannedStep>,
}

impl OperationPlan {
    /// Begin an operation against a stable target.
    pub fn new(target: OperationTarget) -> Self {
        Self {
            bound_targets: vec![target.clone()],
            target,
            preconditions: Vec::new(),
            steps: Vec::new(),
        }
    }

    /// Bind another stable identity required by this operation.
    pub fn bind(mut self, target: OperationTarget) -> Self {
        if !self.bound_targets.contains(&target) {
            self.bound_targets.push(target);
        }
        self
    }

    /// Add a legacy plan-wide condition. It remains for compatibility with
    /// deterministic callers; production plans attach conditions per step.
    pub fn require(mut self, check: OperationCheck) -> Self {
        self.preconditions.push(check);
        self
    }

    /// Add an ordered command step.
    pub fn then(mut self, step: PlannedStep) -> Self {
        self.steps.push(step);
        self
    }

    /// Return a redacted preview from the exact plan that execution consumes.
    pub fn redacted_preview(&self) -> Vec<CommandPreview> {
        self.steps
            .iter()
            .map(PlannedStep::redacted_preview)
            .collect()
    }

    pub(crate) fn append(&mut self, mut next: OperationPlan) {
        for target in next.bound_targets.drain(..) {
            if !self.bound_targets.contains(&target) {
                self.bound_targets.push(target);
            }
        }
        self.preconditions.append(&mut next.preconditions);
        self.steps.append(&mut next.steps);
    }
}

/// Result of evaluating a precondition or postcondition.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CheckStatus {
    Satisfied,
    Unsatisfied,
    Unavailable,
}

/// Observed state after a complete or partial operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ReconciliationStatus {
    Verified,
    Partial { detail: String },
    Unavailable { detail: String },
}

/// Injectable, deterministic postcondition/reconciliation seam.
pub trait OperationReconciler: Send + Sync {
    /// Evaluate a condition against current observed state.
    fn check(&self, target: &OperationTarget, check: &OperationCheck) -> CheckStatus;

    /// Re-read affected state after a complete or partial run.
    fn reconcile(&self, plan: &OperationPlan) -> ReconciliationStatus;
}

/// Why an ordered command step was not run.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SkipKind {
    /// The step's typed desired state was observed before command execution.
    AlreadySatisfied { check: String },
    /// A preceding command or observable postcondition failed.
    DownstreamFailure { failed_step: String },
    /// A legacy plan-wide condition made every step unnecessary.
    PlanAlreadySatisfied { check: String },
}

/// A skipped command with concrete retry/downstream evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SkippedStep {
    pub id: String,
    pub reason: String,
    pub kind: SkipKind,
}

/// Whether a completed command's required postcondition was observed.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum StepVerification {
    Verified,
    Unverified { check: String },
}

/// A completed command step with per-step verification evidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CompletedStep {
    pub id: String,
    pub preview: CommandPreview,
    pub verification: StepVerification,
}

/// A failed command or observable postcondition, without raw process output.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FailedStep {
    pub id: String,
    pub error: CoreError,
}

/// Honest outcome of one user-visible plan.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct OperationReport {
    pub completed: Vec<CompletedStep>,
    pub skipped: Vec<SkippedStep>,
    pub compensated: Vec<CompletedStep>,
    pub failed: Option<FailedStep>,
    pub reconciliation: ReconciliationStatus,
}

impl OperationReport {
    /// Whether every step was completed or safely skipped and all required
    /// postconditions were observed.
    pub fn is_complete(&self) -> bool {
        self.failed.is_none()
            && self
                .completed
                .iter()
                .all(|step| matches!(step.verification, StepVerification::Verified))
            && matches!(self.reconciliation, ReconciliationStatus::Verified)
    }

    /// Whether the report must be surfaced as a partial/unverified outcome.
    pub fn is_partial(&self) -> bool {
        !self.is_complete()
    }
}

/// Whether a plan can be completed without acquiring elevation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum PlanPreflight {
    NoCommandsRequired,
    NeedsExecution,
    CannotVerify { check: String },
}

/// Evaluate typed already-satisfied checks before elevation. This permits a
/// retry which needs no command to avoid authentication and preserves a
/// fail-closed outcome when a retry state cannot be observed.
pub fn preflight_plan(plan: &OperationPlan, reconciler: &dyn OperationReconciler) -> PlanPreflight {
    for check in &plan.preconditions {
        match reconciler.check(&plan.target, check) {
            CheckStatus::Satisfied => return PlanPreflight::NoCommandsRequired,
            CheckStatus::Unsatisfied => {}
            CheckStatus::Unavailable => {
                return PlanPreflight::CannotVerify {
                    check: check.description.clone(),
                };
            }
        }
    }
    let mut requires_command = false;
    for step in &plan.steps {
        let Some(check) = &step.already_satisfied else {
            requires_command = true;
            continue;
        };
        match reconciler.check(&plan.target, check) {
            CheckStatus::Satisfied => {}
            CheckStatus::Unsatisfied => requires_command = true,
            CheckStatus::Unavailable => {
                return PlanPreflight::CannotVerify {
                    check: check.description.clone(),
                };
            }
        }
    }
    if requires_command {
        PlanPreflight::NeedsExecution
    } else {
        PlanPreflight::NoCommandsRequired
    }
}

/// Execute the supplied plan exactly as it was previewed.
///
/// A successful command with an unavailable postcondition is recorded as
/// unverified and does not suppress later required work. Observable failures
/// stop later commands and explicitly record each downstream skipped step.
pub fn execute_plan(
    runner: &dyn CommandRunner,
    grant: ElevationGrant,
    plan: &OperationPlan,
    reconciler: &dyn OperationReconciler,
) -> OperationReport {
    let mut report = OperationReport {
        completed: Vec::new(),
        skipped: Vec::new(),
        compensated: Vec::new(),
        failed: None,
        reconciliation: ReconciliationStatus::Unavailable {
            detail: "operation did not reach reconciliation".to_owned(),
        },
    };

    for check in &plan.preconditions {
        match reconciler.check(&plan.target, check) {
            CheckStatus::Satisfied => {
                for step in &plan.steps {
                    report.skipped.push(SkippedStep {
                        id: step.id.clone(),
                        reason: check.description.clone(),
                        kind: SkipKind::PlanAlreadySatisfied {
                            check: check.description.clone(),
                        },
                    });
                }
                report.reconciliation = reconciler.reconcile(plan);
                return report;
            }
            CheckStatus::Unsatisfied => {}
            CheckStatus::Unavailable => {
                fail_and_skip(
                    &mut report,
                    plan,
                    0,
                    "preconditions",
                    CoreError::PartialCompletion {
                        step: check.description.clone(),
                    },
                );
                report.reconciliation = reconciler.reconcile(plan);
                return report;
            }
        }
    }

    for (index, step) in plan.steps.iter().enumerate() {
        if let Some(check) = &step.already_satisfied {
            match reconciler.check(&plan.target, check) {
                CheckStatus::Satisfied => {
                    report.skipped.push(SkippedStep {
                        id: step.id.clone(),
                        reason: check.description.clone(),
                        kind: SkipKind::AlreadySatisfied {
                            check: check.description.clone(),
                        },
                    });
                    continue;
                }
                CheckStatus::Unsatisfied => {}
                CheckStatus::Unavailable => {
                    fail_and_skip(
                        &mut report,
                        plan,
                        index,
                        &step.id,
                        CoreError::PartialCompletion {
                            step: check.description.clone(),
                        },
                    );
                    report.reconciliation = reconciler.reconcile(plan);
                    return report;
                }
            }
        }

        let preview = step.redacted_preview();
        match runner.run(grant, &step.command) {
            Ok(_) => report.completed.push(CompletedStep {
                id: step.id.clone(),
                preview,
                verification: StepVerification::Verified,
            }),
            Err(error) => {
                fail_and_skip(&mut report, plan, index, &step.id, error);
                report.reconciliation = reconciler.reconcile(plan);
                return report;
            }
        }
        if let Some(check) = &step.postcondition {
            match reconciler.check(&plan.target, check) {
                CheckStatus::Satisfied => {}
                CheckStatus::Unavailable => {
                    if let Some(completed) = report.completed.last_mut() {
                        completed.verification = StepVerification::Unverified {
                            check: check.description.clone(),
                        };
                    }
                }
                CheckStatus::Unsatisfied => {
                    fail_and_skip(
                        &mut report,
                        plan,
                        index,
                        &step.id,
                        CoreError::PostconditionFailed {
                            step: check.description.clone(),
                        },
                    );
                    report.reconciliation = reconciler.reconcile(plan);
                    return report;
                }
            }
        }
    }

    report.reconciliation = reconciler.reconcile(plan);
    report
}

fn fail_and_skip(
    report: &mut OperationReport,
    plan: &OperationPlan,
    failure_index: usize,
    failed_step: &str,
    error: CoreError,
) {
    report.failed = Some(FailedStep {
        id: failed_step.to_owned(),
        error,
    });
    for step in plan.steps.iter().skip(failure_index + 1) {
        report.skipped.push(SkippedStep {
            id: step.id.clone(),
            reason: format!("not run after {failed_step} failed"),
            kind: SkipKind::DownstreamFailure {
                failed_step: failed_step.to_owned(),
            },
        });
    }
}

/// A deterministic reconciler useful for tests and dry-run callers.
#[derive(Clone, Debug)]
pub struct FixedReconciler {
    pub check_status: CheckStatus,
    pub reconciliation: ReconciliationStatus,
}

impl FixedReconciler {
    /// Always observe verified state and satisfied postconditions.
    pub fn verified() -> Self {
        Self {
            check_status: CheckStatus::Satisfied,
            reconciliation: ReconciliationStatus::Verified,
        }
    }
}

impl OperationReconciler for FixedReconciler {
    fn check(&self, _: &OperationTarget, _: &OperationCheck) -> CheckStatus {
        self.check_status.clone()
    }

    fn reconcile(&self, _: &OperationPlan) -> ReconciliationStatus {
        self.reconciliation.clone()
    }
}

/// Convert a partial report into a typed result for callers that only accept
/// success/failure while preserving the report for user-facing presentation.
pub fn require_complete(report: OperationReport) -> CoreResult<OperationReport> {
    if report.is_complete() {
        Ok(report)
    } else if let Some(failed) = &report.failed {
        Err(failed.error.clone())
    } else if let Some(unverified) =
        report
            .completed
            .iter()
            .find_map(|step| match &step.verification {
                StepVerification::Unverified { check } => Some(check),
                StepVerification::Verified => None,
            })
    {
        Err(CoreError::PartialCompletion {
            step: unverified.clone(),
        })
    } else {
        Err(CoreError::PartialCompletion {
            step: "reconciliation".to_owned(),
        })
    }
}
