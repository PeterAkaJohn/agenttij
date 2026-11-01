//! Reading the state files that `hooks/agenttij-state.sh` writes.
//!
//! State lives in files on the host rather than in plugin memory for two
//! reasons: a sidebar in a session that started *later* still sees every
//! running agent, and a sidebar that crashes or reloads loses nothing.
//!
//! We shell out instead of reading the files directly because a plugin's
//! `/tmp` is a mount of `$TMPDIR/zellij`, which is dropped from the mount set
//! if it does not exist when the plugin loads — a state a plugin cannot
//! recover from on its own. A `sh` fork per tick always works.

use crate::agent::{Agent, Status};
use std::collections::BTreeMap;

/// Host directory the hook writes to. Kept in sync with [`SCAN_SCRIPT`] and
/// `hooks/agenttij-state.sh` (see `scan_script_uses_the_state_dir`).
pub const STATE_DIR: &str = "/tmp/agenttij";

/// Prints the current unix time, then one state line per tracked pane.
///
/// It no longer lists sessions. That used to fork a `zellij` client which dialled
/// every session's socket — 12ms against 4ms for the rest of the script — and
/// `get_session_list()` answers the same question as a host call. It has to be
/// that call and not `SessionUpdate`: the update learns about *other* sessions by
/// reading their `session-metadata.kdl`, which is never written when a user has
/// `session_serialization false`, while `get_session_list` scans the socket
/// directory (`scan_session_list_default_dirs`) exactly as the CLI does.
///
/// Trailing `true` keeps the exit code clean when the glob matches nothing,
/// which is the normal case with no agents running — and is also why the
/// directory is not created here: the hook makes it when it has something to
/// write, and a `mkdir` per tick is a fork per tick for nothing.
const SCAN_SCRIPT: &str = "date +%s; cat /tmp/agenttij/from 2>/dev/null; \
     cat /tmp/agenttij/open-at 2>/dev/null; \
     cat /tmp/agenttij/*.state /tmp/agenttij/*.seen 2>/dev/null; true";

/// Marks the session you jumped away from, in the scan output.
const FROM_PREFIX: &str = "from=";

/// When a row was last on screen with someone looking:
/// `seen=<session>\t<pane>\t<unix>`, one file per row beside its state file.
///
/// Beside the state rather than in the arrangement, because the arrangement is
/// read once at load and a look in one session has to reach the sidebars of all
/// the others; the scan reads this directory every tick anyway. A file per row,
/// written only by the sidebar of the session the row is in, so no two writers
/// share one - and never the state file itself, which is the hook's.
const SEEN_PREFIX: &str = "seen=";

/// Records that `pane`, a row of `session`, was looked at `at`.
pub fn seen_command(session: &str, pane: u32, at: u64) -> [String; 7] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        record_look("\"$2\""),
        session.to_owned(),
        pane.to_string(),
        at.to_string(),
        session.replace('/', "_"),
    ]
}

/// The shell that writes a look down: `$0` the session, `$1` the pane, `$3`
/// the session as a file name, and `at` whatever says when.
fn record_look(at: &str) -> String {
    format!(
        "mkdir -p {STATE_DIR} && printf '{SEEN_PREFIX}%s\\t%s\\t%s\\n' \"$0\" \"$1\" {at} \
         > \"{STATE_DIR}/$3.$1.seen\""
    )
}

/// A directory picker asking for a row: `at=<session>\t<unix>\t<path>`.
///
/// A file rather than a message, because messaging another plugin needs
/// `MessageAndLaunchOtherPlugins` — a permission this plugin does not ask for,
/// and an ungranted one leaves a prompt too wide to read in a sidebar. The scan
/// already carries `from=` between instances the same way, so this costs no fork
/// of its own.
const OPEN_PREFIX: &str = "at=";
/// A palette asking for a whole workspace back: `rs=<session>\t<unix>\t<which>`.
/// The sidebar holds the arrangement already, so the name of the workspace is the
/// whole message.
const RESTORE_PREFIX: &str = "rs=";
/// The file that carries either request, beside the state files.
pub const OPEN_FILE: &str = "open-at";

/// Asks the sidebar in `session` to build `workspace` again.
pub fn restore_command(session: &str, now: u64, workspace: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!("mkdir -p {STATE_DIR} && printf '%s' \"$0\" > {STATE_DIR}/{OPEN_FILE}"),
        format!("{RESTORE_PREFIX}{session}\t{now}\t{workspace}\n"),
    ]
}

/// What a picker asked for, if anything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Opening {
    /// The session whose sidebar should do it — the file is machine-wide, and a
    /// row belongs to the session the picker was opened in.
    pub session: String,
    /// When it was asked, on the same host clock the scan reads.
    pub asked_at: u64,
    pub path: String,
}

/// Asks the sidebar in `session` for a row in `path`.
pub fn open_at_command(session: &str, now: u64, path: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!("mkdir -p {STATE_DIR} && printf '%s' \"$0\" > {STATE_DIR}/{OPEN_FILE}"),
        format!("{OPEN_PREFIX}{session}\t{now}\t{path}\n"),
    ]
}

/// Takes it back off the pile, once it has been done.
pub fn clear_open_command() -> [String; 3] {
    [
        "rm".to_owned(),
        "-f".to_owned(),
        format!("{STATE_DIR}/{OPEN_FILE}"),
    ]
}

fn parse_restoring(line: &str) -> Option<Opening> {
    let mut fields = line.split('\t');
    let session = fields.next()?.trim();
    let asked_at = fields.next()?.trim().parse().ok()?;
    let workspace = fields.next()?.trim();
    (!session.is_empty() && !workspace.is_empty()).then(|| Opening {
        session: session.to_owned(),
        asked_at,
        // The field is a path for `at=` and a session name here; both are "what
        // was asked for", and a second struct for one string is not worth it.
        path: workspace.to_owned(),
    })
}

fn parse_opening(line: &str) -> Option<Opening> {
    let mut fields = line.split('\t');
    let session = fields.next()?.trim();
    let asked_at = fields.next()?.trim().parse().ok()?;
    let path = fields.next()?.trim();
    (!session.is_empty() && path.starts_with('/')).then(|| Opening {
        session: session.to_owned(),
        asked_at,
        path: path.to_owned(),
    })
}

/// A row published by a controller: `row <session> <primary> <current> <count>`.
const ROW_PREFIX: &str = "row=";
/// One pane of such a row: `mem <session> <primary> <pane> <name>`. A line each
/// rather than a list, so a name can contain anything but a tab.
const MEMBER_PREFIX: &str = "mem=";

