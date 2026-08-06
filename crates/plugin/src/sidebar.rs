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
    /// The session we last switched away from, for `b`.
    previous_session: Option<String>,
    /// The open peek pane, so `q` can close it and `p` never stacks two.
    peek: Option<PaneId>,
    /// Set after opening a peek: the focus it stole has to come back to us on a
    /// later event, because the open is applied after our own calls.
    reclaim_focus: bool,
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
            PermissionType::OpenTerminalsOrPlugins, // start an agent pane with `n`
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
                self.reclaim_focus();
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

                // Renaming needs ChangeApplicationState, and permissions are
                // granted asynchronously — doing this in `load` is too early and
                // is denied in silence, leaving the frame showing our wasm path.
                if self.permissions == Permissions::Granted {
                    rename_plugin_pane(get_plugin_ids().plugin_id, &self.config.title);
                }
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
            colors: &self.config.colors,
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
        self.previous_session = result.previous_session;

        let before = std::mem::replace(&mut self.reported, result.agents);
        self.rebuild();

        // Only agents that *became* blocked, so a waiting agent is announced
        // once rather than every second.
        for agent in agent::newly_blocked(&before, &self.reported) {
            actions::notify(&self.config.notify, agent);
        }
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

        // In solo mode the sidebar is the workspace's pane switcher, so it has
        // to list panes that are not agents too — a parked shell you cannot
        // select is a pane you cannot get back to.
        if self.config.solo {
            let rest = panes::list_panes(&self.panes, &agents, &self.current_session);
            agents.extend(rest);
        }

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

        // A peek is a still picture over your work, so the next key dismisses it
        // whatever that key is — and then still does its job, so peeking never
        // costs you a keystroke. q and Esc are the keys that only dismiss.
        let had_peek = self.peek.is_some();
        self.close_peek();

        match key.bare_key {
            BareKey::Char('q') | BareKey::Esc => had_peek,
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
            // A new agent pane that takes over the slot, parking the current
            // one rather than splitting the screen with it.
            BareKey::Char('n') => {
                actions::new_in_slot(&self.panes, &self.current_session, self.config.solo);
                false
            }
            // Back to the session we came from.
            BareKey::Char('b') => {
                if let Some(previous) = self.previous_session.clone() {
                    actions::go_back(&previous, &self.current_session);
                }
                false
            }
            BareKey::Char('p') => {
                if let Some(agent) = self.selected_agent().cloned() {
                    self.peek = actions::preview(&agent);
                    self.reclaim_focus = true;
                    // Sooner than the next tick: a peek that holds the keyboard
                    // is a peek you cannot dismiss.
                    set_timeout(0.1);
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

    /// Takes focus back from a peek pane, which cannot use it.
    fn reclaim_focus(&mut self) {
        if !self.reclaim_focus {
            return;
        }
        self.reclaim_focus = false;
        focus_plugin_pane(get_plugin_ids().plugin_id, false, false);
    }

    fn close_peek(&mut self) {
        if let Some(peek) = self.peek.take() {
            close_pane_with_id(peek);
        }
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
