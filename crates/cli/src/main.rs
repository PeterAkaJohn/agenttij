//! `agenttij`: what an agent runs to open groups and panes in the sidebar of the
//! session it is in. The request rides a `zellij pipe`; the sidebar answers in
//! a file (`agenttij_core::api` says why), printed here as one line of JSON.

mod mcp;

use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use agenttij_core::api::{self, Request};

const USAGE: &str = "\
usage: agenttij <command>

  list                                      projects, their groups, and the groups' panes
  group new [--cwd DIR] [--run CMD] [--worktree] [--show]
                                            a new group, parked unless --show
  pane add [--group N] [--cwd DIR] [--run CMD] [--show]
                                            a pane in your group, or group N
  show PANE                                 puts a pane on screen
  mcp                                       the same, as an MCP server on stdio

Run it inside a Zellij session whose layout has the agenttij sidebar.
Answers are JSON; the exit code is 1 when the sidebar says no.";

/// Long enough for git to make a worktree of a big repository.
const PATIENCE: Duration = Duration::from_secs(20);

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
    let request = match parse(&args) {
        Ok(request) => request,
        Err(why) => {
            if !why.is_empty() {
                eprintln!("agenttij: {why}\n");
            }
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    let answer = ask(&request);
    println!("{answer}");
    if answered_ok(&answer) {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

fn parse(args: &[String]) -> Result<Request, String> {
    let words: Vec<&str> = args.iter().map(String::as_str).collect();
    let (command, flags) = match words.as_slice() {
        ["list", rest @ ..] => ("list", rest),
        ["group", "new", rest @ ..] => ("group new", rest),
        ["pane", "add", rest @ ..] => ("pane add", rest),
        ["show", pane] => {
            let pane = pane.parse().map_err(|_| format!("not a pane: {pane}"))?;
            return Ok(Request::Show { pane });
        }
        [] | ["-h" | "--help" | "help", ..] => return Err(String::new()),
        _ => return Err(format!("unknown command: {}", args.join(" "))),
    };

    let mut cwd = None;
    let mut run = None;
    let mut group = None;
    let mut worktree = false;
    let mut show = false;
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
            ("pane add", "--group") => {
                let text = value()?;
                group = Some(text.parse().map_err(|_| format!("not a group: {text}"))?);
            }
            _ => return Err(format!("{command} takes no {flag}")),
        }
    }
    Ok(match command {
        "list" => Request::List,
        "group new" => Request::NewGroup {
            cwd,
            run,
            worktree,
            show,
        },
        _ => Request::AddPane {
            group,
            cwd,
            run,
            show,
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
    if std::env::var_os("ZELLIJ_SESSION_NAME").is_none() {
        return Err("not inside a Zellij session".to_owned());
    }
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
    answer.starts_with(r#"{"ok":true"#)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(line: &str) -> Vec<String> {
        line.split_whitespace().map(str::to_owned).collect()
    }

    #[test]
    fn commands_read_as_requests() {
        assert_eq!(parse(&words("list")), Ok(Request::List));
        assert_eq!(parse(&words("show 4")), Ok(Request::Show { pane: 4 }));
        assert_eq!(
            parse(&words("pane add --group 3 --run htop --show")),
            Ok(Request::AddPane {
                group: Some(3),
                cwd: None,
                run: Some("htop".into()),
                show: true
            })
        );
        assert_eq!(
            parse(&words("group new --worktree --cwd /src/api")),
            Ok(Request::NewGroup {
                cwd: Some("/src/api".into()),
                run: None,
                worktree: true,
                show: false
            })
        );
        assert!(
            parse(&words("pane add --worktree")).is_err(),
            "not for a pane"
        );
        assert!(parse(&words("pane add --group")).is_err());
        assert!(parse(&words("show x")).is_err());
        assert!(parse(&words("nope")).is_err());
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