/// What a controller says about one of its rows, so a sidebar on another machine
/// can draw it as a row rather than as a single pane.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Row {
    pub session: String,
    /// The pane the row is named by, and the one a sidebar addresses it as.
    pub primary: u32,
    /// Which member is on screen over there.
    pub current: u32,
    /// How many panes the row owns.
    pub panes: usize,
    /// What that machine calls it. Without this a session where nothing reports —
    /// a shell someone left open — would have no name to draw, and a row with no
    /// name is a row a sidebar cannot show.
    pub name: String,
}

/// One pane of a row on another machine, named after whatever runs in it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Member {
    pub session: String,
    pub primary: u32,
    pub pane: u32,
    pub name: String,
}

/// The file a controller writes beside the state files, and a scan reads with the
/// same `cat`.
///
/// Written only when it changes — a controller that redescribed itself every
/// second would be a fork a second on someone else's machine.
pub fn publish_rows_command(rows: &[Row], members: &[Member]) -> [String; 4] {
    let mut text = String::new();
    for row in rows {
        text.push_str(&format!(
            "{ROW_PREFIX}{}\t{}\t{}\t{}\t{}\n",
            row.session, row.primary, row.current, row.panes, row.name
        ));
    }
    for member in members {
        text.push_str(&format!(
            "{MEMBER_PREFIX}{}\t{}\t{}\t{}\n",
            member.session, member.primary, member.pane, member.name
        ));
    }
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!("mkdir -p {STATE_DIR} && printf '%s' \"$0\" > {STATE_DIR}/rows"),
        text,
    ]
}

fn parse_member(line: &str) -> Option<Member> {
    let mut fields = line.split('\t');
    let session = fields.next()?.trim();
    let primary = fields.next()?.trim().parse().ok()?;
    let pane = fields.next()?.trim().parse().ok()?;
    let name = fields.next().unwrap_or_default().trim();
    (!session.is_empty()).then(|| Member {
        session: session.to_owned(),
        primary,
        pane,
        name: name.to_owned(),
    })
}

fn parse_row(line: &str) -> Option<Row> {
    let mut fields = line.split('\t');
    let session = fields.next()?.trim();
    let primary = fields.next()?.trim().parse().ok()?;
    let current = fields.next()?.trim().parse().ok()?;
    let panes = fields.next()?.trim().parse().ok()?;
    let name = fields.next().unwrap_or_default().trim();
    (!session.is_empty()).then(|| Row {
        session: session.to_owned(),
        primary,
        current,
        panes,
        name: name.to_owned(),
    })
}

/// Records the session being left, so the sidebar in the one being entered can
/// take you back.
///
/// A file rather than plugin memory, because the two sidebars are different
/// instances in different processes — the only thing they share is this
/// directory. It rides along on the scan that already runs, so reading it costs
/// nothing, and the session name goes as an argument rather than inside the
/// script.
/// What Zellij remembers about the sessions it can resurrect.
///
/// Its own serialized layouts, not ours: they exist for every session it ever
/// saved, including the ones that ran before any of this was written down. One
/// grep over all of them rather than a fork each, and the parsing happens in
/// [`parse_past`] where it can be tested.
pub fn past_command() -> [String; 3] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        "stat -c '%n:mtime=%Y' \
         \"${XDG_CACHE_HOME:-$HOME/.cache}\"/zellij/*/session_info/*/session-layout.kdl \
         2>/dev/null; \
         grep -HoE 'agenttij\\.wasm|name=\"[^\"]*\"|cwd=\"[^\"]*\"' \
         \"${XDG_CACHE_HOME:-$HOME/.cache}\"/zellij/*/session_info/*/session-layout.kdl \
         2>/dev/null; true"
            .to_owned(),
    ]
}

/// One session Zellij can bring back, as far as its layout tells us.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Past {
    pub session: String,
    /// Whether a sidebar was in it. A session of someone else's making is still
    /// somewhere to go, it is just not yours.
    pub ours: bool,
    /// The projects it was working on, from the pane titles the sidebar wrote
    /// and any directory the layout kept.
    pub projects: Vec<String>,
    /// When its layout was last written, as a unix time - which is the last
    /// moment the session was doing anything. Zellij's own duration for a dead
    /// session is the layout file's *creation* time (`find_resurrectable_sessions`,
    /// `background_jobs.rs`), so a session made three weeks ago and worked in
    /// until yesterday reads as three weeks old. 0 when nothing said.
    pub seen_at: u64,
}

/// Reads that grep. Lines look like
/// `<cache>/zellij/<version>/session_info/<session>/session-layout.kdl:name="· api 1/3"`.
pub fn parse_past(text: &str) -> Vec<Past> {
    let mut out: Vec<Past> = Vec::new();

    for line in text.lines() {
        let Some((path, found)) = line.split_once(".kdl:") else {
            continue;
        };
        let Some(session) = path
            .rsplit('/')
            .nth(1)
            .filter(|session| !session.is_empty())
        else {
            continue;
        };

        let at = match out.iter().position(|past| past.session == session) {
            Some(at) => at,
            None => {
                out.push(Past {
                    session: session.to_owned(),
                    ..Past::default()
                });
                out.len() - 1
            }
        };

        if found.contains("agenttij.wasm") {
            out[at].ours = true;
            continue;
        }
        if let Some(stamp) = found.strip_prefix("mtime=") {
            out[at].seen_at = stamp.trim().parse().unwrap_or_default();
            continue;
        }
        let Some(value) = found
            .split_once('"')
            .and_then(|(_, rest)| rest.rsplit_once('"'))
            .map(|(value, _)| value)
        else {
            continue;
        };
        let project = match found.starts_with("cwd=") {
            true => crate::project::display(value).to_owned(),
            // A pane title the sidebar wrote: `<glyph> <project> <n>/<m>`. The
            // glyph says nothing here and the position even less.
            false => match pane_project(value) {
                Some(project) => project,
                None => continue,
            },
        };
        if !project.is_empty() && !out[at].projects.contains(&project) {
            out[at].projects.push(project);
        }
    }

    out
}

/// The project out of a pane title the sidebar set, or `None` for a title it did
/// not set (a tab name, the sidebar's own pane, a shell prompt).
fn pane_project(title: &str) -> Option<String> {
    let (glyph, rest) = title.split_once(' ')?;
    // One character, and not a word: every status glyph is a symbol.
    if glyph.chars().count() != 1 || glyph.chars().all(char::is_alphanumeric) {
        return None;
    }
    let project = match rest.rsplit_once(' ') {
        // `2/3` at the end is a position, not part of the name.
        Some((name, position)) if position.contains('/') && !position.contains('.') => name,
        _ => rest,
    };
    (!project.is_empty()).then(|| project.to_owned())
}

