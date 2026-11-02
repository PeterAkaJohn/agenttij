//! What an agent can ask the sidebar for: projects made of groups made of
//! panes, over `zellij pipe`.
//!
//! A request is a pipe named `agenttij.<verb>` whose arguments ride in the
//! pipe's `--args` (`key=value,key=value`). `pane` is always the caller's own
//! pane, `$ZELLIJ_PANE_ID`, which is what "my group" means, and `id` names the
//! request.
//!
//! The answer is one line of JSON in `<STATE_DIR>/api/<id>/reply`, not on the
//! pipe. Writing to the pipe (`cli_pipe_output`) needs `ReadCliPipes`, and a new
//! permission is the expensive kind: a session still running an older build
//! rewrites the grant without it. And a pipe reaches the sidebar once per
//! attached client - each client runs its own instance - so the instances claim
//! the request first, with a `mkdir` only one of them can win.
//!
//! Here so the plugin, which answers, and the CLI, which asks, cannot drift.

use std::collections::BTreeMap;

use crate::scan::STATE_DIR;

/// What every pipe name of this API starts with.
pub const PREFIX: &str = "agenttij.";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// The projects of this session, their groups, and the groups' panes.
    List,
    /// A new group: where it starts, what its first pane runs, whether in a
    /// fresh git worktree of the caller's repository, and whether it takes the
    /// screen. Parked behind what is on screen unless `show`.
    NewGroup {
        cwd: Option<String>,
        run: Option<String>,
        worktree: bool,
        show: bool,
    },
    /// A pane in a group - the caller's own unless `group` names another -
    /// parked unless `show`.
    AddPane {
        group: Option<u32>,
        cwd: Option<String>,
        run: Option<String>,
        show: bool,
    },
    /// Puts a pane on screen, the way Enter does.
    Show { pane: u32 },
}

impl Request {
    pub fn verb(&self) -> &'static str {
        match self {
            Self::List => "list",
            Self::NewGroup { .. } => "group.new",
            Self::AddPane { .. } => "pane.add",
            Self::Show { .. } => "show",
        }
    }

    /// The pipe's name and its `--args`, for the caller in pane `caller`.
    pub fn to_pipe(&self, id: &str, caller: Option<u32>) -> (String, String) {
        let mut args: Vec<(&str, String)> = vec![("id", id.to_owned())];
        if let Some(caller) = caller {
            args.push(("pane", caller.to_string()));
        }
        let flag = |on: bool| on.then(|| "true".to_owned());
        let mut add = |key, value: Option<String>| {
            if let Some(value) = value {
                args.push((key, value));
            }
        };
        match self {
            Self::List => {}
            Self::NewGroup {
                cwd,
                run,
                worktree,
                show,
            } => {
                add("cwd", cwd.clone());
                add("run", run.clone());
                add("worktree", flag(*worktree));
                add("show", flag(*show));
            }
            Self::AddPane {
                group,
                cwd,
                run,
                show,
            } => {
                add("group", group.map(|group| group.to_string()));
                add("cwd", cwd.clone());
                add("run", run.clone());
                add("show", flag(*show));
            }
            Self::Show { pane } => add("target", Some(pane.to_string())),
        }
        let args = args
            .iter()
            .map(|(key, value)| format!("{key}={}", encode(value)))
            .collect::<Vec<_>>()
            .join(",");
        (format!("{PREFIX}{}", self.verb()), args)
    }

    /// Reads a pipe back: `None` when it is not one of ours or has nowhere to
    /// be answered, and the call - which may be a mistake - when it is.
    pub fn from_pipe(name: &str, args: &BTreeMap<String, String>) -> Option<Call> {
        let verb = name.strip_prefix(PREFIX)?;
        let id = args.get("id").filter(|id| valid_id(id))?.clone();
        let text = |key: &str| {
            args.get(key)
                .map(|value| decode(value))
                .filter(|value| !value.is_empty())
        };
        let flag = |key: &str| text(key).as_deref() == Some("true");
        let number = |key: &str| -> Result<Option<u32>, String> {
            text(key)
                .map(|value| {
                    value
                        .parse()
                        .map_err(|_| format!("{key} is not a pane: {value}"))
                })
                .transpose()
        };
        let caller = number("pane").ok().flatten();
        let read = || -> Result<Request, String> {
            number("pane")?;
            let request = match verb {
                "list" => Request::List,
                "group.new" => Request::NewGroup {
                    cwd: text("cwd"),
                    run: text("run"),
                    worktree: flag("worktree"),
                    show: flag("show"),
                },
                "pane.add" => Request::AddPane {
                    group: number("group")?,
                    cwd: text("cwd"),
                    run: text("run"),
                    show: flag("show"),
                },
                "show" => Request::Show {
                    pane: number("target")?.ok_or("show needs a pane")?,
                },
                other => return Err(format!("no such request: {other}")),
            };
            Ok(request)
        };
        Some(Call {
            id,
            caller,
            request: read(),
        })
    }
}

