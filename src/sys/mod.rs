#![allow(dead_code)] // src/main.rs currently compiles this public module tree privately.

//! Trusted Linux-local account boundary.
//!
//! This module owns the fail-closed data, identity, validation, command, and
//! operation seams.  The legacy [`SystemAdapter`] remains as a narrow facade so
//! the existing application can migrate to the public contracts incrementally.

mod command;
mod data_source;
mod identity;
mod operations;
mod validation;

#[allow(unused_imports)]
pub use command::{
    CommandLimits, CommandPreview, CommandResult, CommandRunner, CommandSpec, ElevationGrant,
    KnownProgram, LocalCommandRunner,
};
#[allow(unused_imports)]
pub use data_source::{
    AccountDataSource, AccountGroup, AccountPaths, AccountSnapshot, AccountUser, Gid,
    LocalFileAccountDataSource, ParseDiagnostic, ParsedRecords, SnapshotState, Uid,
    parse_group_records, parse_passwd_records, parse_shell_records, refresh_retaining,
};
#[allow(unused_imports)]
pub use identity::{FixedIdentityProvider, IdentityProvider, SystemIdentityProvider};
#[allow(unused_imports)]
pub use operations::{
    CheckStatus, CompletedStep, FailedStep, FixedReconciler, GroupTarget, OperationCheck,
    OperationPlan, OperationReconciler, OperationReport, OperationRequest, OperationTarget,
    PlanPreflight, PlannedStep, ReconciliationStatus, SkipKind, SkippedStep, StepVerification,
    UserTarget, execute_plan, preflight_plan, require_complete,
};
#[allow(unused_imports)]
pub use validation::{Gecos, GroupName, PasswordRecord, SecretString, ShellPath, UserName};

use crate::error::{CoreError, CoreResult, Result};
use operations::AccountCondition;
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex},
};

/// Compatibility representation of a local system user.
///
/// New trusted callers should prefer [`AccountUser`], whose fields are
/// validated types.  This structure is retained for the current application
/// integration surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemUser {
    pub uid: u32,
    pub name: String,
    pub primary_gid: u32,
    pub full_name: Option<String>,
    pub home_dir: String,
    pub shell: String,
}

impl From<AccountUser> for SystemUser {
    fn from(user: AccountUser) -> Self {
        Self {
            uid: user.uid.0,
            name: user.name.as_str().to_owned(),
            primary_gid: user.primary_gid.0,
            full_name: user.full_name.map(|name| name.as_str().to_owned()),
            home_dir: user.home_dir.to_string_lossy().into_owned(),
            shell: user.shell.display_label().to_owned(),
        }
    }
}

/// Compatibility representation of a local system group.
///
/// New trusted callers should prefer [`AccountGroup`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SystemGroup {
    pub gid: u32,
    pub name: String,
    pub members: Vec<String>,
}

impl From<AccountGroup> for SystemGroup {
    fn from(group: AccountGroup) -> Self {
        Self {
            gid: group.gid.0,
            name: group.name.as_str().to_owned(),
            members: group
                .members
                .into_iter()
                .map(|member| member.as_str().to_owned())
                .collect(),
        }
    }
}

/// Explicit injected policy for identities which the local manager must not
/// modify. Root is enforced independently and can never be allowlisted.
///
/// The default is fail-closed for service IDs below 1000 and for membership in
/// common elevation groups. Deployments that intentionally manage one of those
/// identities must provide its numeric ID/group in the relevant allowlist.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProtectedIdentityPolicy {
    minimum_mutable_uid: Uid,
    minimum_mutable_gid: Gid,
    allowed_service_users: BTreeSet<Uid>,
    allowed_service_groups: BTreeSet<Gid>,
    elevation_groups: BTreeSet<GroupName>,
    allowed_elevation_membership_groups: BTreeSet<GroupName>,
}

impl ProtectedIdentityPolicy {
    /// A Linux-local fail-closed default. It preserves normal local account
    /// management while requiring explicit policy for service/elevation paths.
    pub fn fail_closed() -> Self {
        Self {
            minimum_mutable_uid: Uid(1000),
            minimum_mutable_gid: Gid(1000),
            allowed_service_users: BTreeSet::new(),
            allowed_service_groups: BTreeSet::new(),
            elevation_groups: [
                GroupName::new("sudo").expect("static valid group"),
                GroupName::new("wheel").expect("static valid group"),
            ]
            .into_iter()
            .collect(),
            allowed_elevation_membership_groups: BTreeSet::new(),
        }
    }

    /// Allow one explicitly reviewed service user to be mutated.
    pub fn allow_service_user(mut self, uid: Uid) -> Self {
        self.allowed_service_users.insert(uid);
        self
    }

    /// Allow one explicitly reviewed service group to be mutated.
    pub fn allow_service_group(mut self, gid: Gid) -> Self {
        self.allowed_service_groups.insert(gid);
        self
    }

    /// Allow intentional membership changes to a configured elevation group.
    pub fn allow_elevation_membership_group(mut self, group: GroupName) -> Self {
        self.allowed_elevation_membership_groups.insert(group);
        self
    }

    fn check_user(&self, target: &UserTarget) -> CoreResult<()> {
        if target.uid == Uid(0) || target.name.as_str() == "root" {
            return Err(target_error("root user is immutable"));
        }
        if target.uid < self.minimum_mutable_uid
            && !self.allowed_service_users.contains(&target.uid)
        {
            return Err(target_error("service user is protected by policy"));
        }
        Ok(())
    }

    fn check_group(&self, target: &GroupTarget) -> CoreResult<()> {
        if target.gid == Gid(0) || target.name.as_str() == "root" {
            return Err(target_error("root group is immutable"));
        }
        if target.gid < self.minimum_mutable_gid
            && !self.allowed_service_groups.contains(&target.gid)
        {
            return Err(target_error("service group is protected by policy"));
        }
        Ok(())
    }

