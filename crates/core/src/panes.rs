//! Reconciling reported agents against the panes Zellij actually has.
//!
//! A status board you cannot trust is worse than no board, so nothing reaches
//! the sidebar without a live pane behind it. This is what covers the cases
//! where no hook ever fires: `kill -9`, a closed pane, a dead session, a
//! session from an older Zellij version, or a session that got renamed after
//! the agent read `ZELLIJ_SESSION_NAME`.

use crate::agent::{Agent, Status};

/// One terminal pane, flattened out of Zellij's per-session `PaneManifest`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaneSnapshot {
    pub session: String,
    /// Tab position, which is what `switch_session_with_focus` wants.
    pub tab: usize,
    pub pane: u32,
    /// Pane title. Usually the foreground process, but an agent that sets its
    /// own terminal title will overwrite it — so this is a discovery hint, not
    /// a source of truth.
    pub title: String,
    /// Suppressed panes are running but not on screen. Solo mode parks agents
    /// here instead of leaving them visible.
    pub suppressed: bool,
}

/// The terminal pane currently on screen in a tab — the slot a solo swap
/// replaces. `None` when every agent there is parked.
pub fn visible_terminal(panes: &[PaneSnapshot], session: &str, tab: usize) -> Option<u32> {
    panes
        .iter()
        .find(|pane| pane.session == session && pane.tab == tab && !pane.suppressed)
        .map(|pane| pane.pane)
}

/// Drops agents that no longer have anything running behind them.
///
/// Liveness is judged at two different resolutions, because that is all Zellij
/// reliably offers:
///
/// * **Session** — from the live session list, which is derived from IPC
///   sockets and is therefore always accurate.
/// * **Pane** — only for sessions we actually have pane data for. With
///   `session_serialization false` a user's other sessions never publish a
///   pane manifest, and treating "no pane data" as "no panes" would hide every
///   cross-session agent, which is the entire point of the sidebar.
///
/// Empty inputs mean "nothing known yet" rather than "nothing alive", so they
/// drop nothing.
pub fn reconcile(
    agents: Vec<Agent>,
    panes: &[PaneSnapshot],
    live_sessions: &[String],
) -> Vec<Agent> {
    agents
        .into_iter()
        .filter(|agent| {
            let session_is_dead = !live_sessions.is_empty()
                && !live_sessions.iter().any(|live| live == &agent.session);
            if session_is_dead {
                return false;
            }

            // Only meaningful where we can see the session's panes at all.
            if has_pane_data(panes, &agent.session) {
                return exists(panes, &agent.session, agent.pane);
            }
            true
        })
        .collect()
}

fn has_pane_data(panes: &[PaneSnapshot], session: &str) -> bool {
    panes.iter().any(|pane| pane.session == session)
}

/// Finds agent panes that are not reporting to us, so tools without hooks
/// still show up in the sidebar instead of being silently absent.
pub fn discover(panes: &[PaneSnapshot], reported: &[Agent], names: &[String]) -> Vec<Agent> {
    panes
        .iter()
        .filter(|pane| looks_like_an_agent(&pane.title, names))
        .filter(|pane| {
            !reported
                .iter()
                .any(|agent| agent.key() == (pane.session.as_str(), pane.pane))
        })
        .map(|pane| Agent {
            session: pane.session.clone(),
            pane: pane.pane,
            status: Status::Unknown,
            reported_at: 0,
            cwd: String::new(),
            title: short_title(&pane.title),
            panes: 1,
            depth: 0,
        })
        .collect()
}

/// Marks the sidebar's own peek panes. They are panes in the session like any
/// other, so without this they turn up in the list as a row showing the shell
/// command that draws them.
pub const PEEK_MARKER: &str = "agenttij-peek";

/// Lists every remaining pane in a session, so the sidebar can switch to panes
/// that are not agents.
///
/// Solo mode needs this: a pane opened with `n` holds a shell until an agent
/// starts reporting from it, and a pane the sidebar cannot list is a pane you
/// cannot get back to once it is parked.
pub fn list_panes(panes: &[PaneSnapshot], listed: &[Agent], session: &str) -> Vec<Agent> {
    panes
        .iter()
        .filter(|pane| pane.session == session)
        .filter(|pane| !pane.title.contains(PEEK_MARKER))
        .filter(|pane| {
            !listed
                .iter()
                .any(|agent| agent.key() == (pane.session.as_str(), pane.pane))
        })
        .map(|pane| Agent {
            session: pane.session.clone(),
            pane: pane.pane,
            status: Status::Pane,
            reported_at: 0,
            cwd: String::new(),
            title: short_title(&pane.title),
            panes: 1,
            depth: 0,
        })
        .collect()
}

