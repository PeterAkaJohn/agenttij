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

/// Dumps a pane's screen, for a peek to render.
///
/// Plain text rather than `--ansi`: a peek pane is usually narrower than what it
/// mirrors, and truncating a line through an escape sequence corrupts the rest
/// of the frame.
pub fn dump_command(session: &str, pane: u32) -> [String; 6] {
    [
        "zellij".to_owned(),
        "--session".to_owned(),
        session.to_owned(),
        "action".to_owned(),
        "dump-screen".to_owned(),
        format!("--pane-id=terminal_{pane}"),
    ]
}

/// Marks our own `RunCommandResult` events, so we ignore anyone else's.
pub const CONTEXT_KEY: &str = "agenttij";
pub const CONTEXT_ORDER: &str = "order";
pub const CONTEXT_SCAN: &str = "scan";
pub const CONTEXT_PEEK: &str = "peek";

pub fn command() -> [&'static str; 3] {
    ["sh", "-c", SCAN_SCRIPT]
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
        let command = dump_command("main", 7);
        assert_eq!(command[2], "main");
        assert!(command.last().unwrap().ends_with("terminal_7"));
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