    fn check_elevation_membership(&self, group: &GroupName) -> CoreResult<()> {
        if self.elevation_groups.contains(group)
            && !self.allowed_elevation_membership_groups.contains(group)
        {
            return Err(target_error(
                "elevation-group membership requires explicit policy allowlist",
            ));
        }
        Ok(())
    }
}

impl Default for ProtectedIdentityPolicy {
    fn default() -> Self {
        Self::fail_closed()
    }
}

/// Adapter-owned trusted boundary. Application callers prepare and execute
/// reports; they do not receive runners, grants, or direct mutation helpers.
pub struct SystemAdapter {
    source: Arc<dyn AccountDataSource>,
    runner: Arc<dyn CommandRunner>,
    identity: Arc<dyn IdentityProvider>,
    policy: ProtectedIdentityPolicy,
    pending_secret: Mutex<Option<SecretString>>,
}

impl SystemAdapter {
    /// Construct the supported Linux-local production adapter without credentials.
    pub fn new() -> Self {
        Self::from_components(
            Arc::new(LocalFileAccountDataSource::new()),
            Arc::new(LocalCommandRunner),
            Arc::new(SystemIdentityProvider),
        )
    }

    /// Construct an adapter with fake or production dependencies and the
    /// fail-closed default protected-identity policy.
    pub fn from_components(
        source: Arc<dyn AccountDataSource>,
        runner: Arc<dyn CommandRunner>,
        identity: Arc<dyn IdentityProvider>,
    ) -> Self {
        Self::from_components_with_policy(
            source,
            runner,
            identity,
            ProtectedIdentityPolicy::default(),
        )
    }

    /// Construct an adapter with an explicitly injected protected-identity
    /// policy. This is the sole policy source for service/elevation membership
    /// mutations; root remains protected independently.
    pub fn from_components_with_policy(
        source: Arc<dyn AccountDataSource>,
        runner: Arc<dyn CommandRunner>,
        identity: Arc<dyn IdentityProvider>,
        policy: ProtectedIdentityPolicy,
    ) -> Self {
        Self {
            source,
            runner,
            identity,
            policy,
            pending_secret: Mutex::new(None),
        }
    }

    /// Supply a one-shot elevation secret for a single planned operation.
    pub fn set_elevation_secret(&self, secret: SecretString) {
        *self
            .pending_secret
            .lock()
            .expect("pending secret mutex poisoned") = Some(secret);
    }