/// Whether Zellij made this name up, rather than someone choosing it.
///
/// Its generator joins two lowercase words with a dash (`sparkling-duck`), so a
/// name of that shape is one nobody meant. A session started as `-s api-server`
/// looks the same and would be renamed too, which is why naming after the folder
/// is something a layout asks for rather than the default.
pub fn looks_generated(name: &str) -> bool {
    let Some((left, right)) = name.split_once('-') else {
        return false;
    };
    let word =
        |part: &str| !part.is_empty() && part.chars().all(|letter| letter.is_ascii_lowercase());
    word(left) && word(right)
}

/// Which boot this is.
///
/// A workspace written down *this* boot is the live state of a session that is
/// still running; one from an earlier boot is a snapshot worth restoring. Pane
/// ids are the same story, one boot at a time. Linux keeps this in a file; a
/// system without it gets an empty string, and everything is then treated as
/// this boot, which is what the plugin did before any of this existed.
pub fn boot_command() -> [String; 2] {
    [
        "cat".to_owned(),
        "/proc/sys/kernel/random/boot_id".to_owned(),
    ]
}

/// The directories you actually go to, best first.
///
/// zoxide keeps that list and ranks it by how often and how recently you were
/// there, which is a better answer than anything the sidebar could work out — and
/// when it is not installed the command fails, the list is empty, and the picker
/// still has every directory this machine is already working in.
pub fn dirs_command() -> [String; 3] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        // `$HOME` is not in the environment a plugin's command inherits —
        // measured: the first line came back empty and every path was drawn
        // unshortened — so it is read out of the passwd entry instead, and
        // handed to zoxide, which otherwise cannot find its own database.
        // `getent` is glibc's; the tilde is the fallback for anything without it.
        "home=\"${HOME:-$(getent passwd \"$(id -u)\" 2>/dev/null | cut -d: -f6)}\"; \
         home=\"${home:-$(cd ~ 2>/dev/null && pwd)}\"; \
         printf '%s\\n' \"$home\"; \
         HOME=\"$home\" zoxide query -l 2>/dev/null; true"
            .to_owned(),
    ]
}

/// Splits that answer into the home directory and the directories themselves.
pub fn parse_dirs(stdout: &str) -> (String, Vec<String>) {
    let mut lines = stdout.lines().map(str::trim);
    let home = lines.next().unwrap_or_default().to_owned();
    (
        home,
        lines
            .filter(|line| line.starts_with('/'))
            .map(str::to_owned)
            .collect(),
    )
}

pub fn remember_command(session: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!("mkdir -p {STATE_DIR} && printf 'from=%s\\n' \"$0\" > {STATE_DIR}/from"),
        session.to_owned(),
    ]
}

/// Reads another machine's state files.
///
/// `ssh` is run directly rather than through a shell, so nothing here has to be
/// quoted twice. `BatchMode` makes a host that wants a password fail instead of
/// hanging, and the timeouts bound how long a machine that has gone away can
/// cost — which matters because this is on the same tick as everything else.
///
/// Set `ControlMaster auto` and `ControlPersist` for these hosts. With a shared
/// connection this is a few milliseconds; without one it is a TCP handshake and
/// a key exchange, every time.
pub fn host_command(host: &str) -> [String; 8] {
    [
        "ssh".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-o".to_owned(),
        "ConnectTimeout=3".to_owned(),
        "-T".to_owned(),
        host.to_owned(),
        // The rows a controller published come back with the same `cat`.
        in_sh(&format!(
            "cat {STATE_DIR}/*.state {STATE_DIR}/rows 2>/dev/null; true"
        )),
    ]
}

/// Dumps a pane's screen, for a peek to render.
///
/// Plain text rather than `--ansi`: a peek pane is usually narrower than what it
/// mirrors, and truncating a line through an escape sequence corrupts the rest
/// of the frame.
///
/// On this machine it also writes the look down, as the sidebar does for a row
/// on screen: peeking at finished work is reading it. In the same shell, so a
/// peek still costs one fork a second; and from any session, which is fine for
/// a file that only ever holds the time of a look.
pub fn dump_command(host: &str, session: &str, pane: u32) -> Vec<String> {
    if host.is_empty() {
        return vec![
            "sh".to_owned(),
            "-c".to_owned(),
            format!(
                "{}; exec zellij --session \"$0\" action dump-screen --pane-id=\"terminal_$1\"",
                record_look("\"$(date +%s)\"")
            ),
            session.to_owned(),
            pane.to_string(),
            String::new(),
            session.replace('/', "_"),
        ];
    }
    // Another machine's pane is read the same way, one ssh further out.
    [
        "ssh",
        "-o",
        "BatchMode=yes",
        "-T",
        host,
        "zellij",
        "--session",
        session,
        "action",
        "dump-screen",
        &format!("--pane-id=terminal_{pane}"),
    ]
    .map(str::to_owned)
    .to_vec()
}

