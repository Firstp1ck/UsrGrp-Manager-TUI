//! Hand-written command-line parsing for the binary.
//!
//! Parsing is pure so it can be tested without a terminal. Effects such as
//! opening the log file happen in [`CliArgs::into_run_options`], which the
//! binary calls before it switches the terminal into TUI mode.

use std::{ffi::OsString, fmt, io, path::PathBuf};

use crate::app::{Logger, RunOptions};

/// What the binary should do after parsing its arguments.
#[derive(Debug, PartialEq, Eq)]
pub enum CliCommand {
    /// Start the TUI with these options.
    Run(CliArgs),
    /// Print usage to stdout and exit 0.
    Help,
    /// Print the version to stdout and exit 0.
    Version,
}

/// Options for a TUI run.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CliArgs {
    pub config_dir: Option<PathBuf>,
    pub dry_run: bool,
    pub log_file: Option<PathBuf>,
}

/// A usage mistake. The binary prints it with the usage text and exits 2.
#[derive(Debug, PartialEq, Eq)]
pub enum CliError {
    Unknown(String),
    MissingValue(&'static str),
    EmptyValue(&'static str),
    Repeated(&'static str),
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown(argument) => write!(f, "unknown argument '{argument}'"),
            Self::MissingValue(flag) => write!(f, "{flag} needs a path"),
            Self::EmptyValue(flag) => write!(f, "{flag} needs a non-empty path"),
            Self::Repeated(flag) => write!(f, "{flag} was given more than once"),
        }
    }
}

impl std::error::Error for CliError {}

/// `usrgrp-manager <version>`.
pub fn version() -> String {
    format!("usrgrp-manager {}", env!("CARGO_PKG_VERSION"))
}

/// Full usage text, ending in a newline.
pub fn usage() -> String {
    format!(
        "{}
Keyboard-driven TUI for local Linux users and groups.

Usage: usrgrp-manager [OPTIONS]

Options:
      --config-dir <PATH>  Read and write theme.conf, filter.conf and keybinds.conf
                           in PATH only, instead of the default search order
      --dry-run            Show the command preview for every change, never run it
      --log-file <PATH>    Append prepared operations, results and errors to PATH
                           (created with mode 0600; passwords are never written)
  -h, --help               Print this help and exit
  -V, --version            Print the version and exit
",
        version()
    )
}

/// Parse the arguments that follow the program name.
///
/// Every argument is validated first, so a usage mistake is reported even
/// when `--help` or `--version` is also present. Otherwise the first of
/// `--help` and `--version` wins. Both `--flag <path>` and `--flag=<path>`
/// are accepted for path options.
pub fn parse<I>(arguments: I) -> Result<CliCommand, CliError>
where
    I: IntoIterator<Item = OsString>,
{
    let mut arguments = arguments.into_iter();
    let mut parsed = CliArgs::default();
    let mut early_exit = None;
    while let Some(argument) = arguments.next() {
        let Some(text) = argument.to_str() else {
            return Err(CliError::Unknown(argument.to_string_lossy().into_owned()));
        };
        let (flag, inline) = match text.split_once('=') {
            Some((flag, value)) if flag.starts_with("--") => (flag, Some(value)),
            _ => (text, None),
        };
        match (flag, inline) {
            ("-h" | "--help", None) => {
                early_exit.get_or_insert(CliCommand::Help);
            }
            ("-V" | "--version", None) => {
                early_exit.get_or_insert(CliCommand::Version);
            }
            ("--dry-run", None) => {
                if parsed.dry_run {
                    return Err(CliError::Repeated("--dry-run"));
                }
                parsed.dry_run = true;
            }
            ("--config-dir", inline) => {
                let value = path_value("--config-dir", inline, &mut arguments)?;
                set_once(&mut parsed.config_dir, "--config-dir", value)?;
            }
            ("--log-file", inline) => {
                let value = path_value("--log-file", inline, &mut arguments)?;
                set_once(&mut parsed.log_file, "--log-file", value)?;
            }
            _ => return Err(CliError::Unknown(text.to_owned())),
        }
    }
    Ok(early_exit.unwrap_or(CliCommand::Run(parsed)))
}

fn path_value(
    flag: &'static str,
    inline: Option<&str>,
    rest: &mut impl Iterator<Item = OsString>,
) -> Result<PathBuf, CliError> {
    let value = match inline {
        Some(value) => OsString::from(value),
        None => rest.next().ok_or(CliError::MissingValue(flag))?,
    };
    if value.is_empty() {
        return Err(CliError::EmptyValue(flag));
    }
    if value.to_str().is_some_and(|value| value.starts_with('-')) && inline.is_none() {
        return Err(CliError::MissingValue(flag));
    }
    Ok(PathBuf::from(value))
}

fn set_once(
    slot: &mut Option<PathBuf>,
    flag: &'static str,
    value: PathBuf,
) -> Result<(), CliError> {
    if slot.is_some() {
        return Err(CliError::Repeated(flag));
    }
    *slot = Some(value);
    Ok(())
}

impl CliArgs {
    /// Open the log file and check the config directory. Called before the
    /// terminal enters raw mode so errors reach the shell as plain text.
    pub fn into_run_options(self) -> io::Result<RunOptions> {
        if let Some(dir) = &self.config_dir
            && dir.exists()
            && !dir.is_dir()
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("--config-dir {} is not a directory", dir.display()),
            ));
        }
        let logger = match &self.log_file {
            Some(path) => Some(Logger::open(path).map_err(|error| {
                io::Error::new(
                    error.kind(),
                    format!("cannot open --log-file {}: {error}", path.display()),
                )
            })?),
            None => None,
        };
        Ok(RunOptions {
            config_dir: self.config_dir,
            dry_run: self.dry_run,
            logger,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Result<CliCommand, CliError> {
        parse(values.iter().map(OsString::from))
    }

    #[test]
    fn no_arguments_runs_the_plain_tui() {
        assert_eq!(args(&[]), Ok(CliCommand::Run(CliArgs::default())));
    }

    #[test]
    fn help_and_version_flags() {
        for flag in ["-h", "--help"] {
            assert_eq!(args(&[flag]), Ok(CliCommand::Help));
        }
        for flag in ["-V", "--version"] {
            assert_eq!(args(&[flag]), Ok(CliCommand::Version));
        }
        assert!(version().starts_with("usrgrp-manager "));
        assert!(usage().contains("--dry-run"));
    }

    #[test]
    fn usage_errors_win_over_help_and_version_in_any_order() {
        for flag in ["-h", "--help", "-V", "--version"] {
            assert_eq!(
                args(&[flag, "--bogus"]),
                Err(CliError::Unknown("--bogus".into()))
            );
            assert_eq!(
                args(&["--bogus", flag]),
                Err(CliError::Unknown("--bogus".into()))
            );
            assert_eq!(
                args(&[flag, "--log-file"]),
                Err(CliError::MissingValue("--log-file"))
            );
            assert_eq!(
                args(&[flag, "--config-dir="]),
                Err(CliError::EmptyValue("--config-dir"))
            );
            assert_eq!(
                args(&["--dry-run", flag, "--dry-run"]),
                Err(CliError::Repeated("--dry-run"))
            );
        }
        assert_eq!(args(&["--help", "--dry-run"]), Ok(CliCommand::Help));
        assert_eq!(args(&["--version", "--help"]), Ok(CliCommand::Version));
    }

    #[test]
    fn path_options_accept_separate_and_inline_values() {
        let expected = CliArgs {
            config_dir: Some(PathBuf::from("/tmp/cfg")),
            dry_run: true,
            log_file: Some(PathBuf::from("ops.log")),
        };
        assert_eq!(
            args(&[
                "--config-dir",
                "/tmp/cfg",
                "--dry-run",
                "--log-file",
                "ops.log"
            ]),
            Ok(CliCommand::Run(expected))
        );
        assert!(matches!(
            args(&["--config-dir=/tmp/cfg"]),
            Ok(CliCommand::Run(CliArgs { config_dir: Some(dir), .. })) if dir.as_path() == std::path::Path::new("/tmp/cfg")
        ));
    }

    #[test]
    fn usage_errors() {
        assert_eq!(args(&["--bogus"]), Err(CliError::Unknown("--bogus".into())));
        assert_eq!(args(&["extra"]), Err(CliError::Unknown("extra".into())));
        assert_eq!(
            args(&["--dry-run=yes"]),
            Err(CliError::Unknown("--dry-run=yes".into()))
        );
        assert_eq!(
            args(&["--log-file"]),
            Err(CliError::MissingValue("--log-file"))
        );
        assert_eq!(
            args(&["--config-dir", "--dry-run"]),
            Err(CliError::MissingValue("--config-dir"))
        );
        assert_eq!(
            args(&["--config-dir="]),
            Err(CliError::EmptyValue("--config-dir"))
        );
        assert_eq!(
            args(&["--dry-run", "--dry-run"]),
            Err(CliError::Repeated("--dry-run"))
        );
        assert_eq!(
            args(&["--log-file", "a", "--log-file", "b"]),
            Err(CliError::Repeated("--log-file"))
        );
    }

    #[test]
    fn into_run_options_opens_log_and_rejects_file_as_config_dir() {
        let dir = tempfile::tempdir().unwrap();
        let log = dir.path().join("ops.log");
        let options = CliArgs {
            config_dir: Some(dir.path().to_path_buf()),
            dry_run: true,
            log_file: Some(log.clone()),
        }
        .into_run_options()
        .unwrap();
        assert!(options.dry_run && options.logger.is_some() && log.is_file());

        let not_a_dir = CliArgs {
            config_dir: Some(log),
            ..CliArgs::default()
        };
        assert!(not_a_dir.into_run_options().is_err());
    }
}