    /// Compile a closed mutation request into one redacted, stable operation plan.
    ///
    /// This is the only command-selection and root-protection path intended for
    /// application integration. It refreshes the injected source once, binds
    /// every existing user/group by numeric ID, name, and snapshot generation,
    /// and rejects malformed or root targets before a command can be selected.
    pub fn prepare_operation(&self, request: OperationRequest) -> CoreResult<OperationPlan> {
        if let OperationRequest::Composite { requests } = request {
            return self.prepare_composite_operation(requests);
        }
        let snapshot = self.snapshot()?;
        let generation = snapshot_generation(&snapshot);
        let plan = match request {
            OperationRequest::Composite { .. } => unreachable!("composite handled before refresh"),
            OperationRequest::AddUserToGroup {
                username,
                groupname,
            } => {
                let username = UserName::new(username)?;
                let groupname = GroupName::new(groupname)?;
                let user = mutable_user_target(&snapshot, &username, generation)?;
                let group = mutable_group_target(&snapshot, &groupname, generation)?;
                let desired = OperationCheck::account(
                    "user is a group member",
                    AccountCondition::GroupMember {
                        user: username.clone(),
                        group: groupname.clone(),
                        present: true,
                    },
                );
                Ok(OperationPlan::new(OperationTarget::User(user))
                    .bind(OperationTarget::Group(group))
                    .then(
                        PlannedStep::new(
                            "add user to group",
                            CommandSpec::new(KnownProgram::GPasswd)
                                .fixed_arg("-a")?
                                .user_name(&username)
                                .group_name(&groupname),
                        )
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                    ))
            }
            OperationRequest::RemoveUserFromGroup {
                username,
                groupname,
            } => {
                let username = UserName::new(username)?;
                let groupname = GroupName::new(groupname)?;
                let user = mutable_user_target(&snapshot, &username, generation)?;
                let group = mutable_group_target(&snapshot, &groupname, generation)?;
                let desired = OperationCheck::account(
                    "user is not a group member",
                    AccountCondition::GroupMember {
                        user: username.clone(),
                        group: groupname.clone(),
                        present: false,
                    },
                );
                Ok(OperationPlan::new(OperationTarget::User(user))
                    .bind(OperationTarget::Group(group))
                    .then(
                        PlannedStep::new(
                            "remove user from group",
                            CommandSpec::new(KnownProgram::GPasswd)
                                .fixed_arg("-d")?
                                .user_name(&username)
                                .group_name(&groupname),
                        )
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                    ))
            }
            OperationRequest::CreateGroup { groupname } => {
                let groupname = GroupName::new(groupname)?;
                ensure_new_name_is_not_root(groupname.as_str(), "group")?;
                let target = match snapshot.groups.iter().find(|group| group.name == groupname) {
                    Some(_) => OperationTarget::Group(mutable_group_target(
                        &snapshot, &groupname, generation,
                    )?),
                    None => OperationTarget::NewGroup(groupname.clone()),
                };
                let desired = OperationCheck::account(
                    "group exists",
                    AccountCondition::GroupExists {
                        name: groupname.clone(),
                        exists: true,
                    },
                );
                Ok(OperationPlan::new(target).then(
                    PlannedStep::new(
                        "create group",
                        CommandSpec::new(KnownProgram::GroupAdd).group_name(&groupname),
                    )
                    .skip_if_satisfied(desired.clone())
                    .with_postcondition(desired),
                ))
            }
            OperationRequest::CreateUser {
                username,
                create_home,
            } => {
                let username = UserName::new(username)?;
                ensure_new_name_is_not_root(username.as_str(), "user")?;
                let target = match snapshot.users.iter().find(|user| user.name == username) {
                    Some(_) => OperationTarget::User(mutable_user_target(
                        &snapshot, &username, generation,
                    )?),
                    None => OperationTarget::NewUser(username.clone()),
                };
                let mut command = CommandSpec::new(KnownProgram::UserAdd);
                if create_home {
                    command = command.fixed_arg("-m")?;
                }
                let desired = OperationCheck::account(
                    "user exists",
                    AccountCondition::UserExists {
                        name: username.clone(),
                        exists: true,
                    },
                );
                Ok(OperationPlan::new(target).then(
                    PlannedStep::new("create user", command.user_name(&username))
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                ))
            }
            OperationRequest::DeleteGroup { groupname } => {
                let groupname = GroupName::new(groupname)?;
                let target = match snapshot.groups.iter().find(|group| group.name == groupname) {
                    Some(_) => OperationTarget::Group(mutable_group_target(
                        &snapshot, &groupname, generation,
                    )?),
                    None => OperationTarget::NewGroup(groupname.clone()),
                };
                let desired = OperationCheck::account(
                    "group is absent",
                    AccountCondition::GroupExists {
                        name: groupname.clone(),
                        exists: false,
                    },
                );
                Ok(OperationPlan::new(target).then(
                    PlannedStep::new(
                        "delete group",
                        CommandSpec::new(KnownProgram::GroupDel).group_name(&groupname),
                    )
                    .skip_if_satisfied(desired.clone())
                    .with_postcondition(desired),
                ))
            }
            OperationRequest::RenameGroup { old_name, new_name } => {
                let old_name = GroupName::new(old_name)?;
                let new_name = GroupName::new(new_name)?;
                ensure_new_name_is_not_root(new_name.as_str(), "group")?;
                let old_exists = snapshot.groups.iter().any(|group| group.name == old_name);
                let group = match snapshot.groups.iter().find(|group| group.name == new_name) {
                    Some(_) if old_exists => {
                        return Err(target_error("replacement group name already exists"));
                    }
                    Some(_) => mutable_group_target(&snapshot, &new_name, generation)?,
                    None => mutable_group_target(&snapshot, &old_name, generation)?,
                };
                let desired = OperationCheck::account(
                    "group identity has new name",
                    AccountCondition::GroupIdentity {
                        gid: group.gid,
                        name: new_name.clone(),
                    },
                );
                Ok(
                    OperationPlan::new(OperationTarget::Group(group.clone())).then(
                        PlannedStep::new(
                            "rename group",
                            CommandSpec::new(KnownProgram::GroupMod)
                                .fixed_arg("-n")?
                                .group_name(&new_name)
                                .group_name(&old_name),
                        )
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                    ),
                )
            }
            OperationRequest::DeleteUser {
                username,
                delete_home,
            } => {
                let username = UserName::new(username)?;
                let target = match snapshot.users.iter().find(|user| user.name == username) {
                    Some(_) => OperationTarget::User(mutable_user_target(
                        &snapshot, &username, generation,
                    )?),
                    None => OperationTarget::NewUser(username.clone()),
                };
                let mut command = CommandSpec::new(KnownProgram::UserDel);
                if delete_home {
                    command = command.fixed_arg("-r")?;
                }
                let desired = OperationCheck::account(
                    "user is absent",
                    AccountCondition::UserExists {
                        name: username.clone(),
                        exists: false,
                    },
                );
                Ok(OperationPlan::new(target).then(
                    PlannedStep::new("delete user", command.user_name(&username))
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                ))
            }
            OperationRequest::ChangeUserShell { username, shell } => {
                let username = UserName::new(username)?;
                let shell = ShellPath::new(shell)?;
                let user = mutable_user_target(&snapshot, &username, generation)?;
                let desired = OperationCheck::account(
                    "user shell matches request",
                    AccountCondition::UserShell {
                        uid: user.uid,
                        shell: shell.clone(),
                    },
                );
                Ok(
                    OperationPlan::new(OperationTarget::User(user.clone())).then(
                        PlannedStep::new(
                            "change user shell",
                            CommandSpec::new(KnownProgram::UserMod)
                                .fixed_arg("-s")?
                                .shell_path(&shell)
                                .user_name(&username),
                        )
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                    ),
                )
            }
            OperationRequest::ChangeUserGecos { username, gecos } => {
                let username = UserName::new(username)?;
                let gecos = Gecos::new(gecos)?;
                let user = mutable_user_target(&snapshot, &username, generation)?;
                let desired = OperationCheck::account(
                    "user GECOS matches request",
                    AccountCondition::UserGecos {
                        uid: user.uid,
                        gecos: Some(gecos.clone()),
                    },
                );
                Ok(
                    OperationPlan::new(OperationTarget::User(user.clone())).then(
                        PlannedStep::new(
                            "change user GECOS",
                            CommandSpec::new(KnownProgram::UserMod)
                                .fixed_arg("-c")?
                                .gecos(&gecos)
                                .user_name(&username),
                        )
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                    ),
                )
            }
            OperationRequest::RenameUser {
                old_username,
                new_username,
            } => {
                let old_username = UserName::new(old_username)?;
                let new_username = UserName::new(new_username)?;
                ensure_new_name_is_not_root(new_username.as_str(), "user")?;
                let old_exists = snapshot.users.iter().any(|user| user.name == old_username);
                let user = match snapshot.users.iter().find(|user| user.name == new_username) {
                    Some(_) if old_exists => {
                        return Err(target_error("replacement user name already exists"));
                    }
                    Some(_) => mutable_user_target(&snapshot, &new_username, generation)?,
                    None => mutable_user_target(&snapshot, &old_username, generation)?,
                };
                let desired = OperationCheck::account(
                    "user identity has new name",
                    AccountCondition::UserIdentity {
                        uid: user.uid,
                        name: new_username.clone(),
                    },
                );
                Ok(
                    OperationPlan::new(OperationTarget::User(user.clone())).then(
                        PlannedStep::new(
                            "rename user",
                            CommandSpec::new(KnownProgram::UserMod)
                                .fixed_arg("-l")?
                                .user_name(&new_username)
                                .user_name(&old_username),
                        )
                        .skip_if_satisfied(desired.clone())
                        .with_postcondition(desired),
                    ),
                )
            }
            OperationRequest::SetUserPassword { record } => {
                let username = record.username().clone();
                let user = mutable_user_target(&snapshot, &username, generation)?;
                let command = CommandSpec::new(KnownProgram::ChPasswd).password_record(record)?;
                // This local-file source intentionally has no shadow reader. A
                // successful child is therefore reported as partial/unobserved,
                // never falsely as a verified password change.
                Ok(OperationPlan::new(OperationTarget::User(user)).then(
                    PlannedStep::new("set user password", command)
                        .with_postcondition(OperationCheck::new("password state is observed")),
                ))
            }
            OperationRequest::ExpireUserPassword { username } => {
                let username = UserName::new(username)?;
                let user = mutable_user_target(&snapshot, &username, generation)?;
                Ok(OperationPlan::new(OperationTarget::User(user)).then(
                    PlannedStep::new(
                        "expire user password",
                        CommandSpec::new(KnownProgram::ChAge)
                            .fixed_arg("-d")?
                            .fixed_arg("0")?
                            .user_name(&username),
                    )
                    .with_postcondition(OperationCheck::new("password expiry is observed")),
                ))
            }
        }?;
        self.validate_policy(&plan)?;
        Ok(plan)
    }

