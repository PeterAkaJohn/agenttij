//! Plugin lifecycle: events in, sidebar out.

use std::collections::BTreeMap;

use agenttij_core::{agent, config::Scope, panes, scan, Agent, Config, PaneSnapshot};
use zellij_tile::prelude::*;

use crate::{actions, render, snapshot};

/// How often the state files are re-read. A `sh` fork at this rate is noise;
/// see `agenttij_core::scan` for why we shell out at all.
const TICK_SECONDS: f64 = 1.0;

#[derive(Default, PartialEq, Eq)]
enum Permissions {
    #[default]
    Pending,
    Granted,
    Denied,
}

#[derive(Default)]
pub struct Sidebar {
    config: Config,
    /// Agents from the last scan of the state files.
    reported: Vec<Agent>,
    /// What the sidebar shows: reported plus discovered, reconciled and sorted.
    agents: Vec<Agent>,
    /// Panes from `SessionUpdate`. Covers this session always, and other
    /// sessions only when they publish a manifest (see `panes::reconcile`).
    panes: Vec<PaneSnapshot>,
    /// Live sessions from the last scan — the reliable liveness signal.
    live_sessions: Vec<String>,
    current_session: String,
    /// Host clock from the last scan.
    now: u64,
    /// The highlighted agent, tracked by pane rather than by index so the
    /// cursor sticks to an agent while the list re-sorts underneath it.
    selected: Option<(String, u32)>,
    permissions: Permissions,
}

impl ZellijPlugin for Sidebar {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_map(&configuration);

        request_permission(&[
            PermissionType::ReadApplicationState, // session and pane metadata
            PermissionType::ChangeApplicationState, // focus a pane, switch session
            PermissionType::RunCommands,          // read state files, open a preview
        ]);
        subscribe(&[
            EventType::Timer,
            EventType::SessionUpdate,
            EventType::RunCommandResult,
            EventType::Key,
            EventType::PermissionRequestResult,
        ]);

        // The sidebar is a pane you navigate to like any other.
        set_selectable(true);
        set_timeout(TICK_SECONDS);
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Timer(_) => {
                self.tick();
                false
            }
            Event::RunCommandResult(_, stdout, _, context) => self.absorb_scan(&stdout, &context),
            Event::SessionUpdate(sessions, _) => {
                self.panes = snapshot::panes(&sessions);
                if let Some(name) = snapshot::current_session(&sessions) {
                    self.current_session = name;
                }
                self.rebuild();
                true
            }
            Event::Key(key) => self.on_key(key),
            Event::PermissionRequestResult(status) => {
                self.permissions = match status {
                    PermissionStatus::Granted => Permissions::Granted,
                    PermissionStatus::Denied => Permissions::Denied,
                };
                true
            }
            _ => false,
        }
    }

    fn render(&mut self, rows: usize, cols: usize) {
        render::draw(&render::View {
            rows,
            cols,
            agents: &self.agents,
            cursor: self.cursor(),
            now: self.now,
            notice: self.notice(),
            current_session: &self.current_session,
        });
    }
}

impl Sidebar {
    /// Re-arms the timer first, so a failed scan never stops the clock.
    fn tick(&mut self) {
        set_timeout(TICK_SECONDS);

        if self.permissions == Permissions::Denied {
            return;
        }
        let context =
            BTreeMap::from([(scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_SCAN.to_owned())]);
        run_command(&scan::command(), context);
    }

    fn absorb_scan(&mut self, stdout: &[u8], context: &BTreeMap<String, String>) -> bool {
        if context.get(scan::CONTEXT_KEY).map(String::as_str) != Some(scan::CONTEXT_SCAN) {
            return false;
        }
        let Some(result) = scan::parse(stdout) else {
            return false;
        };

        self.now = result.now;
        self.live_sessions = result.live_sessions;
        self.reported = result.agents;
        self.rebuild();
        true
    }

    /// Rebuilds the visible list from the last scan and the current panes.
    ///
    /// Idempotent, and safe to call from either input: reconciliation drops
    /// agents whose pane is gone, discovery adds agent panes that are not
    /// reporting, and the sort puts whatever needs attention on top.
    fn rebuild(&mut self) {
        let mut agents = panes::reconcile(self.reported.clone(), &self.panes, &self.live_sessions);
        let discovered = panes::discover(&self.panes, &agents, &self.config.agents);
        agents.extend(discovered);

        if self.config.scope == Scope::Session {
            agents.retain(|agent| agent.session == self.current_session);
        }
        agent::sort_for_display(&mut agents, &self.current_session);

        self.agents = agents;
        self.resync_selection();
    }

    /// Keeps the cursor on a real row after the list changes underneath it.
    fn resync_selection(&mut self) {
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|(session, pane)| self.find(session, *pane).is_some());

        if !still_there {
            self.selected = self
                .agents
                .first()
                .map(|agent| (agent.session.clone(), agent.pane));
        }
    }

    fn on_key(&mut self, key: KeyWithModifier) -> bool {
        // Leave modified keys to Zellij, so the sidebar never eats a binding.
        if !key.key_modifiers.is_empty() {
            return false;
        }

        match key.bare_key {
            BareKey::Down | BareKey::Char('j') => self.move_cursor(1),
            BareKey::Up | BareKey::Char('k') => self.move_cursor(-1),
            BareKey::Enter => {
                if let Some(agent) = self.selected_agent().cloned() {
                    let here = agent.session == self.current_session;
                    if self.config.solo && here {
                        actions::solo(&agent, &self.panes, &self.current_session);
                    } else {
                        actions::go_to(&agent, &self.current_session, &self.panes);
                    }
                }
                false
            }
            BareKey::Char('p') => {
                if let Some(agent) = self.selected_agent() {
                    actions::preview(agent);
                }
                false
            }
            _ => false,
        }
    }

    fn move_cursor(&mut self, delta: isize) -> bool {
        if self.agents.is_empty() {
            return false;
        }

        let last = self.agents.len() - 1;
        let next = self.cursor().saturating_add_signed(delta).min(last);
        self.selected = Some((self.agents[next].session.clone(), self.agents[next].pane));
        true
    }

    fn cursor(&self) -> usize {
        self.selected
            .as_ref()
            .and_then(|(session, pane)| self.find(session, *pane))
            .unwrap_or(0)
    }

    fn selected_agent(&self) -> Option<&Agent> {
        self.agents.get(self.cursor())
    }

    fn find(&self, session: &str, pane: u32) -> Option<usize> {
        self.agents
            .iter()
            .position(|agent| agent.key() == (session, pane))
    }

    fn notice(&self) -> Option<&'static str> {
        match self.permissions {
            Permissions::Pending => Some("waiting for permissions"),
            Permissions::Denied => Some("permissions denied"),
            Permissions::Granted => None,
        }
    }
}