/// Quotes a word for the shell on the far side of an ssh, which sees a command
/// as one string however carefully it was assembled here.
fn quoted(word: &str) -> String {
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Every terminal pane in a session on another machine, gathered into one stack.
///
/// A stack is what solo mode looks like from outside: one pane expanded, the rest
/// collapsed to title lines, and a new pane joining on its own. There is no
/// suppressing a pane through the CLI — that is a plugin call, and a plugin has
/// to be *in* the session to make it.
///
/// Only used when no agenttij answered over there, so nothing it stacks is a
/// pane something else has suppressed. Safe to run twice: re-stacking an already
/// stacked session was measured leaving every pane where it was.
fn stack(session: &str) -> String {
    format!(
        "ids=$(zellij -s {session} action list-panes | grep -o '^terminal_[0-9]*'); \
         [ -n \"$ids\" ] && zellij -s {session} action stack-panes -- $ids"
    )
}

/// Adds a pane to a session on another machine, the way that machine would.
///
/// Asks first: a pipe with no `--plugin` goes to every plugin in that session, so
/// an agenttij running there receives the same message `Alt m` sends here and does
/// the same thing — opens a pane in its slot and parks the one that was there,
/// with real suppressed panes rather than an imitation.
///
/// If nothing answers, the pane count over there will not have moved, and this
/// stacks the session and opens one itself, so it still shows one pane at a
/// time. Stacking first is what makes that work: a new pane joins a stack on its
/// own, while one opened first splits a pane in half and `stack-panes` leaves a
/// grid like that alone. Measured on 0.45.1: new-pane then stack-panes left four
/// panes tiled, stack-panes then new-pane left one stack of four.
/// Which is why it counts rather than looking for a sidebar by name: a plugin's
/// configuration decides its identity and its title is whatever a layout called
/// it, but a pane appearing is a pane appearing.
pub fn remote_pane_command(host: &str, session: &str, cwd: Option<&str>) -> Vec<String> {
    over_ssh(host, remote_pane_script(session, cwd))
}

fn remote_pane_script(session: &str, cwd: Option<&str>) -> String {
    let session = quoted(session);
    let count = format!("zellij -s {session} action list-panes | grep -c '^terminal_'");

    let mut fallback = format!("zellij -s {session} action new-pane");
    if let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty()) {
        // Nested shells: this one is read by `sh` over there, which hands
        // the inner script to `sh -c` inside the pane. `--cwd` was measured being
        // ignored for a session started detached; a `cd` is honoured because it
        // happens inside the pane.
        let script = format!(r#"cd {} 2>/dev/null; exec "${{SHELL:-sh}}""#, quoted(cwd));
        fallback.push_str(&format!(" -- sh -c {}", quoted(&script)));
    }

    // The pipe is given two seconds, which is what it took when it was waited
    // for (Zellij's one-second action timeout, then a `sleep 1`), and then let
    // go: a pipe can stay blocked when nothing releases it - seen twice on
    // 0.45.1, one for five minutes - and that was an ssh hung for as long.
    // Without a payload it also reads stdin, hence `</dev/null`.
    format!(
        "before=$({count}); zellij -s {session} pipe --name add </dev/null >/dev/null 2>&1 & \
         asked=$!; sleep 2; kill $asked 2>/dev/null; \
         [ \"$({count})\" != \"$before\" ] || {{ {}; {fallback}; }}",
        stack(&session)
    )
}

/// One ssh, one script for the far side.
fn over_ssh(host: &str, remote: String) -> Vec<String> {
    vec![
        "ssh".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-T".to_owned(),
        host.to_owned(),
        in_sh(&remote),
    ]
}

/// A script for `sh`, whatever the login shell over there is: ssh hands its
/// command to that shell, and these scripts are POSIX. Under zsh an unquoted
/// `$ids` stays one word, so `stack-panes` got every id as one malformed
/// argument, and a glob with no matches aborts the line, so a machine with rows
/// and no state files read as empty. Both measured with `zsh -c`.
fn in_sh(script: &str) -> String {
    format!("sh -c {}", quoted(script))
}

/// Asks a controller on another machine to do something: add a pane, cycle the
/// row, show one of its panes, close one.
///
/// No `--plugin`, so it reaches whatever is running there whatever configuration
/// gave it life — a plugin's identity is its url *and* its configuration, and we
/// cannot know the configuration a layout on another machine chose.
pub fn remote_pipe_command(
    host: &str,
    session: &str,
    name: &str,
    payload: Option<u32>,
) -> Vec<String> {
    let mut remote = format!("zellij -s {} pipe --name {}", quoted(session), quoted(name));
    if let Some(payload) = payload {
        remote.push_str(&format!(" {payload}"));
    }
    over_ssh(host, remote)
}

/// Opens a session on another machine: an ssh with a terminal, attaching. There
/// is no switching to it — a session belongs to the machine running it — so the
/// honest equivalent is a pane here that is sitting inside it.
pub fn attach_command(host: &str, session: &str) -> [String; 5] {
    [
        "ssh".to_owned(),
        "-t".to_owned(),
        host.to_owned(),
        "zellij".to_owned(),
        format!("attach {session}"),
    ]
}

/// Marks our own `RunCommandResult` events, so we ignore anyone else's.
pub const CONTEXT_KEY: &str = "agenttij";
pub const CONTEXT_ORDER: &str = "order";
pub const CONTEXT_PROJECT: &str = "project";
/// A read of an unhooked agent's screen, for `reading::read`.
pub const CONTEXT_READ: &str = "read";
/// A worktree made for a new row: its path on stdout, or why not on stderr.
pub const CONTEXT_WORKTREE: &str = "worktree";
/// A worktree let go of when its row closed, or kept because it holds work.
pub const CONTEXT_WORKTREE_GONE: &str = "worktree-gone";
pub const CONTEXT_HOST: &str = "host";
/// Which pane a project answer is about.
pub const CONTEXT_PANE: &str = "pane";
/// Names the project a directory belongs to. Kept in step with
/// `hooks/agenttij-state.sh` by `the_hook_and_the_plugin_look_for_the_same_file`.
pub const MARKER: &str = ".agenttij";
pub const CONTEXT_SCAN: &str = "scan";
pub const CONTEXT_PEEK: &str = "peek";
/// The directories worth offering, from zoxide.
pub const CONTEXT_DIRS: &str = "dirs";
/// Which boot this is.
pub const CONTEXT_BOOT: &str = "boot";
/// What Zellij remembers about resurrectable sessions.
pub const CONTEXT_PAST: &str = "past";

pub fn command() -> [&'static str; 3] {
    ["sh", "-c", SCAN_SCRIPT]
}

/// Asks what project a directory belongs to, for a pane that never reported one
/// — a shell someone opened rather than an agent.
///
/// The same three answers the hook gives, in the same order: a `.agenttij` above
/// it, the git root, or the directory itself. Once per pane, cached, and asked
/// again only when the cache rotates: it forks, and the tick is the whole cost of
/// this plugin.
const PROJECT_SCRIPT: &str = r#"d="$0"; p="$d"
while [ -n "$p" ]; do
    if [ -f "$p/.agenttij" ]; then
        IFS= read -r n <"$p/.agenttij" || n=""
        n=$(printf '%s' "$n" | tr -d '[:cntrl:]')
        [ -n "$n" ] || n=${p##*/}
        printf '%s' "$n"
        exit 0
    fi
    p=${p%/*}
done
# The repository a worktree belongs to, not the worktree: its common git
# directory is the main checkout's `.git`. A submodule's is not a `.git`, so it
# keeps its own top level.
c=$(git -C "$d" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) &&
    [ "${c%/.git}" != "$c" ] && { printf '%s' "${c%/.git}"; exit 0; }
git -C "$d" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$d""#;

/// Makes a worktree beside the repository `$0` is in, branched from where `$0`
/// is: `<repo>.worktrees/<repo>-<n>` on branch `<repo>-<n>`, the first number
/// neither a directory nor a branch has taken. Prints its path.
///
/// Beside the repository rather than in it, so nothing has to be ignored; and
/// named after it, because a row is named after its directory.
const WORKTREE_SCRIPT: &str = r#"c=$(git -C "$0" rev-parse --path-format=absolute --git-common-dir 2>/dev/null) ||
    { echo "not in a git repository" >&2; exit 1; }
root=${c%/.git}
[ "$root" != "$c" ] || root=$(git -C "$0" rev-parse --show-toplevel) || exit 1
base=${root##*/}
n=1
while [ -e "$root.worktrees/$base-$n" ] ||
    git -C "$root" show-ref -q --verify "refs/heads/$base-$n"; do
    n=$((n + 1))
done
git -C "$0" worktree add -q -b "$base-$n" "$root.worktrees/$base-$n" HEAD >/dev/null 2>&1 ||
    { echo "git worktree add failed" >&2; exit 1; }
printf '%s' "$root.worktrees/$base-$n""#;

pub fn worktree_command(dir: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        WORKTREE_SCRIPT.to_owned(),
        dir.to_owned(),
    ]
}

/// A pane's screen, read to tell what an agent with no hook is doing. Not
/// `dump_command`: that one is a peek, and a peek writes down that you looked.
pub fn screen_command(session: &str, pane: u32) -> [String; 6] {
    [
        "zellij".to_owned(),
        "--session".to_owned(),
        session.to_owned(),
        "action".to_owned(),
        "dump-screen".to_owned(),
        format!("--pane-id=terminal_{pane}"),
    ]
}

/// Lets a worktree go - without `--force`, so git refuses one holding anything
/// uncommitted, and that work stays where it is - and then its branch, with
/// `-d`, which git refuses for one with commits not merged: a branch with work
/// on it stays, and one with none no longer piles up. Fails only if the
/// worktree stayed.
pub fn worktree_gone_command(repo: &str, worktree: &str) -> [String; 5] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        r#"git -C "$0" worktree remove "$1" || exit 1
git -C "$0" branch -d "${1##*/}" >/dev/null 2>&1
exit 0"#
            .to_owned(),
        repo.to_owned(),
        worktree.to_owned(),
    ]
}

