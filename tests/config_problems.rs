//! Configuration problems are visible: a startup dialog, the status bar, and
//! a section in the help modal. Config roots point at a tempdir, never the
//! host or the repository.

mod common;

use std::{sync::Arc, time::SystemTime};

use ratatui::{Terminal, backend::TestBackend};
use usrgrp_manager::{
    app::{
        AppState, CONFIG_PROBLEMS_TITLE, CachedDiagnostics, Clock, DiagnosticProvider,
        FixedConfigRootProvider, ModalState,
    },
    sys::{AccountSnapshot, SystemAdapter},
    ui,
};

struct FixedClock;
impl Clock for FixedClock {
    fn now(&self) -> SystemTime {
        SystemTime::UNIX_EPOCH
    }
}

struct NoDiagnostics;
impl DiagnosticProvider for NoDiagnostics {
    fn refresh(&self, _: &AccountSnapshot, _: SystemTime) -> CachedDiagnostics {
        CachedDiagnostics::default()
    }
}

/// App whose only config root is `dir`, with a broken theme.conf loaded.
fn app_with_broken_theme(dir: &tempfile::TempDir) -> AppState {
    std::fs::write(dir.path().join("theme.conf"), "unknown = #FFFFFF\n").unwrap();
    let adapter = Arc::new(SystemAdapter::from_components(
        Arc::new(common::FixtureSource(Ok(AccountSnapshot::empty()))),
        Arc::new(common::FakeRunner::succeeds()),
        Arc::new(common::FixtureIdentity::uid(1000)),
    ));
    let mut app = AppState::with_dependencies(
        adapter,
        AccountSnapshot::empty(),
        Arc::new(FixedClock),
        Arc::new(NoDiagnostics),
        Arc::new(FixedConfigRootProvider(dir.path().to_path_buf())),
    );
    app.load_configuration();
    app
}

fn screen(app: &AppState) -> String {
    let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
    terminal.draw(|frame| ui::render(frame, app)).unwrap();
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

#[test]
fn broken_theme_is_recorded_and_listed_in_help() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_broken_theme(&dir);
    assert_eq!(app.diagnostics.config_messages.len(), 1);
    assert!(app.diagnostics.config_messages[0].starts_with("theme configuration"));

    let screen_text = screen(&app);
    assert!(screen_text.contains("config: 1 error (see ? help)"));

    app.push_modal(ModalState::Help { scroll: 0 });
    let screen_text = screen(&app);
    assert!(screen_text.contains(CONFIG_PROBLEMS_TITLE));
    assert!(screen_text.contains("theme configuration"));
}

#[test]
fn startup_dialog_lists_problems_and_says_defaults_are_used() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_broken_theme(&dir);
    app.show_config_problems();
    match app.modal() {
        Some(ModalState::Info { title, message }) => {
            assert_eq!(title.as_deref(), Some(CONFIG_PROBLEMS_TITLE));
            assert!(message.starts_with("theme configuration"));
            assert!(message.ends_with("Defaults are in use for the affected file."));
        }
        _ => panic!("expected the configuration dialog"),
    }
    let screen_text = screen(&app);
    assert!(screen_text.contains(CONFIG_PROBLEMS_TITLE));
    assert!(screen_text.contains("Defaults are in use for the affected file."));
}

#[test]
fn clean_configuration_opens_no_dialog_and_help_has_no_section() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = app_with_broken_theme(&dir);
    app.diagnostics.config_messages.clear();
    app.show_config_problems();
    assert!(app.modal().is_none());
    assert!(!screen(&app).contains("config:"));
    app.push_modal(ModalState::Help { scroll: 0 });
    assert!(!screen(&app).contains(CONFIG_PROBLEMS_TITLE));
}
