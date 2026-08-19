//! What the sidebar does with the agent you picked: peek at it, or go to it.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

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
pub fn go_to_target(
    target: &agenttij_core::jump::Target,
    current_session: &str,
    slot: Option<u32>,
    solo: bool,
) {
    use agenttij_core::jump::Target;
    match target {
        // The same as picking it in the sidebar: takes the slot rather than
        // splitting it, when the palette was told this is a solo workspace.
        Target::Remote { host, session } => {
            attach(host, session, slot, solo);
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
        // Neither of these is somewhere to *go*, and both need the template and
        // the slot the sidebar owns. See `ask_for_row` and `ask_for_workspace`.
        Target::Dir { .. } | Target::Workspace { .. } => {}
    }
}

/// Asks the sidebar in this session to open a row in `path`.
///
/// Through a file on the scan rather than a message: `pipe_message_to_plugin`
/// needs `MessageAndLaunchOtherPlugins`, which this plugin does not request —
/// an ungranted permission means a prompt in a pane too narrow to read it, and
/// a sidebar that silently does nothing.
pub fn ask_for_row(session: &str, now: u64, path: &str) {
    let command = agenttij_core::scan::open_at_command(session, now, path);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
}

/// Asks the sidebar in this session to build a remembered workspace again.
///
/// The name is the whole message: the sidebar holds the arrangement file, so it
/// already knows which rows that workspace had and where they worked.
pub fn ask_for_workspace(session: &str, now: u64, workspace: &str) {
    let command = agenttij_core::scan::restore_command(session, now, workspace);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
}

/// Takes the request back off the pile, once the row exists.
pub fn clear_row_request() {
    let command = agenttij_core::scan::clear_open_command();
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
}

/// Opens the directory picker: this plugin again, floating, listing places to
/// start rather than places to go.
pub fn pick_dir(own_url: &str) -> Option<PaneId> {
    // Exactly what the `Alt G` binding says, or that binding would open a second
    // picker instead of this one.
    let configuration = BTreeMap::from([
        ("dirs".to_owned(), "true".to_owned()),
        ("pane_title".to_owned(), "open in".to_owned()),
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

/// Opens a terminal in a directory, taking the slot like anything else.
pub fn open_at(cwd: &str, slot: Option<u32>, solo: bool) -> Option<PaneId> {
    let opened = open_terminal(PathBuf::from(cwd))?;
    if let Some(slot) = slot.filter(|_| solo) {
        hide_pane_with_id(PaneId::Terminal(slot));
    }
    focus_pane_with_id(opened, false, false);
    Some(opened)
}

/// Writes down the session being left, for the sidebar in the one being entered.
pub fn remember(session: &str) {
    let command = agenttij_core::scan::remember_command(session);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
}

/// Goes back to a session, remembering this one on the way so the flip works in
/// both directions.
pub fn leave_for(session: &str, current_session: &str) {
    remember(current_session);
    switch_session_with_focus(session, None, None);
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

/// Opens a row in the workspace slot, parking whatever was there: the pane you
/// see, and behind it the companions the layout's `group` asks for.
///
/// Two steps rather than one: open the pane normally, then hide the old
/// occupant. Opening *in place of* the slot looks simpler and destroys panes —
/// see `show_in_slot` for why anything built on replacement does.
///
/// An empty template is one plain shell, which is what `a` wants and what a row
/// was before templates existed.
pub fn open_row(
    all_panes: &[PaneSnapshot],
    session: &str,
    solo: bool,
    template: &[String],
    at: Option<&str>,
) -> (Option<u32>, Vec<u32>) {
    let tab = get_focused_pane_info().ok().map(|(tab, _)| tab);
    let slot = tab.and_then(|tab| panes::visible_terminal(all_panes, session, tab));

    // Where the row works: the directory you picked, or the one the row on
    // screen is in — a new row is nearly always more of the same work.
    let cwd = match at {
        Some(at) => PathBuf::from(at),
        None => slot
            .and_then(|slot| get_pane_cwd(PaneId::Terminal(slot)).ok())
            .unwrap_or_else(own_cwd),
    };

    let Some(head) = spawn(
        template.first().map(String::as_str).unwrap_or_default(),
        &cwd,
    ) else {
        return (None, Vec::new());
    };

    // The new pane arrives as a split; hiding the old one collapses it back to
    // one on screen, without the suppression chain a replacement would build.
    if let Some(slot) = slot.filter(|_| solo) {
        hide_pane_with_id(PaneId::Terminal(slot));
    }

    let parked = park(&cwd, template);
    // Last, and after the parking. A pane opened from a plugin takes focus once
    // the handler returns, and the last one opened here is one we suppressed.
    focus_pane_with_id(PaneId::Terminal(head), false, false);
    (Some(head), parked)
}

/// Fills a row out to what the layout asked for: the template's companions,
/// parked behind a pane that is already there.
///
/// A layout can say what its first pane *runs*, but nothing in a layout can park
/// a pane — so without this the row a session opens with would be the one row
/// missing the companions every later row gets.
pub fn fill_row(head: u32, template: &[String]) -> Vec<u32> {
    let cwd = get_pane_cwd(PaneId::Terminal(head)).unwrap_or_else(|_| own_cwd());
    let parked = park(&cwd, template);
    focus_pane_with_id(PaneId::Terminal(head), false, false);
    parked
}

/// Opens the template's companions and hides each one as it arrives.
fn park(cwd: &Path, template: &[String]) -> Vec<u32> {
    let mut parked = Vec::new();
    for command in template.iter().skip(1) {
        if let Some(pane) = spawn(command, cwd) {
            hide_pane_with_id(PaneId::Terminal(pane));
            parked.push(pane);
        }
    }
    parked
}

/// One pane of a row: a command pane when the template says what to run, a
/// plain shell when it does not.
///
/// A command pane is as usable as a shell for this — measured, since the note in
/// AGENTS.md said otherwise: a real keypress reaches a command pane's stdin. It
/// is `write-chars` and the like that never arrive.
fn spawn(command: &str, cwd: &Path) -> Option<u32> {
    let mut words = command.split_whitespace();
    let opened = match words.next() {
        Some(program) => open_command_pane(
            CommandToRun {
                path: PathBuf::from(program),
                args: words.map(str::to_owned).collect(),
                cwd: Some(cwd.to_path_buf()),
            },
            BTreeMap::new(),
        ),
        None => open_terminal(cwd),
    };

    match opened {
        Some(PaneId::Terminal(pane)) => Some(pane),
        // Neither call can hand back a plugin pane, and a row is terminals.
        _ => None,
    }
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

/// Asks the controller on another machine to do something to one of its rows.
pub fn ask(host: &str, session: &str, name: &str, payload: Option<u32>) {
    let command = agenttij_core::scan::remote_pipe_command(host, session, name, payload);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
}

/// Opens a pane inside a session on another machine, where the work is.
pub fn add_remote_pane(host: &str, session: &str, cwd: Option<&str>) {
    let command = agenttij_core::scan::remote_pane_command(host, session, cwd);
    let words: Vec<&str> = command.iter().map(String::as_str).collect();
    run_command(&words, BTreeMap::new());
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
pub fn jump(own_url: &str, solo: bool) -> Option<PaneId> {
    let configuration = BTreeMap::from([
        ("jump".to_owned(), "true".to_owned()),
        ("pane_title".to_owned(), "jump".to_owned()),
        // So a palette opened from a solo sidebar knows to park rather than
        // split when it opens something. One opened by the global keybind knows
        // nothing about your layout and does the safe thing instead.
        ("solo".to_owned(), solo.to_string()),
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
