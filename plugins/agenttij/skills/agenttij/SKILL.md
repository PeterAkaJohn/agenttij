---
name: agenttij
description: Use when running inside Zellij with the agenttij sidebar and you need a long-lived process (dev server, watcher, log tail, REPL, a second agent) or a separate workspace - open a pane in your group or a new group instead of backgrounding it in your own shell.
---

# Panes and groups through the agenttij sidebar

The sidebar shows **projects**, each made of **groups** (its rows), each made of **panes**.
One pane of one group is on screen at a time; the user cycles through a group's panes and moves between groups.
Your own pane is in a group, and "your group" is where things you open belong.

Use the `agenttij` MCP tools, or the `agenttij` command in a shell - they are the same:

| Tool | Command | What it does |
| --- | --- | --- |
| `list_groups` | `agenttij list` | Projects, groups, panes, which group is on screen, each group's status |
| `add_pane` | `agenttij pane add [--run CMD] [--cwd DIR] [--group N] [--show]` | A pane in your group (or group N) |
| `new_group` | `agenttij group new [--run CMD] [--cwd DIR] [--worktree] [--show]` | A new group from the layout's template |
| `show_pane` | `agenttij show PANE` | Puts a pane on screen |

## How to use it well

- Start servers, watchers and log tails with `add_pane` and `run`, not with `&` or `nohup` in your own shell.
  The user can look at them by cycling the group, and they survive your shell.
- What you open is **parked**: it runs, but the screen and the focus stay where they are.
  Pass `show` only when the user asked to see it - taking the screen from someone mid-sentence is rude.
- `run` is a command line run through `sh`, so quoting and arguments work as in a shell.
  Without `run` the pane is a shell.
- `new_group` with `worktree` starts in a fresh git worktree of the repository, so another agent there does not write over your files.
  Use it for a parallel task in the same repository.
- Answers are JSON. Errors say why: `{"ok":false,"error":"..."}`.
  `no answer from the sidebar` means this session's layout has no agenttij sidebar - tell the user rather than retrying.
- Panes you open are the user's panes: do not open many, and say what you opened and why.
