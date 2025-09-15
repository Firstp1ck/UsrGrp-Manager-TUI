use std::fmt::{Display, Formatter};

pub type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T> = std::result::Result<T, DynError>;

#[allow(dead_code)]
pub trait Context<T> {
    fn with_ctx<F: FnOnce() -> String>(self, f: F) -> Result<T>;
}

#[allow(dead_code)]
#[derive(Debug)]
pub struct WithContextError {
    pub context: String,
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
pub struct SimpleError(pub String);

impl SimpleError {
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

pub fn simple_error(msg: impl Into<String>) -> DynError {
    Box::new(SimpleError::new(msg))
}
