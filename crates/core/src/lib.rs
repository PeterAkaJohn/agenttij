//! Agent tracking logic for the agenttij Zellij sidebar.
//!
//! This crate has no dependencies and knows nothing about Zellij's API. The
//! plugin crate adapts Zellij's `SessionInfo` into [`panes::PaneSnapshot`] at
//! the boundary, which keeps every decision in here testable on the host with
//! a plain `cargo test`.

pub mod agent;
pub mod config;
pub mod format;
pub mod panes;
pub mod scan;

pub use agent::{Agent, Status};
pub use config::Config;
pub use panes::PaneSnapshot;
