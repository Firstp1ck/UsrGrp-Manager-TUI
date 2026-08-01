mod common;

use std::{
    path::PathBuf,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::SystemTime,
};

use usrgrp_manager::{
    app::{AppState, CachedDiagnostics, Clock, ConfigRootProvider, DiagnosticProvider, InputMode},
    search::apply_filters_and_search,
    sys::{AccountSnapshot, SystemAdapter},
};

#[test]
fn search_query_is_bounded_without_host_io() {
    let mut app = AppState::new();
    app.input_mode = InputMode::SearchUsers;
    app.search_query = "x".repeat(10_000);
    apply_filters_and_search(&mut app);
    assert_eq!(app.search_query.len(), 256);
}

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
    }
}

struct CountingDiagnostics(Arc<AtomicUsize>);
impl DiagnosticProvider for CountingDiagnostics {
    fn refresh(&self, _: &AccountSnapshot, _: SystemTime) -> CachedDiagnostics {
        self.0.fetch_add(1, Ordering::SeqCst);
        CachedDiagnostics::default()
    }
}

struct FixedRoots(PathBuf);
impl ConfigRootProvider for FixedRoots {
    fn roots(&self) -> Vec<PathBuf> {
        vec![self.0.clone()]
    }
}

#[test]
fn injected_clock_roots_and_diagnostics_bound_effects_outside_rendering() {
    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("theme.conf"), "unknown = #FFFFFF\n").unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let adapter = Arc::new(SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(AccountSnapshot::empty()))),
        Arc::new(common::FakeRunner::succeeds()),
        Arc::new(common::FixtureIdentity::uid(0)),
    ));
    let mut app = AppState::with_dependencies(
        adapter,
        AccountSnapshot::empty(),
        Arc::new(FixedClock),
        Arc::new(CountingDiagnostics(calls.clone())),
        Arc::new(FixedRoots(directory.path().to_path_buf())),
    );

    app.load_configuration();
    app.refresh_accounts();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(app.diagnostics.config_messages.len(), 1);
    assert!(app.diagnostics.config_messages[0].contains("line 1"));
    assert!(app.diagnostics.config_messages[0].len() <= 256);
}
