//! Library crate for usrgrp-manager.
//!
//! This crate exposes the building blocks of the TUI:
//! - Application state and update loop (`app`)
//! - Error and result types (`error`)
//! - In-memory search helpers (`search`)
//! - System interaction layer for users/groups (`sys`)
//! - UI rendering and widgets (`ui`)
//!
//! It is used by the `usrgrp-manager` binary and by tests.
#![doc = include_str!("../README.md")]
#![deny(rustdoc::broken_intra_doc_links)]
#![cfg_attr(docsrs, feature(doc_cfg))]

pub mod app;
pub mod error;
pub mod search;
pub mod sys;
pub mod ui;

// Re-export commonly used items at the crate root for convenience
/// Convenient error and result types shared across the crate.
pub use error::{DynError, Result};
