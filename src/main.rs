use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use crossterm::event::{self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, Borders, Cell, Paragraph, Row, Table, TableState, List, ListItem, Clear};
use ratatui::{Frame, Terminal};
use std::time::{Duration, Instant};

mod sys;

struct AppState {
    started_at: Instant,
    users_all: Vec<sys::SystemUser>,
    users: Vec<sys::SystemUser>,
    groups_all: Vec<sys::SystemGroup>,
    groups: Vec<sys::SystemGroup>,
    active_tab: ActiveTab,
    selected_user_index: usize,
    selected_group_index: usize,
    rows_per_page: usize,
    _table_state: TableState,
    input_mode: InputMode,
    search_query: String,
    theme: Theme,
    modal: Option<ModalState>,
    users_focus: UsersFocus,
}

impl AppState {
    fn new() -> Self {
        let adapter = crate::sys::SystemAdapter::new();
        let mut users_all = adapter.list_users().unwrap_or_default();
        users_all.sort_by_key(|u| u.uid);
        let groups_all = adapter.list_groups().unwrap_or_default();
        Self {
            started_at: Instant::now(),
            users: users_all.clone(),
            users_all,
            groups: groups_all.clone(),
            groups_all,
            active_tab: ActiveTab::Users,
            selected_user_index: 0,
            selected_group_index: 0,
            rows_per_page: 10,
            _table_state: TableState::default(),
            input_mode: InputMode::Normal,
            search_query: String::new(),
            theme: Theme::dark(),
            modal: None,
            users_focus: UsersFocus::UsersList,
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ActiveTab {
    Users,
    Groups,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum UsersFocus {
    UsersList,
    MemberOf,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum InputMode {
    Normal,
    SearchUsers,
    SearchGroups,
    Modal,
}

#[derive(Clone, Copy, Debug)]
struct Theme {
    text: Color,
    _muted: Color,
    title: Color,
    border: Color,
    header_bg: Color,
    header_fg: Color,
    status_bg: Color,
    status_fg: Color,
    highlight_fg: Color,
    highlight_bg: Color,
}

impl Theme {
    fn dark() -> Self {
        Self {
            text: Color::Gray,
            _muted: Color::DarkGray,
            title: Color::Cyan,
            border: Color::Gray,
            header_bg: Color::Black,
            header_fg: Color::Cyan,
            status_bg: Color::DarkGray,
            status_fg: Color::Black,
            highlight_fg: Color::Yellow,
            highlight_bg: Color::Reset,
        }
    }
}

fn init_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

#[derive(Parser, Debug)]
#[command(name = "usrgrp-manager", version, about = "UNIX users/groups browser")] 
struct Cli {
    /// Log level, e.g. info, debug, trace
    #[arg(long, env = "USRGRP_MANAGER_LOG", default_value = "info")]
    log: String,

    /// Force file parsing of /etc/passwd and /etc/group (if built with feature)
    #[arg(long, action = ArgAction::SetTrue)]
    file_parse: bool,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    // Initialize tracing subscriber
    let env_filter = tracing_subscriber::EnvFilter::try_new(cli.log.clone()).unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).without_time().init();

    #[cfg(feature = "file-parse")]
    if !cli.file_parse {
        tracing::info!("feature 'file-parse' is enabled at build time; runtime flag is ignored");
    }

    let mut terminal = init_terminal().context("init terminal")?;

    let res = run_app(&mut terminal);

    // Restore terminal
    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();
    terminal.show_cursor().ok();

    if let Err(err) = res {
        tracing::error!(error = ?err, "application error");
    }
    Ok(())
}

fn run_app(terminal: &mut Terminal<CrosstermBackend<std::io::Stdout>>) -> Result<()> {
    let mut app = AppState::new();

    loop {
        terminal.draw(|f| {
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
            let tabs = match app.active_tab {
                ActiveTab::Users => "[Users]  Groups",
                ActiveTab::Groups => "Users  [Groups]",
            };
            let prompt = match app.input_mode {
                InputMode::Normal => String::new(),
                InputMode::SearchUsers => format!("  Search users: {}", app.search_query),
                InputMode::SearchGroups => format!("  Search groups: {}", app.search_query),
                InputMode::Modal => String::new(),
            };
            let p = Paragraph::new(format!(
                "usrgrp-manager ({who})  {tabs}{prompt}  users:{}  groups:{}  — Tab: switch tab; Shift-Tab: member-of; /: search; Enter: apply; Esc: cancel; q: quit",
                app.users.len(), app.groups.len()
            ))
            .alignment(Alignment::Center)
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
                    // Left: Users list
                    render_users_table(f, body[0], &mut app);
                    // Right top: Details
                    render_user_details(f, right[0], &app);
                    // Right bottom: Member of table
                    render_user_groups(f, right[1], &mut app);
                }
                ActiveTab::Groups => {
                    // Left: Groups list
                    render_groups_table(f, body[0], &mut app);
                    // Right top: Group details
                    render_group_details(f, right[0], &app);
                    // Right bottom: Group members
                    render_group_members(f, right[1], &mut app);
                }
            }

            // Bottom status bar
            render_status_bar(f, root[2], &app);

            // Modal overlay (popup)
            if app.modal.is_some() {
                render_modal(f, f.area(), &mut app);
            }
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    match app.input_mode {
                        InputMode::Normal => match key.code {
                            KeyCode::Char('q') => break,
                            KeyCode::Esc => { /* ignore */ }
                            KeyCode::Char('/') => {
                                app.search_query.clear();
                                app.input_mode = match app.active_tab { ActiveTab::Users => InputMode::SearchUsers, ActiveTab::Groups => InputMode::SearchGroups };
                            }
                            KeyCode::Tab => {
                                app.active_tab = match app.active_tab { ActiveTab::Users => ActiveTab::Groups, ActiveTab::Groups => ActiveTab::Users };
                            }
                            KeyCode::BackTab => {
                                if let ActiveTab::Users = app.active_tab {
                                    app.users_focus = match app.users_focus { UsersFocus::UsersList => UsersFocus::MemberOf, UsersFocus::MemberOf => UsersFocus::UsersList };
                                }
                            }
                            KeyCode::Enter => {
                                match app.active_tab {
                                    ActiveTab::Users => {
                                        if !app.users.is_empty() {
                                            // If focus is on MemberOf list, open Group actions for the selected group
                                            if let UsersFocus::MemberOf = app.users_focus {
                                                if let Some(u) = app.users.get(app.selected_user_index) {
                                                    let uname = u.name.clone();
                                                    let pgid = u.primary_gid;
                                                    let groups_for_user: Vec<sys::SystemGroup> = app
                                                        .groups
                                                        .iter()
                                                        .filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &uname))
                                                        .cloned()
                                                        .collect();
                                                    if let Some(sel_group) = groups_for_user.get(app.selected_group_index) {
                                                        // set global selected_group_index to match this group in the main groups list
                                                        if let Some(idx) = app.groups.iter().position(|g| g.gid == sel_group.gid) {
                                                            app.selected_group_index = idx;
                                                        }
                                                        app.modal = Some(ModalState::GroupsActions { selected: 0, target_gid: Some(sel_group.gid) });
                                                        app.input_mode = InputMode::Modal;
                                                    }
                                                }
                                            } else {
                                                app.modal = Some(ModalState::Actions { selected: 0 });
                                                app.input_mode = InputMode::Modal;
                                            }
                                        }
                                    }
                                    ActiveTab::Groups => {
                                        if !app.groups.is_empty() {
                                            app.modal = Some(ModalState::GroupsActions { selected: 0, target_gid: None });
                                            app.input_mode = InputMode::Modal;
                                        }
                                    }
                                }
                            }
                            KeyCode::Up | KeyCode::Char('k') => match app.active_tab {
                                ActiveTab::Users => {
                                    match app.users_focus {
                                        UsersFocus::UsersList => { if app.selected_user_index > 0 { app.selected_user_index -= 1; } }
                                        UsersFocus::MemberOf => {
                                            if app.selected_group_index > 0 { app.selected_group_index -= 1; }
                                        }
                                    }
                                }
                                ActiveTab::Groups => { if app.selected_group_index > 0 { app.selected_group_index -= 1; } }
                            },
                            KeyCode::Down | KeyCode::Char('j') => match app.active_tab {
                                ActiveTab::Users => {
                                    match app.users_focus {
                                        UsersFocus::UsersList => { if app.selected_user_index + 1 < app.users.len() { app.selected_user_index += 1; } }
                                        UsersFocus::MemberOf => {
                                            // Compute member-of count for current user
                                            let groups_len = if let Some(u) = app.users.get(app.selected_user_index) {
                                                let name = u.name.clone();
                                                let pgid = u.primary_gid;
                                                app.groups.iter().filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &name)).count()
                                            } else { 0 };
                                            if app.selected_group_index + 1 < groups_len { app.selected_group_index += 1; }
                                        }
                                    }
                                }
                                ActiveTab::Groups => { if app.selected_group_index + 1 < app.groups.len() { app.selected_group_index += 1; } }
                            },
                            KeyCode::Left | KeyCode::Char('h') => {
                                let rpp = app.rows_per_page.max(1);
                                match app.active_tab {
                                    ActiveTab::Users => match app.users_focus {
                                        UsersFocus::UsersList => { if app.selected_user_index >= rpp { app.selected_user_index -= rpp; } else { app.selected_user_index = 0; } }
                                        UsersFocus::MemberOf => { if app.selected_group_index >= rpp { app.selected_group_index -= rpp; } else { app.selected_group_index = 0; } }
                                    },
                                    ActiveTab::Groups => {
                                        if app.selected_group_index >= rpp { app.selected_group_index -= rpp; } else { app.selected_group_index = 0; }
                                    }
                                }
                            }
                            KeyCode::Right | KeyCode::Char('l') => {
                                let rpp = app.rows_per_page.max(1);
                                match app.active_tab {
                                    ActiveTab::Users => match app.users_focus {
                                        UsersFocus::UsersList => {
                                            let new_idx = app.selected_user_index.saturating_add(rpp);
                                            app.selected_user_index = new_idx.min(app.users.len().saturating_sub(1));
                                        }
                                        UsersFocus::MemberOf => {
                                            let groups_len = if let Some(u) = app.users.get(app.selected_user_index) {
                                                let name = u.name.clone();
                                                let pgid = u.primary_gid;
                                                app.groups.iter().filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &name)).count()
                                            } else { 0 };
                                            let new_idx = app.selected_group_index.saturating_add(rpp);
                                            app.selected_group_index = new_idx.min(groups_len.saturating_sub(1));
                                        }
                                    },
                                    ActiveTab::Groups => {
                                        let new_idx = app.selected_group_index.saturating_add(rpp);
                                        app.selected_group_index = new_idx.min(app.groups.len().saturating_sub(1));
                                    }
                                }
                            }
                            _ => {}
                        },
                        InputMode::Modal => {
                            handle_modal_key(&mut app, key.code);
                        }
                        InputMode::SearchUsers | InputMode::SearchGroups => match key.code {
                            KeyCode::Enter => {
                                apply_search(&mut app);
                                app.input_mode = InputMode::Normal;
                            }
                            KeyCode::Esc => {
                                app.input_mode = InputMode::Normal;
                                app.search_query.clear();
                            }
                            KeyCode::Backspace => { app.search_query.pop(); }
                            KeyCode::Char(c) => { app.search_query.push(c); }
                            _ => {}
                        },
                    }
                }
            }
        }

        // placeholder to avoid tight loop
        let _uptime = app.started_at.elapsed();
    }

    Ok(())
}

