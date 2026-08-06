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
