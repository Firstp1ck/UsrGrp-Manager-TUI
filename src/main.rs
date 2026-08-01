//! Binary entry point.  The binary consumes the library module tree directly.

use std::process::ExitCode;

use usrgrp_manager::{app, terminal::TerminalSession};

fn main() -> ExitCode {
    let mut session = match TerminalSession::enter() {
        Ok(session) => session,
        Err(error) => {
            eprintln!("terminal initialization failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = app::run(session.terminal_mut());
    let cleanup = session.restore();
    if let Err(error) = result {
        eprintln!("application error: {error}");
        return ExitCode::FAILURE;
    }
    if let Err(error) = cleanup {
        eprintln!("terminal cleanup failed: {error}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}
