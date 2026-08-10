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
const SCAN_SCRIPT: &str =
    "date +%s; cat /tmp/agenttij/from 2>/dev/null; cat /tmp/agenttij/*.state 2>/dev/null; true";

/// Marks the session you jumped away from, in the scan output.
const FROM_PREFIX: &str = "from=";

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
        format!("cat {STATE_DIR}/*.state {STATE_DIR}/rows 2>/dev/null; true"),
    ]
}

/// Dumps a pane's screen, for a peek to render.
///
/// Plain text rather than `--ansi`: a peek pane is usually narrower than what it
/// mirrors, and truncating a line through an escape sequence corrupts the rest
/// of the frame.
pub fn dump_command(host: &str, session: &str, pane: u32) -> Vec<String> {
    let mut command = Vec::new();
    if !host.is_empty() {
        // Another machine's pane is read the same way, one ssh further out.
        command.extend([
            "ssh".to_owned(),
            "-o".to_owned(),
            "BatchMode=yes".to_owned(),
            "-T".to_owned(),
            host.to_owned(),
        ]);
    }
    command.extend([
        "zellij".to_owned(),
        "--session".to_owned(),
        session.to_owned(),
        "action".to_owned(),
        "dump-screen".to_owned(),
        format!("--pane-id=terminal_{pane}"),
    ]);
    command
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
/// opens one itself and stacks the session so it still shows one pane at a time.
/// Which is why it counts rather than looking for a sidebar by name: a plugin's
/// configuration decides its identity and its title is whatever a layout called
/// it, but a pane appearing is a pane appearing.
pub fn remote_pane_command(host: &str, session: &str, cwd: Option<&str>) -> Vec<String> {
    let session = quoted(session);
    let count = format!("zellij -s {session} action list-panes | grep -c '^terminal_'");

    let mut fallback = format!("zellij -s {session} action new-pane");
    if let Some(cwd) = cwd.filter(|cwd| !cwd.is_empty()) {
        // Three shells deep: this one is read by the remote shell, which hands
        // the inner script to `sh -c` inside the pane. `--cwd` was measured being
        // ignored for a session started detached; a `cd` is honoured because it
        // happens inside the pane.
        let script = format!(r#"cd {} 2>/dev/null; exec "${{SHELL:-sh}}""#, quoted(cwd));
        fallback.push_str(&format!(" -- sh -c {}", quoted(&script)));
    }

    let remote = format!(
        "before=$({count}); zellij -s {session} pipe --name add >/dev/null 2>&1; sleep 1; \
         [ \"$({count})\" != \"$before\" ] || {{ {fallback}; {} ; }}",
        stack(&session)
    );
    over_ssh(host, remote)
}

/// One ssh, one command string for the shell on the far side.
fn over_ssh(host: &str, remote: String) -> Vec<String> {
    vec![
        "ssh".to_owned(),
        "-o".to_owned(),
        "BatchMode=yes".to_owned(),
        "-T".to_owned(),
        host.to_owned(),
        remote,
    ]
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
pub const CONTEXT_HOST: &str = "host";
/// Which pane a project answer is about.
pub const CONTEXT_PANE: &str = "pane";
/// Names the project a directory belongs to. Kept in step with
/// `hooks/agenttij-state.sh` by `the_hook_and_the_plugin_look_for_the_same_file`.
pub const MARKER: &str = ".agenttij";
pub const CONTEXT_SCAN: &str = "scan";
pub const CONTEXT_PEEK: &str = "peek";

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
git -C "$d" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$d""#;

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
        format!("{ORDER_DIR}; cat \"$d/order\" 2>/dev/null; true"),
    ]
}

/// The text goes as an *argument*, never inside the script: a project is a path,
/// and a path may contain anything a shell would rather it did not.
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
    /// Rows a controller published, when this scan read a machine that has one.
    pub rows: Vec<Row>,
    /// The panes those rows hold.
    pub members: Vec<Member>,
}

/// Parses scan output. Unreadable lines are skipped rather than failing the
/// whole scan: one malformed file should not blank the sidebar.
pub fn parse(stdout: &[u8]) -> Option<Scan> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());

    let now = lines.next()?.trim().parse().ok()?;

    let mut from = None;
    let mut agents = Vec::new();
    let mut rows = Vec::new();
    let mut members = Vec::new();
    for line in lines {
        if let Some(row) = line.strip_prefix(ROW_PREFIX) {
            rows.extend(parse_row(row));
        } else if let Some(member) = line.strip_prefix(MEMBER_PREFIX) {
            members.extend(parse_member(member));
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
        rows,
        members,
    })
}

/// `<status>\t<session>\t<pane>\t<unix-seconds>\t<cwd>[\t<root>[\t<host>]]`
///
/// The last two are newer than the first five, and a hook older than the plugin
/// is the normal state of an upgrade — so a line without them is not a broken
/// line: the project falls back to the working directory, and no host means this
/// machine.
fn parse_agent(line: &str) -> Option<Agent> {
    let mut fields = line.split('\t');
    let status = Status::parse(fields.next()?.trim())?;
    let session = fields.next()?.trim();
    let pane = fields.next()?.trim().parse().ok()?;
    let reported_at = fields.next()?.trim().parse().ok()?;
    let cwd = fields.next().unwrap_or_default().trim();
    let root = fields.next().unwrap_or_default().trim();
    let host = fields.next().unwrap_or_default().trim();

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
        host: host.to_owned(),
        panes: 1,
        ..Agent::default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two places resolve a project — the hook for agents, the plugin for panes
    /// nobody reports on — and they have to agree, or a marker would group one
    /// and not the other.
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
        assert_eq!(ask[5], "zellij -s 'box' pipe --name 'show' 3");
        assert!(!remote_pipe_command("dev1", "box", "cycle", None)[5].ends_with(' '));
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
    fn a_project_and_a_host_are_read_when_they_are_there() {
        let out = b"1754400000\nrunning\tmain\t3\t1754399990\t/home/pp/api/crates/core\t/home/pp/api\tdev1\n";
        let agent = &parse(out).expect("parses").agents[0];

        assert_eq!(agent.root, "/home/pp/api");
        assert_eq!(agent.host, "dev1");
    }

    #[test]
    fn the_dump_command_names_the_pane() {
        let command = dump_command("", "main", 7);
        assert_eq!(command[2], "main");
        assert!(command.last().unwrap().ends_with("terminal_7"));
    }

    #[test]
    fn adding_a_pane_over_there_asks_that_machine_first() {
        let command = remote_pane_command("dev1", "box", Some("/srv/api"));
        assert_eq!(command[0], "ssh");
        assert_eq!(command[4], "dev1");
        let remote = &command[5];

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
        assert!(remote.contains("stack-panes"));
    }

    #[test]
    fn a_directory_nobody_knows_is_not_an_empty_one() {
        let remote = &remote_pane_command("dev1", "box", None)[5];
        assert!(remote.contains("action new-pane;"), "{remote}");
        assert!(!remote.contains("sh -c"));
        // And whatever the shell over there would have made of a quote.
        assert!(remote_pane_command("dev1", "it's", None)[5].contains(r"-s 'it'\''s' pipe"));
    }

    #[test]
    fn a_pane_on_another_machine_is_read_one_ssh_further_out() {
        let command = dump_command("dev1", "main", 7);
        assert_eq!(command[0], "ssh");
        assert!(command.contains(&"dev1".to_owned()));
        assert!(command.last().unwrap().ends_with("terminal_7"));

        assert!(host_command("dev1").last().unwrap().contains(STATE_DIR));
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
