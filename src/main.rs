//! Binary entry point.  The binary consumes the library module tree directly.

use std::process::ExitCode;

use usrgrp_manager::{
    app,
    cli::{self, CliCommand},
    terminal::TerminalSession,
};

/// Exit status for command-line usage errors.
const USAGE_ERROR: u8 = 2;

fn main() -> ExitCode {
    // Everything that can fail on bad input happens before the terminal
    // switches to raw mode, so the shell sees plain error text.
    let arguments = match cli::parse(std::env::args_os().skip(1)) {
        Ok(CliCommand::Help) => {
            print!("{}", cli::usage());
            return ExitCode::SUCCESS;
        }
        Ok(CliCommand::Version) => {
            println!("{}", cli::version());
            return ExitCode::SUCCESS;
        }
        Ok(CliCommand::Run(arguments)) => arguments,
        Err(error) => {
            eprintln!("usrgrp-manager: {error}\n\n{}", cli::usage());
            return ExitCode::from(USAGE_ERROR);
        }
    };
    let options = match arguments.into_run_options() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("usrgrp-manager: {error}");
            return ExitCode::from(USAGE_ERROR);
        }
    };
    let mut session = match TerminalSession::enter() {
        Ok(session) => session,
        Err(error) => {
            eprintln!("terminal initialization failed: {error}");
            return ExitCode::FAILURE;
        }
    };
    let result = app::run_with(session.terminal_mut(), options);
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
