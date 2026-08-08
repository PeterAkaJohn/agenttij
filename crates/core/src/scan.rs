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

/// Prints the current unix time, then every live session as `session=<name>`,
/// then one state line per tracked pane.
///
/// The session list comes from `zellij list-sessions` because it is derived
/// from IPC sockets, which exist the moment a session starts. `SessionUpdate`
/// cannot be used for this: it learns about *other* sessions by reading their
/// `session-metadata.kdl`, which is never written when a user has
/// `session_serialization false` — leaving every cross-session agent invisible.
/// `EXITED` lines are resurrectable corpses, not live sessions.
///
/// Trailing `true` keeps the exit code clean when the glob matches nothing,
/// which is the normal case with no agents running.
const SCAN_SCRIPT: &str = "mkdir -p /tmp/agenttij && date +%s; \
     zellij list-sessions --no-formatting 2>/dev/null \
     | grep -v EXITED | sed 's/[[:space:]].*//; s/^/session=/'; \
     cat /tmp/agenttij/*.state 2>/dev/null; true";

/// The same without the session list, for the ticks in between: reading the
/// state files is a `cat`, while listing sessions forks a `zellij` client that
/// dials every session's socket. Sessions do not appear and vanish at 1Hz.
const STATE_SCRIPT: &str =
    "mkdir -p /tmp/agenttij && date +%s; cat /tmp/agenttij/*.state 2>/dev/null; true";

/// Marks a live-session line in the scan output.
const SESSION_PREFIX: &str = "session=";

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
pub const CONTEXT_SCAN: &str = "scan";
pub const CONTEXT_PEEK: &str = "peek";

pub fn command(with_sessions: bool) -> [&'static str; 3] {
    let script = if with_sessions {
        SCAN_SCRIPT
    } else {
        STATE_SCRIPT
    };
    ["sh", "-c", script]
}

#[derive(Debug, PartialEq, Eq)]
pub struct Scan {
    /// Host clock, read in the same breath as the state files so ages are
    /// measured against the same clock that wrote them.
    pub now: u64,
    /// Every session currently running on this machine.
    pub live_sessions: Vec<String>,
    pub agents: Vec<Agent>,
}

/// Parses scan output. Unreadable lines are skipped rather than failing the
/// whole scan: one malformed file should not blank the sidebar.
pub fn parse(stdout: &[u8]) -> Option<Scan> {
    let text = String::from_utf8_lossy(stdout);
    let mut lines = text.lines().filter(|line| !line.trim().is_empty());

    let now = lines.next()?.trim().parse().ok()?;

    let mut live_sessions = Vec::new();
    let mut agents = Vec::new();
    for line in lines {
        if let Some(session) = line.strip_prefix(SESSION_PREFIX) {
            let session = session.trim();
            if !session.is_empty() {
                live_sessions.push(session.to_owned());
            }
        } else {
            agents.extend(parse_agent(line));
        }
    }

    Some(Scan {
        now,
        live_sessions,
        agents,
    })
}

/// `<status>\t<session>\t<pane>\t<unix-seconds>\t<cwd>`
fn parse_agent(line: &str) -> Option<Agent> {
    let mut fields = line.split('\t');
    let status = Status::parse(fields.next()?.trim())?;
    let session = fields.next()?.trim();
    let pane = fields.next()?.trim().parse().ok()?;
    let reported_at = fields.next()?.trim().parse().ok()?;
    let cwd = fields.next().unwrap_or_default().trim();

    if session.is_empty() {
        return None;
    }

    Some(Agent {
        session: session.to_owned(),
        pane,
        status,
        reported_at,
        cwd: cwd.to_owned(),
        title: String::new(),
        panes: 1,
        depth: 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_script_uses_the_state_dir() {
        assert!(SCAN_SCRIPT.contains(STATE_DIR));
        assert!(STATE_SCRIPT.contains(STATE_DIR));
    }

    #[test]
    fn only_the_full_scan_lists_sessions() {
        assert!(command(true)[2].contains("list-sessions"));
        assert!(!command(false)[2].contains("list-sessions"));
    }

    #[test]
    fn parses_live_sessions_and_agents_in_one_pass() {
        let out = b"1754400000\nsession=main\nsession=other\nrunning\tmain\t3\t1754399990\t/x\n";
        let scan = parse(out).expect("parses");

        assert_eq!(scan.live_sessions, vec!["main", "other"]);
        assert_eq!(scan.agents.len(), 1);
        assert_eq!(scan.agents[0].key(), ("main", 3));
    }

    #[test]
    fn a_session_named_like_a_state_line_is_still_a_session() {
        let out = b"1754400000\nsession=running\n";
        let scan = parse(out).expect("parses");

        assert_eq!(scan.live_sessions, vec!["running"]);
        assert_eq!(scan.agents, vec![]);
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
