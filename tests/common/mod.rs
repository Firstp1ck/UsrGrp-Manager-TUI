#![allow(dead_code)]

use std::{collections::VecDeque, sync::Mutex};
use usrgrp_manager::{
    error::{CoreError, CoreResult},
    sys::{
        AccountDataSource, AccountSnapshot, CommandPreview, CommandResult, CommandRunner,
        CommandSpec, ElevationGrant, IdentityProvider, Uid,
    },
};

/// Fixture-backed account source; it never reads host account files.
#[derive(Clone)]
pub struct FixtureSource(pub CoreResult<AccountSnapshot>);

impl AccountDataSource for FixtureSource {
    fn refresh(&self) -> CoreResult<AccountSnapshot> {
        self.0.clone()
    }
}

/// Fixture-backed effective identity; it never inspects process or proc state.
#[derive(Clone)]
pub struct FixtureIdentity(pub CoreResult<Uid>);

impl FixtureIdentity {
    pub fn uid(uid: u32) -> Self {
        Self(Ok(Uid(uid)))
    }
}

impl IdentityProvider for FixtureIdentity {
    fn effective_uid(&self) -> CoreResult<Uid> {
        self.0.clone()
    }
}

/// Safe command observation emitted by [`FakeRunner`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecordedCommand {
    pub grant: ElevationGrant,
    pub preview: CommandPreview,
}

/// A deterministic runner which records only redacted previews and never spawns.
pub struct FakeRunner {
    pub calls: Mutex<Vec<RecordedCommand>>,
    authentication: CoreResult<ElevationGrant>,
    failures: Mutex<VecDeque<(usize, CoreError)>>,
}

impl FakeRunner {
    pub fn succeeds() -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            authentication: Ok(ElevationGrant::SudoTimestamp),
            failures: Mutex::new(VecDeque::new()),
        }
    }

    pub fn failing_at(step: usize, error: CoreError) -> Self {
        let runner = Self::succeeds();
        runner.failures.lock().unwrap().push_back((step, error));
        runner
    }

    pub fn authentication_fails(error: CoreError) -> Self {
        Self {
            calls: Mutex::new(Vec::new()),
            authentication: Err(error),
            failures: Mutex::new(VecDeque::new()),
        }
    }

    pub fn recorded(&self) -> Vec<RecordedCommand> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for FakeRunner {
    fn authenticate(
        &self,
        _secret: usrgrp_manager::sys::SecretString,
    ) -> CoreResult<ElevationGrant> {
        self.authentication.clone()
    }

    fn run(&self, grant: ElevationGrant, spec: &CommandSpec) -> CoreResult<CommandResult> {
        let mut calls = self.calls.lock().unwrap();
        let number = calls.len() + 1;
        calls.push(RecordedCommand {
            grant,
            preview: spec.redacted_preview(),
        });
        drop(calls);
        if let Some((_, error)) = self
            .failures
            .lock()
            .unwrap()
            .iter()
            .find(|(failed_step, _)| *failed_step == number)
            .cloned()
        {
            return Err(error);
        }
        Ok(CommandResult::new(success_status(), Vec::new(), Vec::new()))
    }
}

#[cfg(unix)]
fn success_status() -> std::process::ExitStatus {
    use std::os::unix::process::ExitStatusExt;
    std::process::ExitStatus::from_raw(0)
}

#[cfg(not(unix))]
fn success_status() -> std::process::ExitStatus {
    panic!("fixture runner is only supported for the Linux-local contract")
}
