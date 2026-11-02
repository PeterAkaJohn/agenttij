//! `agenttij`: what an agent runs to open groups and panes in the sidebar of the
//! session it is in. The request rides a `zellij pipe`; the sidebar answers in
//! a file (`agenttij_core::api` says why), printed here as one line of JSON.
//! `read` asks Zellij directly, and `wait` asks the sidebar until it hears what
//! it is waiting for.

mod mcp;

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agenttij_core::api::{self, Request};
use serde_json::{json, Value};

const USAGE: &str = "\
usage: agenttij <command>

  list                                      projects, their groups, and the groups' panes
  group new [--cwd DIR] [--run CMD] [--worktree] [--show]
                                            a new group, parked unless --show
  pane add [--group N] [--cwd DIR] [--run CMD] [--show]
                                            a pane in your group, or group N
  pane close PANE                           closes a pane an agent opened, and no other
  show PANE                                 puts a pane on screen
  read PANE [--lines N]                     what a pane shows; with --lines, the last N
                                            lines of its scrollback
  wait GROUP [--timeout SECONDS]            until the group's agent is done or needs
                                            input (300 seconds at most by default)
  mcp                                       the same, as an MCP server on stdio

Run it inside a Zellij session whose layout has the agenttij sidebar.
Answers are JSON, except what `read` prints; the exit code is 1 on a no.";

/// Long enough for git to make a worktree of a big repository.
const PATIENCE: Duration = Duration::from_secs(20);

/// How long `wait` waits unless told: well inside the ten minutes an agent's
/// shell tool allows a command.
pub const WAIT: u64 = 300;