    fn prepare_composite_operation(
        &self,
        requests: Vec<OperationRequest>,
    ) -> CoreResult<OperationPlan> {
        if requests.is_empty() {
            return Err(target_error(
                "composite operation must contain at least one step",
            ));
        }
        let initial_snapshot = self.snapshot()?;
        let generation = snapshot_generation(&initial_snapshot);
        // Compile every child request against this one captured snapshot. The
        // temporary source prevents a composite preview from performing a host
        // refresh per child while retaining the ordinary typed compiler.
        let compiler = SystemAdapter::from_components_with_policy(
            Arc::new(SnapshotSource(initial_snapshot.clone())),
            self.runner.clone(),
            self.identity.clone(),
            self.policy.clone(),
        );
        let mut planned_new_users = BTreeSet::new();
        let mut aggregate: Option<OperationPlan> = None;

        for request in requests {
            let next = match request {
                OperationRequest::Composite { .. } => {
                    return Err(target_error(
                        "nested composite operations are not supported",
                    ));
                }
                OperationRequest::CreateUser {
                    username,
                    create_home,
                } => {
                    let validated = UserName::new(&username)?;
                    planned_new_users.insert(validated);
                    compiler.prepare_operation(OperationRequest::CreateUser {
                        username,
                        create_home,
                    })?
                }
                OperationRequest::SetUserPassword { record }
                    if planned_new_users.contains(record.username())
                        && !initial_snapshot
                            .users
                            .iter()
                            .any(|user| user.name == *record.username()) =>
                {
                    self.prepare_password_for_new_user(record)
                }
                OperationRequest::ExpireUserPassword { username }
                    if planned_new_users.contains(&UserName::new(&username)?)
                        && !initial_snapshot
                            .users
                            .iter()
                            .any(|user| user.name.as_str() == username) =>
                {
                    self.prepare_expiry_for_new_user(username)
                }
                OperationRequest::AddUserToGroup {
                    username,
                    groupname,
                } if planned_new_users.contains(&UserName::new(&username)?)
                    && !initial_snapshot
                        .users
                        .iter()
                        .any(|user| user.name.as_str() == username) =>
                {
                    self.prepare_membership_for_new_user(
                        username,
                        groupname,
                        true,
                        &initial_snapshot,
                        generation,
                    )?
                }
                OperationRequest::RemoveUserFromGroup {
                    username,
                    groupname,
                } if planned_new_users.contains(&UserName::new(&username)?)
                    && !initial_snapshot
                        .users
                        .iter()
                        .any(|user| user.name.as_str() == username) =>
                {
                    self.prepare_membership_for_new_user(
                        username,
                        groupname,
                        false,
                        &initial_snapshot,
                        generation,
                    )?
                }
                request => compiler.prepare_operation(request)?,
            };
            match &mut aggregate {
                Some(plan) => plan.append(next),
                None => aggregate = Some(next),
            }
        }
        let plan = aggregate.expect("non-empty request vector checked above");
        self.validate_policy(&plan)?;
        Ok(plan)
    }

    fn prepare_password_for_new_user(&self, record: PasswordRecord) -> OperationPlan {
        let username = record.username().clone();
        let command = CommandSpec::new(KnownProgram::ChPasswd)
            .password_record(record)
            .expect("password record contract is fixed");
        OperationPlan::new(OperationTarget::NewUser(username)).then(
            PlannedStep::new("set user password", command)
                .with_postcondition(OperationCheck::new("password state is observed")),
        )
    }

    fn prepare_expiry_for_new_user(&self, username: String) -> OperationPlan {
        let username = UserName::new(username).expect("validated while composing request");
        OperationPlan::new(OperationTarget::NewUser(username.clone())).then(
            PlannedStep::new(
                "expire user password",
                CommandSpec::new(KnownProgram::ChAge)
                    .fixed_arg("-d")
                    .expect("fixed contract")
                    .fixed_arg("0")
                    .expect("fixed contract")
                    .user_name(&username),
            )
            .with_postcondition(OperationCheck::new("password expiry is observed")),
        )
    }

