# agenttij — plan

A Zellij sidebar that tracks coding-agent panes (Claude Code and friends)
across every session: what they are, where they are, and whether they are
running, idle, done, or waiting on you. Navigate to it like any other pane,
pick an agent, and either peek at it or jump to it.

Herdr does this as its own multiplexer. This does it inside Zellij.

## Decisions

| | |
|---|---|
| Navigation | Preview first (`p`), switch on demand (`Enter`) |
| Discovery | Claude Code hooks for exact state; process-name discovery so unhooked tools still appear |
| Interaction | Read-only. The sidebar navigates; it never types into an agent |
| Scope | Publishable plugin — config, installer, docs, CI |

## Verified constraints

Read out of `zellij-tile`/`zellij-server` 0.44.3 source, not docs — the docs
contradict themselves on the render-report API. These are the facts the design
is built on; re-check them on a Zellij major bump.

**Cross-session state is metadata-only.** `Event::SessionUpdate` carries
`SessionInfo` for every live session (name, tabs, `PaneManifest`), but never
pane contents. `SessionInfo` also has no `cwd`, and `PaneInfo.terminal_command`
is only populated for *command* panes — a pane where you typed `claude` in your
shell has neither. So the agent must report its own state and cwd; the plugin
cannot discover them.

**`Event::PaneRenderReport` is not a background monitor.** New in 0.44, push-based
and diffed (you only get panes whose viewport changed), but `Tab::render`
(`tab/mod.rs:3300`) returns early when a tab has no connected clients — so it
only covers the tab you are currently looking at. Useless for watching
background agents. `get_pane_scrollback` *does* reach any tab, but only within
the plugin's own session.

**Content across sessions exists only via the CLI**, and only `dump-screen` is
trustworthy. `zellij -s NAME action dump-screen -p terminal_N` queries the pane
directly and works wherever the pane is. `zellij -s NAME subscribe -p
terminal_N` looks better — it streams — but it is fed by the same render
pipeline, so it delivers one initial snapshot and then goes silent for any pane
in a tab nobody is watching. *Measured, not deduced:* previewing a pane in a
background tab of another session froze at the snapshot while the pane kept
changing. The preview therefore polls `dump-screen` once a second.

**Other sessions' pane manifests may never exist.** `SessionUpdate` learns
about other sessions by reading their `session-metadata.kdl`
(`background_jobs.rs:766`), which Zellij never writes when a user sets
`session_serialization false` — as this machine does. Reconciling against pane
data alone therefore hides every cross-session agent. The authoritative
liveness signal is `zellij list-sessions`, which is derived from IPC sockets
and is correct the instant a session starts.

**`switch_session_with_focus` does not need a tab position.** Verified live:
with `tab_position: None` and only a pane id, the client still landed on a pane
in a *background* tab of the target session. This is what makes jumping work
without pane manifests.

**Zellij's default floating pane is 40x10.** Too small to preview an agent —
it re-wraps an 80-column pane into ribbon. The preview asks for 80%x80%.

**A plugin's `/tmp` is a mount of `$TMPDIR/zellij`**, and
`create_wasi_ctx` (`plugin_loader.rs:432`) filters out mounts whose host
directory does not exist at load time. `/tmp/zellij` does not exist on a fresh
boot. A plugin cannot fix a missing mount from inside itself, so state is read
via one `sh` fork per tick instead. That also frees the state dir from living
inside Zellij's own directories.

**`ZELLIJ_SESSION_NAME` / `ZELLIJ_PANE_ID`** are set in every pane's
environment — the link between an agent process and a Zellij pane, and the
reason the hook approach works at all. Note they go stale if a session is
renamed, so the plugin reconciles reported names against `SessionUpdate` and
drops what it cannot find.

**Sessions are version-scoped.** `SessionUpdate` and `zellij ls` only see
sessions of the running Zellij version, so state from an older version's
sessions must be reaped, not trusted.

## Architecture

State flows one way: the agent reports, a file records, the sidebar reads.

```
Claude Code hook ──> /tmp/agenttij/<session>.<pane>.state
                          │
             sh: date + list-sessions + cat, 1/tick
                          v
   SessionUpdate ──> [ sidebar plugin ] ──> Enter: switch_session_with_focus
   (pane detail)                        └──> p:     dump-screen, polled 1/s
```

