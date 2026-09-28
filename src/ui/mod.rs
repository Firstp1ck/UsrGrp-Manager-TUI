//! Immutable UI rendering entry point.
pub mod components;
pub mod groups;
pub mod layout;
pub mod users;

use ratatui::{Frame, layout::Rect, widgets::Paragraph};

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
    let layout = layout::main_layout(frame.area(), app.show_keybinds);
    components::render_header(frame, layout.header, app);
    match app.active_tab {
        ActiveTab::Users => {
            users::render_users_table(frame, layout.table, app);
            users::render_user_details(frame, layout.details, app);
            users::render_user_groups(frame, layout.members, app);
        }
        ActiveTab::Groups => {
            groups::render_groups_table(frame, layout.table, app);
            groups::render_group_details(frame, layout.details, app);
            groups::render_group_members(frame, layout.members, app);
        }
    }
    if let Some(area) = layout.keybinds {
        components::render_keybinds_panel(frame, area, app);
    }
    components::render_status_bar(frame, layout.status, app);
    if let Some(modal) = app.modal() {
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