    fn prepare_membership_for_new_user(
        &self,
        username: String,
        groupname: String,
        present: bool,
        snapshot: &AccountSnapshot,
        generation: u64,
    ) -> CoreResult<OperationPlan> {
        let username = UserName::new(username)?;
        let groupname = GroupName::new(groupname)?;
        let group = mutable_group_target(snapshot, &groupname, generation)?;
        let desired = OperationCheck::account(
            if present {
                "user is a group member"
            } else {
                "user is not a group member"
            },
            AccountCondition::GroupMember {
                user: username.clone(),
                group: groupname.clone(),
                present,
            },
        );
        let flag = if present { "-a" } else { "-d" };
        Ok(
            OperationPlan::new(OperationTarget::NewUser(username.clone()))
                .bind(OperationTarget::Group(group))
                .then(
                    PlannedStep::new(
                        if present {
                            "add user to group"
                        } else {
                            "remove user from group"
                        },
                        CommandSpec::new(KnownProgram::GPasswd)
                            .fixed_arg(flag)?
                            .user_name(&username)
                            .group_name(&groupname),
                    )
                    .skip_if_satisfied(desired.clone())
                    .with_postcondition(desired),
                ),
        )
    }

    fn validate_policy(&self, plan: &OperationPlan) -> CoreResult<()> {
        let membership_change = plan.steps.iter().any(|step| {
            matches!(
                step.id.as_str(),
                "add user to group" | "remove user from group"
            )
        });
        for target in &plan.bound_targets {
            match target {
                OperationTarget::User(target) => self.policy.check_user(target)?,
                OperationTarget::Group(target) => {
                    self.policy.check_group(target)?;
                    if membership_change {
                        self.policy.check_elevation_membership(&target.name)?;
                    }
                }
                OperationTarget::NewUser(name) => {
                    ensure_new_name_is_not_root(name.as_str(), "user")?;
                }
                OperationTarget::NewGroup(name) => {
                    ensure_new_name_is_not_root(name.as_str(), "group")?;
                }
            }
        }
        Ok(())
    }

    /// Execute exactly a plan returned by [`Self::prepare_operation`].
    ///
    /// Stable bindings and protected policy are rechecked before elevation.
    /// A preflight that observes all steps already satisfied executes no child
    /// and consumes no one-shot authentication secret.
    pub fn execute_prepared_operation(&self, plan: &OperationPlan) -> CoreResult<OperationReport> {
        let snapshot = self.snapshot()?;
        validate_plan_bindings(&snapshot, plan)?;
        self.validate_policy(plan)?;
        let reconciler = AccountSourceReconciler {
            source: self.source.as_ref(),
        };
        match preflight_plan(plan, &reconciler) {
            PlanPreflight::NoCommandsRequired | PlanPreflight::CannotVerify { .. }
                if !plan.steps.is_empty() =>
            {
                Ok(execute_plan(
                    self.runner.as_ref(),
                    ElevationGrant::Direct,
                    plan,
                    &reconciler,
                ))
            }
            _ if plan.steps.is_empty() => Ok(execute_plan(
                self.runner.as_ref(),
                ElevationGrant::Direct,
                plan,
                &reconciler,
            )),
            PlanPreflight::NeedsExecution => {
                let grant = self.elevation_grant()?;
                Ok(execute_plan(self.runner.as_ref(), grant, plan, &reconciler))
            }
            PlanPreflight::NoCommandsRequired | PlanPreflight::CannotVerify { .. } => {
                unreachable!("non-empty plan handled above")
            }
        }
    }

    /// Refresh account data while retaining a known-good prior snapshot as stale.
    pub fn refresh_state(&self, prior: Option<AccountSnapshot>) -> SnapshotState {
        refresh_retaining(self.source.as_ref(), prior)
    }

    /// Read users from the configured account data source.
    pub(crate) fn list_users(&self) -> Result<Vec<SystemUser>> {
        self.source
            .refresh()
            .map(|snapshot| snapshot.users.into_iter().map(SystemUser::from).collect())
            .map_err(Into::into)
    }

    /// Read groups from the configured account data source.
    pub(crate) fn list_groups(&self) -> Result<Vec<SystemGroup>> {
        self.source
            .refresh()
            .map(|snapshot| snapshot.groups.into_iter().map(SystemGroup::from).collect())
            .map_err(Into::into)
    }

    /// List validated shell paths from the configured account data source.
    pub(crate) fn list_shells(&self) -> Result<Vec<String>> {
        self.source
            .refresh()
            .map(|snapshot| {
                snapshot
                    .shells
                    .into_iter()
                    .map(|shell| shell.as_str().to_owned())
                    .collect()
            })
            .map_err(Into::into)
    }

    /// Add a mutable user to a mutable group with `gpasswd -a`.
    pub(crate) fn add_user_to_group(&self, username: &str, groupname: &str) -> Result<()> {
        let username = UserName::new(username)?;
        let groupname = GroupName::new(groupname)?;
        self.ensure_mutable_user(&username)?;
        self.ensure_mutable_group(&groupname)?;
        self.execute(
            CommandSpec::new(KnownProgram::GPasswd)
                .fixed_arg("-a")?
                .user_name(&username)
                .group_name(&groupname),
        )
    }

    /// Remove a mutable user from a mutable group with `gpasswd -d`.
    pub(crate) fn remove_user_from_group(&self, username: &str, groupname: &str) -> Result<()> {
        let username = UserName::new(username)?;
        let groupname = GroupName::new(groupname)?;
        self.ensure_mutable_user(&username)?;
        self.ensure_mutable_group(&groupname)?;
        self.execute(
            CommandSpec::new(KnownProgram::GPasswd)
                .fixed_arg("-d")?
                .user_name(&username)
                .group_name(&groupname),
        )
    }

