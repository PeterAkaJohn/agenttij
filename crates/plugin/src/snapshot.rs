//! Adapting Zellij's session metadata into `agenttij-core`'s neutral types.
//!
//! This is the only place that knows both vocabularies, which is what keeps
//! the decision-making in `core` testable without a WASM host.

use agenttij_core::PaneSnapshot;
use zellij_tile::prelude::*;

/// Flattens every terminal pane in every live session into one list.
///
/// Suppressed panes are kept: they are hidden from the user but still running,
/// so an agent in one is alive and must not be reaped.
pub fn panes(sessions: &[SessionInfo]) -> Vec<PaneSnapshot> {
    let mut panes: Vec<PaneSnapshot> = sessions
        .iter()
        .flat_map(|session| {
            session.panes.panes.iter().flat_map(move |(tab, panes)| {
                panes
                    .iter()
                    .filter(|pane| !pane.is_plugin)
                    .map(move |pane| PaneSnapshot {
                        session: session.name.clone(),
                        tab: *tab,
                        pane: pane.id,
                        title: pane.title.clone(),
                        suppressed: pane.is_suppressed,
                        floating: pane.is_floating,
                    })
            })
        })
        .collect();

    // Zellij hands panes over in a `HashMap`, whose iteration order is not
    // stable between updates. Sorting here keeps every downstream decision —
    // which pane holds the slot, what order rows appear in — from shuffling.
    panes.sort_by(|left, right| {
        (&left.session, left.tab, left.pane).cmp(&(&right.session, right.tab, right.pane))
    });
    panes
}

/// Whether anyone is looking at our tab: a client attached to this session, with
/// this tab the one open.
pub fn watched(sessions: &[SessionInfo], plugin_id: u32) -> bool {
    let Some(tab) = own_tab(sessions, plugin_id) else {
        return false;
    };
    sessions
        .iter()
        .find(|session| session.is_current_session)
        .is_some_and(|session| {
            session.connected_clients > 0
                && session
                    .tabs
                    .iter()
                    .any(|info| info.position == tab && info.active)
        })
}

/// Whether our own pane is a floating one.
pub fn floating(sessions: &[SessionInfo], plugin_id: u32) -> bool {
    sessions
        .iter()
        .find(|session| session.is_current_session)
        .and_then(|session| {
            session
                .panes
                .panes
                .values()
                .flatten()
                .find(|pane| pane.is_plugin && pane.id == plugin_id)
        })
        .is_some_and(|pane| pane.is_floating)
}

/// The tab our own pane sits in.
///
/// Taken from the manifest we are handed rather than `get_focused_pane_info`,
/// which is a round-trip to the server — and one made on every rebuild and every
/// key that moves a pane, which is enough to be felt.
pub fn own_tab(sessions: &[SessionInfo], plugin_id: u32) -> Option<usize> {
    sessions
        .iter()
        .find(|session| session.is_current_session)?
        .panes
        .panes
        .iter()
        .find(|(_, panes)| {
            panes
                .iter()
                .any(|pane| pane.is_plugin && pane.id == plugin_id)
        })
        .map(|(tab, _)| *tab)
}

/// The session we are running in.
///
/// Read from the metadata rather than `ZELLIJ_SESSION_NAME`, which is captured
/// when a pane is created and goes stale if the session is renamed.
pub fn current_session(sessions: &[SessionInfo]) -> Option<String> {
    sessions
        .iter()
        .find(|session| session.is_current_session)
        .map(|session| session.name.clone())
}

/// Our own plugin url, needed to open a peek — a peek is another instance of
/// this same plugin, and a plugin can only be launched by url.
///
/// Read from our own pane's `plugin_url`. Not its title: that is the url only
/// until something renames the pane, so a sidebar reloaded after naming itself
/// never learned it - no peek, no palette, no help - and one launched at runtime
/// is titled `(<cwd>) - file:/…`, which is no url at all. Two other routes are
/// dead ends: `SessionInfo.plugins` is empty in practice, and
/// `get_session_environment_variables()` *panics*, taking the plugin with it.
pub fn own_url(sessions: &[SessionInfo], plugin_id: u32) -> Option<String> {
    sessions
        .iter()
        .find(|session| session.is_current_session)?
        .panes
        .panes
        .values()
        .flatten()
        .find(|pane| pane.is_plugin && pane.id == plugin_id)?
        .plugin_url
        .clone()
}
