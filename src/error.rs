//! Error utilities and common result types.
//!
//! Provides a crate-wide `Result` alias, a boxed error type (`DynError`),
//! and helpers to attach contextual information to errors.
//!
use std::fmt::{Display, Formatter};

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
