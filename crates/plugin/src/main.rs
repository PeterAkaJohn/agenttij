//! agenttij — a Zellij sidebar that tracks coding-agent panes across sessions.
//!
//! Zellij plugins are single-threaded WASI modules executed by the `wasmi`
//! interpreter: no threads, no reactor, and therefore no async runtime. The
//! host provides concurrency instead — anything that takes time is handed over
//! and comes back as an event (`run_command` → `RunCommandResult`,
//! `set_timeout` → `Timer`). Every method here must return promptly.

mod actions;
mod render;
mod sidebar;
mod snapshot;

use zellij_tile::prelude::*;

register_plugin!(sidebar::Sidebar);