    /// Create a non-root local group.
    pub(crate) fn create_group(&self, groupname: &str) -> Result<()> {
        let groupname = GroupName::new(groupname)?;
        self.ensure_new_name_is_not_root(groupname.as_str(), "group")?;
        self.execute(CommandSpec::new(KnownProgram::GroupAdd).group_name(&groupname))
    }

    /// Create a non-root local user, optionally with a home directory.
    pub(crate) fn create_user(&self, username: &str, create_home: bool) -> Result<()> {
        let username = UserName::new(username)?;
        self.ensure_new_name_is_not_root(username.as_str(), "user")?;
        let mut spec = CommandSpec::new(KnownProgram::UserAdd);
        if create_home {
            spec = spec.fixed_arg("-m")?;
        }
        self.execute(spec.user_name(&username))
    }

    /// Delete a mutable group.  A confirmed absent group is an idempotent success.
    pub(crate) fn delete_group(&self, groupname: &str) -> Result<()> {
        let groupname = GroupName::new(groupname)?;
        let snapshot = self.snapshot()?;
        let group = snapshot.groups.iter().find(|group| group.name == groupname);
        match group {
            None => Ok(()),
            Some(group) if group.gid == Gid(0) || group.name.as_str() == "root" => {
                Err(Box::new(CoreError::Validation {
                    field: "group",
                    reason: "root group is immutable",
                }))
            }
            Some(_) => {
                self.execute(CommandSpec::new(KnownProgram::GroupDel).group_name(&groupname))
            }
        }
    }

    /// Rename a mutable group.
    pub(crate) fn rename_group(&self, old_name: &str, new_name: &str) -> Result<()> {
        let old_name = GroupName::new(old_name)?;
        let new_name = GroupName::new(new_name)?;
        self.ensure_mutable_group(&old_name)?;
        self.ensure_new_name_is_not_root(new_name.as_str(), "group")?;
        self.execute(
            CommandSpec::new(KnownProgram::GroupMod)
                .fixed_arg("-n")?
                .group_name(&new_name)
                .group_name(&old_name),
        )
    }

    /// Delete a mutable user.  Existing product behavior may request home
    /// deletion; the typed plan API never compensates by deleting homes.
    pub(crate) fn delete_user(&self, username: &str, delete_home: bool) -> Result<()> {
        let username = UserName::new(username)?;
        self.ensure_mutable_user(&username)?;
        let mut spec = CommandSpec::new(KnownProgram::UserDel);
        if delete_home {
            spec = spec.fixed_arg("-r")?;
        }
        self.execute(spec.user_name(&username))
    }

    /// Change the shell for a mutable user.
    pub(crate) fn change_user_shell(&self, username: &str, new_shell: &str) -> Result<()> {
        let username = UserName::new(username)?;
        let shell = ShellPath::new(new_shell)?;
        self.ensure_mutable_user(&username)?;
        self.execute(
            CommandSpec::new(KnownProgram::UserMod)
                .fixed_arg("-s")?
                .shell_path(&shell)
                .user_name(&username),
        )
    }

    /// Change the GECOS field for a mutable user.
    pub(crate) fn change_user_fullname(&self, username: &str, new_fullname: &str) -> Result<()> {
        let username = UserName::new(username)?;
        let full_name = Gecos::new(new_fullname)?;
        self.ensure_mutable_user(&username)?;
        self.execute(
            CommandSpec::new(KnownProgram::UserMod)
                .fixed_arg("-c")?
                .gecos(&full_name)
                .user_name(&username),
        )
    }

    /// Rename a mutable user.
    pub(crate) fn change_username(&self, old_username: &str, new_username: &str) -> Result<()> {
        let old_username = UserName::new(old_username)?;
        let new_username = UserName::new(new_username)?;
        self.ensure_mutable_user(&old_username)?;
        self.ensure_new_name_is_not_root(new_username.as_str(), "user")?;
        self.execute(
            CommandSpec::new(KnownProgram::UserMod)
                .fixed_arg("-l")?
                .user_name(&new_username)
                .user_name(&old_username),
        )
    }

    /// Set a password through `chpasswd` stdin only.
    pub(crate) fn set_user_password(&self, username: &str, password: &str) -> Result<()> {
        let username = UserName::new(username)?;
        self.ensure_mutable_user(&username)?;
        let record = PasswordRecord::new(username, SecretString::new(password))?;
        let spec = CommandSpec::new(KnownProgram::ChPasswd).password_record(record)?;
        self.execute(spec)
    }

    /// Expire a mutable user's password with `chage -d 0`.
    pub(crate) fn expire_user_password(&self, username: &str) -> Result<()> {
        let username = UserName::new(username)?;
        self.ensure_mutable_user(&username)?;
        self.execute(
            CommandSpec::new(KnownProgram::ChAge)
                .fixed_arg("-d")?
                .fixed_arg("0")?
                .user_name(&username),
        )
    }

    fn snapshot(&self) -> CoreResult<AccountSnapshot> {
        self.source.refresh()
    }

    fn ensure_new_name_is_not_root(&self, name: &str, field: &'static str) -> CoreResult<()> {
        if name == "root" {
            return Err(CoreError::Validation {
                field,
                reason: "root identity is immutable",
            });
        }
        Ok(())
    }

    fn ensure_mutable_user(&self, username: &UserName) -> CoreResult<()> {
        let user = self
            .snapshot()?
            .users
            .into_iter()
            .find(|user| user.name == *username)
            .ok_or(CoreError::Validation {
                field: "user",
                reason: "target was not found in the current snapshot",
            })?;
        if user.uid == Uid(0) || user.name.as_str() == "root" {
            return Err(CoreError::Validation {
                field: "user",
                reason: "root user is immutable",
            });
        }
        Ok(())
    }

