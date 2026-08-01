use std::sync::Arc;

use ratatui::{Terminal, backend::TestBackend};
use usrgrp_manager::{
    app::AppState,
    sys::{AccountSnapshot, SystemAdapter},
    ui,
};

#[test]
fn small_terminal_fallback_has_a_stable_snapshot_message() {
    let app = AppState::with_adapter(Arc::new(SystemAdapter::new()), AccountSnapshot::empty());
    let mut terminal = Terminal::new(TestBackend::new(20, 5)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();

    let snapshot = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect::<String>();
    assert!(snapshot.contains("terminal too small"));
}

#[test]
fn render_does_not_mutate_stable_selection_identity() {
    let mut app = AppState::with_adapter(Arc::new(SystemAdapter::new()), AccountSnapshot::empty());
    app.selected_user_uid = Some(1000);
    app.selected_group_gid = Some(1000);
    let before = (app.selected_user_uid, app.selected_group_gid);
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|frame| ui::render(frame, &app)).unwrap();
    assert_eq!(before, (app.selected_user_uid, app.selected_group_gid));
}