pub fn project_command(cwd: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        PROJECT_SCRIPT.to_owned(),
        cwd.to_owned(),
    ]
}

/// Where an arrangement is kept: a cache file, since it is a preference about
/// project paths rather than anything to do with a session. `$HOME` is expanded
/// by the shell because a plugin cannot read its own environment —
/// `get_session_environment_variables` panics and takes the plugin with it.
const ORDER_DIR: &str = r#"d="${XDG_CACHE_HOME:-$HOME/.cache}/agenttij""#;

pub fn read_order_command() -> [String; 3] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        // Both halves in one fork: what every sidebar shares, and one file per
        // session that only that session writes.
        format!("{ORDER_DIR}; cat \"$d/order\" \"$d/sessions/\"* 2>/dev/null; true"),
    ]
}

/// The text goes as an *argument*, never inside the script: a project is a path,
/// and a path may contain anything a shell would rather it did not.
/// A session's own half, in a file named after it — so two sidebars saving a
/// second apart cannot write over each other's rows.
pub fn write_session_command(session: &str, text: &str) -> [String; 5] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        // The name goes as an argument too: it is a file name here, and a session
        // may be called anything its user typed.
        format!("{ORDER_DIR}; mkdir -p \"$d/sessions\" && printf '%s' \"$0\" > \"$d/sessions/$1\""),
        text.to_owned(),
        session.replace('/', "_"),
    ]
}

pub fn write_order_command(text: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!("{ORDER_DIR}; mkdir -p \"$d\" && printf '%s' \"$0\" > \"$d/order\""),
        text.to_owned(),
    ]
}

#[derive(Debug, PartialEq, Eq)]
pub struct Scan {
    /// Host clock, read in the same breath as the state files so ages are
    /// measured against the same clock that wrote them.
    pub now: u64,
    pub agents: Vec<Agent>,
    /// The session someone jumped away from, if anyone has.
    pub from: Option<String>,
    /// A row someone asked a picker for, if anyone has.
    pub opening: Option<Opening>,
    /// A workspace someone asked the palette to build again.
    pub restoring: Option<Opening>,
    /// Rows a controller published, when this scan read a machine that has one.
    pub rows: Vec<Row>,
    /// The panes those rows hold.
    pub members: Vec<Member>,
    /// When each row was last looked at, by session and primary pane.
    pub seen: BTreeMap<(String, u32), u64>,
}

/// Parses scan output. Unreadable lines are skipped rather than failing the
/// whole scan: one malformed file should not blank the sidebar.
pub fn parse(stdout: &[u8]) -> Option<Scan> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());

    let now = lines.next()?.trim().parse().ok()?;

    let mut from = None;
    let mut opening = None;
    let mut restoring = None;
    let mut agents = Vec::new();
    let mut rows = Vec::new();
    let mut members = Vec::new();
    let mut seen = BTreeMap::new();
    for line in lines {
        if let Some(row) = line.strip_prefix(SEEN_PREFIX) {
            let mut fields = row.split('\t').map(str::trim);
            if let (Some(session), Some(Ok(pane)), Some(Ok(at))) = (
                fields.next().filter(|session| !session.is_empty()),
                fields.next().map(str::parse),
                fields.next().map(str::parse),
            ) {
                seen.insert((session.to_owned(), pane), at);
            }
        } else if let Some(row) = line.strip_prefix(ROW_PREFIX) {
            rows.extend(parse_row(row));
        } else if let Some(member) = line.strip_prefix(MEMBER_PREFIX) {
            members.extend(parse_member(member));
        } else if let Some(asked) = line.strip_prefix(OPEN_PREFIX) {
            opening = parse_opening(asked);
        } else if let Some(asked) = line.strip_prefix(RESTORE_PREFIX) {
            // The same three fields, and the last one is a session name rather
            // than a path — so it is not held to `starts_with('/')`.
            restoring = parse_restoring(asked);
        } else if let Some(session) = line.strip_prefix(FROM_PREFIX) {
            if !session.trim().is_empty() {
                from = Some(session.trim().to_owned());
            }
        } else {
            agents.extend(parse_agent(line));
        }
    }

    Some(Scan {
        now,
        agents,
        from,
        opening,
        restoring,
        rows,
        members,
        seen,
    })
}