    fn ensure_mutable_group(&self, groupname: &GroupName) -> CoreResult<()> {
        let group = self
            .snapshot()?
            .groups
            .into_iter()
            .find(|group| group.name == *groupname)
            .ok_or(CoreError::Validation {
                field: "group",
                reason: "target was not found in the current snapshot",
            })?;
        if group.gid == Gid(0) || group.name.as_str() == "root" {
            return Err(CoreError::Validation {
                field: "group",
                reason: "root group is immutable",
            });
        }
        Ok(())
    }

    fn execute(&self, spec: CommandSpec) -> Result<()> {
        let grant = self.elevation_grant().map_err(Box::new)?;
        self.runner
            .run(grant, &spec)
            .map(|_| ())
            .map_err(Into::into)
    }

    fn elevation_grant(&self) -> CoreResult<ElevationGrant> {
        if self.identity.effective_uid()? == Uid(0) {
            return Ok(ElevationGrant::Direct);
        }
        // A grant is intentionally scoped to this one execution. The secret is
        // consumed for `sudo -v` and discarded immediately; no stale timestamp
        // survives to bypass a later one-shot reauthentication request.
        let secret = self
            .pending_secret
            .lock()
            .expect("pending secret mutex poisoned")
            .take()
            .ok_or(CoreError::AuthenticationRequired)?;
        self.runner.authenticate(secret)
    }
}

fn ensure_new_name_is_not_root(name: &str, field: &'static str) -> CoreResult<()> {
    if name == "root" {
        return Err(CoreError::Validation {
            field,
            reason: "root identity is immutable",
        });
    }
    Ok(())
}

fn target_error(reason: &'static str) -> CoreError {
    CoreError::Validation {
        field: "operation target",
        reason,
    }
}

fn mutable_user_target(
    snapshot: &AccountSnapshot,
    name: &UserName,
    generation: u64,
) -> CoreResult<UserTarget> {
    let user = snapshot
        .users
        .iter()
        .find(|user| user.name == *name)
        .ok_or_else(|| target_error("user was not found in the current snapshot"))?;
    if user.uid == Uid(0) || user.name.as_str() == "root" {
        return Err(target_error("root user is immutable"));
    }
    Ok(UserTarget {
        uid: user.uid,
        name: user.name.clone(),
        generation,
    })
}

fn mutable_group_target(
    snapshot: &AccountSnapshot,
    name: &GroupName,
    generation: u64,
) -> CoreResult<GroupTarget> {
    let group = snapshot
        .groups
        .iter()
        .find(|group| group.name == *name)
        .ok_or_else(|| target_error("group was not found in the current snapshot"))?;
    if group.gid == Gid(0) || group.name.as_str() == "root" {
        return Err(target_error("root group is immutable"));
    }
    Ok(GroupTarget {
        gid: group.gid,
        name: group.name.clone(),
        generation,
    })
}

fn validate_plan_bindings(snapshot: &AccountSnapshot, plan: &OperationPlan) -> CoreResult<()> {
    let generation = snapshot_generation(snapshot);
    for target in &plan.bound_targets {
        match target {
            OperationTarget::User(target) => {
                if target.generation != generation
                    || !snapshot
                        .users
                        .iter()
                        .any(|user| user.uid == target.uid && user.name == target.name)
                {
                    return Err(target_error(
                        "user changed since the operation was prepared",
                    ));
                }
                if target.uid == Uid(0) || target.name.as_str() == "root" {
                    return Err(target_error("root user is immutable"));
                }
            }
            OperationTarget::Group(target) => {
                if target.generation != generation
                    || !snapshot
                        .groups
                        .iter()
                        .any(|group| group.gid == target.gid && group.name == target.name)
                {
                    return Err(target_error(
                        "group changed since the operation was prepared",
                    ));
                }
                if target.gid == Gid(0) || target.name.as_str() == "root" {
                    return Err(target_error("root group is immutable"));
                }
            }
            OperationTarget::NewUser(name) => {
                if snapshot.users.iter().any(|user| user.name == *name) {
                    return Err(target_error("new user name is no longer available"));
                }
            }
            OperationTarget::NewGroup(name) => {
                if snapshot.groups.iter().any(|group| group.name == *name) {
                    return Err(target_error("new group name is no longer available"));
                }
            }
        }
    }
    Ok(())
}