/// What a run of `agenttij` does.
#[derive(Debug, PartialEq, Eq)]
enum Run {
    Ask(Request),
    Read { pane: u32, lines: Option<usize> },
    Wait { group: u32, timeout: u64 },
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("mcp") {
        return match mcp::serve() {
            Ok(()) => ExitCode::SUCCESS,
            Err(why) => {
                eprintln!("agenttij mcp: {why}");
                ExitCode::from(2)
            }
        };
    }
    let run = match parse(&args) {
        Ok(run) => run,
        Err(why) => {
            if !why.is_empty() {
                eprintln!("agenttij: {why}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let answer = match run {
        Run::Ask(request) => ask(&request),
        Run::Read { pane, lines } => match read(pane, lines) {
            Ok(text) => {
                println!("{text}");
                return ExitCode::SUCCESS;
            }
            Err(why) => api::error(&why),
        },
        Run::Wait { group, timeout } => wait(group, timeout),
    };
    println!("{answer}");
    if answered_ok(&answer) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn parse(args: &[String]) -> Result<Run, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let number = |text: &str, what: &str| -> Result<u32, String> {
        text.parse().map_err(|_| format!("not a {what}: {text}"))
    };
    let mut target = 0;
    let (command, flags) = match words.as_slice() {
        ["list", rest @ ..] => ("list", rest),
        ["group", "new", rest @ ..] => ("group new", rest),
        ["pane", "add", rest @ ..] => ("pane add", rest),
        ["pane", "close", pane] => {
            let pane = number(pane, "pane")?;
            return Ok(Run::Ask(Request::Close { pane }));
        }
        ["show", pane] => {
            let pane = number(pane, "pane")?;
            return Ok(Run::Ask(Request::Show { pane }));
        }
        ["read", pane, rest @ ..] => {
            target = number(pane, "pane")?;
            ("read", rest)
        }
        ["wait", group, rest @ ..] => {
            target = number(group, "group")?;
            ("wait", rest)
        }
        [] | ["-h" | "--help" | "help", ..] => return Err(String::new()),
        _ => return Err(format!("unknown command: {}", args.join(" "))),
    };

    let mut cwd = None;
    let mut run = None;
    let mut group = None;
    let mut worktree = false;
    let mut show = false;
    let mut lines = None;
    let mut timeout = WAIT;
    let mut flags = flags.iter();
    while let Some(flag) = flags.next() {
        let mut value = || {
            flags
                .next()
                .map(|value| value.to_string())
                .ok_or(format!("{flag} needs a value"))
        };
        match (command, *flag) {
            ("group new" | "pane add", "--cwd") => cwd = Some(absolute(&value()?)),
            ("group new" | "pane add", "--run") => run = Some(value()?),
            ("group new" | "pane add", "--show") => show = true,
            ("group new", "--worktree") => worktree = true,
            ("pane add", "--group") => group = Some(number(&value()?, "group")?),
            ("read", "--lines") => {
                let text = value()?;
                lines = Some(text.parse().map_err(|_| format!("not a count: {text}"))?);
            }
            ("wait", "--timeout") => {
                let text = value()?;
                timeout = text.parse().map_err(|_| format!("not seconds: {text}"))?;
            }
            _ => return Err(format!("{command} takes no {flag}")),
        }
    }
    Ok(match command {
        "list" => Run::Ask(Request::List),
        "group new" => Run::Ask(Request::NewGroup {
            cwd,
            run,
            worktree,
            show,
        }),
        "pane add" => Run::Ask(Request::AddPane {
            group,
            cwd,
            run,
            show,
        }),
        "read" => Run::Read {
            pane: target,
            lines,
        },
        _ => Run::Wait {
            group: target,
            timeout,
        },
    })
}

/// The sidebar resolves a directory against its own, so a relative one is made
/// absolute against the caller's first.
fn absolute(dir: &str) -> String {
    std::path::absolute(dir)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| dir.to_owned())
}

/// Sends a request to the sidebar of this session and waits for its answer,
/// which is always one line of JSON - a failure to get one included.
pub fn ask(request: &Request) -> String {
    match exchange(request) {
        Ok(answer) => answer,
        Err(why) => api::error(&why),
    }
}

fn exchange(request: &Request) -> Result<String, String> {
    inside_zellij()?;
    let caller = std::env::var("ZELLIJ_PANE_ID")
        .ok()
        .and_then(|pane| pane.trim().parse().ok());
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let id = format!("{}-{nanos}", std::process::id());
    let (name, args) = request.to_pipe(&id, caller);

    // With no payload and stdin not a terminal, `zellij pipe` sends one message
    // and returns - unless stdin stays open, when it waits to read more.
    let mut pipe = Command::new("zellij")
        .args(["pipe", "--name", &name, "--args", &args])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|why| format!("cannot run zellij: {why}"))?;
    let dir = api::reply_dir(&id);
    let reply = Path::new(&dir).join("reply");
    let started = Instant::now();
    let answer = loop {
        if let Ok(answer) = std::fs::read_to_string(&reply) {
            break Ok(answer.trim().to_owned());
        }
        if let Ok(Some(status)) = pipe.try_wait() {
            if !status.success() {
                let mut stderr = String::new();
                if let Some(mut out) = pipe.stderr.take() {
                    std::io::Read::read_to_string(&mut out, &mut stderr).ok();
                }
                break Err(format!("zellij pipe failed: {}", stderr.trim()));
            }
        }
        if started.elapsed() > PATIENCE {
            break Err(
                "no answer from the sidebar: is agenttij's sidebar in this session's layout?"
                    .to_owned(),
            );
        }
        std::thread::sleep(Duration::from_millis(25));
    };
    // A `zellij pipe` still waiting by now never will; it must not outlive us.
    if pipe.try_wait().ok().flatten().is_none() {
        pipe.kill().ok();
        pipe.wait().ok();
    }
    std::fs::remove_dir_all(&dir).ok();
    answer
}

fn answered_ok(answer: &str) -> bool {
    serde_json::from_str::<Value>(answer).is_ok_and(|answer| answer["ok"] == true)
}

/// What a pane of this session shows, or with `lines` the end of everything it
/// printed. Trailing blank lines go: the screen is padded to its height.
pub fn read(pane: u32, lines: Option<usize>) -> Result<String, String> {
    inside_zellij()?;
    // `dump-screen` prints nothing and succeeds for a pane that is not there,
    // which would read as a blank screen.
    let listed = zellij(&["action", "list-panes", "--json"])?;
    let panes: Value = serde_json::from_str(&listed).map_err(|why| why.to_string())?;
    let found = panes.as_array().is_some_and(|panes| {
        panes
            .iter()
            .any(|entry| entry["id"] == pane && entry["is_plugin"] == false)
    });
    if !found {
        return Err(format!("no pane {pane} in this session"));
    }
    let id = pane.to_string();
    let mut args = vec!["action", "dump-screen", "--pane-id", &id];
    if lines.is_some() {
        args.push("--full");
    }
    let screen = zellij(&args)?;
    let mut kept: Vec<&str> = screen.lines().collect();
    while kept.last().is_some_and(|line| line.trim().is_empty()) {
        kept.pop();
    }
    let from = lines.map_or(0, |lines| kept.len().saturating_sub(lines));
    Ok(kept[from..].join("\n"))
}

/// Asks the sidebar about a group until its agent has something for you -
/// finished, read or not, or waiting on an answer - or until `timeout`
/// seconds have gone by.
pub fn wait(group: u32, timeout: u64) -> String {
    let started = Instant::now();
    loop {
        let answer = ask(&Request::List);
        let listed: Value = serde_json::from_str(&answer).unwrap_or(Value::Null);
        if listed["ok"] != true {
            return answer;
        }
        let status = listed["projects"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|project| project["groups"].as_array().into_iter().flatten())
            .find(|view| view["group"] == group)
            .and_then(|view| view["status"].as_str())
            .map(str::to_owned);
        let Some(status) = status else {
            return api::error(&format!("no group {group} in this session"));
        };
        if matches!(status.as_str(), "done" | "unread" | "needs-input") {
            // By hand, so `ok` comes first like every other answer: `json!`
            // sorts its keys.
            return format!(
                r#"{{"ok":true,"group":{group},"status":{}}}"#,
                json!(status)
            );
        }
        if started.elapsed() >= Duration::from_secs(timeout) {
            return api::error(&format!("group {group} is still {status} after {timeout}s"));
        }
        std::thread::sleep(Duration::from_secs(2));
    }
}

fn inside_zellij() -> Result<(), String> {
    match std::env::var_os("ZELLIJ_SESSION_NAME") {
        Some(_) => Ok(()),
        None => Err("not inside a Zellij session".to_owned()),
    }
}

/// Runs a Zellij CLI command against this session and returns what it printed.
fn zellij(args: &[&str]) -> Result<String, String> {
    let out = Command::new("zellij")
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|why| format!("cannot run zellij: {why}"))?;
    if !out.status.success() {
        let why = String::from_utf8_lossy(&out.stderr);
        return Err(format!("zellij {} failed: {}", args.join(" "), why.trim()));
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn commands_read_as_requests() {
        assert_eq!(parse(&words("list")), Ok(Run::Ask(Request::List)));
        assert_eq!(
            parse(&words("show 4")),
            Ok(Run::Ask(Request::Show { pane: 4 }))
        );
        assert_eq!(
            parse(&words("pane close 9")),
            Ok(Run::Ask(Request::Close { pane: 9 }))
        );
        assert_eq!(
            parse(&words("pane add --group 3 --run htop --show")),
            Ok(Run::Ask(Request::AddPane {
                group: Some(3),
                cwd: None,
                run: Some("htop".into()),
                show: true
            }))
        );
        assert_eq!(
            parse(&words("group new --worktree --cwd /src/api")),
            Ok(Run::Ask(Request::NewGroup {
                cwd: Some("/src/api".into()),
                run: None,
                worktree: true,
                show: false
            }))
        );
        assert_eq!(
            parse(&words("read 3 --lines 40")),
            Ok(Run::Read {
                pane: 3,
                lines: Some(40)
            })
        );
        assert_eq!(
            parse(&words("wait 8")),
            Ok(Run::Wait {
                group: 8,
                timeout: WAIT
            })
        );
        assert!(parse(&words("wait 8 --lines 3")).is_err(), "not for a wait");
        assert!(parse(&words("read")).is_err());
        assert!(
            parse(&words("pane add --worktree")).is_err(),
            "not for a pane"
        );
        assert!(parse(&words("pane add --group")).is_err());
        assert!(parse(&words("show x")).is_err());
        assert!(parse(&words("nope")).is_err());
    }

    #[test]
    fn an_answer_is_ok_by_what_it_says_not_by_its_key_order() {
        assert!(answered_ok(r#"{"ok":true,"pane":3,"group":0}"#));
        assert!(answered_ok(r#"{"group":4,"ok":true}"#));
        assert!(!answered_ok(r#"{"ok":false,"error":"no"}"#));
        assert!(!answered_ok("not json"));
    }

    #[test]
    fn outside_zellij_the_answer_says_so() {
        // `cargo test` inside a Zellij pane would ask the real sidebar.
        if std::env::var_os("ZELLIJ_SESSION_NAME").is_some() {
            return;
        }
        let answer = ask(&Request::List);
        assert!(!answered_ok(&answer));
        assert!(answer.contains("not inside a Zellij session"), "{answer}");
    }
}
