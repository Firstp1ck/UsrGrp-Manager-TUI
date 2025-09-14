use anyhow::{Context, Result};
use clap::{ArgAction, Parser};
use crossterm::execute;
use crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::backend::CrosstermBackend;
use ratatui::Terminal;

mod sys;
mod app;
mod ui;
mod search;

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
    let env_filter = tracing_subscriber::EnvFilter::try_new(cli.log.clone()).unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(env_filter).without_time().init();

    #[cfg(feature = "file-parse")]
    if !cli.file_parse {
        tracing::info!("feature 'file-parse' is enabled at build time; runtime flag is ignored");
    }

    let mut terminal = init_terminal().context("init terminal")?;

    let res = app::run(&mut terminal);

    disable_raw_mode().ok();
    execute!(terminal.backend_mut(), LeaveAlternateScreen, DisableMouseCapture).ok();
    terminal.show_cursor().ok();

    if let Err(err) = res {
        tracing::error!(error = ?err, "application error");
    }
    Ok(())
}