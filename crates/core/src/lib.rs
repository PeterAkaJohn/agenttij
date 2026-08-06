//! Agent tracking logic for the agenttij Zellij sidebar.
//!
//! This crate has no dependencies and knows nothing about Zellij's API. The
//! plugin crate adapts Zellij's `SessionInfo` into [`panes::PaneSnapshot`] at
//! the boundary, which keeps every decision in here testable on the host with
//! a plain `cargo test`.

pub mod agent;
pub mod color;
pub mod config;
pub mod format;
pub mod group;
pub mod panes;
pub mod scan;

pub use agent::{Agent, Status};
pub use color::Colors;
pub use config::Config;
pub use group::{Group, Groups};
pub use panes::PaneSnapshot;
