//! What the sidebar does with the agent you picked: peek at it, or go to it.

use std::collections::BTreeMap;
use std::path::PathBuf;

use agenttij_core::{panes, Agent, Config, PaneSnapshot};
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

/// Opens a fresh terminal in the workspace slot, parking whatever is there.
///
/// Two steps rather than one: open the pane normally, then swap it into the slot
/// with the old occupant suppressed. Opening *in place of* the slot looks
/// simpler and destroys panes — Zellij stacks suppressed panes behind a
/// replacement, and replacing a pane that is itself a replacement drops the one
/// in the middle. Measured: three panes went in, two came out.
pub fn new_in_slot(all_panes: &[PaneSnapshot], session: &str, solo: bool) -> Option<PaneId> {
    let tab = get_focused_pane_info().ok().map(|(tab, _)| tab);
    let slot = tab.and_then(|tab| panes::visible_terminal(all_panes, session, tab));

    let cwd = slot
        .and_then(|slot| get_pane_cwd(PaneId::Terminal(slot)).ok())
        .unwrap_or_else(own_cwd);

    let opened = open_terminal(cwd);
    let (Some(opened), Some(slot)) = (opened, slot.filter(|_| solo)) else {
        return opened;
    };

    // The new pane arrives as a split; this collapses it back to one on screen.
    show_in_slot_from(opened, slot);
    focus_pane_with_id(opened, false, false);
    Some(opened)
}

/// Brings a pane into the slot, parking whoever was there. This is the move
/// behind picking a row, cycling within one, and adding to one.
pub fn show_in_slot(target: u32, slot: Option<u32>) {
    match slot {
        Some(slot) if slot == target => focus_pane_with_id(PaneId::Terminal(target), false, false),
        Some(slot) => show_in_slot_from(PaneId::Terminal(target), slot),
        None => show_pane_with_id(PaneId::Terminal(target), false, true),
    }
}

fn show_in_slot_from(target: PaneId, slot: u32) {
    replace_pane_with_existing_pane(PaneId::Terminal(slot), target, true);
}

fn own_cwd() -> PathBuf {
    get_plugin_ids().initial_cwd
}

/// Opens the keybind list: this plugin again, floating, in help mode.
///
/// Same shape as a peek and for the same reason — a plugin pane can hold focus
/// and read the key that dismisses it, which no other kind of pane can.
pub fn help(own_url: &str) -> Option<PaneId> {
    let configuration = BTreeMap::from([
        ("help".to_owned(), "true".to_owned()),
        ("pane_title".to_owned(), "keys".to_owned()),
    ]);
    let coordinates = FloatingPaneCoordinates::new(
        Some("15%".to_owned()),
        Some("15%".to_owned()),
        Some("70%".to_owned()),
        Some("70%".to_owned()),
        None,
        None,
    );

    open_plugin_pane_floating(own_url, configuration, coordinates, BTreeMap::new())
}

/// Opens a peek: another instance of this plugin, floating, mirroring an
/// agent's pane once a second.
///
/// A peek has to be a plugin pane. A command pane cannot read the keyboard —
/// Zellij gives it /dev/null for stdin, and even a real keypress is not
/// readable from /dev/tty — and a floating pane is only on screen while it holds
/// focus. So a command pane peek is either invisible or undismissable. A plugin
/// pane receives keys, so it can hold focus, stay visible, and close itself.
pub fn preview(agent: &Agent, own_url: &str, config: &Config) -> Option<PaneId> {
    let mut configuration = BTreeMap::from([
        (
            "peek".to_owned(),
            format!("{}:{}", agent.session, agent.pane),
        ),
        ("pane_title".to_owned(), format!("peek {}", agent.label())),
    ]);
    // Colours are the user's, and a peek is an instance of the same plugin, so
    // carry them across rather than reverting to the defaults.
    if !config.colors_raw.is_empty() {
        configuration.insert("colors".to_owned(), config.colors_raw.clone());
    }

    // Zellij's default floating pane is 40x10, which re-wraps an agent pane into
    // unreadable ribbon. Ask for something you can actually read.
    let coordinates = FloatingPaneCoordinates::new(
        Some("10%".to_owned()),
        Some("10%".to_owned()),
        Some("80%".to_owned()),
        Some("80%".to_owned()),
        None,
        None,
    );

    open_plugin_pane_floating(own_url, configuration, coordinates, BTreeMap::new())
}
