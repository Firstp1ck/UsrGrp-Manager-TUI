//! Immutable UI rendering entry point.
pub mod components;
pub mod groups;
pub mod layout;
pub mod users;

use ratatui::{Frame, layout::Rect, text::Line, widgets::Paragraph};

use crate::app::{ActiveTab, AppState, ModalState};

/// Render a frame from cached state only.  No renderer takes `&mut AppState` or
/// performs filesystem/process I/O.
pub fn render(frame: &mut Frame, app: &AppState) {
    if layout::below_minimum(frame.area()) {
        frame.render_widget(
            Paragraph::new("terminal too small; resize to at least 30x8"),
            frame.area(),
        );
        return;
    }
    // The pane widths do not depend on the details height: take the inner
    // width from a first pass, wrap, then size the pane from the row count.
    let details_width = layout::main_layout(
        frame.area(),
        app.active_tab,
        app.show_keybinds,
        layout::details_base_rows(app.active_tab),
    )
    .details
    .width
    .saturating_sub(2);
    let layout = layout::main_layout(
        frame.area(),
        app.active_tab,
        app.show_keybinds,
        details_rows(app, details_width),
    );
    let (details_title, details) = details_content(app);
    let wrapped = components::wrap_details(&details, usize::from(details_width));
    components::render_header(frame, layout.header, app);
    match app.active_tab {
        ActiveTab::Users => {
            users::render_users_table(frame, layout.table, app);
            users::render_user_groups(frame, layout.members, app);
        }
        ActiveTab::Groups => {
            groups::render_groups_table(frame, layout.table, app);
            groups::render_group_members(frame, layout.members, app);
        }
    }
    components::render_details(frame, layout.details, app, details_title, wrapped);
    if let Some(area) = layout.keybinds {
        components::render_keybinds_panel(frame, area, app);
    }
    components::render_status_bar(frame, layout.status, app);
    if let Some(modal) = app.modal() {
        render_modal(frame, frame.area(), app, modal);
    }
}

/// Title and styled lines of the details pane for the selected user or group.
fn details_content(app: &AppState) -> (&'static str, Vec<Line<'static>>) {
    let (title, details) = match app.active_tab {
        ActiveTab::Users => ("Details", users::selected_user_details(app)),
        ActiveTab::Groups => ("Group details", groups::selected_group_details(app)),
    };
    (title, components::details_lines(app, &details))
}

/// Rows the selected user's or group's details take when wrapped by
/// character at `inner_width`. The renderer and the page keys both size the
/// details pane, and so the member pane, from this.
pub fn details_rows(app: &AppState, inner_width: u16) -> usize {
    let (_, details) = details_content(app);
    components::wrap_details(&details, usize::from(inner_width)).len()
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