/// `<status>\t<session>\t<pane>\t<unix-seconds>\t<cwd>[\t<root>[\t\t<changes>]]`
///
/// The fields after the first five are newer, and a hook older than the plugin
/// is the normal state of an upgrade — so a line without them is not a broken
/// line: the project falls back to the working directory.
///
/// The seventh was the host, appended to each line read over ssh. Hosts are set
/// after parsing now, since an appended field lands wherever the line happens to
/// end; the hook leaves the seventh empty, so a plugin from before reads no host
/// rather than taking the changes for one.
fn parse_agent(line: &str) -> Option<Agent> {
    let mut fields = line.split('\t');
    let status = Status::parse(fields.next()?.trim())?;
    let session = fields.next()?.trim();
    let pane = fields.next()?.trim().parse().ok()?;
    let reported_at = fields.next()?.trim().parse().ok()?;
    let cwd = fields.next().unwrap_or_default().trim();
    let root = fields.next().unwrap_or_default().trim();
    let changes = fields.nth(1).and_then(parse_changes);

    if session.is_empty() {
        return None;
    }

    Some(Agent {
        session: session.to_owned(),
        pane,
        status,
        reported_at,
        cwd: cwd.to_owned(),
        root: if root.is_empty() { cwd } else { root }.to_owned(),
        changes,
        panes: 1,
        ..Agent::default()
    })
}

