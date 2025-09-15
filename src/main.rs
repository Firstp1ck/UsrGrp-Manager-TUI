use crate::error::Result;
use crossterm::execute;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

mod sys;
mod app;
mod ui;
mod search;
mod error;

fn init_terminal() -> Result<Terminal<CrosstermBackend<std::io::Stdout>>> {
    enable_raw_mode()?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

fn main() -> Result<()> {
    let mut terminal = init_terminal().map_err(|e| format!("init terminal: {}", e))?;

    let res = app::run(&mut terminal);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();
    terminal.show_cursor().ok();

    if let Err(err) = res {
        eprintln!("application error: {err}");
    }
    Ok(())
}