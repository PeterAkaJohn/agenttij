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

/// Goes wherever the palette was pointing.
///
/// A session switch needs no tab position — verified live, a pane id alone lands
/// on a pane in a background tab — which is what makes this work without the
/// pane manifests other sessions never publish.
pub fn go_to_target(target: &agenttij_core::jump::Target, current_session: &str) {
    use agenttij_core::jump::Target;
    match target {
        Target::Remote { host, session } => {
            attach(host, session, None, false);
        }
        Target::Pane { session, pane } if session == current_session => {
            focus_pane_with_id(PaneId::Terminal(*pane), false, false);
        }
        Target::Pane { session, pane } => {
            switch_session_with_focus(session, None, Some((*pane, false)));
        }
        // A resurrectable one comes back on being switched to.
        Target::Session { name, .. } if name != current_session => {
            switch_session_with_focus(name, None, None);
        }
        Target::Session { .. } => {}
    }
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
/// Two steps rather than one: open the pane normally, then hide the old
/// occupant. Opening *in place of* the slot looks simpler and destroys panes —
/// see `show_in_slot` for why anything built on replacement does.
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

    // The new pane arrives as a split; hiding the old one collapses it back to
    // one on screen, without the suppression chain a replacement would build.
    hide_pane_with_id(PaneId::Terminal(slot));
    focus_pane_with_id(opened, false, false);
    Some(opened)
}

/// Brings a pane into the slot, parking whoever was there. This is the move
/// behind picking a row, cycling within one, and adding to one.
///
/// Show the newcomer, then hide the incumbent — deliberately *not*
/// `replace_pane_with_existing_pane`, which destroys panes here. Zellij keeps
/// suppressed panes in a map keyed by the pane that replaced them, so a pane
/// that is already someone's value gets orphaned the moment it becomes a key:
/// a third pane joining a row made the second disappear. `hide_pane_with_id`
/// files a pane under *its own* id (`tab/mod.rs`, `suppress_pane`), so no pane
/// depends on another to come back.
pub fn show_in_slot(target: u32, slot: Option<u32>) {
    if slot == Some(target) {
        focus_pane_with_id(PaneId::Terminal(target), false, false);
        return;
    }

    show_pane_with_id(PaneId::Terminal(target), false, true);
    if let Some(slot) = slot {
        hide_pane_with_id(PaneId::Terminal(slot));
    }
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

/// Attaches to a session on another machine, in a pane here.
///
/// The closest thing to jumping that exists: Zellij cannot show a pane it does
/// not own, and it does not own anything on that host.
pub fn attach(host: &str, session: &str, slot: Option<u32>, solo: bool) -> Option<PaneId> {
    let command = agenttij_core::scan::attach_command(host, session);
    let run = CommandToRun {
        path: PathBuf::from(&command[0]),
        args: command[1..].to_vec(),
        // A command pane reports no directory of its own, and a row with no
        // directory belongs to no project — it would arrive in the nameless one
        // rather than beside the work it was opened from.
        cwd: slot
            .and_then(|slot| get_pane_cwd(PaneId::Terminal(slot)).ok())
            .or_else(|| Some(own_cwd())),
    };

    let opened = open_command_pane(run, BTreeMap::new())?;
    // Takes the slot rather than splitting it, the same as anything else the
    // sidebar opens.
    if let Some(slot) = slot.filter(|_| solo) {
        hide_pane_with_id(PaneId::Terminal(slot));
    }
    focus_pane_with_id(opened, false, false);
    Some(opened)
}

/// Opens the jump palette: this plugin again, floating, in jump mode.
///
/// Wider than the keybind list and anchored high, because it is a thing you read
/// while typing rather than a page you consult.
pub fn jump(own_url: &str) -> Option<PaneId> {
    let configuration = BTreeMap::from([
        ("jump".to_owned(), "true".to_owned()),
        ("pane_title".to_owned(), "jump".to_owned()),
    ]);
    let coordinates = FloatingPaneCoordinates::new(
        Some("20%".to_owned()),
        Some("15%".to_owned()),
        Some("60%".to_owned()),
        Some("60%".to_owned()),
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
        // Empty for a pane on this machine, which is the usual case.
        ("peek_host".to_owned(), agent.host.clone()),
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