/// Squeezes a pane title into something that fits a sidebar.
///
/// Titles are often a shell's `user@host:~/path/to/dir`, where the only useful
/// part is the last path segment.
pub fn short_title(title: &str) -> String {
    let trimmed = title.trim().trim_end_matches('/');
    let tail = trimmed
        .rsplit_once('/')
        .map(|(_, tail)| tail)
        .or_else(|| trimmed.rsplit_once(':').map(|(_, tail)| tail))
        .unwrap_or(trimmed);

    let tail = tail.trim();
    if tail.is_empty() {
        return trimmed.to_string();
    }
    tail.to_string()
}

/// Shells that mean "nothing in particular is running here".
const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "fish", "dash", "nu"];

/// Names a pane by the program running in it — `nvim`, `lazygit` — falling back
/// to the row's own name when that is just a shell.
///
/// A pane sitting at a prompt has no better name than the row it belongs to;
/// saying "zsh" three times over tells you nothing about which is which.
pub fn program_name(command: &[String], fallback: &str) -> String {
    let Some(program) = command.first() else {
        return fallback.to_string();
    };
    let program = program
        .rsplit('/')
        .next()
        .unwrap_or(program)
        .trim()
        .trim_start_matches('-'); // login shells arrive as "-zsh"

    if program.is_empty() || SHELLS.contains(&program) {
        return fallback.to_string();
    }
    program.to_string()
}

/// Tab position of a pane, needed to land on it when switching sessions.
pub fn tab_of(panes: &[PaneSnapshot], session: &str, pane: u32) -> Option<usize> {
    panes
        .iter()
        .find(|candidate| candidate.session == session && candidate.pane == pane)
        .map(|candidate| candidate.tab)
}

fn exists(panes: &[PaneSnapshot], session: &str, pane: u32) -> bool {
    panes
        .iter()
        .any(|candidate| candidate.session == session && candidate.pane == pane)
}

