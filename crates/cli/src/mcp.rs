//! `agenttij mcp`: the same requests as tools, for an agent that speaks MCP.
//! JSON-RPC over stdio, one message per line, as the MCP stdio transport has it.

use std::io::{BufRead, Write};

use agenttij_core::api::Request;
use serde_json::{json, Value};

/// Answered when the client asks for a version we do not know. The tools here
/// use nothing newer than the first release had.
const PROTOCOL: &str = "2025-06-18";

pub fn serve() -> std::io::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout().lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let in_zellij = std::env::var_os("ZELLIJ_SESSION_NAME").is_some();
        if let Some(reply) = handle(&line, in_zellij) {
            writeln!(stdout, "{reply}")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// One message in, at most one out: notifications get no answer.
///
/// Outside Zellij there is no sidebar to ask, so there are no tools either -
/// rather than four that can only fail, in the context of every session of an
/// agent that has this server configured everywhere.
fn handle(line: &str, in_zellij: bool) -> Option<Value> {
    let message: Value = match serde_json::from_str(line) {
        Ok(message) => message,
        Err(why) => return Some(failure(Value::Null, -32700, &format!("parse error: {why}"))),
    };
    let id = message.get("id").cloned()?;
    let method = message.get("method").and_then(Value::as_str).unwrap_or("");
    let params = message.get("params").cloned().unwrap_or(Value::Null);
    let result = match method {
        "initialize" => {
            let asked = params.get("protocolVersion").and_then(Value::as_str);
            json!({
                "protocolVersion": asked.unwrap_or(PROTOCOL),
                "capabilities": { "tools": {} },
                "serverInfo": { "name": "agenttij", "version": env!("CARGO_PKG_VERSION") },
                "instructions": if in_zellij { INSTRUCTIONS } else { "" },
            })
        }
        "ping" => json!({}),
        "tools/list" if in_zellij => json!({ "tools": tools() }),
        "tools/list" => json!({ "tools": [] }),
        "tools/call" => call(&params),
        _ => return Some(failure(id, -32601, &format!("no method {method}"))),
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn failure(id: Value, code: i64, message: &str) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "error": { "code": code, "message": message } })
}

const INSTRUCTIONS: &str = "\
You are running in a Zellij session with the agenttij sidebar. It shows projects, \
each made of groups (the sidebar's rows), each made of panes; one pane of one group \
is on screen at a time. Use these tools to open a pane beside your own (a dev \
server, a log, a second agent) or a new group, instead of running long-lived \
things in your own shell. What you open is parked behind what the user is \
looking at unless you pass show, so it never takes the screen by surprise.";

fn tools() -> Value {
    let cwd = json!({ "type": "string", "description": "Directory to start in. Defaults to where you are (or the group's directory, for another group)." });
    let run = json!({ "type": "string", "description": "Command line to run in the new pane, through sh. A shell when omitted." });
    let show = json!({ "type": "boolean", "description": "Put it on screen, the way the user's Enter does. Leave false unless the user should look at it now." });
    json!([
        {
            "name": "list_groups",
            "description": "The projects of this Zellij session, their groups and each group's panes: which group is on screen, which pane is current, and each group's status (needs-input, unread, running, done, idle, unknown, pane).",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "new_group",
            "description": "Opens a new group (a row in the sidebar), built from the layout's group template with its first pane running `run`. Parked unless `show`. Returns the new group's id, which is its first pane.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "cwd": cwd,
                    "run": run,
                    "worktree": { "type": "boolean", "description": "Start it in a fresh git worktree of the repository at cwd, so two agents on one repository do not write over each other's files." },
                    "show": show,
                },
            },
        },
        {
            "name": "add_pane",
            "description": "Opens a pane in your own group, or in `group`, parked behind what is on screen unless `show`. The user reaches it by cycling through the group. Returns the pane id.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "group": { "type": "integer", "description": "The group to add to, as list_groups names it. Your own group when omitted." },
                    "cwd": cwd,
                    "run": run,
                    "show": show,
                },
            },
        },
        {
            "name": "show_pane",
            "description": "Puts a pane of this session on screen, as the user's Enter on it would. Only when the user asked to see it.",
            "inputSchema": {
                "type": "object",
                "properties": { "pane": { "type": "integer", "description": "Pane id, as list_groups names it." } },
                "required": ["pane"],
            },
        },
    ])
}

fn call(params: &Value) -> Value {
    let name = params.get("name").and_then(Value::as_str).unwrap_or("");
    let args = params.get("arguments").cloned().unwrap_or(Value::Null);
    let text = |key: &str| {
        args.get(key)
            .and_then(Value::as_str)
            .filter(|value| !value.is_empty())
            .map(str::to_owned)
    };
    let flag = |key: &str| args.get(key).and_then(Value::as_bool).unwrap_or(false);
    let number = |key: &str| {
        args.get(key)
            .and_then(Value::as_u64)
            .and_then(|value| u32::try_from(value).ok())
    };
    let request = match name {
        "list_groups" => Ok(Request::List),
        "new_group" => Ok(Request::NewGroup {
            cwd: text("cwd").map(|dir| crate::absolute(&dir)),
            run: text("run"),
            worktree: flag("worktree"),
            show: flag("show"),
        }),
        "add_pane" => Ok(Request::AddPane {
            group: number("group"),
            cwd: text("cwd").map(|dir| crate::absolute(&dir)),
            run: text("run"),
            show: flag("show"),
        }),
        "show_pane" => number("pane")
            .map(|pane| Request::Show { pane })
            .ok_or("show_pane needs a pane".to_owned()),
        other => Err(format!("no tool {other}")),
    };
    let answer = match request {
        Ok(request) => crate::ask(&request),
        Err(why) => agenttij_core::api::error(&why),
    };
    json!({
        "content": [{ "type": "text", "text": answer }],
        "isError": !crate::answered_ok(&answer),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn answer(line: &str) -> Value {
        handle(line, true).expect("answered")
    }

    #[test]
    fn speaks_enough_mcp_to_be_used() {
        let init = answer(
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        );
        assert_eq!(init["result"]["protocolVersion"], "2025-03-26");
        assert_eq!(init["result"]["serverInfo"]["name"], "agenttij");
        assert!(handle(
            r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
            true
        )
        .is_none());

        let listed = answer(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#);
        let names: Vec<&str> = listed["result"]["tools"]
            .as_array()
            .expect("tools")
            .iter()
            .filter_map(|tool| tool["name"].as_str())
            .collect();
        assert_eq!(names, ["list_groups", "new_group", "add_pane", "show_pane"]);
        let outside = handle(r#"{"jsonrpc":"2.0","id":2,"method":"tools/list"}"#, false);
        assert_eq!(outside.expect("answered")["result"]["tools"], json!([]));

        let wrong = answer(
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"show_pane","arguments":{}}}"#,
        );
        assert_eq!(wrong["result"]["isError"], true);
        assert_eq!(
            answer(r#"{"jsonrpc":"2.0","id":4,"method":"nope"}"#)["error"]["code"],
            -32601
        );
        assert_eq!(answer("{")["error"]["code"], -32700);
    }
}
