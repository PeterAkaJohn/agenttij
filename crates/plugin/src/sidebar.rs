//! Plugin lifecycle: events in, sidebar out.

use std::collections::{BTreeMap, BTreeSet};

use agenttij_core::{
    agent, config::Scope, format, order, panes, project, scan, Agent, Config, Group, Groups, Kind,
    PaneSnapshot, Status,
};
use zellij_tile::prelude::*;

use crate::{
    actions,
    jump::{Act, Jump},
    render, snapshot,
};

/// End of text: the byte a terminal sends for Ctrl-C, and the only thing this
/// plugin ever writes into a pane.
const INTERRUPT: u8 = 0x03;

/// How many ticks between asking a host what it has. Slower than the local scan
/// because every one is an ssh, and much slower after one fails: a machine that
/// is asleep should cost a connection attempt a minute, not one a second.
const HOST_TICKS: u8 = 5;
const HOST_BACKOFF: u8 = 60;

/// How many ticks between session lists. Sessions come and go far more slowly
/// than agent state, and asking is a round-trip that walks the socket directory.
const SESSION_TICKS: u8 = 5;

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

/// Something that cannot be undone, waiting for the second press that does it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Pending {
    Close,
    Interrupt,
}

/// What the cursor is on. A project is addressed by its root rather than by a
/// pane, because it is not one — it stands for every row that shares that root.
#[derive(Clone, Debug, PartialEq, Eq)]
enum Selection {
    Row { session: String, pane: u32 },
    Project(String),
}

impl Selection {
    fn of(agent: &Agent) -> Self {
        match agent.kind {
            Kind::Project { .. } => Self::Project(project::root(agent).to_owned()),
            _ => Self::Row {
                session: agent.session.clone(),
                pane: agent.pane,
            },
        }
    }
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
    /// What a second press would do, and to what. Closing a pane and
    /// interrupting an agent are the two things here that cannot be undone, so
    /// both take two presses and say what they are about to take.
    pending: Option<(Pending, Selection)>,
    /// Show only what needs you, across every project and session.
    only_blocked: bool,
    /// Rows showing their panes underneath them, by primary.
    expanded: BTreeSet<u32>,
    /// How you left the sidebar: the order you put things in, and what you
    /// folded away. Written when it changes and read back when one starts.
    arrangement: order::Arrangement,
    /// Whether the remembered arrangement has been asked for yet.
    order_read: bool,
    /// A project being named, and what has been typed so far.
    renaming: Option<(String, String)>,
    /// Which git roots each project holds, from before folding hid any of them —
    /// naming a project has to reach every root inside it, or the halves of a
    /// merged one would come apart the moment you renamed it.
    project_roots: BTreeMap<String, BTreeSet<String>>,
    /// The rows each project holds in this session, kept from before folding
    /// dropped them — a folded project still has to be openable and closable.
    projects: BTreeMap<String, Vec<u32>>,
    /// The position label each pane currently carries, so a pane is only renamed
    /// when what it should say has actually changed.
    positions: BTreeMap<u32, String>,
    /// Cached answers to "where is this pane" and "what is running in it".
    ///
    /// Both are host round-trips, and asking per row on every rebuild made the
    /// sidebar lag under the burst of updates that arrives exactly when you are
    /// moving around in it. Asked once per pane and refreshed on a slow tick, so
    /// a `cd` still shows up.
    cwds: BTreeMap<u32, String>,
    programs: BTreeMap<u32, String>,
    /// What project a pane's directory belongs to, and the directory it was
    /// asked about — so a `cd` re-asks and nothing else does. Only panes nothing
    /// reports on need this; an agent's hook has already answered it.
    roots: BTreeMap<u32, (String, String)>,
    /// The pane whose cached answers are refreshed next. One per tick, so a
    /// stale `cd` is noticed without a burst of round-trips landing on whatever
    /// keypress happens to be in flight.
    refresh_next: usize,
    /// Ticks until the session list is re-read.
    sessions_in: u8,
    /// The tab we live in, from the manifest rather than a host call.
    tab: Option<usize>,
    /// The open peek pane, so `q` can close it and `p` never stacks two.
    peek: Option<PaneId>,
    /// Lines of the pane this instance is peeking at, when it is a peek.
    peeked: Vec<String>,
    /// The palette, when this instance is one.
    palette: Jump,
    /// Sessions Zellij can bring back, for the palette to offer.
    dead_sessions: Vec<String>,
    /// What each watched machine last told us, and when to ask it again.
    remote: BTreeMap<String, Vec<Agent>>,
    hosts_in: BTreeMap<String, u8>,
    /// The palette has jumped and is waiting to disappear. Closing a focused
    /// floating pane hands focus back to whatever had it before, which undoes
    /// the jump — so the close waits for the tick after it.
    leaving: bool,
    /// Our own plugin url, for launching a peek instance of ourselves.
    own_url: Option<String>,
    /// Whether the pane has been named yet.
    named: bool,
    /// Our own pane id. Read once: `get_plugin_ids` is a host round-trip, and it
    /// answers the same thing every time.
    plugin_id: u32,
    /// Which panes belong to which row. Only meaningful in solo mode, where a
    /// row is a place to work rather than a single pane.
    groups: Groups,
    current_session: String,
    /// Host clock from the last scan.
    now: u64,
    /// The highlighted agent, tracked by pane rather than by index so the
    /// cursor sticks to an agent while the list re-sorts underneath it.
    selected: Option<Selection>,
    permissions: Permissions,
}