fn snapshot_generation(snapshot: &AccountSnapshot) -> u64 {
    // Stable FNV-1a fingerprint. This deliberately rejects execution when any
    // observed account snapshot changes between preview and confirmation.
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    let mut feed = |value: &[u8]| {
        for byte in value {
            hash ^= u64::from(*byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash ^= 0xff;
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    };
    for user in &snapshot.users {
        feed(&user.uid.0.to_le_bytes());
        feed(user.name.as_str().as_bytes());
        feed(&user.primary_gid.0.to_le_bytes());
        feed(user.home_dir.to_string_lossy().as_bytes());
        feed(user.shell.as_str().as_bytes());
        if let Some(gecos) = &user.full_name {
            feed(gecos.as_str().as_bytes());
        }
    }
    for group in &snapshot.groups {
        feed(&group.gid.0.to_le_bytes());
        feed(group.name.as_str().as_bytes());
        for member in &group.members {
            feed(member.as_str().as_bytes());
        }
    }
    for shell in &snapshot.shells {
        feed(shell.as_str().as_bytes());
    }
    hash
}

/// In-memory snapshot used only while compiling a composite request. It
/// prevents per-child host reads without becoming an execution/reconciliation
/// source or exposing mutable account data outside the adapter.
#[derive(Clone)]
struct SnapshotSource(AccountSnapshot);

impl AccountDataSource for SnapshotSource {
    fn refresh(&self) -> CoreResult<AccountSnapshot> {
        Ok(self.0.clone())
    }
}

struct AccountSourceReconciler<'a> {
    source: &'a dyn AccountDataSource,
}

impl AccountSourceReconciler<'_> {
    fn status(snapshot: &AccountSnapshot, condition: &AccountCondition) -> CheckStatus {
        match condition {
            AccountCondition::Opaque => CheckStatus::Unavailable,
            AccountCondition::UserExists { name, exists } => {
                let found = snapshot.users.iter().any(|user| user.name == *name);
                if found == *exists {
                    CheckStatus::Satisfied
                } else {
                    CheckStatus::Unsatisfied
                }
            }
            AccountCondition::GroupExists { name, exists } => {
                let found = snapshot.groups.iter().any(|group| group.name == *name);
                if found == *exists {
                    CheckStatus::Satisfied
                } else {
                    CheckStatus::Unsatisfied
                }
            }
            AccountCondition::UserIdentity { uid, name } => {
                if snapshot
                    .users
                    .iter()
                    .any(|user| user.uid == *uid && user.name == *name)
                {
                    CheckStatus::Satisfied
                } else {
                    CheckStatus::Unsatisfied
                }
            }
            AccountCondition::GroupIdentity { gid, name } => {
                if snapshot
                    .groups
                    .iter()
                    .any(|group| group.gid == *gid && group.name == *name)
                {
                    CheckStatus::Satisfied
                } else {
                    CheckStatus::Unsatisfied
                }
            }
            AccountCondition::GroupMember {
                user,
                group,
                present,
            } => match snapshot
                .groups
                .iter()
                .find(|candidate| candidate.name == *group)
            {
                Some(group) => {
                    let found = group.members.iter().any(|member| member == user);
                    if found == *present {
                        CheckStatus::Satisfied
                    } else {
                        CheckStatus::Unsatisfied
                    }
                }
                None => CheckStatus::Unsatisfied,
            },
            AccountCondition::UserShell { uid, shell } => {
                match snapshot.users.iter().find(|user| user.uid == *uid) {
                    Some(user) if user.shell == *shell => CheckStatus::Satisfied,
                    Some(_) => CheckStatus::Unsatisfied,
                    None => CheckStatus::Unsatisfied,
                }
            }
            AccountCondition::UserGecos { uid, gecos } => {
                match snapshot.users.iter().find(|user| user.uid == *uid) {
                    Some(user) if user.full_name == *gecos => CheckStatus::Satisfied,
                    Some(_) => CheckStatus::Unsatisfied,
                    None => CheckStatus::Unsatisfied,
                }
            }
        }
    }
}

impl OperationReconciler for AccountSourceReconciler<'_> {
    fn check(&self, _: &OperationTarget, check: &OperationCheck) -> CheckStatus {
        match self.source.refresh() {
            Ok(snapshot) => Self::status(&snapshot, check.condition()),
            Err(_) => CheckStatus::Unavailable,
        }
    }

    fn reconcile(&self, plan: &OperationPlan) -> ReconciliationStatus {
        let snapshot = match self.source.refresh() {
            Ok(snapshot) => snapshot,
            Err(_) => {
                return ReconciliationStatus::Unavailable {
                    detail: "account refresh failed during reconciliation".to_owned(),
                };
            }
        };
        let mut unavailable = false;
        for check in plan
            .steps
            .iter()
            .filter_map(|step| step.postcondition.as_ref())
        {
            match Self::status(&snapshot, check.condition()) {
                CheckStatus::Satisfied => {}
                CheckStatus::Unsatisfied => {
                    return ReconciliationStatus::Partial {
                        detail: check.description.clone(),
                    };
                }
                CheckStatus::Unavailable => unavailable = true,
            }
        }
        if unavailable {
            ReconciliationStatus::Unavailable {
                detail: "a required postcondition is unavailable from the local account source"
                    .to_owned(),
            }
        } else {
            ReconciliationStatus::Verified
        }
    }
}

impl Default for SystemAdapter {
    fn default() -> Self {
        Self::new()
    }
}

/// Resolve the current username using checked effective UID and local account data.
///
/// This compatibility helper returns `None` when identity or account data cannot
/// be observed.  It never treats an unknown identity as root.
pub fn current_username() -> Option<String> {
    let uid = SystemIdentityProvider.effective_uid().ok()?;
    LocalFileAccountDataSource::new()
        .refresh()
        .ok()?
        .users
        .into_iter()
        .find(|user| user.uid == uid)
        .map(|user| user.name.as_str().to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct StaticSource(AccountSnapshot);

    impl AccountDataSource for StaticSource {
        fn refresh(&self) -> CoreResult<AccountSnapshot> {
            Ok(self.0.clone())
        }
    }

    struct NoRun;

    impl CommandRunner for NoRun {
        fn authenticate(&self, _: SecretString) -> CoreResult<ElevationGrant> {
            Ok(ElevationGrant::SudoTimestamp)
        }

        fn run(&self, _: ElevationGrant, _: &CommandSpec) -> CoreResult<CommandResult> {
            unreachable!("root identity protection must prevent this invocation")
        }
    }

    #[test]
    fn root_identity_is_blocked_before_runner_use() {
        let source = StaticSource(AccountSnapshot {
            users: vec![AccountUser {
                uid: Uid(0),
                name: UserName::new("root").unwrap(),
                primary_gid: Gid(0),
                full_name: None,
                home_dir: "/root".into(),
                shell: ShellPath::new("/bin/sh").unwrap(),
            }],
            groups: vec![],
            shells: vec![],
            diagnostics: vec![],
        });
        let adapter = SystemAdapter::from_components(
            Arc::new(source),
            Arc::new(NoRun),
            Arc::new(FixedIdentityProvider::uid(Uid(0))),
        );
        assert!(adapter.delete_user("root", false).is_err());
    }

    #[allow(dead_code)]
    struct CallCounter(Mutex<usize>);
}
