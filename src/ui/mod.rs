//! UI rendering entry point and modal routing.
//!
//! Renders the high-level layout (header, body, status bar) and delegates to
//! users/groups submodules and shared components.
//!
pub mod components;
pub mod groups;
pub mod users;

use ratatui::Frame;
use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::Style;
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::app::{ActiveTab, AppState, ModalState};

/// Render the entire UI frame, including header, body, footer, and modals.
pub fn render(f: &mut Frame, app: &mut AppState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(5),
                Constraint::Min(5),
                Constraint::Length(1),
            ]
            .as_ref(),
        )
        .split(f.area());
    let body = if app.show_keybinds {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints(
                [
                    Constraint::Percentage(41), // main table
                    Constraint::Percentage(34), // details/members
                    Constraint::Percentage(25), // keybinds panel
                ]
                .as_ref(),
            )
            .split(root[1])
    } else {
        Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)].as_ref())
            .split(root[1])
    };
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(16), Constraint::Min(5)].as_ref())
        .split(body[1]);

    let who = crate::sys::current_username().unwrap_or_else(|| "unknown".to_string());
    let tabs = match app.active_tab {
        ActiveTab::Users => "[Users]  Groups",
        ActiveTab::Groups => "Users  [Groups]",
    };
    let prompt = match app.input_mode {
        crate::app::InputMode::Normal => String::new(),
        crate::app::InputMode::SearchUsers => format!("  Search users: {}", app.search_query),
        crate::app::InputMode::SearchGroups => format!("  Search groups: {}", app.search_query),
        crate::app::InputMode::Modal => String::new(),
    };
    // Inline key hints removed; dedicated keybinds panel is shown on the right now.
    let p = Paragraph::new(format!(
        "usrgrp-manager ({who})  {tabs}{prompt}\nusers:{}  groups:{}",
        app.users.len(),
        app.groups.len()
    ))
    .block(
        Block::default()
            .title("usrgrp-manager")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    )
    .style(Style::default().fg(app.theme.header_fg));
    f.render_widget(p, root[0]);

    match app.active_tab {
        ActiveTab::Users => {
            users::render_users_table(f, body[0], app);
            users::render_user_details(f, right[0], app);
            users::render_user_groups(f, right[1], app);
        }
        ActiveTab::Groups => {
            groups::render_groups_table(f, body[0], app);
            groups::render_group_details(f, right[0], app);
            groups::render_group_members(f, right[1], app);
        }
    }

    // Keybindings panel on the far right (if enabled)
    if app.show_keybinds {
        components::render_keybinds_panel(f, body[2], app);
    }

    components::render_status_bar(f, root[2], app);

    if app.modal.is_some() {
        render_modal(f, f.area(), app);
    }
}

/// Route modal rendering to the appropriate submodule.
fn render_modal(f: &mut Frame, area: Rect, app: &mut AppState) {
    if let Some(state) = app.modal.clone() {
        match state.clone() {
            ModalState::Actions { .. }
            | ModalState::ModifyMenu { .. }
            | ModalState::ModifyGroupsAdd { .. }
            | ModalState::ModifyGroupsRemove { .. }
            | ModalState::ModifyDetailsMenu { .. }
            | ModalState::ModifyShell { .. }
            | ModalState::ModifyTextInput { .. }
            | ModalState::DeleteConfirm { .. }
            | ModalState::UserAddInput { .. }
            | ModalState::ModifyPasswordMenu { .. }
            | ModalState::ChangePassword { .. } => {
                users::render_user_modal(f, area, app, &state);
            }
            ModalState::GroupsActions { .. }
            | ModalState::GroupAddInput { .. }
            | ModalState::GroupDeleteConfirm { .. }
            | ModalState::GroupModifyMenu { .. }
            | ModalState::GroupModifyAddMembers { .. }
            | ModalState::GroupModifyRemoveMembers { .. }
            | ModalState::GroupRenameInput { .. } => {
                groups::render_group_modal(f, area, app, &state);
            }
            ModalState::ConfirmRemoveUserFromGroup { .. } => {
                users::render_user_modal(f, area, app, &state);
            }
            ModalState::Info { .. } => {
                components::render_info_modal(f, area, app, &state);
            }
            ModalState::Help { scroll } => {
                components::render_help_modal(f, area, app, scroll);
            }
            ModalState::SudoPrompt { .. } => {
                components::render_sudo_modal(f, area, app, &state);
            }
            ModalState::FilterMenu { .. } => {
                components::render_filter_modal(f, area, app, &state);
            }
        }
    }
}