impl ZellijPlugin for Sidebar {
    fn load(&mut self, configuration: BTreeMap<String, String>) {
        self.config = Config::from_map(&configuration);
        self.plugin_id = get_plugin_ids().plugin_id;

        request_permission(&[
            PermissionType::ReadApplicationState, // session and pane metadata
            PermissionType::ChangeApplicationState, // focus a pane, switch session
            PermissionType::RunCommands,          // read state files, open a preview
            PermissionType::OpenTerminalsOrPlugins, // start an agent pane with `n`
            PermissionType::WriteToStdin,         // the interrupt byte, and nothing else
        ]);
        subscribe(&[
            EventType::Timer,
            EventType::SessionUpdate,
            EventType::RunCommandResult,
            EventType::Key,
            EventType::PermissionRequestResult,
        ]);

        // The sidebar is a pane you navigate to like any other — and so is the
        // palette, which has to hold focus to be typed into at all. Unselectable
        // meant focus bounced straight back to the sidebar, whose next keystroke
        // closes whatever floating instance it opened.
        set_selectable(true);
        // The keybind list never changes, so it has nothing to wake up for.
        if !self.config.help {
            set_timeout(TICK_SECONDS);
        }
    }

    fn update(&mut self, event: Event) -> bool {
        match event {
            Event::Timer(_) => {
                self.tick();
                false
            }
            Event::RunCommandResult(exit, stdout, _, context) => {
                if context.get(scan::CONTEXT_KEY).map(String::as_str) == Some(scan::CONTEXT_HOST) {
                    let host = context.get(scan::CONTEXT_PANE).cloned().unwrap_or_default();
                    return self.absorb_host(&host, exit, &stdout);
                }
                self.absorb_scan(&stdout, &context)
            }
            Event::SessionUpdate(sessions, _) => {
                // Zellij sends this every second whether or not anything moved.
                // An identical pane list means there is nothing to reconcile and
                // nothing new to draw, and redrawing anyway cost a second full
                // render every second, forever. The scan tick keeps ages fresh.
                let panes = snapshot::panes(&sessions);
                if panes == self.panes && self.tab.is_some() && self.named {
                    return false;
                }
                self.panes = panes;
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
                self.tab = snapshot::own_tab(&sessions, self.plugin_id).or(self.tab);
                if self.own_url.is_none() {
                    self.own_url = snapshot::own_url(&sessions, self.plugin_id);
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
        if self.config.help {
            render::draw_peek(&agenttij_core::help::lines(cols), rows, cols);
            return;
        }
        if self.config.peek.is_some() {
            render::draw_peek(&self.peeked, rows, cols);
            return;
        }
        if self.config.jump {
            self.palette.draw(rows, cols, &self.config.colors);
            return;
        }

        let prompt = self.prompt();
        let on_screen = self.slot();
        let on_screen_row =
            on_screen.and_then(|pane| self.groups.group_of(pane).map(Group::primary));
        render::draw(&render::View {
            on_screen,
            on_screen_row,
            rows,
            cols,
            prompt: prompt.as_deref(),
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
        // The jump has landed by now, so this pane can go without taking the
        // focus with it.
        if self.leaving {
            close_self();
            return;
        }
        set_timeout(TICK_SECONDS);

        // One pane per tick rather than everything at once: the answers go stale
        // slowly (a `cd`, a program starting), and a burst of round-trips is
        // exactly what makes a keypress wait.
        let known: BTreeSet<u32> = self
            .cwds
            .keys()
            .chain(self.programs.keys())
            .copied()
            .collect();
        let known: Vec<u32> = known.into_iter().collect();
        if !known.is_empty() {
            self.refresh_next = (self.refresh_next + 1) % known.len();
            let pane = known[self.refresh_next];
            self.cwds.remove(&pane);
            self.programs.remove(&pane);
        }

        if self.permissions == Permissions::Denied || self.config.help {
            return;
        }

        // The arrangement, once. It outlives the plugin, so a reload — or
        // tomorrow — finds the projects where you left them.
        if !self.order_read {
            self.order_read = true;
            let command = scan::read_order_command();
            let words: Vec<&str> = command.iter().map(String::as_str).collect();
            let context =
                BTreeMap::from([(scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_ORDER.to_owned())]);
            run_command(&words, context);
        }

        if let Some((session, pane)) = self.config.peek.clone() {
            let command = scan::dump_command(&self.config.peek_host, &session, pane);
            let words: Vec<&str> = command.iter().map(String::as_str).collect();
            let context =
                BTreeMap::from([(scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_PEEK.to_owned())]);
            run_command(&words, context);
            return;
        }

        // Which sessions are alive, from the host rather than from a forked
        // `zellij list-sessions`. It has to come from here and not `SessionUpdate`:
        // that only learns of other sessions through files a session with
        // `session_serialization false` never writes, while this walks the socket
        // directory, which is true the moment a session starts.
        if self.sessions_in == 0 {
            self.sessions_in = SESSION_TICKS;
            if let Ok(sessions) = get_session_list() {
                self.live_sessions = sessions
                    .live_sessions
                    .iter()
                    .map(|session| session.name.clone())
                    .collect();
                // Only the palette offers these, and only it pays for keeping
                // them: a dead session is not something to watch.
                if self.config.jump {
                    self.dead_sessions = sessions
                        .resurrectable_sessions
                        .iter()
                        .map(|(name, _)| name.clone())
                        .collect();
                }
            }
        }
        self.sessions_in = self.sessions_in.saturating_sub(1);

        for host in self.config.hosts.clone() {
            let due = self.hosts_in.entry(host.clone()).or_default();
            if *due > 0 {
                *due -= 1;
                continue;
            }
            *due = HOST_TICKS;

            let command = scan::host_command(&host);
            let words: Vec<&str> = command.iter().map(String::as_str).collect();
            run_command(
                &words,
                BTreeMap::from([
                    (scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_HOST.to_owned()),
                    (scan::CONTEXT_PANE.to_owned(), host),
                ]),
            );
        }

        let context =
            BTreeMap::from([(scan::CONTEXT_KEY.to_owned(), scan::CONTEXT_SCAN.to_owned())]);
        run_command(&scan::command(), context);
    }

    /// What a machine answered, or that it did not.
    ///
    /// A host that fails is asked again much later rather than dropped: a laptop
    /// closing its lid should not delete the agents it had, it should stop
    /// mentioning them — which is what happens, since its rows go with its answer.
    fn absorb_host(&mut self, host: &str, exit: Option<i32>, stdout: &[u8]) -> bool {
        if exit != Some(0) {
            self.remote.remove(host);
            self.hosts_in.insert(host.to_owned(), HOST_BACKOFF);
            self.rebuild();
            return true;
        }

        let text = String::from_utf8_lossy(stdout);
        // The host is stamped on as it is read: the file on that machine has no
        // idea it is being looked at from here.
        let tagged: String = text
            .lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| format!("{line}\t{host}\n"))
            .collect();

        let agents = scan::parse(&format!("0\n{tagged}").into_bytes())
            .map(|scan| scan.agents)
            .unwrap_or_default();
        self.remote.insert(host.to_owned(), agents);
        self.rebuild();
        true
    }

    fn absorb_scan(&mut self, stdout: &[u8], context: &BTreeMap<String, String>) -> bool {
        if context.get(scan::CONTEXT_KEY).map(String::as_str) == Some(scan::CONTEXT_ORDER) {
            self.arrangement = order::decode(&String::from_utf8_lossy(stdout));
            self.rebuild();
            return true;
        }
        if context.get(scan::CONTEXT_KEY).map(String::as_str) == Some(scan::CONTEXT_PROJECT) {
            let project = String::from_utf8_lossy(stdout).trim().to_owned();
            let pane = context
                .get(scan::CONTEXT_PANE)
                .and_then(|pane| pane.parse::<u32>().ok());
            if let (Some(pane), false) = (pane, project.is_empty()) {
                if let Some((_, known)) = self.roots.get_mut(&pane) {
                    *known = project;
                }
            }
            self.rebuild();
            return true;
        }
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

        let before = std::mem::replace(&mut self.reported, result.agents);
        if self.config.jump {
            // Panes nothing reports on are places you go to as much as agents
            // are — a shell you left `cd`ed somewhere is exactly the thing you
            // cannot otherwise find.
            let mut everything = self.reported.clone();
            for agents in self.remote.values() {
                everything.extend(agents.iter().cloned());
            }
            everything.extend(panes::list_panes(
                &self.panes,
                &everything,
                &self.current_session,
            ));
            self.palette.refresh(agenttij_core::jump::entries(
                &everything,
                &self.live_sessions,
                &self.dead_sessions,
                &self.current_session,
            ));
            return true;
        }
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
        let mut reported = self.reported.clone();
        for agents in self.remote.values() {
            reported.extend(agents.iter().cloned());
        }
        let mut agents = panes::reconcile(reported, &self.panes, &self.live_sessions);
        let discovered = panes::discover(&self.panes, &agents, &self.config.agents);
        agents.extend(discovered);

        // In solo mode a row is a group of panes, so the rows are the groups:
        // one per agent, with its companions folded inside rather than listed.
        if self.config.solo {
            agents = self.group_rows(agents);
        }

        // Only what needs you: rows, not the panes underneath them — a pane is
        // never the thing that is blocked.
        if self.only_blocked {
            agents.retain(|agent| agent.status == Status::NeedsInput);
        }

        if self.config.scope == Scope::Session {
            agents.retain(|agent| agent.session == self.current_session);
        }
        agent::sort_for_display(&mut agents, &self.current_session);
        let mut agents = self.arranged(agents);
        if self.config.solo {
            agents = self.with_expanded_panes(agents);
        }

        // Before folding, which drops rows the sidebar still has to be able to
        // act on.
        self.projects.clear();
        self.project_roots.clear();
        for row in agents.iter().filter(|row| row.kind == Kind::Row) {
            let key = project::key(row, &self.arrangement.names).to_owned();
            self.project_roots
                .entry(key.clone())
                .or_default()
                .insert(project::root(row).to_owned());
            // Only this session's rows can be shown, closed or interrupted.
            if row.session == self.current_session {
                self.projects.entry(key).or_default().push(row.pane);
            }
        }
        let agents = project::group(agents, &self.arrangement.folded, &self.arrangement.names);

        self.agents = agents;
        self.resync_selection();
        self.label_positions();
    }

    /// Keeps the cursor on a real row after the list changes underneath it.
    fn resync_selection(&mut self) {
        let still_there = self
            .selected
            .as_ref()
            .is_some_and(|selection| self.find(selection).is_some());

        // A pane opened a moment ago is not a row yet — it becomes one when the
        // manifest catches up — so a selection pointing at a live pane is
        // waiting, not stale, and moving the cursor off it would undo the
        // following that put it there.
        let arriving = matches!(
            self.selected.as_ref(),
            Some(Selection::Row { session, pane })
                if self.panes.iter().any(|snapshot| {
                    snapshot.session == *session && snapshot.pane == *pane
                })
        );

        if !still_there && !arriving {
            self.selected = self.agents.first().map(Selection::of);
        }
    }

    fn on_key(&mut self, key: KeyWithModifier) -> bool {
        // Leave modified keys to Zellij, so the sidebar never eats a binding —
        // except Shift, which is not a binding, it is a capital letter. Some
        // terminals report it as one and some report the modifier separately, so
        // both arrive here as the capital.
        let shift = key.key_modifiers.len() == 1 && key.key_modifiers.contains(&KeyModifier::Shift);
        if !key.key_modifiers.is_empty() && !shift {
            return false;
        }
        let bare_key = match key.bare_key {
            BareKey::Char(letter) if shift => BareKey::Char(letter.to_ascii_uppercase()),
            other => other,
        };

        // The palette reads everything: a q is a letter here.
        if self.config.jump {
            return match self.palette.key(bare_key) {
                Act::Stay => true,
                Act::Close => {
                    close_self();
                    false
                }
                Act::Go(target) => {
                    actions::go_to_target(&target, &self.current_session);
                    self.leaving = true;
                    set_timeout(0.1);
                    false
                }
            };
        }

        // A peek is a still picture of someone else's pane: there is nothing to
        // type into it, so any key dismisses it. This is why a peek is a plugin
        // pane and not a command pane — a command pane cannot read a key at all,
        // and a floating pane is only visible while it holds focus, so the two
        // together made a peek impossible to dismiss.
        if self.config.peek.is_some() || self.config.help {
            close_self();
            return false;
        }

        // Naming a project takes every key until it is done, so a name can
        // contain a q, a d, or anything else that means something out here.
        if let Some((project, typed)) = self.renaming.take() {
            return self.type_name(project, typed, bare_key);
        }

        // A peek is a still picture over your work, so the next key dismisses it
        // whatever that key is — and then still does its job, so peeking never
        // costs you a keystroke. q and Esc are the keys that only dismiss.
        let had_peek = self.peek.is_some();
        self.close_peek();
        // Any key at all disarms a pending delete, including the one that then
        // does something else entirely.
        let armed = self.pending.take();

        match bare_key {
            BareKey::Char('q') | BareKey::Esc => had_peek,
            BareKey::Down | BareKey::Char('j') => self.move_cursor(1),
            BareKey::Up | BareKey::Char('k') => self.move_cursor(-1),
            BareKey::Enter => {
                let Some(agent) = self.selected_agent().cloned() else {
                    return false;
                };
                // A folded project has nothing to go to until it is open, so
                // Enter opens it; an open one goes to the first row inside.
                if let Kind::Project { folded } = agent.kind {
                    let project = project::key(&agent, &self.arrangement.names).to_owned();
                    if folded {
                        self.arrangement.folded.remove(&project);
                        self.save_order();
                        self.rebuild();
                        return true;
                    }
                    if let Some(row) = self.rows_of(&project).first().copied() {
                        self.switch_to_row(row);
                    }
                    return false;
                }

                // A session belongs to the machine running it, so there is
                // nothing to switch to: open a pane that is attached to it.
                if !agent.host.is_empty() {
                    actions::attach(&agent.host, &agent.session);
                    return false;
                }

                let here = agent.session == self.current_session;
                if self.config.solo && here && agent.kind == Kind::Pane {
                    self.show_pane(agent.pane);
                } else if self.config.solo && here {
                    self.switch_to_row(agent.pane);
                } else {
                    actions::go_to(&agent, &self.current_session, &self.panes);
                }
                false
            }
            // Everywhere you could go, filtered by typing.
            BareKey::Char('/') => {
                self.close_peek();
                if let Some(url) = self.own_url.clone() {
                    self.peek = actions::jump(&url);
                }
                false
            }
            // Every key and what it does, for when you have forgotten.
            BareKey::Char('?') => {
                self.close_peek();
                if let Some(url) = self.own_url.clone() {
                    self.peek = actions::help(&url);
                }
                false
            }
            // Show or hide a row's other panes, so you can go straight to one.
            BareKey::Tab => {
                if let Some(agent) = self.selected_agent().cloned() {
                    if let Kind::Project { folded } = agent.kind {
                        let project = project::key(&agent, &self.arrangement.names).to_owned();
                        if folded {
                            self.arrangement.folded.remove(&project);
                        } else {
                            self.arrangement.folded.insert(project);
                        }
                        self.save_order();
                        self.rebuild();
                        return true;
                    }
                    let row = if agent.kind == Kind::Pane {
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
            // Move what the cursor is on, rather than the cursor: J and K put the
            // list in the order you want it in.
            BareKey::Char('J') => self.shift_selection(true),
            BareKey::Char('K') => self.shift_selection(false),
            // Name a project — and name two of them the same to make them one.
            BareKey::Char('r') => {
                let Some(agent) = self.selected_agent() else {
                    return false;
                };
                if matches!(agent.kind, Kind::Project { .. }) {
                    let project = project::key(agent, &self.arrangement.names).to_owned();
                    self.renaming = Some((project, String::new()));
                }
                true
            }
            // Project to project, wrapping, so a long list is a couple of keys
            // rather than a scroll.
            BareKey::Char(']') => self.jump_project(1),
            BareKey::Char('[') => self.jump_project(-1),
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
            // Close what the cursor is on: a row takes its panes with it, a
            // pane inside an opened row goes on its own. Panes in another
            // session are not ours to close, so `d` does not arm on them.
            BareKey::Char('d') => self.arm(Pending::Close, armed),
            // Ctrl-C to the agent without going to it: the reason to watch a
            // runaway is to stop it.
            BareKey::Char('c') => self.arm(Pending::Interrupt, armed),
            // Only what needs you. The list is a status board; this is the
            // question it exists to answer, so it gets one key.
            BareKey::Char('!') => {
                self.only_blocked = !self.only_blocked;
                self.rebuild();
                true
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
        self.selected = Some(Selection::of(&self.agents[next]));
        true
    }

    /// Closes the selection, and puts something back on screen if it was what
    /// you were looking at.
    fn delete(&mut self, agent: &Agent) {
        let closing: Vec<u32> = match agent.kind {
            // A project takes every row in it, and every row takes its panes.
            Kind::Project { .. } => self
                .rows_of(project::key(agent, &self.arrangement.names))
                .iter()
                .flat_map(|row| self.groups.closing(*row, true))
                .collect(),
            Kind::Row => self.groups.closing(agent.pane, true),
            Kind::Pane => vec![agent.pane],
        };
        if closing.is_empty() {
            return;
        }
        let slot = self.slot();

        // What takes the slot afterwards, worked out now while what is closing is
        // still in the list: groups hear about the closure on the next pane
        // update, so they cannot be asked once it has happened.
        let successor = self.successor(agent, &closing);

        for pane in &closing {
            // Show it first, even when it is already on screen. Zellij reads a
            // close for a *suppressed* pane as "put it back": `close_pane`
            // (tab/mod.rs) hands it to `replace_pane_with_suppressed_pane`
            // instead of closing it, which un-suppresses it over whatever is on
            // screen — measured: deleting a row of three emptied the tab, taking
            // the sidebar with it. Showing a pane that is already visible only
            // costs a log line, and asking our pane list which ones are parked
            // would trust it for a second longer than it is true.
            show_pane_with_id(PaneId::Terminal(*pane), false, false);
            close_pane_with_id(PaneId::Terminal(*pane));
        }
        if agent.kind == Kind::Row {
            self.expanded.remove(&agent.pane);
        }
        if let Kind::Project { .. } = agent.kind {
            self.arrangement
                .folded
                .remove(project::key(agent, &self.arrangement.names));
            self.save_order();
        }
        if self.previous_row.is_some_and(|row| closing.contains(&row)) {
            self.previous_row = None;
        }
        // The cursor follows the slot rather than jumping to the top; the next
        // rebuild repairs it if what it lands on is gone too.
        self.selected = successor
            .filter(|_| agent.kind != Kind::Pane)
            .map(|pane| Selection::Row {
                session: self.current_session.clone(),
                pane,
            });

        // Closing what was on screen leaves the workspace empty, and a parked
        // pane does not come back on its own — so show whatever follows it.
        let emptied = slot.is_some_and(|slot| closing.contains(&slot));
        if let Some(target) = successor.filter(|_| emptied) {
            self.groups.show(target);
            actions::show_in_slot(target, None);
        }
    }

    /// First press arms, second press does it, and the footer says what "it" is.
    /// Anything in another session is not ours to close or signal, so `d` and `c`
    /// do not arm on it.
    fn arm(&mut self, action: Pending, armed: Option<(Pending, Selection)>) -> bool {
        let Some(agent) = self.selected_agent().cloned() else {
            return false;
        };
        let target = Selection::of(&agent);
        let ours =
            agent.session == self.current_session || matches!(agent.kind, Kind::Project { .. });

        if armed == Some((action, target.clone())) {
            match action {
                Pending::Close => self.delete(&agent),
                Pending::Interrupt => self.interrupt(&agent),
            }
        } else if ours {
            self.pending = Some((action, target));
        }
        true
    }

    /// Interrupts the agent: the byte Ctrl-C puts on the terminal, which makes
    /// the tty signal whatever is in the foreground. The pane stays; only what it
    /// is running stops.
    ///
    /// `send_sigint_to_pane_id` is the obvious call and does not work here. It
    /// signals the pane's own child (`pty.rs`, `send_sigint_to_pane`), which is
    /// the shell — and an interactive shell ignores SIGINT, so the agent under it
    /// carries on. Measured: a `sleep` in the pane survived it untouched.
    fn interrupt(&mut self, agent: &Agent) {
        for pane in self.interrupting(agent) {
            write_to_pane_id(vec![INTERRUPT], PaneId::Terminal(pane));
        }
    }

    /// A row means its agent, a pane means itself, and a project means every
    /// agent in it.
    fn interrupting(&self, agent: &Agent) -> Vec<u32> {
        match agent.kind {
            Kind::Project { .. } => self.rows_of(project::key(agent, &self.arrangement.names)),
            Kind::Row | Kind::Pane => vec![agent.pane],
        }
    }

    /// What should take the slot when these panes are gone: the row below a
    /// closed row, another pane of the row when one pane went, and for a project
    /// the first row that is not going with it.
    fn successor(&self, agent: &Agent, closing: &[u32]) -> Option<u32> {
        let row = match agent.kind {
            Kind::Pane => {
                return self
                    .groups
                    .group_of(agent.pane)
                    .and_then(|group| group.members.iter().find(|member| **member != agent.pane))
                    .copied()
            }
            Kind::Row => agent::row_after(&self.agents, &self.current_session, agent.pane)
                .map(|row| row.pane),
            Kind::Project { .. } => self
                .agents
                .iter()
                .find(|other| {
                    other.kind == Kind::Row
                        && other.session == self.current_session
                        && !closing.contains(&other.pane)
                })
                .map(|other| other.pane),
        };
        row.map(|row| self.groups.current_of(row).unwrap_or(row))
    }

    /// A keystroke while a name is being typed. Enter keeps it, Esc drops it,
    /// and anything else is either a letter or ignored.
    fn type_name(&mut self, project: String, mut typed: String, key: BareKey) -> bool {
        match key {
            BareKey::Enter => self.name_project(&project, typed.trim()),
            // Dropped: `renaming` was taken on the way in.
            BareKey::Esc => {}
            BareKey::Backspace => {
                typed.pop();
                self.renaming = Some((project, typed));
            }
            BareKey::Char(letter) => {
                typed.push(letter);
                self.renaming = Some((project, typed));
            }
            _ => self.renaming = Some((project, typed)),
        }
        true
    }

    /// Calls a project something, which is also how two of them become one: the
    /// name goes on every git root inside it, so anything started in any of them
    /// lands here from now on. An empty name takes the roots back to being
    /// themselves.
    fn name_project(&mut self, project: &str, name: &str) {
        let Some(mut roots) = self.project_roots.get(project).cloned() else {
            return;
        };

        // Anything already called this joins it. "Two projects with the same
        // name are one" has to mean the name on screen: a project still keyed by
        // its path shows the last part of that path, so naming another one
        // `dotfiles` when a `~/code/dotfiles` is sitting there has to merge them,
        // or you get two identical-looking headers and no way to tell why.
        if !name.is_empty() {
            for (key, others) in &self.project_roots {
                if project::display(key) == name {
                    roots.extend(others.iter().cloned());
                }
            }
        }

        for root in roots {
            if name.is_empty() {
                self.arrangement.names.remove(&root);
            } else {
                self.arrangement.names.insert(root, name.to_owned());
            }
        }

        // Follow the project to whatever it is called now.
        self.selected = (!name.is_empty()).then(|| Selection::Project(name.to_owned()));
        self.save_order();
        self.rebuild();
    }

    /// Moves the selected project among projects, or the selected row among the
    /// rows of its project. Panes inside a row keep the order they joined in.
    ///
    /// Against what is on screen, not against what is remembered: the order you
    /// are looking at is the one you are rearranging.
    fn shift_selection(&mut self, down: bool) -> bool {
        let Some(agent) = self.selected_agent().cloned() else {
            return false;
        };

        match agent.kind {
            Kind::Project { .. } => {
                let natural: Vec<String> = self
                    .agents
                    .iter()
                    .filter(|other| matches!(other.kind, Kind::Project { .. }))
                    .map(|other| project::key(other, &self.arrangement.names).to_owned())
                    .collect();
                let key = project::key(&agent, &self.arrangement.names).to_owned();
                order::shift(&mut self.arrangement.projects, &natural, &key, down);
            }
            Kind::Row => {
                let project = project::key(&agent, &self.arrangement.names).to_owned();
                let natural: Vec<(String, u32)> = self
                    .agents
                    .iter()
                    .filter(|other| {
                        other.kind == Kind::Row
                            && project::key(other, &self.arrangement.names) == project
                    })
                    .map(|other| (other.session.clone(), other.pane))
                    .collect();
                let key = (agent.session.clone(), agent.pane);
                let remembered = self.arrangement.rows.entry(project).or_default();
                order::shift(remembered, &natural, &key, down);
            }
            // A pane's place in its row is the order it joined in, which is the
            // only thing `v` can cycle through predictably.
            Kind::Pane => return false,
        }

        self.save_order();
        self.rebuild();
        true
    }

    /// Writes the arrangement out, so a reload does not lose it. Only on a move,
    /// which is rare — and last writer wins, which is the right answer when two
    /// sidebars disagree about where a project belongs.
    fn save_order(&self) {
        let text = order::encode(&self.arrangement);
        let command = scan::write_order_command(&text);
        let words: Vec<&str> = command.iter().map(String::as_str).collect();
        run_command(&words, BTreeMap::new());
    }

    /// The rows in the order you asked for: projects first, each project's rows
    /// inside it, and anything nobody has moved left where the sort put it.
    ///
    /// One pass over the flat list, before the panes and the headers go in:
    /// grouping keeps the relative order it is given, so arranging the rows here
    /// arranges the projects too.
    fn arranged(&self, rows: Vec<Agent>) -> Vec<Agent> {
        if self.arrangement.projects.is_empty() && self.arrangement.rows.is_empty() {
            return rows;
        }

        let mut projects: Vec<String> = Vec::new();
        for row in rows.iter().filter(|row| row.kind == Kind::Row) {
            let key = project::key(row, &self.arrangement.names).to_owned();
            if !projects.contains(&key) {
                projects.push(key);
            }
        }
        let projects = order::arrange(projects, &self.arrangement.projects, String::clone);

        let mut wanted: Vec<(String, u32)> = Vec::new();
        for project in &projects {
            let mine: Vec<(String, u32)> = rows
                .iter()
                .filter(|row| {
                    row.kind == Kind::Row && project::key(row, &self.arrangement.names) == project
                })
                .map(|row| (row.session.clone(), row.pane))
                .collect();
            let remembered = self
                .arrangement
                .rows
                .get(project)
                .map(Vec::as_slice)
                .unwrap_or(&[]);
            wanted.extend(order::arrange(mine, remembered, Clone::clone));
        }

        order::arrange(rows, &wanted, |row| (row.session.clone(), row.pane))
    }

    /// Moves the cursor to the next project header, wrapping at the ends.
    fn jump_project(&mut self, delta: isize) -> bool {
        let headers: Vec<usize> = self
            .agents
            .iter()
            .enumerate()
            .filter(|(_, agent)| matches!(agent.kind, Kind::Project { .. }))
            .map(|(at, _)| at)
            .collect();
        let here = self.cursor();

        let target = if delta > 0 {
            headers.iter().find(|at| **at > here).or(headers.first())
        } else {
            headers
                .iter()
                .rev()
                .find(|at| **at < here)
                .or(headers.last())
        };
        let Some(target) = target.and_then(|at| self.agents.get(*at)) else {
            return false;
        };
        self.selected = Some(Selection::of(target));
        true
    }

    /// What an armed key would take, so the second press is never a surprise —
    /// and, with nothing armed, what the list is currently hiding.
    fn prompt(&self) -> Option<String> {
        if let Some((_, typed)) = self.renaming.as_ref() {
            return Some(format!("name: {typed}▏"));
        }
        let Some((action, selection)) = self.pending.as_ref() else {
            return self.only_blocked.then(|| "⚠ only — ! shows all".to_owned());
        };
        let agent = self.agents.get(self.find(selection)?)?;

        if *action == Pending::Interrupt {
            return Some(match self.interrupting(agent).len() {
                0 | 1 => format!("interrupt {}? c", agent.label()),
                agents => format!("interrupt {} ×{agents}? c", agent.label()),
            });
        }

        let panes = match agent.kind {
            Kind::Project { .. } => self
                .rows_of(project::key(agent, &self.arrangement.names))
                .iter()
                .map(|row| self.groups.closing(*row, true).len())
                .sum(),
            Kind::Row => self.groups.closing(agent.pane, true).len(),
            Kind::Pane => 1,
        };
        Some(match panes {
            0 | 1 => format!("close {}? d", agent.label()),
            panes => format!("close {} +{}? d", agent.label(), panes - 1),
        })
    }

    /// Names our pane, once. Renaming needs ChangeApplicationState, and
    /// permissions arrive asynchronously, so `load` is too early — a command
    /// issued before the grant is denied in silence.
    fn name_self(&mut self) {
        if self.named || self.permissions != Permissions::Granted {
            return;
        }
        // The sidebar has to wait: its own url is read from this very title, and
        // renaming first would erase it. A peek or the help list never launches
        // anything, so it has nothing to wait for.
        let needs_url = self.config.peek.is_none() && !self.config.help;
        if needs_url && self.own_url.is_none() {
            return;
        }
        self.named = true;
        rename_plugin_pane(self.plugin_id, &self.config.title);
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

        // What is running in each pane of an expanded row, asked once per pane.
        let expanded: Vec<u32> = self
            .expanded
            .iter()
            .flat_map(|primary| self.groups.members_of(*primary).to_vec())
            .filter(|member| !self.programs.contains_key(member))
            .collect();
        for member in expanded {
            let command = get_pane_running_command(PaneId::Terminal(member)).unwrap_or_default();
            // The fallback is filled in at render time, where the row is known.
            self.programs
                .insert(member, panes::program_name(&command, ""));
        }

        // A row is named after the project it is in. An agent reports its own cwd
        // through the hook; a plain pane has to be asked, because its title is a
        // shell prompt and says whatever the user's prompt says.
        for row in rows.iter_mut().filter(|row| row.cwd.is_empty()) {
            let cwd = match self.cwds.get(&row.pane) {
                Some(cwd) => cwd.clone(),
                None => {
                    let cwd = get_pane_cwd(PaneId::Terminal(row.pane))
                        .map(|cwd| cwd.to_string_lossy().into_owned())
                        .unwrap_or_default();
                    self.cwds.insert(row.pane, cwd.clone());
                    cwd
                }
            };
            row.cwd = cwd;
        }
        // Nothing resolved a project for these — a plain pane never reported —
        // so ask the same question the hook answers, once per directory. The
        // directory stands in until the answer lands, which is also the answer
        // when there is no marker and no repository.
        for row in rows.iter_mut().filter(|row| row.root.is_empty()) {
            if row.cwd.is_empty() {
                continue;
            }
            match self.roots.get(&row.pane) {
                Some((asked, project)) if *asked == row.cwd => row.root = project.clone(),
                _ => {
                    self.roots
                        .insert(row.pane, (row.cwd.clone(), row.cwd.clone()));
                    row.root = row.cwd.clone();

                    let command = scan::project_command(&row.cwd);
                    let words: Vec<&str> = command.iter().map(String::as_str).collect();
                    run_command(
                        &words,
                        BTreeMap::from([
                            (
                                scan::CONTEXT_KEY.to_owned(),
                                scan::CONTEXT_PROJECT.to_owned(),
                            ),
                            (scan::CONTEXT_PANE.to_owned(), row.pane.to_string()),
                        ]),
                    );
                }
            }
        }

        rows.extend(remote);
        rows
    }

    /// Names each pane in a row `<row> 2/3`, so a pane says where it sits in its
    /// row while the sidebar is folded away or off screen.
    ///
    /// Renaming pins a pane's title, which otherwise follows the running command
    /// — hence `position "false"` for anyone who would rather keep that.
    fn label_positions(&mut self) {
        if !self.config.position || !self.config.solo {
            return;
        }

        // Only the pane on screen. A hidden pane has been lifted out of the tab
        // and a rename does not reach it — and it does not need one, since the
        // point is telling you where *this* pane sits while you look at it.
        let Some(visible) = self.slot() else { return };
        let Some(group) = self.groups.group_of(visible) else {
            return;
        };
        let (members, primary) = (group.members.clone(), group.primary());

        let Some(index) = members.iter().position(|member| *member == visible) else {
            return;
        };
        let label = self
            .agents
            .iter()
            .find(|agent| agent.pane == primary)
            .map(|agent| agent.label().to_owned())
            .unwrap_or_default();

        let Some(name) = format::pane_position(&label, index, members.len()) else {
            return;
        };
        if self.positions.get(&visible) == Some(&name) {
            return;
        }
        rename_pane_with_id(PaneId::Terminal(visible), &name);
        self.positions.insert(visible, name);
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
                // What is running in it, not the pane title: a shell's title is
                // its prompt, which is the row's own name three times over.
                // From the cache `group_rows` filled a moment ago — asking the
                // host here instead cost one round-trip per hidden pane per
                // rebuild, which on a row of seven is what made j and k crawl.
                let title = self
                    .programs
                    .get(&member)
                    .filter(|program| !program.is_empty())
                    .cloned()
                    .unwrap_or_else(|| row.label().to_owned());

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
        panes::visible_terminal(&self.panes, &self.current_session, self.tab?)
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
        self.selected = Some(Selection::Row {
            session: self.current_session.clone(),
            pane: primary,
        });
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
        let opened = actions::new_in_slot(&self.panes, &self.current_session, self.config.solo);
        // Follow it. Otherwise the cursor stays on the row you left while the
        // screen shows the one you just made, and the next key goes somewhere
        // you are not looking — which is what `Alt g` felt like.
        if let Some(PaneId::Terminal(pane)) = opened {
            self.selected = Some(Selection::Row {
                session: self.current_session.clone(),
                pane,
            });
        }
    }

    fn close_peek(&mut self) {
        if let Some(peek) = self.peek.take() {
            close_pane_with_id(peek);
        }
    }

    fn cursor(&self) -> usize {
        self.selected
            .as_ref()
            .and_then(|selection| self.find(selection))
            .unwrap_or(0)
    }

    fn selected_agent(&self) -> Option<&Agent> {
        self.agents.get(self.cursor())
    }

    fn find(&self, selection: &Selection) -> Option<usize> {
        self.agents
            .iter()
            .position(|agent| Selection::of(agent) == *selection)
    }

    /// The rows a project holds, whether or not it is folded open.
    fn rows_of(&self, project: &str) -> Vec<u32> {
        self.projects.get(project).cloned().unwrap_or_default()
    }

    fn notice(&self) -> Option<&'static str> {
        match self.permissions {
            Permissions::Pending => Some("waiting for permissions"),
            Permissions::Denied => Some("permissions denied"),
            // "no agents" would be a lie while a filter is on.
            Permissions::Granted if self.only_blocked && self.agents.is_empty() => {
                Some("nothing needs you")
            }
            Permissions::Granted => None,
        }
    }
}
