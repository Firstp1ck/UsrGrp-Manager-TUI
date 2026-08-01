//! Error utilities and common result types.
//!
//! Provides a crate-wide `Result` alias, a boxed error type (`DynError`),
//! and helpers to attach contextual information to errors.
//!
use std::{
    fmt::{Display, Formatter},
    io,
    time::Duration,
};

/// Errors at the trusted system boundary.
///
/// Variants intentionally contain only classified, non-secret information so they
/// may safely be shown in user-facing diagnostics and operation reports.
#[allow(dead_code)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CoreError {
    /// A caller must obtain elevation before a privileged operation can run.
    AuthenticationRequired,
    /// Elevation credentials were rejected.
    AuthenticationDenied,
    /// An authenticated elevation session cannot be used non-interactively.
    AuthenticationCapability,
    /// The requested system integration is unavailable on this platform.
    UnsupportedPlatform,
    /// A fixed, expected executable was not found.
    MissingExecutable { program: &'static str },
    /// A child process exceeded its execution bound and was terminated.
    Timeout {
        program: &'static str,
        timeout: Duration,
    },
    /// A child process produced more output than the configured bound.
    OutputLimit { program: &'static str, limit: usize },
    /// A command exited unsuccessfully without a more specific classification.
    ExitStatus {
        program: &'static str,
        code: Option<i32>,
    },
    /// Validated data did not meet the documented local-file contract.
    Validation {
        field: &'static str,
        reason: &'static str,
    },
    /// A data refresh failed and callers must retain their known-good snapshot.
    Refresh {
        operation: &'static str,
        kind: io::ErrorKind,
    },
    /// A system I/O operation failed.
    Io {
        operation: &'static str,
        kind: io::ErrorKind,
    },
    /// A plan completed only some of its ordered work.
    PartialCompletion { step: String },
    /// A required postcondition was not observed.
    PostconditionFailed { step: String },
}

#[allow(dead_code)]
impl CoreError {
    /// Classify an I/O error without embedding potentially sensitive OS text.
    pub fn io(operation: &'static str, error: &io::Error) -> Self {
        if error.kind() == io::ErrorKind::NotFound {
            Self::MissingExecutable { program: operation }
        } else {
            Self::Io {
                operation,
                kind: error.kind(),
            }
        }
    }

    /// Return whether this error is specifically an elevation prompt condition.
    pub const fn authentication_required(&self) -> bool {
        matches!(self, Self::AuthenticationRequired)
    }
}

impl Display for CoreError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AuthenticationRequired => write!(f, "authentication required"),
            Self::AuthenticationDenied => write!(f, "authentication denied"),
            Self::AuthenticationCapability => {
                write!(
                    f,
                    "authenticated elevation is unavailable for this operation"
                )
            }
            Self::UnsupportedPlatform => write!(f, "unsupported platform"),
            Self::MissingExecutable { program } => {
                write!(f, "required executable unavailable: {program}")
            }
            Self::Timeout { program, timeout } => {
                write!(f, "{program} timed out after {} ms", timeout.as_millis())
            }
            Self::OutputLimit { program, limit } => {
                write!(f, "{program} exceeded the {limit}-byte output limit")
            }
            Self::ExitStatus { program, code } => match code {
                Some(code) => write!(f, "{program} exited with status {code}"),
                None => write!(f, "{program} exited without a status code"),
            },
            Self::Validation { field, reason } => write!(f, "invalid {field}: {reason}"),
            Self::Refresh { operation, kind } => write!(f, "could not refresh {operation}: {kind}"),
            Self::Io { operation, kind } => write!(f, "{operation} failed: {kind}"),
            Self::PartialCompletion { step } => {
                write!(f, "operation partially completed at {step}")
            }
            Self::PostconditionFailed { step } => write!(f, "postcondition not observed: {step}"),
        }
    }
}

impl std::error::Error for CoreError {}

/// Result type used by the trusted system boundary.
pub type CoreResult<T> = std::result::Result<T, CoreError>;

/// A boxed error type that is `Send + Sync + 'static` for ergonomic error handling.
pub type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;
/// Crate-wide `Result` alias using [`DynError`].
pub type Result<T> = std::result::Result<T, DynError>;

#[allow(dead_code)]
/// Extension trait to attach lazily-evaluated context to errors.
pub trait Context<T> {
    /// Convert an error into [`DynError`] while adding a context message produced by `f`.
    fn with_ctx<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

#[allow(dead_code)]
#[derive(Debug)]
/// Error wrapper that carries a context message alongside the source error.
pub struct WithContextError {
    /// Human-readable context describing where/why the error occurred.
    pub context: String,
    /// The underlying error.
    pub source: DynError,
}

impl Display for WithContextError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.context, self.source)
    }
}

impl std::error::Error for WithContextError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&*self.source)
    }
}

impl<T, E> Context<T> for std::result::Result<T, E>
where
    E: std::error::Error + Send + Sync + 'static,
{
    /// Add context to any error type by wrapping it into [`WithContextError`].
    fn with_ctx<F: FnOnce() -> String>(self, f: F) -> Result<T> {
        self.map_err(|e| {
            Box::new(WithContextError {
                context: f(),
                source: e.into(),
            }) as DynError
        })
    }
}

#[allow(dead_code)]
/// Attach context to a `Result`, returning a crate-wide [`Result`].
pub fn with_context<T, E, F>(res: std::result::Result<T, E>, f: F) -> Result<T>
where
    E: std::error::Error + Send + Sync + 'static,
    F: FnOnce() -> String,
{
    res.map_err(|e| {
        Box::new(WithContextError {
            context: f(),
            source: e.into(),
        }) as DynError
    })
}

#[derive(Debug)]
/// Simple string error for lightweight failures.
pub struct SimpleError(pub String);

impl SimpleError {
    /// Construct a new [`SimpleError`] from any string-like message.
    pub fn new(msg: impl Into<String>) -> Self {
        Self(msg.into())
    }
}

impl std::fmt::Display for SimpleError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for SimpleError {}

/// Create a boxed [`SimpleError`] in one step.
pub fn simple_error(msg: impl Into<String>) -> DynError {
    Box::new(SimpleError::new(msg))
}
