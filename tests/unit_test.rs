//! Small public, host-independent API checks.

use usrgrp_manager::{
    app::{AppState, Theme, keymap::Keymap},
    search::{ShadowState, parse_shadow_records},
};

#[test]
fn default_app_construction_is_pure_and_empty() {
    let app = AppState::new();
    assert!(app.users.is_empty());
    assert!(app.groups.is_empty());
    assert!(matches!(
        app.diagnostics.shadow,
        ShadowState::Unavailable { .. }
    ));
}

#[test]
fn indexed_theme_color_round_trips_without_loss() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("theme.conf");
    let mut theme = Theme::mocha();
    theme.title = ratatui::style::Color::Indexed(42);
    theme.write_file(path.to_str().unwrap()).unwrap();
    assert_eq!(
        Theme::from_file(path.to_str().unwrap()).unwrap().title,
        theme.title
    );
}

#[test]
fn keymap_has_deterministic_binding_snapshot() {
    let map = Keymap::default();
    assert!(map.all_bindings().len() >= 20);
}

#[test]
fn shadow_parser_is_deterministic() {
    let status = parse_shadow_records("alice:!:1:0:30::::\n", 40);
    assert!(status["alice"].locked);
    assert!(status["alice"].expired);
}
