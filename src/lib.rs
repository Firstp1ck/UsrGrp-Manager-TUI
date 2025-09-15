// Library exports for usrgrp-manager
// This file makes the public types and functions available for testing

pub mod app;
pub mod error;
pub mod search;
pub mod sys;
pub mod ui;

// Re-export commonly used items at the crate root for convenience
pub use error::{DynError, Result};
