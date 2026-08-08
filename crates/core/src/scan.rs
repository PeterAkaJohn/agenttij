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
const SCAN_SCRIPT: &str = "date +%s; cat /tmp/agenttij/*.state 2>/dev/null; true";

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
        format!("cat {STATE_DIR}/*.state 2>/dev/null; true"),
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
}

/// Parses scan output. Unreadable lines are skipped rather than failing the
/// whole scan: one malformed file should not blank the sidebar.
pub fn parse(stdout: &[u8]) -> Option<Scan> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());

    let now = lines.next()?.trim().parse().ok()?;
    let agents = lines.filter_map(parse_agent).collect();

    Some(Scan { now, agents })
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