fn looks_like_an_agent(title: &str, names: &[String]) -> bool {
    let title = title.to_lowercase();
    names.iter().any(|name| title.contains(name.as_str()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(session: &str, tab: usize, pane: u32, title: &str) -> PaneSnapshot {
        PaneSnapshot {
            session: session.into(),
            tab,
            pane,
            title: title.into(),
            suppressed: false,
        }
    }

    fn parked(session: &str, tab: usize, pane: u32, title: &str) -> PaneSnapshot {
        PaneSnapshot {
            suppressed: true,
            ..self::pane(session, tab, pane, title)
        }
    }

    fn agent(session: &str, pane: u32) -> Agent {
        Agent {
            session: session.into(),
            pane,
            status: Status::Running,
            reported_at: 10,
            cwd: "/x".into(),
            title: String::new(),
            panes: 1,
            depth: 0,
        }
    }

    fn names() -> Vec<String> {
        vec!["claude".to_string(), "codex".to_string()]
    }

    fn live(names: &[&str]) -> Vec<String> {
        names.iter().map(|name| name.to_string()).collect()
    }

    #[test]
    fn keeps_agents_with_a_live_pane() {
        let panes = vec![pane("main", 0, 3, "zsh")];
        assert_eq!(
            reconcile(vec![agent("main", 3)], &panes, &live(&["main"])),
            vec![agent("main", 3)]
        );
    }

    #[test]
    fn drops_agents_whose_pane_is_gone() {
        let panes = vec![pane("main", 0, 3, "zsh")];
        assert_eq!(
            reconcile(vec![agent("main", 9)], &panes, &live(&["main"])),
            vec![]
        );
    }

    #[test]
    fn drops_agents_whose_session_is_gone() {
        let panes = vec![pane("main", 0, 3, "zsh")];
        assert_eq!(
            reconcile(vec![agent("dead", 3)], &panes, &live(&["main"])),
            vec![]
        );
    }

    /// The `session_serialization false` case: another session is alive but
    /// publishes no pane manifest, so its agents must survive.
    #[test]
    fn keeps_agents_in_a_live_session_we_have_no_pane_data_for() {
        let panes = vec![pane("main", 0, 3, "zsh")];
        let agents = vec![agent("other", 7)];

        assert_eq!(
            reconcile(agents.clone(), &panes, &live(&["main", "other"])),
            agents
        );
    }

    #[test]
    fn no_information_yet_is_not_a_reason_to_drop_anything() {
        let agents = vec![agent("main", 3)];
        assert_eq!(reconcile(agents.clone(), &[], &[]), agents);
    }

    #[test]
    fn a_dead_session_is_dropped_even_without_pane_data() {
        assert_eq!(
            reconcile(vec![agent("dead", 3)], &[], &live(&["main"])),
            vec![]
        );
    }

    #[test]
    fn discovers_unreported_agent_panes_by_title() {
        let panes = vec![pane("main", 1, 4, "codex"), pane("main", 0, 3, "zsh")];
        let found = discover(&panes, &[], &names());

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].key(), ("main", 4));
        assert_eq!(found[0].status, Status::Unknown);
    }

    #[test]
    fn discovery_never_shadows_a_reporting_agent() {
        let panes = vec![pane("main", 0, 3, "claude")];
        assert_eq!(discover(&panes, &[agent("main", 3)], &names()), vec![]);
    }

    #[test]
    fn the_visible_slot_skips_parked_panes() {
        let panes = vec![
            parked("main", 0, 3, "claude"),
            pane("main", 0, 4, "claude"),
            pane("main", 1, 5, "claude"),
        ];

        assert_eq!(visible_terminal(&panes, "main", 0), Some(4));
        assert_eq!(visible_terminal(&panes, "main", 1), Some(5));
        assert_eq!(visible_terminal(&panes, "other", 0), None);
    }

    #[test]
    fn every_pane_parked_leaves_no_slot() {
        let panes = vec![
            parked("main", 0, 3, "claude"),
            parked("main", 0, 4, "claude"),
        ];
        assert_eq!(visible_terminal(&panes, "main", 0), None);
    }

    #[test]
    fn lists_panes_that_are_not_agents() {
        let panes = vec![
            pane("main", 0, 3, "pp@host:~/personal/agenttij"),
            pane("main", 0, 4, "claude"),
            pane("other", 0, 9, "zsh"),
        ];
        let listed = discover(&panes, &[], &names());

        let extra = list_panes(&panes, &listed, "main");
        assert_eq!(extra.len(), 1, "only the non-agent pane in this session");
        assert_eq!(extra[0].key(), ("main", 3));
        assert_eq!(extra[0].status, Status::Pane);
        assert_eq!(extra[0].label(), "agenttij");
    }

    /// A peek is the sidebar's own doing, and showing the command that draws it
    /// as a switchable row is nonsense.
    #[test]
    fn our_own_peek_panes_are_not_listed() {
        let panes = vec![
            pane("main", 0, 3, "zsh"),
            pane(
                "main",
                0,
                9,
                "sh -c while :; do clear; zellij … agenttij-peek main 3",
            ),
        ];

        let listed = list_panes(&panes, &[], "main");
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].key(), ("main", 3));
    }

    #[test]
    fn listing_panes_never_duplicates_a_row() {
        let panes = vec![pane("main", 0, 3, "zsh")];
        let listed = list_panes(&panes, &[], "main");
        assert_eq!(list_panes(&panes, &listed, "main"), vec![]);
    }

    #[test]
    fn a_pane_is_named_after_what_runs_in_it() {
        let command = |words: &[&str]| words.iter().map(|w| w.to_string()).collect::<Vec<_>>();

        assert_eq!(program_name(&command(&["nvim", "."]), "api"), "nvim");
        assert_eq!(
            program_name(&command(&["/usr/bin/lazygit"]), "api"),
            "lazygit"
        );
        // A shell is not a program worth naming a row after.
        assert_eq!(program_name(&command(&["zsh"]), "api"), "api");
        assert_eq!(program_name(&command(&["-zsh"]), "api"), "api");
        assert_eq!(program_name(&command(&["/bin/bash"]), "api"), "api");
        assert_eq!(program_name(&[], "api"), "api");
    }

    #[test]
    fn titles_shrink_to_something_readable() {
        assert_eq!(short_title("pp@host:~/personal/agenttij"), "agenttij");
        assert_eq!(short_title("~/personal/agenttij/"), "agenttij");
        assert_eq!(short_title("nvim ."), "nvim .");
        assert_eq!(short_title("zsh"), "zsh");
        assert_eq!(short_title(""), "");
    }

    #[test]
    fn finds_the_tab_to_land_on() {
        let panes = vec![pane("main", 0, 3, "zsh"), pane("main", 2, 7, "claude")];

        assert_eq!(tab_of(&panes, "main", 7), Some(2));
        assert_eq!(tab_of(&panes, "main", 99), None);
    }
}
