//! usrgrp-manager binary entry point.
//!
//! Initializes the terminal in raw mode, runs the TUI event loop,
//! and restores the terminal state on exit.
//!
use crate::error::Result;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

mod app;
mod error;
mod search;
mod sys;
mod ui;

/// Initialize a Crossterm-backed `ratatui` terminal in raw mode.
fn init_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

/// Program entry point: run the TUI and report any top-level error to stderr.
fn main() -> Result<()> {
    let mut terminal = init_terminal().map_err(|e| format!("init terminal: {}", e))?;

    let res = app::run(&mut terminal);

    disable_raw_mode().ok();
    execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    )
    .ok();
    terminal.show_cursor().ok();

    if let Err(err) = res {
        eprintln!("application error: {err}");
    }
    Ok(())
}
