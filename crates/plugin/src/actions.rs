//! What the sidebar does with the agent you picked: peek at it, or go to it.

use std::collections::BTreeMap;

use agenttij_core::{panes, Agent, PaneSnapshot};
use zellij_tile::prelude::*;

/// Focuses an agent's pane.
///
/// In this session that is a plain focus. Across sessions it detaches this
/// client and reattaches to the target — unavoidable, that is what switching a
/// session *is* — landing directly on the agent's pane.
/// `switch_session_with_focus` wants a tab *position*, so we resolve it from
/// the metadata we already hold.
pub fn go_to(agent: &Agent, current_session: &str, all_panes: &[PaneSnapshot]) {
    if agent.session == current_session {
        focus_pane_with_id(PaneId::Terminal(agent.pane), false, false);
        return;
    }

    let tab = panes::tab_of(all_panes, &agent.session, agent.pane);
    switch_session_with_focus(&agent.session, tab, Some((agent.pane, false)));
}

/// Repeatedly dumps an agent's pane into a floating pane, so you can check on
/// it without leaving this session.
///
/// `zellij subscribe` looks like the natural fit here and is not: it is fed by
/// the render pipeline, and `Tab::render` skips tabs that have no client
/// watching them. Previewing an agent in a background tab or an unattended
/// session — the whole point — gets you one initial snapshot and then silence.
/// `dump-screen` queries the pane directly, so it works wherever the pane is.
///
/// The cost is a poll instead of a stream: one `dump-screen` per second while
/// the preview is open, and a redraw you can see. Worth it for a preview that
/// is never quietly stale.
pub fn preview(agent: &Agent) {
    // Arguments are passed positionally rather than interpolated, so a session
    // name with a quote or a space in it cannot break out of the script.
    const POLL: &str = "while :; do clear; \
         zellij --session \"$1\" action dump-screen --pane-id \"$2\" --ansi; \
         sleep 1; done";

    let command = CommandToRun {
        path: "sh".into(),
        args: vec![
            "-c".to_owned(),
            POLL.to_owned(),
            "agenttij-preview".to_owned(),
            agent.session.clone(),
            format!("terminal_{}", agent.pane),
        ],
        cwd: None,
    };

    // Zellij's default floating pane is 40x10, which re-wraps a normal agent
    // pane into unreadable ribbon. Ask for something you can actually read.
    let coordinates = FloatingPaneCoordinates::new(
        Some("10%".to_owned()),
        Some("10%".to_owned()),
        Some("80%".to_owned()),
        Some("80%".to_owned()),
        None, // pinned
        None, // borderless
    );

    open_command_pane_floating(command, coordinates, BTreeMap::new());
}