/// A request as it arrived: where to answer it, who asked, and what for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub id: String,
    pub caller: Option<u32>,
    pub request: Result<Request, String>,
}

/// An id is a directory name in the state directory, so nothing that could
/// climb out of it.
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|char| char.is_ascii_alphanumeric() || char == '-')
}

/// Marks the plugin's commands for this API in `RunCommandResult`.
pub const CONTEXT_CLAIM: &str = "api-claim";
pub const CONTEXT_ANSWER: &str = "api-answer";
/// Which request a command was about.
pub const CONTEXT_ID: &str = "api-id";

/// Where request `id` is answered.
pub fn reply_dir(id: &str) -> String {
    format!("{STATE_DIR}/api/{id}")
}

/// Succeeds for exactly one of the sidebars that heard request `id`.
pub fn claim_command(id: &str) -> [String; 4] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!("mkdir -p {STATE_DIR}/api && mkdir \"{STATE_DIR}/api/$0\" 2>/dev/null"),
        id.to_owned(),
    ]
}

/// Writes the answer whole: the asker polls for `reply` and must never read
/// half of one.
pub fn answer_command(id: &str, answer: &str) -> [String; 5] {
    [
        "sh".to_owned(),
        "-c".to_owned(),
        format!(
            "d=\"{STATE_DIR}/api/$0\" && printf '%s\\n' \"$1\" > \"$d/reply.part\" \
             && mv \"$d/reply.part\" \"$d/reply\""
        ),
        id.to_owned(),
        answer.to_owned(),
    ]
}

/// Zellij splits `--args` on every comma, with no way to escape one, so a
/// value carries its commas - and the escape itself - percent-encoded.
fn encode(value: &str) -> String {
    value.replace('%', "%25").replace(',', "%2C")
}

fn decode(value: &str) -> String {
    value.replace("%2C", ",").replace("%25", "%")
}

/// A status as the list says it: the hook's words, plus `unread` and the two
/// the sidebar works out for itself.
pub fn status_word(status: crate::agent::Status) -> &'static str {
    use crate::agent::Status;
    match status {
        Status::NeedsInput => "needs-input",
        Status::Unread => "unread",
        Status::Running => "running",
        Status::Done => "done",
        Status::Idle => "idle",
        Status::Unknown => "unknown",
        Status::Pane => "pane",
    }
}

/// One group, as the list reports it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GroupView {
    pub project: String,
    /// The group's first pane, which is how it is named everywhere.
    pub group: u32,
    pub status: &'static str,
    pub cwd: String,
    pub panes: Vec<u32>,
    /// The member on screen when the group is, and the one that comes back
    /// when it is shown.
    pub current: u32,
    pub on_screen: bool,
}

pub fn ok() -> String {
    r#"{"ok":true}"#.to_owned()
}

