//! Plugin lifecycle: events in, sidebar out.

use std::collections::{BTreeMap, BTreeSet};

use agenttij_core::{
    agent, config::Scope, panes, scan, Agent, Config, Groups, PaneSnapshot, Status,
};
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
    /// The row we were on before this one, for flipping back to it.
    previous_row: Option<u32>,
    /// Rows showing their panes underneath them, by primary.
    expanded: BTreeSet<u32>,
    /// The open peek pane, so `q` can close it and `p` never stacks two.
    peek: Option<PaneId>,
    /// Lines of the pane this instance is peeking at, when it is a peek.
    peeked: Vec<String>,
    /// Our own plugin url, for launching a peek instance of ourselves.
    own_url: Option<String>,
    /// Whether the pane has been named yet.
    named: bool,
    /// Which panes belong to which row. Only meaningful in solo mode, where a
    /// row is a place to work rather than a single pane.
    groups: Groups,
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
                self.tick();
                false
            }
            Event::RunCommandResult(_, stdout, _, context) => self.absorb_scan(&stdout, &context),
            Event::SessionUpdate(sessions, _) => {
                self.panes = snapshot::panes(&sessions);
                if let Some(name) = snapshot::current_session(&sessions) {
                    self.current_session = name;
                }

                // Only here: reconciling against a stale pane list would drop a
                // companion added since the last update, which is exactly how
                // the grouping used to fall apart the moment it was made.
                let here: Vec<u32> = self
                    .panes
                    .iter()
                    .filter(|pane| pane.session == self.current_session)
                    .map(|pane| pane.pane)
                    .collect();
                self.groups.reconcile(&here);
                if self.own_url.is_none() {
                    self.own_url = snapshot::own_url(&sessions, get_plugin_ids().plugin_id);
                }
                // After the url is captured, not before: the title *is* the url
                // until we overwrite it.
                self.name_self();
                self.rebuild();
                true
            }
            Event::Key(key) => self.on_key(key),
            Event::PermissionRequestResult(status) => {
                self.permissions = match status {
                    PermissionStatus::Granted => Permissions::Granted,
                    PermissionStatus::Denied => Permissions::Denied,
                };

                self.name_self();
                true
            }
            _ => false,
        }
    }

    /// Messages from a keybind, via `MessagePlugin`. This is how cycling works
    /// while the *agent* has focus, which is the point of it — you are typing to
    /// an agent and want the editor beside it, without going through the sidebar
    /// first.
    fn pipe(&mut self, message: PipeMessage) -> bool {
        match message.name.as_str() {
            "cycle" => self.cycle(),
            "back" => self.go_back(),
            "new" => self.new_row(),
            "add" => self.add_to_row(),
            _ => {}
        }
        false
    }

    fn render(&mut self, rows: usize, cols: usize) {
        if self.config.peek.is_some() {
            render::draw_peek(&self.peeked, rows, cols);
            return;
        }

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

        if let Some((session, pane)) = self.config.peek.clone() {
            let command = scan::dump_command(&session, pane);
            let words: Vec<&str> = command.iter().map(String::as_str).collect();
            let context =
                BTreeMap::from([(scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_PEEK.to_owned())]);
            run_command(&words, context);
            return;
        }

        let context =
            BTreeMap::from([(scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_SCAN.to_owned())]);
        run_command(&scan::command(), context);
    }

    fn absorb_scan(&mut self, stdout: &[u8], context: &BTreeMap<String, String>) -> bool {
        if context.get(scan::CONTEXT_KEY).map(String::as_str) == Some(scan::CONTEXT_PEEK) {
            self.peeked = String::from_utf8_lossy(stdout)
                .lines()
                .map(str::to_owned)
                .collect();
            return true;
        }
        if context.get(scan::CONTEXT_KEY).map(String::as_str) != Some(scan::CONTEXT_SCAN) {
            return false;
        }
        let Some(result) = scan::parse(stdout) else {
            return false;
        };

        self.now = result.now;
        self.live_sessions = result.live_sessions;

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

        // In solo mode a row is a group of panes, so the rows are the groups:
        // one per agent, with its companions folded inside rather than listed.
        if self.config.solo {
            agents = self.group_rows(agents);
        }

        if self.config.scope == Scope::Session {
            agents.retain(|agent| agent.session == self.current_session);
        }
        agent::sort_for_display(&mut agents, &self.current_session);
        if self.config.solo {
            agents = self.with_expanded_panes(agents);
        }

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

        // A peek is a still picture of someone else's pane: there is nothing to
        // type into it, so any key dismisses it. This is why a peek is a plugin
        // pane and not a command pane — a command pane cannot read a key at all,
        // and a floating pane is only visible while it holds focus, so the two
        // together made a peek impossible to dismiss.
        if self.config.peek.is_some() {
            close_self();
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
                    if self.config.solo && here && agent.depth > 0 {
                        self.show_pane(agent.pane);
                    } else if self.config.solo && here {
                        self.switch_to_row(agent.pane);
                    } else {
                        actions::go_to(&agent, &self.current_session, &self.panes);
                    }
                }
                false
            }
            // Show or hide a row's other panes, so you can go straight to one.
            BareKey::Tab => {
                if let Some(agent) = self.selected_agent().cloned() {
                    let row = if agent.depth > 0 {
                        self.groups
                            .group_of(agent.pane)
                            .map(|group| group.primary())
                    } else {
                        Some(agent.pane)
                    };
                    if let Some(row) = row.filter(|row| self.groups.members_of(*row).len() > 1) {
                        if !self.expanded.remove(&row) {
                            self.expanded.insert(row);
                        }
                        self.rebuild();
                    }
                }
                true
            }
            // Cycle to the next pane in the row on screen. The same thing is a
            // keybind away when the agent itself has focus, which is the point.
            BareKey::Char('v') => {
                self.cycle();
                false
            }
            // Add a pane to the selected row: an editor beside the agent, a log,
            // whatever. It joins the group instead of becoming a row of its own.
            BareKey::Char('a') => {
                self.add_to_row();
                false
            }
            // A new agent pane that takes over the slot, parking the current
            // one rather than splitting the screen with it.
            BareKey::Char('n') => {
                self.new_row();
                false
            }
            // Back to the session we came from.
            BareKey::Char('b') => {
                self.go_back();
                false
            }
            BareKey::Char('p') => {
                if let (Some(agent), Some(url)) =
                    (self.selected_agent().cloned(), self.own_url.clone())
                {
                    self.peek = actions::preview(&agent, &url, &self.config);
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

    /// Names our pane, once. Renaming needs ChangeApplicationState, and
    /// permissions arrive asynchronously, so `load` is too early — a command
    /// issued before the grant is denied in silence.
    fn name_self(&mut self) {
        if self.named || self.permissions != Permissions::Granted || self.own_url.is_none() {
            return;
        }
        self.named = true;
        rename_plugin_pane(get_plugin_ids().plugin_id, &self.config.title);
    }

    /// One row per group, named by the group's primary.
    ///
    /// Anything in another session is left alone — grouping is a workspace idea,
    /// and a remote agent is a row on its own by definition.
    fn group_rows(&mut self, agents: Vec<Agent>) -> Vec<Agent> {
        // Panes with no agent reporting on them still need a name for their row.
        let plain = panes::list_panes(&self.panes, &[], &self.current_session);
        let (local, remote): (Vec<Agent>, Vec<Agent>) = agents
            .into_iter()
            .partition(|agent| agent.session == self.current_session);

        let mut rows: Vec<Agent> = self
            .groups
            .rows()
            .filter_map(|(primary, _members)| {
                local
                    .iter()
                    .find(|agent| agent.pane == primary)
                    .or_else(|| plain.iter().find(|entry| entry.pane == primary))
                    .cloned()
            })
            .collect();

        rows.extend(remote);
        rows
    }

    /// Splices an expanded row's panes in underneath it.
    ///
    /// After sorting, not before: a child has to stay under its parent, and the
    /// sort has no idea the two are related.
    fn with_expanded_panes(&self, rows: Vec<Agent>) -> Vec<Agent> {
        let mut out = Vec::with_capacity(rows.len());

        for row in rows {
            let expanded = self.expanded.contains(&row.pane) && row.session == self.current_session;
            let members = if expanded {
                self.groups.members_of(row.pane).to_vec()
            } else {
                Vec::new()
            };
            out.push(row.clone());

            for member in members.into_iter().filter(|member| *member != row.pane) {
                let title = self
                    .panes
                    .iter()
                    .find(|pane| pane.session == self.current_session && pane.pane == member)
                    .map(|pane| panes::short_title(&pane.title))
                    .unwrap_or_default();

                out.push(Agent {
                    pane: member,
                    status: Status::Pane,
                    reported_at: 0,
                    cwd: String::new(),
                    title,
                    panes: 1,
                    depth: 1,
                    ..row.clone()
                });
            }
        }
        out
    }

    /// The pane on screen in our tab, which is the one the slot holds.
    fn slot(&self) -> Option<u32> {
        let (tab, _) = get_focused_pane_info().ok()?;
        panes::visible_terminal(&self.panes, &self.current_session, tab)
    }

    /// Shows one particular pane of a row, rather than whichever the row was
    /// last on.
    fn show_pane(&mut self, pane: u32) {
        let slot = self.slot();
        if let Some(leaving) = slot
            .and_then(|visible| self.groups.group_of(visible))
            .map(|group| group.primary())
        {
            let arriving = self.groups.group_of(pane).map(|group| group.primary());
            if arriving != Some(leaving) {
                self.previous_row = Some(leaving);
            }
        }

        self.groups.show(pane);
        actions::show_in_slot(pane, slot);
    }

    /// Flips to the row we were on before this one.
    fn go_back(&mut self) {
        let Some(previous) = self.previous_row else {
            return;
        };
        if self.groups.group_of(previous).is_some() {
            self.switch_to_row(previous);
        }
    }

    /// Shows a row: its current member takes the slot, and the row we left
    /// becomes the one `b` flips back to.
    fn switch_to_row(&mut self, primary: u32) {
        let slot = self.slot();
        let leaving = slot
            .and_then(|visible| self.groups.group_of(visible))
            .map(|group| group.primary());
        if let Some(leaving) = leaving.filter(|leaving| *leaving != primary) {
            self.previous_row = Some(leaving);
        }

        let target = self.groups.current_of(primary).unwrap_or(primary);
        self.groups.show(target);
        self.selected = Some((self.current_session.clone(), primary));
        actions::show_in_slot(target, slot);
    }

    /// Shows the next pane in the row currently on screen.
    fn cycle(&mut self) {
        let Some(visible) = self.slot() else { return };
        let Some(target) = self.groups.next_after(visible) else {
            return;
        };

        self.groups.show(target);
        actions::show_in_slot(target, Some(visible));
    }

    /// Opens a pane in the row that is on screen, parking what was there.
    ///
    /// Anchored on what is on screen rather than where the cursor is, because
    /// the same action is a keybind away while an *agent* has focus and there is
    /// no cursor involved then. It also keeps the two consistent: the pane you
    /// get always belongs to the row you were looking at.
    fn add_to_row(&mut self) {
        let Some(visible) = self.slot() else {
            self.new_row();
            return;
        };
        if let Some(PaneId::Terminal(opened)) =
            actions::new_in_slot(&self.panes, &self.current_session, self.config.solo)
        {
            self.groups.add(visible, opened);
        }
    }

    /// A pane of its own: reconciliation turns anything ungrouped into a row.
    fn new_row(&mut self) {
        actions::new_in_slot(&self.panes, &self.current_session, self.config.solo);
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