#[derive(Clone, Debug)]
enum ModalState {
    Actions { selected: usize },
    ModifyMenu { selected: usize },
    ModifyGroupsAdd { selected: usize, offset: usize },
    ModifyGroupsRemove { selected: usize, offset: usize },
    ModifyDetailsMenu { selected: usize },
    ModifyShell { selected: usize, offset: usize, shells: Vec<String> },
    ModifyTextInput { field: ModifyField, value: String },
    DeleteConfirm { selected: usize, allowed: bool },
    Info { message: String },
    // Groups tab modals
    GroupsActions { selected: usize, target_gid: Option<u32> },
    GroupAddInput { name: String },
    GroupDeleteConfirm { selected: usize },
    GroupModifyMenu { selected: usize, target_gid: Option<u32> },
    GroupModifyAddMembers { selected: usize, offset: usize, target_gid: Option<u32> },
    GroupModifyRemoveMembers { selected: usize, offset: usize, target_gid: Option<u32> },
}

#[derive(Clone, Debug)]
enum ModifyField { Username, Fullname }

fn handle_modal_key(app: &mut AppState, code: KeyCode) {
    match &mut app.modal {
        Some(ModalState::Actions { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => { // Modify
                            app.modal = Some(ModalState::ModifyMenu { selected: 0 });
                        }
                        1 => { // Delete
                            if let Some(user) = app.users.get(app.selected_user_index) {
                                let allowed = user.uid >= 1000 && user.uid <= 1999;
                                if allowed {
                                    app.modal = Some(ModalState::DeleteConfirm { selected: 1, allowed }); // default to No
                                } else {
                                    app.modal = Some(ModalState::Info { message: format!("Deletion not allowed. Only UID 1000-1999 allowed: {}", user.name) });
                                }
                            } else {
                                close_modal(app);
                            }
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyMenu { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::ModifyGroupsAdd { selected: 0, offset: 0 }),
                        1 => app.modal = Some(ModalState::ModifyGroupsRemove { selected: 0, offset: 0 }),
                        2 => app.modal = Some(ModalState::ModifyDetailsMenu { selected: 0 }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyGroupsAdd { selected, offset }) => {
            let total = app.groups_all.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 { *selected -= 1; }
                    if *selected < *offset { *offset = *selected; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total { *selected += 1; }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step { *selected -= step; } else { *selected = 0; }
                    if *selected < *offset { *offset = *selected; }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Enter => {
                    let group_name = app.groups_all.get(*selected).map(|g| g.name.clone());
                    if let (Some(user), Some(group_name)) = (app.users.get(app.selected_user_index), group_name) {
                        let adapter = crate::sys::SystemAdapter::new();
                        match adapter.add_user_to_group(&user.name, &group_name) {
                            Ok(_) => {
                                // refresh group data
                                app.groups_all = adapter.list_groups().unwrap_or_default();
                                app.groups = app.groups_all.clone();
                                app.modal = Some(ModalState::Info { message: format!("Added '{}' to group '{}'", user.name, group_name) });
                            }
                            Err(e) => {
                                app.modal = Some(ModalState::Info { message: format!("Failed to add: {}", e) });
                            }
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyGroupsRemove { selected, offset }) => {
            // Build list of groups the user currently belongs to
            let (username, primary_gid) = if let Some(u) = app.users.get(app.selected_user_index) { (u.name.clone(), u.primary_gid) } else { (String::new(), 0) };
            let user_groups: Vec<sys::SystemGroup> = app.groups_all.iter().filter(|g| g.gid == primary_gid || g.members.iter().any(|m| m == &username)).cloned().collect();
            let total = user_groups.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => {
                    if *selected > 0 { *selected -= 1; }
                    if *selected < *offset { *offset = *selected; }
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    if *selected + 1 < total { *selected += 1; }
                }
                KeyCode::PageUp => {
                    let step = 10usize;
                    if *selected >= step { *selected -= step; } else { *selected = 0; }
                    if *selected < *offset { *offset = *selected; }
                }
                KeyCode::PageDown => {
                    let step = 10usize;
                    *selected = (*selected + step).min(total.saturating_sub(1));
                }
                KeyCode::Enter => {
                    if let (Some(user), Some(group)) = (app.users.get(app.selected_user_index), user_groups.get(*selected)) {
                        // Don't attempt to remove from primary group
                        if group.gid == user.primary_gid {
                            app.modal = Some(ModalState::Info { message: "Cannot remove user from primary group.".to_string() });
                        } else {
                            let adapter = crate::sys::SystemAdapter::new();
                            match adapter.remove_user_from_group(&user.name, &group.name) {
                                Ok(_) => {
                                    app.groups_all = adapter.list_groups().unwrap_or_default();
                                    app.groups = app.groups_all.clone();
                                    app.modal = Some(ModalState::Info { message: format!("Removed '{}' from group '{}'", user.name, group.name) });
                                }
                                Err(e) => {
                                    app.modal = Some(ModalState::Info { message: format!("Failed to remove: {}", e) });
                                }
                            }
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyDetailsMenu { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::ModifyTextInput { field: ModifyField::Username, value: String::new() }),
                        1 => app.modal = Some(ModalState::ModifyTextInput { field: ModifyField::Fullname, value: String::new() }),
                        2 => {
                            let adapter = crate::sys::SystemAdapter::new();
                            let shells = adapter.list_shells().unwrap_or_default();
                            app.modal = Some(ModalState::ModifyShell { selected: 0, offset: 0, shells });
                        }
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyShell { selected, offset, shells }) => {
            let total = shells.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } },
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } },
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); },
                KeyCode::Enter => {
                    if let (Some(user), Some(new_shell)) = (app.users.get(app.selected_user_index), shells.get(*selected)) {
                        let adapter = crate::sys::SystemAdapter::new();
                        match adapter.change_user_shell(&user.name, new_shell) {
                            Ok(_) => {
                                // refresh users list
                                app.users_all = adapter.list_users().unwrap_or_default();
                                app.users_all.sort_by_key(|u| u.uid);
                                app.users = app.users_all.clone();
                                app.modal = Some(ModalState::Info { message: format!("Changed shell to '{}'", new_shell) });
                            }
                            Err(e) => {
                                app.modal = Some(ModalState::Info { message: format!("Failed to change shell: {}", e) });
                            }
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::ModifyTextInput { field, value }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Enter => {
                    if let Some(user) = app.users.get(app.selected_user_index) {
                        let adapter = crate::sys::SystemAdapter::new();
                        let res = match field { ModifyField::Username => adapter.change_username(&user.name, value), ModifyField::Fullname => adapter.change_user_fullname(&user.name, value) };
                        match res {
                            Ok(_) => {
                                app.users_all = adapter.list_users().unwrap_or_default();
                                app.users_all.sort_by_key(|u| u.uid);
                                app.users = app.users_all.clone();
                                app.modal = Some(ModalState::Info { message: "Changed successfully".to_string() });
                            }
                            Err(e) => { app.modal = Some(ModalState::Info { message: format!("Failed to change: {}", e) }); }
                        }
                    } else { close_modal(app); }
                }
                KeyCode::Backspace => { value.pop(); }
                KeyCode::Char(c) => { value.push(c); }
                _ => {}
            }
        }
        Some(ModalState::DeleteConfirm { selected, allowed }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Left | KeyCode::Right => { *selected = if *selected == 0 { 1 } else { 0 }; },
                KeyCode::Enter => {
                    if *selected == 0 {
                        if *allowed {
                            // Deletion action not implemented; show info and close
                            if let Some(user) = app.users.get(app.selected_user_index) {
                                app.modal = Some(ModalState::Info { message: format!("Would delete user '{}'(uid {}). Not implemented.", user.name, user.uid) });
                            } else {
                                close_modal(app);
                            }
                        } else {
                            app.modal = Some(ModalState::Info { message: "Deletion not allowed.".to_string() });
                        }
                    } else {
                        close_modal(app);
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::Info { .. }) => {
            match code {
                KeyCode::Esc | KeyCode::Enter => close_modal(app),
                _ => {}
            }
        }
        Some(ModalState::GroupsActions { selected, target_gid }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 2 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::GroupAddInput { name: String::new() }),
                        1 => app.modal = Some(ModalState::GroupDeleteConfirm { selected: 1 }),
                        2 => app.modal = Some(ModalState::GroupModifyMenu { selected: 0, target_gid: *target_gid }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupAddInput { name }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Enter => {
                    let adapter = crate::sys::SystemAdapter::new();
                    match adapter.create_group(&name) {
                        Ok(_) => {
                            app.groups_all = adapter.list_groups().unwrap_or_default();
                            app.groups = app.groups_all.clone();
                            app.modal = Some(ModalState::Info { message: format!("Created group '{}'", name) });
                        }
                        Err(e) => app.modal = Some(ModalState::Info { message: format!("Failed to create group: {}", e) }),
                    }
                }
                KeyCode::Backspace => { name.pop(); }
                KeyCode::Char(c) => { name.push(c); }
                _ => {}
            }
        }
        Some(ModalState::GroupDeleteConfirm { selected }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Left | KeyCode::Right => { *selected = if *selected == 0 { 1 } else { 0 }; },
                KeyCode::Enter => {
                    if *selected == 0 {
                        let group_name_opt = app.groups.get(app.selected_group_index).map(|g| g.name.clone());
                        if let Some(group_name) = group_name_opt {
                            let adapter = crate::sys::SystemAdapter::new();
                            match adapter.delete_group(&group_name) {
                                Ok(_) => {
                                    app.groups_all = adapter.list_groups().unwrap_or_default();
                                    app.groups = app.groups_all.clone();
                                    app.modal = Some(ModalState::Info { message: format!("Deleted group '{}'", group_name) });
                                }
                                Err(e) => app.modal = Some(ModalState::Info { message: format!("Failed to delete group: {}", e) }),
                            }
                        } else { close_modal(app); }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyMenu { selected, target_gid }) => {
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected < 1 { *selected += 1; } },
                KeyCode::Enter => {
                    match *selected {
                        0 => app.modal = Some(ModalState::GroupModifyAddMembers { selected: 0, offset: 0, target_gid: *target_gid }),
                        1 => app.modal = Some(ModalState::GroupModifyRemoveMembers { selected: 0, offset: 0, target_gid: *target_gid }),
                        _ => {}
                    }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyAddMembers { selected, offset, target_gid }) => {
            let total = app.users_all.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } },
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } },
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); },
                KeyCode::Enter => {
                    let group_name = if let Some(gid) = *target_gid {
                        app.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone())
                    } else {
                        app.groups.get(app.selected_group_index).map(|g| g.name.clone())
                    };
                    let user_name = app.users_all.get(*selected).map(|u| u.name.clone());
                    if let (Some(group_name), Some(user_name)) = (group_name, user_name) {
                        let adapter = crate::sys::SystemAdapter::new();
                        match adapter.add_user_to_group(&user_name, &group_name) {
                            Ok(_) => {
                                app.groups_all = adapter.list_groups().unwrap_or_default();
                                app.groups = app.groups_all.clone();
                                app.modal = Some(ModalState::Info { message: format!("Added '{}' to '{}'", user_name, group_name) });
                            }
                            Err(e) => app.modal = Some(ModalState::Info { message: format!("Failed to add: {}", e) }),
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        Some(ModalState::GroupModifyRemoveMembers { selected, offset, target_gid }) => {
            let group_name = if let Some(gid) = *target_gid { app.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone()).unwrap_or_default() } else { app.groups.get(app.selected_group_index).map(|g| g.name.clone()).unwrap_or_default() };
            let members: Vec<String> = if let Some(gid) = *target_gid { app.groups.iter().find(|g| g.gid == gid).map(|g| g.members.clone()).unwrap_or_default() } else { app.groups.get(app.selected_group_index).map(|g| g.members.clone()).unwrap_or_default() };
            let total = members.len();
            match code {
                KeyCode::Esc => close_modal(app),
                KeyCode::Up | KeyCode::Char('k') => { if *selected > 0 { *selected -= 1; } if *selected < *offset { *offset = *selected; } },
                KeyCode::Down | KeyCode::Char('j') => { if *selected + 1 < total { *selected += 1; } },
                KeyCode::PageUp => { let step = 10usize; if *selected >= step { *selected -= step; } else { *selected = 0; } if *selected < *offset { *offset = *selected; } },
                KeyCode::PageDown => { let step = 10usize; *selected = (*selected + step).min(total.saturating_sub(1)); },
                KeyCode::Enter => {
                    if let Some(username) = members.get(*selected) {
                        let adapter = crate::sys::SystemAdapter::new();
                        let gname_opt = if let Some(gid) = *target_gid { app.groups.iter().find(|g| g.gid == gid).map(|g| g.name.clone()) } else { Some(group_name.clone()) };
                        if let Some(group_name) = gname_opt {
                            match adapter.remove_user_from_group(username, &group_name) {
                                Ok(_) => {
                                    app.groups_all = adapter.list_groups().unwrap_or_default();
                                    app.groups = app.groups_all.clone();
                                    app.modal = Some(ModalState::Info { message: format!("Removed '{}' from '{}'", username, group_name) });
                                }
                                Err(e) => app.modal = Some(ModalState::Info { message: format!("Failed to remove: {}", e) }),
                            }
                        }
                    } else { close_modal(app); }
                }
                _ => {}
            }
        }
        None => {}
    }
}

fn close_modal(app: &mut AppState) {
    app.modal = None;
    app.input_mode = InputMode::Normal;
}

fn render_modal(f: &mut Frame, area: Rect, app: &mut AppState) {
    if let Some(state) = &app.modal {
        match state.clone() {
            ModalState::Actions { selected } => {
                let width = 30u16;
                let height = 7u16;
                let rect = centered_rect(width, height, area);
                let items = vec![
                    ListItem::new("Modify"),
                    ListItem::new("Delete"),
                ];
                let list = List::new(items)
                    .block(Block::default().title("Actions").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)))
                    .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD))
                    .highlight_symbol("▶ ");
                f.render_widget(Clear, rect);
                f.render_widget(list, rect);
                // draw selection cursor via Paragraph overlay if needed? List highlight requires state; emulate by drawing selected marker
                // We will re-render with manual text to show selection
                let options = ["Modify", "Delete"]; 
                let mut text = String::new();
                for (idx, label) in options.iter().enumerate() {
                    if idx == selected { text.push_str(&format!("▶ {}\n", label)); } else { text.push_str(&format!("  {}\n", label)); }
                }
                let p = Paragraph::new(text).block(Block::default().title("Actions").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::ModifyMenu { selected } => {
                let width = 34u16;
                let height = 7u16;
                let rect = centered_rect(width, height, area);
                let options = ["Add group", "Remove group", "Change details"];
                let mut text = String::new();
                for (idx, label) in options.iter().enumerate() {
                    if idx == selected { text.push_str(&format!("▶ {}\n", label)); } else { text.push_str(&format!("  {}\n", label)); }
                }
                let p = Paragraph::new(text)
                    .block(Block::default().title("Modify").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::ModifyDetailsMenu { selected } => {
                let width = 34u16;
                let height = 8u16;
                let rect = centered_rect(width, height, area);
                let options = ["Username", "Fullname", "Shell"];
                let mut text = String::new();
                for (idx, label) in options.iter().enumerate() {
                    if idx == selected { text.push_str(&format!("▶ {}\n", label)); } else { text.push_str(&format!("  {}\n", label)); }
                }
                let p = Paragraph::new(text)
                    .block(Block::default().title("Change details").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::ModifyShell { selected, offset, shells } => {
                let width = (area.width.saturating_sub(10)).min(60).max(40);
                let height = (area.height.saturating_sub(6)).min(20).max(8);
                let rect = centered_rect(width, height, area);
                let visible_capacity = rect.height.saturating_sub(2) as usize;
                let start = offset.min(shells.len());
                let end = (start + visible_capacity).min(shells.len());
                let slice = &shells[start..end];
                let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
                for (i, sh) in slice.iter().enumerate() {
                    let abs_index = start + i;
                    let marker = if abs_index == selected { "▶ " } else { "  " };
                    items.push(ListItem::new(format!("{}{}", marker, sh)));
                }
                let list = List::new(items)
                    .block(Block::default().title("Select shell").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)))
                    .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                f.render_widget(Clear, rect);
                f.render_widget(list, rect);
            }
            ModalState::ModifyTextInput { field, value } => {
                let width = 50u16;
                let height = 7u16;
                let rect = centered_rect(width, height, area);
                let title = match field { ModifyField::Username => "Change username", ModifyField::Fullname => "Change full name" };
                let msg = format!("{}:\n{}", title, value);
                let p = Paragraph::new(msg)
                    .block(Block::default().title("Input").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::ModifyGroupsAdd { selected, offset } => {
                let width = (area.width.saturating_sub(10)).min(60).max(40);
                let height = (area.height.saturating_sub(6)).min(20).max(8);
                let rect = centered_rect(width, height, area);
                let visible_capacity = rect.height.saturating_sub(2) as usize; // minus borders
                let mut off = offset;
                if selected < off { off = selected; }
                if selected >= off.saturating_add(visible_capacity) {
                    off = selected + 1 - visible_capacity;
                }
                let start = off.min(app.groups_all.len());
                let end = (start + visible_capacity).min(app.groups_all.len());
                let slice = &app.groups_all[start..end];
                let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
                for (i, g) in slice.iter().enumerate() {
                    let abs_index = start + i;
                    let marker = if abs_index == selected { "▶ " } else { "  " };
                    items.push(ListItem::new(format!("{}{} ({})", marker, g.name, g.gid)));
                }
                let list = List::new(items)
                    .block(Block::default().title("Add to group").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)))
                    .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                f.render_widget(Clear, rect);
                f.render_widget(list, rect);
            }
            ModalState::ModifyGroupsRemove { selected, offset } => {
                // Build list of groups the user currently belongs to
                let (username, primary_gid) = if let Some(u) = app.users.get(app.selected_user_index) { (u.name.clone(), u.primary_gid) } else { (String::new(), 0) };
                let user_groups: Vec<sys::SystemGroup> = app.groups_all.iter().filter(|g| g.gid == primary_gid || g.members.iter().any(|m| m == &username)).cloned().collect();
                let width = (area.width.saturating_sub(10)).min(60).max(40);
                let height = (area.height.saturating_sub(6)).min(20).max(8);
                let rect = centered_rect(width, height, area);
                let visible_capacity = rect.height.saturating_sub(2) as usize;
                let mut off = offset;
                if selected < off { off = selected; }
                if selected >= off.saturating_add(visible_capacity) {
                    off = selected + 1 - visible_capacity;
                }
                let start = off.min(user_groups.len());
                let end = (start + visible_capacity).min(user_groups.len());
                let slice = &user_groups[start..end];
                let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
                for (i, g) in slice.iter().enumerate() {
                    let abs_index = start + i;
                    let marker = if abs_index == selected { "▶ " } else { "  " };
                    items.push(ListItem::new(format!("{}{} ({})", marker, g.name, g.gid)));
                }
                let list = List::new(items)
                    .block(Block::default().title("Remove from group").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)))
                    .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                f.render_widget(Clear, rect);
                f.render_widget(list, rect);
            }
            ModalState::DeleteConfirm { selected, allowed } => {
                let width = 50u16;
                let height = 7u16;
                let rect = centered_rect(width, height, area);
                let (name, uid) = if let Some(u) = app.users.get(app.selected_user_index) { (u.name.clone(), u.uid) } else { (String::new(), 0) };
                let mut body = format!("Delete user '{name}' (uid {uid})?\n\n");
                if allowed {
                    let yes = if selected == 0 { "[Yes]" } else { " Yes " };
                    let no = if selected == 1 { "[No]" } else { " No  " };
                    body.push_str(&format!("  {}    {}", yes, no));
                } else {
                    body.push_str("Deletion not allowed (only UID 1000-1999 allowed). Press Esc.");
                }
                let p = Paragraph::new(body)
                    .block(Block::default().title("Confirm delete").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::Info { message } => {
                let width = (message.len() as u16 + 10).min(area.width - 4).max(30);
                let height = 5u16;
                let rect = centered_rect(width, height, area);
                let p = Paragraph::new(message)
                    .block(Block::default().title("Info").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::GroupsActions { selected, target_gid } => {
                let width = 36u16;
                let height = 8u16;
                let rect = centered_rect(width, height, area);
                let options = ["Add group", "Remove group", "Modify group (members)"];
                let mut text = String::new();
                for (idx, label) in options.iter().enumerate() {
                    if idx == selected { text.push_str(&format!("▶ {}\n", label)); } else { text.push_str(&format!("  {}\n", label)); }
                }
                let p = Paragraph::new(text)
                    .block(Block::default().title("Group actions").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::GroupAddInput { name } => {
                let width = 48u16;
                let height = 7u16;
                let rect = centered_rect(width, height, area);
                let msg = format!("New group name:\n{}", name);
                let p = Paragraph::new(msg)
                    .block(Block::default().title("Create group").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::GroupDeleteConfirm { selected } => {
                let width = 50u16;
                let height = 7u16;
                let rect = centered_rect(width, height, area);
                let name = app.groups.get(app.selected_group_index).map(|g| g.name.clone()).unwrap_or_default();
                let mut body = format!("Delete group '{}' ?\n\n", name);
                let yes = if selected == 0 { "[Yes]" } else { " Yes " };
                let no = if selected == 1 { "[No]" } else { " No  " };
                body.push_str(&format!("  {}    {}", yes, no));
                let p = Paragraph::new(body)
                    .block(Block::default().title("Confirm delete").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::GroupModifyMenu { selected, target_gid } => {
                let width = 40u16;
                let height = 8u16;
                let rect = centered_rect(width, height, area);
                let options = ["Add member", "Remove member"];
                let mut text = String::new();
                for (idx, label) in options.iter().enumerate() {
                    if idx == selected { text.push_str(&format!("▶ {}\n", label)); } else { text.push_str(&format!("  {}\n", label)); }
                }
                let p = Paragraph::new(text)
                    .block(Block::default().title("Modify group").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)));
                f.render_widget(Clear, rect);
                f.render_widget(p, rect);
            }
            ModalState::GroupModifyAddMembers { selected, offset, target_gid } => {
                let users = &app.users_all;
                let width = (area.width.saturating_sub(10)).min(60).max(40);
                let height = (area.height.saturating_sub(6)).min(20).max(8);
                let rect = centered_rect(width, height, area);
                let visible_capacity = rect.height.saturating_sub(2) as usize;
                let start = offset.min(users.len());
                let end = (start + visible_capacity).min(users.len());
                let slice = &users[start..end];
                let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
                for (i, u) in slice.iter().enumerate() {
                    let abs_index = start + i;
                    let marker = if abs_index == selected { "▶ " } else { "  " };
                    items.push(ListItem::new(format!("{}{} ({})", marker, u.name, u.uid)));
                }
                let list = List::new(items)
                    .block(Block::default().title("Add member to group").borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)))
                    .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                f.render_widget(Clear, rect);
                f.render_widget(list, rect);
            }
            ModalState::GroupModifyRemoveMembers { selected, offset, target_gid } => {
                let name = app.groups.get(app.selected_group_index).map(|g| g.name.clone()).unwrap_or_default();
                let members = app.groups.get(app.selected_group_index).map(|g| g.members.clone()).unwrap_or_default();
                let width = (area.width.saturating_sub(10)).min(60).max(40);
                let height = (area.height.saturating_sub(6)).min(20).max(8);
                let rect = centered_rect(width, height, area);
                let visible_capacity = rect.height.saturating_sub(2) as usize;
                let start = offset.min(members.len());
                let end = (start + visible_capacity).min(members.len());
                let slice = &members[start..end];
                let mut items: Vec<ListItem> = Vec::with_capacity(slice.len());
                for (i, m) in slice.iter().enumerate() {
                    let abs_index = start + i;
                    let marker = if abs_index == selected { "▶ " } else { "  " };
                    items.push(ListItem::new(format!("{}{}", marker, m)));
                }
                let list = List::new(items)
                    .block(Block::default().title(format!("Remove member from '{}'", name)).borders(Borders::ALL).border_style(Style::default().fg(app.theme.border)))
                    .highlight_style(Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD));
                f.render_widget(Clear, rect);
                f.render_widget(list, rect);
            }
        }
    }
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect { x, y, width: width.min(area.width), height: height.min(area.height) }
}

fn render_users_table(f: &mut Frame, area: Rect, app: &mut AppState) {
    // compute rows_per_page based on area height (minus header)
    let body_height = area.height.saturating_sub(3) as usize; // rough: borders+header
    if body_height > 0 { app.rows_per_page = body_height; }

    let start = (app.selected_user_index / app.rows_per_page) * app.rows_per_page;
    let end = (start + app.rows_per_page).min(app.users.len());
    let slice = &app.users[start..end];

    let rows = slice.iter().enumerate().map(|(i, u)| {
        let absolute_index = start + i;
        let style = if absolute_index == app.selected_user_index {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(u.uid.to_string()),
            Cell::from(u.name.clone()),
            Cell::from(u.primary_gid.to_string()),
            Cell::from(u.home_dir.clone()),
            Cell::from(u.shell.clone()),
        ]).style(style)
    });

    let widths = [
        Constraint::Length(8),
        Constraint::Length(24),
        Constraint::Length(8),
        Constraint::Percentage(40),
        Constraint::Percentage(40),
    ];

    let header = Row::new(vec!["UID", "USER", "GID", "HOME", "SHELL"]).style(
        Style::default().fg(app.theme.title).add_modifier(Modifier::BOLD),
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title("Users")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .row_highlight_style(Style::default().fg(app.theme.highlight_fg).bg(app.theme.highlight_bg).add_modifier(Modifier::REVERSED))
        .column_spacing(1);

    f.render_widget(table, area);
}

#[allow(dead_code)]
fn render_groups_table(f: &mut Frame, area: Rect, app: &mut AppState) {
    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 { app.rows_per_page = body_height; }

    let start = (app.selected_group_index / app.rows_per_page) * app.rows_per_page;
    let end = (start + app.rows_per_page).min(app.groups.len());
    let slice = &app.groups[start..end];

    let rows = slice.iter().enumerate().map(|(i, g)| {
        let absolute_index = start + i;
        let style = if absolute_index == app.selected_group_index {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(g.gid.to_string()),
            Cell::from(g.name.clone()),
        ]).style(style)
    });

    let widths = [Constraint::Length(8), Constraint::Percentage(100)];
    let header = Row::new(vec!["GID", "GROUP"]).style(
        Style::default().fg(app.theme.title).add_modifier(Modifier::BOLD),
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title("Groups")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .row_highlight_style(Style::default().fg(app.theme.highlight_fg).bg(app.theme.highlight_bg).add_modifier(Modifier::REVERSED))
        .column_spacing(1);

    f.render_widget(table, area);
}

fn render_group_details(f: &mut Frame, area: Rect, app: &AppState) {
    let group = app.groups.get(app.selected_group_index);
    let (name, gid, members) = match group {
        Some(g) => (g.name.clone(), g.gid, g.members.len()),
        None => (String::new(), 0, 0),
    };
    let text = format!("Group: {name}\nGID: {gid}\nMembers: {members}");
    let p = Paragraph::new(text).style(Style::default().fg(app.theme.text)).block(
        Block::default()
            .title("Group Details")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    );
    f.render_widget(p, area);
}

fn render_group_members(f: &mut Frame, area: Rect, app: &mut AppState) {
    let members = app
        .groups
        .get(app.selected_group_index)
        .map(|g| g.members.clone())
        .unwrap_or_default();

    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 { app.rows_per_page = body_height; }
    let start = 0;
    let end = members.len().min(app.rows_per_page);
    let slice = &members[start..end];

    let rows = slice.iter().map(|m| {
        Row::new(vec![Cell::from(m.clone())]).style(Style::default())
    });

    let widths = [Constraint::Percentage(100)];
    let header = Row::new(vec!["Members"]).style(
        Style::default().fg(app.theme.title).add_modifier(Modifier::BOLD),
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title("Group Members")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .column_spacing(1);
    f.render_widget(table, area);
}

fn render_user_details(f: &mut Frame, area: Rect, app: &AppState) {
    let user = app.users.get(app.selected_user_index);
    let (username, fullname, uid, gid, home, shell) = match user {
        Some(u) => (
            u.name.clone(),
            u.full_name.clone().unwrap_or_default(),
            u.uid,
            u.primary_gid,
            u.home_dir.clone(),
            u.shell.clone(),
        ),
        None => (String::new(), String::new(), 0, 0, String::new(), String::new()),
    };

    let text = format!(
        "Username: {username}\nFullname: {fullname}\nUID: {uid}\nGID: {gid}\nHome directory: {home}\nShell: {shell}"
    );
    let p = Paragraph::new(text).style(Style::default().fg(app.theme.text)).block(
        Block::default()
            .title("Details")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border)),
    );
    f.render_widget(p, area);
}

fn render_user_groups(f: &mut Frame, area: Rect, app: &mut AppState) {
    // Filter groups for selected user
    let groups = if let Some(u) = app.users.get(app.selected_user_index) {
        let name = u.name.clone();
        let pgid = u.primary_gid;
        app.groups
            .iter()
            .filter(|g| g.gid == pgid || g.members.iter().any(|m| m == &name))
            .cloned()
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };

    // Clamp selection to current list
    if !groups.is_empty() {
        if app.selected_group_index >= groups.len() {
            app.selected_group_index = groups.len() - 1;
        }
    } else {
        app.selected_group_index = 0;
    }

    // paging
    let body_height = area.height.saturating_sub(3) as usize;
    if body_height > 0 { app.rows_per_page = body_height; }
    let start = (app.selected_group_index / app.rows_per_page) * app.rows_per_page;
    let end = (start + app.rows_per_page).min(groups.len());
    let slice = &groups[start..end];

    let rows = slice.iter().enumerate().map(|(i, g)| {
        let absolute_index = start + i;
        let style = if absolute_index == app.selected_group_index {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default()
        };
        Row::new(vec![
            Cell::from(g.gid.to_string()),
            Cell::from(g.name.clone()),
        ]).style(style)
    });

    let widths = [Constraint::Length(8), Constraint::Percentage(100)];
    let header = Row::new(vec!["GID", "Name"]).style(
        Style::default().fg(app.theme.title).add_modifier(Modifier::BOLD),
    );

    let table = Table::new(rows, widths)
        .header(header)
        .block(
            Block::default()
                .title("Member of")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.theme.border)),
        )
        .column_spacing(1);
    f.render_widget(table, area);
}

fn render_status_bar(f: &mut Frame, area: Rect, app: &AppState) {
    let mode = match app.input_mode {
        InputMode::Normal => "NORMAL",
        InputMode::SearchUsers => "SEARCH(users)",
        InputMode::SearchGroups => "SEARCH(groups)",
        InputMode::Modal => "MODAL",
    };
    let msg = format!(
        "mode: {mode}  users:{}  groups:{}  rows/page:{}",
        app.users.len(),
        app.groups.len(),
        app.rows_per_page
    );
    let p = Paragraph::new(msg).style(Style::default().fg(app.theme.status_fg).bg(app.theme.status_bg));
    f.render_widget(p, area);
}

fn apply_search(app: &mut AppState) {
    let q = app.search_query.to_lowercase();
    match app.input_mode {
        InputMode::SearchUsers => {
            if q.is_empty() {
                app.users = app.users_all.clone();
            } else {
                app.users = app
                    .users_all
                    .iter()
                    .filter(|u| {
                        u.name.to_lowercase().contains(&q)
                            || u.full_name.as_deref().unwrap_or("").to_lowercase().contains(&q)
                            || u.home_dir.to_lowercase().contains(&q)
                            || u.shell.to_lowercase().contains(&q)
                            || u.uid.to_string().contains(&q)
                            || u.primary_gid.to_string().contains(&q)
                    })
                    .cloned()
                    .collect();
            }
            app.selected_user_index = 0.min(app.users.len().saturating_sub(1));
        }
        InputMode::SearchGroups => {
            if q.is_empty() {
                app.groups = app.groups_all.clone();
            } else {
                app.groups = app
                    .groups_all
                    .iter()
                    .filter(|g| {
                        g.name.to_lowercase().contains(&q)
                            || g.gid.to_string().contains(&q)
                            || g.members.iter().any(|m| m.to_lowercase().contains(&q))
                    })
                    .cloned()
                    .collect();
            }
            app.selected_group_index = 0.min(app.groups.len().saturating_sub(1));
        }
        InputMode::Normal => {}
        InputMode::Modal => {}
    }
}

