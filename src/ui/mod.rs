pub mod users;
pub mod groups;
pub mod components;

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Style};
use ratatui::widgets::{Block, Borders, Paragraph, Clear};
use ratatui::{Frame};

use crate::app::{AppState, ActiveTab, ModalState};
use crate::sys;

pub fn render(f: &mut Frame, app: &mut AppState) {
    let root = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(5), Constraint::Length(1)].as_ref())
        .split(f.area());
    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)].as_ref())
        .split(root[1]);
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(5)].as_ref())
        .split(body[1]);

    let who = crate::sys::current_username().unwrap_or_else(|| "unknown".to_string());
    let tabs = match app.active_tab { ActiveTab::Users => "[Users]  Groups", ActiveTab::Groups => "Users  [Groups]" };
    let prompt = match app.input_mode {
        crate::app::InputMode::Normal => String::new(),
        crate::app::InputMode::SearchUsers => format!("  Search users: {}", app.search_query),
        crate::app::InputMode::SearchGroups => format!("  Search groups: {}", app.search_query),
        crate::app::InputMode::Modal => String::new(),
    };
    let p = Paragraph::new(format!(
        "usrgrp-manager ({who})  {tabs}{prompt}  users:{}  groups:{}  — Tab: switch tab; Shift-Tab: member-of; /: search; Enter: apply; Esc: cancel; q: quit",
        app.users.len(), app.groups.len()
    ))
    .block(
        Block::default()
            .title("usrgrp-manager")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    )
    .style(Style::default().fg(app.theme.header_fg).bg(app.theme.header_bg));
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

    components::render_status_bar(f, root[2], app);

    if app.modal.is_some() {
        render_modal(f, f.area(), app);
    }
}

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
            | ModalState::DeleteConfirm { .. } => {
                users::render_user_modal(f, area, app, &state);
            }
            ModalState::GroupsActions { .. }
            | ModalState::GroupAddInput { .. }
            | ModalState::GroupDeleteConfirm { .. }
            | ModalState::GroupModifyMenu { .. }
            | ModalState::GroupModifyAddMembers { .. }
            | ModalState::GroupModifyRemoveMembers { .. } => {
                groups::render_group_modal(f, area, app, &state);
            }
            ModalState::Info { .. } => {
                components::render_info_modal(f, area, app, &state);
            }
            ModalState::SudoPrompt { .. } => {
                components::render_sudo_modal(f, area, app, &state);
            }
        }
    }
}