//! What the sidebar does with the agent you picked: peek at it, or go to it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use agenttij_core::{panes, scan, Agent, PaneSnapshot};
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

    remember(current_session);
    let tab = panes::tab_of(all_panes, &agent.session, agent.pane);
    switch_session_with_focus(&agent.session, tab, Some((agent.pane, false)));
}

/// Goes back to the session we last left.
pub fn go_back(previous: &str, current_session: &str) {
    if previous == current_session {
        return;
    }
    remember(current_session);
    switch_session(Some(previous));
}

/// Records the session we are leaving, so the sidebar on the other side knows
/// the way back. It has to go through a file: the plugin over there is a
/// different instance in a different process with no memory of this one.
fn remember(session: &str) {
    let command = scan::remember_previous(session);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
}

/// Tells you an agent is blocked, when you are not looking at the sidebar.
pub fn notify(command: &[String], agent: &Agent) {
    if command.is_empty() {
        return;
    }
    let message = format!("{} needs input", agent.label());
    let mut words: Vec<&str> = command.iter().map(String::as_str).collect();
    words.push("agenttij");
    words.push(&message);
    run_command(&words, BTreeMap::new());
}

/// Puts an agent's pane in the workspace slot and parks whoever was there.
///
/// This is the alternative to a pane stack: a stack keeps every member on
/// screen as a title line, whereas replacing the slot leaves exactly one agent
/// visible. Parked ("suppressed") panes keep running — they are only off screen.
///
/// The slot is whichever terminal pane is currently visible in this tab, so the
/// arrangement repairs itself: open agents however you like, and the first swap
/// parks the extras.
pub fn solo(agent: &Agent, all_panes: &[PaneSnapshot], session: &str) {
    let target = PaneId::Terminal(agent.pane);
    let tab = get_focused_pane_info().ok().map(|(tab, _)| tab);
    let slot = tab.and_then(|tab| panes::visible_terminal(all_panes, session, tab));

    match slot {
        // Already on screen: nothing to swap, just go there.
        Some(slot) if slot == agent.pane => focus_pane_with_id(target, false, false),
        Some(slot) => replace_pane_with_existing_pane(PaneId::Terminal(slot), target, true),
        // Everything is parked, so there is no slot to take over.
        None => show_pane_with_id(target, false, true),
    }
}

/// Opens a fresh terminal in the workspace slot, parking whatever is there.
///
/// `close_replaced_pane: false` suspends the replaced pane instead of closing
/// it, and Zellij brings it back when the new pane exits — so starting an agent
/// never costs you the one you were looking at, and the slot is never empty.
///
/// Without solo mode there is no slot to manage, so this is an ordinary new
/// pane.
pub fn new_in_slot(all_panes: &[PaneSnapshot], session: &str, solo: bool) {
    let tab = get_focused_pane_info().ok().map(|(tab, _)| tab);
    let slot = tab.and_then(|tab| panes::visible_terminal(all_panes, session, tab));

    let Some(slot) = slot.filter(|_| solo) else {
        open_terminal(own_cwd());
        return;
    };

    let replaced = PaneId::Terminal(slot);
    let cwd = get_pane_cwd(replaced).unwrap_or_else(|_| own_cwd());

    // Opening in place deliberately does not move focus, so we do it ourselves:
    // a new pane you have to navigate to is not much of a shortcut.
    if let Some(opened) = open_terminal_pane_in_place_of_pane_id(replaced, cwd, false) {
        focus_pane_with_id(opened, false, false);
    }
}

fn own_cwd() -> PathBuf {
    get_plugin_ids().initial_cwd
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
///
/// Returns the pane it opened, so the sidebar can close it again.
pub fn preview(agent: &Agent) -> Option<PaneId> {
    // Arguments are passed positionally rather than interpolated, so a session
    // name with a quote or a space in it cannot break out of the script.
    //
    // A command pane cannot read the keyboard at all: Zellij gives it /dev/null
    // for stdin, and a real keypress delivered to the focused pane does not
    // reach a read on /dev/tty either (measured with scripts/press-keys.sh). So
    // the pane just redraws, and the sidebar handles dismissal — see
    // `Sidebar::reclaim_focus`.
    const POLL: &str = "while :; do \
         clear; zellij --session \"$1\" action dump-screen --pane-id \"$2\" --ansi; \
         sleep 1; \
       done";

    let command = CommandToRun {
        path: "sh".into(),
        args: vec![
            "-c".to_owned(),
            POLL.to_owned(),
            "agenttij-peek".to_owned(),
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

    // The peek takes focus whether we like it or not — and cannot use it. The
    // caller takes focus back on the next event; doing it here is too early,
    // since the open is applied afterwards and would win.
    open_command_pane_floating(command, coordinates, BTreeMap::new())
}