Files, not plugin memory or pipes, because a sidebar in a session that started
*later* must still see every running agent, and a reload must lose nothing.
Push via `zellij pipe` is a latency optimisation to add only if a 1s tick feels
slow.

### Workspace

Two crates, because the boundary is real: everything that can be tested on the
host, and everything that can only run inside a WASM plugin host.

```
crates/core    agenttij-core — zero dependencies, 100% unit-testable:
               status/agent types, state-file parsing, discovery, reaping,
               sorting, config parsing, age formatting
crates/plugin  agenttij — the wasm binary: lifecycle, event wiring, rendering,
               navigation. Adapts SessionInfo into core's neutral types
hooks/         the shell hook the agent runs
layouts/       left and right sidebar layouts
scripts/       installer
```

`core` deliberately does not depend on `zellij-tile`: the plugin crate converts
`SessionInfo` into a flat `PaneSnapshot` list at the boundary. That keeps the
logic testable with a plain `cargo test` and keeps the WASM surface thin. A
third crate for the hook would be ceremony — it is 20 lines of `sh`.

### State machine

Claude Code hook events, mapped by the argument the hook is registered with:

| Hook event | Status |
|---|---|
| `SessionStart` | `idle` |
| `UserPromptSubmit` | `running` |
| `Notification` | `needs-input` |
| `Stop` | `done` |
| `SessionEnd` | removes the file |

`SubagentStop` is deliberately **not** registered: it can fire after the main
turn has already ended and would revive an idle pane. (Herdr's own integration
carries a comment about exactly this bug.)

Sort order is attention-first: `needs-input`, `running`, `done`, `idle`,
`unknown`; ties broken by most recently active.

### Trust

A status board you cannot trust is worse than none, so every tick reconciles
against `SessionUpdate` and drops agents whose session or pane no longer
exists. This is what covers `kill -9`, a closed pane, and a dead session —
cases where no hook ever fires.

## Milestones

All done. `CommandChanged` was dropped from M5: `SessionUpdate` already carries
pane titles, so a second discovery path earned nothing.

- **M0** ✅ Hook + state files.
- **M1** ✅ Workspace, `core` types + parsing, 32 tests.
- **M2** ✅ Plugin renders the list read-only.
- **M3** ✅ `j`/`k` + `Enter`, in-session and across sessions.
- **M4** ✅ `p` preview, polled `dump-screen`.
- **M5** ✅ Reconciliation and process-name discovery.
- **M6** ✅ Installer, layouts, config, README, CI.

## How it was verified

Plugin panes are invisible to every capture path Zellij offers — `dump-screen`
and `subscribe` both return nothing for them, including for known-good plugins
like `zjstatus`. So the sidebar's rendering is covered by unit tests on the
layout arithmetic, and its *behaviour* was verified in a live session through
observable side effects:

| Claim | How it was shown |
|---|---|
| Timer + `run_command` + permissions work | deleted `/tmp/agenttij`; the plugin's own `mkdir` recreated it |
| Scan parses, list is populated, selection works | `p` opened a preview aimed at the right pane |
| `j` moves the cursor | `j` then `p` previewed the *second* agent |
| Sort is attention-first | the `needs-input` agent was selected without moving |
| Dead sessions are reaped | a dead agent given top sort priority was still not selectable |
| Cross-session agents survive with no pane manifest | agent in another session stayed listed and previewable |
| Preview streams a background tab in another session | marker echoed into the target appeared in the preview |
| Peeking does not disturb the target | target pane stayed `9 78` before and after |
| `Enter` switches and lands on the pane | `list-clients` showed the client move sessions onto `terminal_1` |
| Hook registration is safe and reversible | ran against a copy of the real `settings.json`: herdr preserved, idempotent over 3 runs, uninstall round-trips |

Two design errors were caught this way and are worth remembering, because both
looked right on paper: the `subscribe`-based preview (silent for background
tabs) and reconciling against pane manifests (hides every cross-session agent
when `session_serialization` is off).

## Deferred

- Push updates via `zellij pipe` — only if the tick feels slow.
- Scrollback heuristics for unhooked tools — breaks on every upstream TUI change.
- Acting on agents from the sidebar (interrupt, answer prompts) — read-only by decision.
- `set_pane_color` to tint an agent's pane by status — same-session only, cosmetic.