pub fn opened(pane: u32, group: u32) -> String {
    format!(r#"{{"ok":true,"pane":{pane},"group":{group}}}"#)
}

pub fn error(why: &str) -> String {
    format!(r#"{{"ok":false,"error":{}}}"#, string(why))
}

/// Projects in the order their first group comes, each with its groups.
pub fn listing(groups: &[GroupView]) -> String {
    let mut projects: Vec<(&str, Vec<&GroupView>)> = Vec::new();
    for view in groups {
        match projects.iter_mut().find(|(name, _)| *name == view.project) {
            Some((_, members)) => members.push(view),
            None => projects.push((&view.project, vec![view])),
        }
    }
    let projects: Vec<String> = projects
        .iter()
        .map(|(name, groups)| {
            let groups: Vec<String> = groups
                .iter()
                .map(|view| {
                    let panes: Vec<String> = view.panes.iter().map(u32::to_string).collect();
                    format!(
                        r#"{{"group":{},"status":{},"cwd":{},"panes":[{}],"current":{},"on_screen":{}}}"#,
                        view.group,
                        string(view.status),
                        string(&view.cwd),
                        panes.join(","),
                        view.current,
                        view.on_screen
                    )
                })
                .collect();
            format!(
                r#"{{"project":{},"groups":[{}]}}"#,
                string(name),
                groups.join(",")
            )
        })
        .collect();
    format!(r#"{{"ok":true,"projects":[{}]}}"#, projects.join(","))
}

/// A JSON string, escaped.
fn string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for char in text.chars() {
        match char {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            char if (char as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", char as u32)),
            char => out.push(char),
        }
    }
    out.push('"');
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// What Zellij does to `--args` before the plugin sees them.
    fn as_zellij_reads(args: &str) -> BTreeMap<String, String> {
        args.split(',')
            .filter(|pair| !pair.is_empty())
            .map(|pair| {
                let mut parts = pair.split('=');
                let key = parts.next().unwrap_or_default().to_owned();
                (key, parts.collect::<Vec<_>>().join("="))
            })
            .collect()
    }

    #[test]
    fn a_request_survives_the_pipe() {
        let requests = [
            Request::List,
            Request::NewGroup {
                cwd: Some("/home/pp/api".into()),
                run: Some("claude --model x".into()),
                worktree: true,
                show: false,
            },
            Request::AddPane {
                group: None,
                cwd: None,
                run: Some("npm run dev -- --port=3000,3001 50%".into()),
                show: true,
            },
            Request::Show { pane: 7 },
        ];
        for request in requests {
            let (name, args) = request.to_pipe("41-7", Some(4));
            let back = Request::from_pipe(&name, &as_zellij_reads(&args));
            let call = Call {
                id: "41-7".into(),
                caller: Some(4),
                request: Ok(request.clone()),
            };
            assert_eq!(back, Some(call), "{name} {args}");
        }
    }

    #[test]
    fn a_pipe_that_is_not_ours_or_is_wrong_says_so() {
        let id = as_zellij_reads("id=1");
        assert_eq!(Request::from_pipe("cycle", &id), None, "the sidebar's own");
        let wrong = |name, args: &str| {
            Request::from_pipe(name, &as_zellij_reads(args)).map(|call| call.request.is_err())
        };
        assert_eq!(wrong("agenttij.nope", "id=1"), Some(true));
        assert_eq!(wrong("agenttij.show", "id=1"), Some(true));
        assert_eq!(wrong("agenttij.list", "id=1,pane=x"), Some(true));
        assert_eq!(wrong("agenttij.list", "id=1,pane=3"), Some(false));
        // Nowhere to answer, so nothing to do.
        assert_eq!(wrong("agenttij.list", ""), None);
        assert_eq!(wrong("agenttij.list", "id=../x"), None);
    }

    #[test]
    fn one_sidebar_claims_a_request_and_its_answer_lands_whole() {
        let dir = std::env::temp_dir().join(format!("agenttij-api-{}", std::process::id()));
        let at = |command: &[String]| -> Vec<String> {
            command
                .iter()
                .map(|word| word.replace(STATE_DIR, &dir.to_string_lossy()))
                .collect()
        };
        let run = |command: Vec<String>| {
            std::process::Command::new(&command[0])
                .args(&command[1..])
                .status()
                .expect("sh runs")
                .success()
        };
        let first = run(at(&claim_command("9-1")));
        let second = run(at(&claim_command("9-1")));
        let answered = run(at(&answer_command("9-1", r#"{"ok":true,"a b":"$1"}"#)));
        let reply = std::fs::read_to_string(dir.join("api/9-1/reply"));
        std::fs::remove_dir_all(&dir).ok();
        assert!(first && !second, "only one claim wins");
        assert!(answered);
        assert_eq!(reply.expect("written"), "{\"ok\":true,\"a b\":\"$1\"}\n");
        assert_eq!(reply_dir("9-1"), format!("{STATE_DIR}/api/9-1"));
    }

    #[test]
    fn answers_are_json() {
        assert_eq!(opened(12, 3), r#"{"ok":true,"pane":12,"group":3}"#);
        assert_eq!(
            error("no \"sidebar\"\n"),
            r#"{"ok":false,"error":"no \"sidebar\"\n"}"#
        );
        let view = |project: &str, group| GroupView {
            project: project.into(),
            group,
            status: "running",
            cwd: "/x".into(),
            panes: vec![group, group + 1],
            current: group,
            on_screen: group == 0,
        };
        assert_eq!(
            listing(&[view("api", 0), view("web", 3), view("api", 6)]),
            concat!(
                r#"{"ok":true,"projects":["#,
                r#"{"project":"api","groups":["#,
                r#"{"group":0,"status":"running","cwd":"/x","panes":[0,1],"current":0,"on_screen":true},"#,
                r#"{"group":6,"status":"running","cwd":"/x","panes":[6,7],"current":6,"on_screen":false}]},"#,
                r#"{"project":"web","groups":["#,
                r#"{"group":3,"status":"running","cwd":"/x","panes":[3,4],"current":3,"on_screen":false}]}]}"#
            )
        );
    }
}
