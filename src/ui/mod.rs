//! Immutable UI rendering entry point.
pub mod components;
pub mod groups;
pub mod users;

use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph},
};

use crate::app::{ActiveTab, AppState, ModalState};

/// Render a frame from cached state only.  No renderer takes `&mut AppState` or
/// performs filesystem/process I/O.
pub fn render(frame: &mut Frame, app: &AppState) {
    if frame.area().width < 30 || frame.area().height < 8 {
        frame.render_widget(
            Paragraph::new("terminal too small; resize to at least 30x8"),
            frame.area(),
        );
        return;
    }
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(4),
            Constraint::Min(4),
            Constraint::Length(1),
        ])
        .split(frame.area());
    let body = if app.show_keybinds && root[1].width >= 90 {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(45),
                Constraint::Percentage(35),
                Constraint::Percentage(20),
            ])
            .split(root[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(root[1])
    };
    let details = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(7), Constraint::Min(5)])
        .split(body[1]);
    let tabs = match app.active_tab {
        ActiveTab::Users => "[Users]  Groups",
        ActiveTab::Groups => "Users  [Groups]",
    };
    let stale = app
        .diagnostics
        .stale_reason
        .as_ref()
        .map_or(String::new(), |reason| format!("\nSTALE: {reason}"));
    frame.render_widget(
        Paragraph::new(format!(
            "usrgrp-manager ({})  {tabs}\nusers:{} groups:{} shadow:{}{stale}",
            app.current_username,
            app.users.len(),
            app.groups.len(),
            app.diagnostics.shadow.availability_label()
        ))
        .style(Style::default().fg(app.theme.header_fg))
        .block(
            Block::default()
                .title("usrgrp-manager")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        ),
        root[0],
    );
    match app.active_tab {
        ActiveTab::Users => {
            users::render_users_table(frame, body[0], app);
            users::render_user_details(frame, details[0], app);
            users::render_user_groups(frame, details[1], app);
        }
        ActiveTab::Groups => {
            groups::render_groups_table(frame, body[0], app);
            groups::render_group_details(frame, details[0], app);
            groups::render_group_members(frame, details[1], app);
        }
    }
    if body.len() == 3 {
        components::render_keybinds_panel(frame, body[2], app);
    }
    components::render_status_bar(frame, root[2], app);
    if let Some(modal) = &app.modal {
        render_modal(frame, frame.area(), app, modal);
    }
}

fn render_modal(frame: &mut Frame, area: Rect, app: &AppState, modal: &ModalState) {
    match modal {
        ModalState::Info { .. }
        | ModalState::Help { .. }
        | ModalState::SudoPrompt { .. }
        | ModalState::FilterMenu { .. }
        | ModalState::OperationConfirm { .. } => components::render_modal(frame, area, app, modal),
        ModalState::GroupsActions { .. }
        | ModalState::GroupAddInput { .. }
        | ModalState::GroupDeleteConfirm { .. }
        | ModalState::GroupModifyMenu { .. }
        | ModalState::GroupModifyAddMembers { .. }
        | ModalState::GroupModifyRemoveMembers { .. }
        | ModalState::GroupRenameInput { .. } => {
            groups::render_group_modal(frame, area, app, modal)
        }
        _ => users::render_user_modal(frame, area, app, modal),
    }
}