/// `+<added>-<removed>`, as the hook writes it. Nothing changed is no changes.
fn parse_changes(raw: &str) -> Option<(u32, u32)> {
    let (added, removed) = raw.trim().strip_prefix('+')?.split_once('-')?;
    let changes = (added.parse().ok()?, removed.parse().ok()?);
    (changes != (0, 0)).then_some(changes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two places resolve a project — the hook for agents, the plugin for panes
    /// nobody reports on — and they have to agree, or a marker would group one
    /// and not the other.
    #[test]
    fn the_directories_come_back_with_the_home_they_are_shortened_against() {
        let (home, dirs) = parse_dirs("/home/pp\n/home/pp/personal/agenttij\n/tmp\n");
        assert_eq!(home, "/home/pp");
        assert_eq!(dirs, vec!["/home/pp/personal/agenttij", "/tmp"]);

        // No zoxide is not an error, it is an empty list.
        assert_eq!(parse_dirs("/home/pp\n"), ("/home/pp".to_owned(), vec![]));
        // And anything that is not a path is not a directory.
        let (_, dirs) = parse_dirs("/home/pp\nzsh: command not found: zoxide\n/tmp\n");
        assert_eq!(dirs, vec!["/tmp"]);
    }

    #[test]
    fn a_session_writes_its_own_half_and_reads_every_half() {
        let write = write_session_command("main", "g\tmain\t1,2\n");
        assert!(write[2].contains("sessions/$1"), "named after the session");
        assert_eq!(write[4], "main");
        // A name is a file name here, and a `/` in one would be a directory.
        assert_eq!(write_session_command("a/b", "")[4], "a_b");
        // One read for both halves.
        let read = read_order_command();
        assert!(read[2].contains("$d/order") && read[2].contains("sessions/"));
    }

    #[test]
    fn the_hook_and_the_plugin_look_for_the_same_file() {
        let hook = include_str!("../../../hooks/agenttij-state.sh");
        assert!(hook.contains(MARKER), "the hook stopped reading {MARKER}");
        assert!(PROJECT_SCRIPT.contains(MARKER));
        assert!(
            hook.contains("rev-parse --show-toplevel")
                && PROJECT_SCRIPT.contains("rev-parse --show-toplevel"),
            "both fall back to the git root"
        );
    }

    #[test]
    fn scan_script_uses_the_state_dir_and_forks_nothing_else() {
        assert!(SCAN_SCRIPT.contains(STATE_DIR));
        // The session list is a host call now; forking a client for it cost
        // three times the rest of the script.
        assert!(!command()[2].contains("list-sessions"));
    }

    #[test]
    fn a_controller_is_asked_by_pipe() {
        let ask = remote_pipe_command("dev1", "box", "show", Some(3));
        assert_eq!(ask[0], "ssh");
        assert_eq!(ask[5], in_sh("zellij -s 'box' pipe --name 'show' 3"));
        assert_eq!(
            remote_pipe_command("dev1", "box", "cycle", None)[5],
            in_sh("zellij -s 'box' pipe --name 'cycle'")
        );
    }

    #[test]
    fn a_controller_rows_ride_along_with_the_state_files() {
        let published = publish_rows_command(
            &[Row {
                session: "box".into(),
                primary: 0,
                current: 3,
                panes: 2,
                name: "api".into(),
            }],
            &[Member {
                session: "box".into(),
                primary: 0,
                pane: 3,
                name: "nvim".into(),
            }],
        );
        assert!(published[2].contains(STATE_DIR));

        let scan = parse(format!("0\n{}", published[3]).as_bytes()).expect("parses");
        assert_eq!(scan.rows.len(), 1);
        assert_eq!(scan.rows[0].current, 3);
        assert_eq!(scan.rows[0].panes, 2);
        assert_eq!(scan.rows[0].name, "api", "a row a sidebar can draw");
        assert_eq!(scan.members[0].name, "nvim", "so the row can be opened up");
        assert!(
            scan.agents.is_empty(),
            "and none of it is mistaken for an agent"
        );
    }

    #[test]
    fn a_picked_directory_rides_along_the_same_way() {
        let scan = parse(b"1700000000\nat=main\t1699999990\t/home/pp/api\n").unwrap();
        let opening = scan.opening.unwrap();

        assert_eq!(opening.session, "main");
        assert_eq!(opening.asked_at, 1699999990);
        assert_eq!(opening.path, "/home/pp/api");
        // The scan reads it, so the script has to.
        assert!(SCAN_SCRIPT.contains(OPEN_FILE));
        assert!(open_at_command("main", 17, "/tmp")[3].starts_with(OPEN_PREFIX));

        // Half a line, or a path that is not one, asks for nothing.
        assert!(parse(b"1\nat=main\n").unwrap().opening.is_none());
        assert!(parse(b"1\nat=main\t2\trelative\n")
            .unwrap()
            .opening
            .is_none());
        assert!(parse(b"1\n").unwrap().opening.is_none());
    }

    /// Zellij's own layouts are the only record of a session that died before
    /// agenttij wrote anything down, which is most of them.
    #[test]
    fn a_generated_session_name_is_two_lowercase_words() {
        assert!(looks_generated("sparkling-duck"));
        assert!(looks_generated("quiet-apple"));
        assert!(!looks_generated("agenttij"), "one word is a choice");
        assert!(!looks_generated("Work-Thing"), "capitals are a choice");
        assert!(!looks_generated("api-2"), "so are digits");
        assert!(!looks_generated("-duck") && !looks_generated("duck-"));
    }

    #[test]
    fn a_past_session_is_read_out_of_zellij_own_layout() {
        let text = "\
/home/pp/.cache/zellij/c1/session_info/quiet-apple/session-layout.kdl:mtime=1700000000\n\
/home/pp/.cache/zellij/c1/session_info/quiet-apple/session-layout.kdl:agenttij.wasm\n\
/home/pp/.cache/zellij/c1/session_info/quiet-apple/session-layout.kdl:name=\"agents\"\n\
/home/pp/.cache/zellij/c1/session_info/quiet-apple/session-layout.kdl:name=\"Tab #1\"\n\
/home/pp/.cache/zellij/c1/session_info/quiet-apple/session-layout.kdl:name=\"· wayfarers 1/5\"\n\
/home/pp/.cache/zellij/c1/session_info/quiet-apple/session-layout.kdl:cwd=\"/home/pp/work/api\"\n\
/home/pp/.cache/zellij/c1/session_info/someone-else/session-layout.kdl:name=\"Tab #1\"\n";

        let past = parse_past(text);
        assert_eq!(past.len(), 2);

        let ours = &past[0];
        assert_eq!(ours.session, "quiet-apple");
        assert!(ours.ours, "a sidebar was in it");
        assert_eq!(
            ours.projects,
            vec!["wayfarers", "api"],
            "the project out of a title the sidebar wrote, and a directory"
        );
        assert_eq!(
            ours.seen_at, 1_700_000_000,
            "when it was last written, which is when it was last used"
        );

        let theirs = &past[1];
        assert_eq!(theirs.session, "someone-else");
        assert!(!theirs.ours);
        assert!(
            theirs.projects.is_empty(),
            "a tab name is not a project, and neither is the sidebar's own pane"
        );

        // And the grep has to look where Zellij actually writes.
        assert!(past_command()[2].contains("session_info"));
        assert!(
            past_command()[2].contains("mtime=%Y"),
            "and when each was last written, to put them in order"
        );
        assert!(past_command()[2].contains("session-layout.kdl"));
    }

    #[test]
    fn a_workspace_to_rebuild_rides_the_same_file() {
        let scan = parse(b"1700000000\nrs=main\t1699999999\tapi\n").unwrap();
        let asked = scan.restoring.unwrap();

        assert_eq!(
            (asked.session.as_str(), asked.path.as_str()),
            ("main", "api")
        );
        assert_eq!(asked.asked_at, 1699999999);
        assert!(
            scan.opening.is_none(),
            "the two requests are not each other"
        );
        assert!(restore_command("main", 17, "api")[3].starts_with(RESTORE_PREFIX));
        assert!(parse(b"1\nrs=main\t2\n").unwrap().restoring.is_none());
    }

    #[test]
    fn the_session_you_came_from_rides_along_with_the_scan() {
        let out = b"1754400000\nfrom=main\nrunning\tother\t3\t1754399990\t/x\n";
        let scan = parse(out).expect("parses");

        assert_eq!(scan.from.as_deref(), Some("main"));
        assert_eq!(scan.agents.len(), 1, "and is not mistaken for an agent");
        assert!(remember_command("main")[2].contains(STATE_DIR));
    }

    #[test]
    fn a_line_without_a_project_falls_back_to_the_working_directory() {
        // What a hook older than the plugin writes.
        let out = b"1754400000\nrunning\tmain\t3\t1754399990\t/home/pp/api/crates/core\n";
        let agent = &parse(out).expect("parses").agents[0];

        assert_eq!(agent.root, "/home/pp/api/crates/core");
        assert_eq!(agent.host, "", "no host means this machine");
    }

    #[test]
    fn a_project_and_what_changed_are_read_when_they_are_there() {
        let out = b"1754400000\ndone\tmain\t3\t1754399990\t/home/pp/api/crates/core\t/home/pp/api\t\t+120-30\n";
        let agent = &parse(out).expect("parses").agents[0];

        assert_eq!(agent.root, "/home/pp/api");
        assert_eq!(agent.changes, Some((120, 30)));
        assert_eq!(
            agent.host, "",
            "a host is the reader's to set, not the line's"
        );

        // Nothing changed, or something unreadable, is no changes.
        for raw in ["+0-0", "", "12", "+x-1", "+1-"] {
            let line = format!("1754400000\ndone\tmain\t3\t1\t/a\t/a\t\t{raw}\n");
            assert_eq!(
                parse(line.as_bytes()).expect("parses").agents[0].changes,
                None,
                "{raw:?}"
            );
        }
    }

    #[test]
    fn a_host_appended_by_an_older_plugin_is_not_read_as_anything() {
        let out = b"1754400000\nrunning\tmain\t3\t1754399990\t/home/pp/api\t/home/pp/api\tdev1\n";
        let agent = &parse(out).expect("parses").agents[0];
        assert_eq!((agent.host.as_str(), agent.changes), ("", None));
    }

    #[test]
    fn a_peek_reads_the_pane_and_writes_the_look_down() {
        let command = dump_command("", "main", 7);
        assert_eq!(&command[..2], ["sh", "-c"]);
        assert_eq!(&command[3..], ["main", "7", "", "main"]);
        let script = &command[2];
        assert!(
            script.contains("dump-screen --pane-id=\"terminal_$1\""),
            "{script}"
        );
        assert!(
            script.contains(&format!("{STATE_DIR}/$3.$1.seen")),
            "{script}"
        );
        // The same line the sidebar writes, with the time of the look.
        assert!(
            script.contains(&format!("printf '{SEEN_PREFIX}")),
            "{script}"
        );
        assert!(script.contains("\"$(date +%s)\""), "{script}");
    }

    #[test]
    fn adding_a_pane_over_there_asks_that_machine_first() {
        let command = remote_pane_command("dev1", "box", Some("/srv/api"));
        assert_eq!(command[0], "ssh");
        assert_eq!(command[4], "dev1");
        let remote = &remote_pane_script("box", Some("/srv/api"));
        assert_eq!(command[5], in_sh(remote));

        // Ask: every plugin in that session, so an agenttij there does it the way
        // it would locally.
        assert!(remote.contains("zellij -s 'box' pipe --name add"));
        // Then check whether anything happened, rather than looking for a sidebar
        // by a name a layout chose.
        assert!(remote.contains("list-panes | grep -c '^terminal_'"));
        // And only otherwise do it ourselves, in the agent's directory.
        assert!(remote.contains(
            r#"new-pane -- sh -c 'cd '\''/srv/api'\'' 2>/dev/null; exec "${SHELL:-sh}"'"#
        ));
        // Stacked before the pane opens, or the pane splits the grid and the
        // stack never forms.
        assert!(remote.find("stack-panes") < remote.find("action new-pane"));
    }

    #[test]
    fn a_directory_nobody_knows_is_not_an_empty_one() {
        let remote = &remote_pane_script("box", None);
        assert!(remote.contains("action new-pane;"), "{remote}");
        assert!(!remote.contains("sh -c"));
        // And whatever the shell over there would have made of a quote.
        assert!(remote_pane_script("it's", None).contains(r"-s 'it'\''s' pipe"));
    }

    #[test]
    fn a_pane_on_another_machine_is_read_one_ssh_further_out() {
        let command = dump_command("dev1", "main", 7);
        assert_eq!(command[0], "ssh");
        assert!(command.contains(&"dev1".to_owned()));
        assert!(command.last().unwrap().ends_with("terminal_7"));

        let read = host_command("dev1");
        assert!(read.last().unwrap().starts_with("sh -c '"));
        assert!(read.last().unwrap().contains(STATE_DIR));
    }

    #[test]
    fn parses_time_then_agents() {
        let out = b"1754400000\nrunning\tmain\t3\t1754399990\t/home/pp/api\n";
        let scan = parse(out).expect("parses");

        assert_eq!(scan.now, 1754400000);
        assert_eq!(scan.agents.len(), 1);
        assert_eq!(scan.agents[0].status, Status::Running);
        assert_eq!(scan.agents[0].key(), ("main", 3));
        assert_eq!(scan.agents[0].label(), "api");
    }

    #[test]
    fn a_look_written_down_is_read_back_by_the_scan() {
        // The real command, pointed at a directory of its own.
        let dir = std::env::temp_dir().join(format!("agenttij-seen-{}", std::process::id()));
        let mut command = seen_command("main", 3, 1_754_400_123);
        command[2] = command[2].replace(STATE_DIR, &dir.to_string_lossy());
        let ran = std::process::Command::new(&command[0])
            .args(&command[1..])
            .status()
            .expect("sh runs");
        let written = std::fs::read_to_string(dir.join("main.3.seen"));
        std::fs::remove_dir_all(&dir).ok();
        assert!(ran.success());

        let scan =
            parse(format!("1754400200\n{}", written.expect("written")).as_bytes()).expect("parses");
        assert_eq!(scan.seen.get(&("main".to_owned(), 3)), Some(&1_754_400_123));
        assert!(scan.agents.is_empty(), "a look is not an agent");
        assert!(SCAN_SCRIPT.contains("*.seen"), "and the scan reads them");
    }

    #[test]
    fn a_worktree_is_made_beside_its_repository_and_filed_under_it() {
        let run = |command: &[String], at: &std::path::Path| {
            let out = std::process::Command::new(&command[0])
                .args(&command[1..])
                .current_dir(at)
                .output()
                .expect("runs");
            (
                out.status.success(),
                String::from_utf8_lossy(&out.stdout).into_owned(),
            )
        };
        let git = |args: &str, at: &std::path::Path| {
            let words: Vec<String> = args.split(' ').map(str::to_owned).collect();
            run(&[vec!["git".to_owned()], words].concat(), at).0
        };
        let base = std::env::temp_dir().join(format!("agenttij-wt-{}", std::process::id()));
        let repo = base.join("api");
        std::fs::create_dir_all(repo.join("crates")).expect("made");
        std::fs::write(repo.join("f"), "a\n").expect("written");
        std::fs::write(repo.join("crates/g"), "b\n").expect("written");
        assert!(git("init -q", &repo) && git("add -A", &repo));
        assert!(git(
            "-c user.email=t@t -c user.name=t commit -qm one",
            &repo
        ));

        let crates = repo.join("crates").to_string_lossy().into_owned();
        let (made, first) = run(&worktree_command(&crates), &base);
        let (_, second) = run(&worktree_command(&crates), &base);
        let (_, project) = run(&project_command(&format!("{first}/crates")), &base);
        let (outside, _) = run(&worktree_command(&base.to_string_lossy()), &base);
        let listed = std::process::Command::new("git")
            .args(["-C", &repo.to_string_lossy(), "worktree", "list"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        // The second gets a commit of its own; the first has none.
        let second_dir = std::path::PathBuf::from(&second);
        std::fs::write(second_dir.join("f"), "b\n").expect("written");
        assert!(git(
            "-c user.email=t@t -c user.name=t commit -qam two",
            &second_dir
        ));
        let repo_path = repo.to_string_lossy().into_owned();
        let (first_gone, _) = run(&worktree_gone_command(&repo_path, &first), &base);
        let (second_gone, _) = run(&worktree_gone_command(&repo_path, &second), &base);
        let branches = std::process::Command::new("git")
            .args(["-C", &repo_path, "branch", "--list"])
            .output()
            .map(|out| String::from_utf8_lossy(&out.stdout).into_owned())
            .unwrap_or_default();
        std::fs::remove_dir_all(&base).ok();

        let repo = repo.to_string_lossy();
        assert!(made);
        assert_eq!(first, format!("{repo}.worktrees/api-1"));
        assert_eq!(
            second,
            format!("{repo}.worktrees/api-2"),
            "the next free number"
        );
        assert_eq!(project, repo, "a worktree files under its repository");
        assert!(
            listed.contains("[api-1]") && listed.contains("[api-2]"),
            "{listed}"
        );
        assert!(!outside, "nothing to branch from outside a repository");
        assert!(first_gone && second_gone, "both clean, so both removed");
        assert!(
            !branches.contains("api-1"),
            "no work on it: the branch went too"
        );
        assert!(
            branches.contains("api-2"),
            "a commit on it: the branch stays"
        );
    }

    #[test]
    fn no_agents_is_a_successful_empty_scan() {
        let scan = parse(b"1754400000\n").expect("parses");
        assert_eq!(scan.agents, vec![]);
    }

    #[test]
    fn one_bad_line_does_not_lose_the_good_ones() {
        let out = b"1754400000\ngarbage\nrunning\tmain\tnot-a-pane\t1\t/x\ndone\tmain\t4\t2\t/y\n";
        let scan = parse(out).expect("parses");

        assert_eq!(scan.agents.len(), 1);
        assert_eq!(scan.agents[0].key(), ("main", 4));
    }

    #[test]
    fn missing_clock_is_a_failed_scan() {
        assert_eq!(parse(b""), None);
        assert_eq!(parse(b"nonsense\n"), None);
    }
}
