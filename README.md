# agenttij

A Zellij sidebar that tracks coding agents — Claude Code and friends — across
every session on the machine. It shows what each one is doing, sorts whatever
needs you to the top, and lets you peek at an agent or jump to it.

```
┌ agents ────────────┐
│ ● agenttij     2m  │  ● needs input
│ ◐ api-refactor 12s │  ◐ running
│ ✓ docs-fix     5m  │  ✓ done
│ ○ scratch      1h  │  ○ idle
│                    │
│ j/k ↵ go  p peek   │
└────────────────────┘
```

The sidebar is an ordinary pane: focus it with your usual pane navigation, then

| key | |
|---|---|
| `j` / `k`, `↓` / `↑` | move |
| `Enter` | go to that agent (switches session if needed) |
| `p` | peek at it in a floating pane, without leaving your session |

Peeking is the point. Checking on an agent shouldn't cost you a detach and a
reattach, so `p` gives you a live view of its pane wherever it is — another
session, a background tab, a session nobody is attached to.

## Install

Needs the `wasm32-wasip1` target and Zellij 0.44+.

```sh
rustup target add wasm32-wasip1
./scripts/install.sh
zellij --new-session-with-layout agenttij-left
```

The installer builds the plugin, drops it in `~/.config/zellij/plugins`,
installs the sidebar layouts, and registers the Claude Code hook in
`~/.claude/settings.json` (backing the file up first, leaving other tools'
hooks alone, and replacing rather than duplicating its own entries on re-run).
`./scripts/install.sh --uninstall` reverses all of it.

Which side the sidebar sits on is a layout property, not a plugin setting —
use `agenttij-right`, or edit the layout and change `size=26` to taste.

## Configuration

One knob, for the process names used to spot agents that aren't reporting:

```kdl
pane size=26 {
    plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
        agents "claude,codex,aider,gemini,my-agent"
    }
}
```

## How it works

State flows one way. The agent reports, a file records, the sidebar reads.

```
Claude Code hook ──> /tmp/agenttij/<session>.<pane>.state
                          │
                     sh + cat, 1/tick
                          v
   SessionUpdate ──> [ sidebar plugin ] ──> Enter: switch_session_with_focus
   (this session)                      └──> p:     dump-screen, polled
```

The hook is registered once per Claude Code event and passes its state as an
argument, so it needs no JSON parsing — 20 lines of `sh`, no dependencies. It
keys state by `$ZELLIJ_SESSION_NAME` and `$ZELLIJ_PANE_ID`, which is what ties
an agent process to a Zellij pane.

| Hook event | Status |
|---|---|
| `SessionStart` | idle |
| `UserPromptSubmit` | running |
| `Notification` | needs input |
| `Stop` | done |
| `SessionEnd` | removed |

State goes in files rather than plugin memory so that a sidebar in a session
that started later still sees every running agent, and a reload loses nothing.

Agents that never report are still listed, discovered by pane title, as
`?`/unknown. Note that Claude Code sets its own terminal title, so this
fallback mostly helps *other* tools; hooked agents don't need it.

Nothing reaches the sidebar without something alive behind it. Every tick
reconciles against the live session list and, where available, the pane list —
which is what covers `kill -9`, closed panes and dead sessions, where no hook
ever fires.

### No async runtime

Zellij plugins are single-threaded WASI modules run by the `wasmi` interpreter:
no threads, no reactor, so no `tokio` — and none is wanted. The host is the
runtime. Anything slow is handed over and comes back as an event:
`run_command` → `RunCommandResult`, `set_timeout` → `Timer`.

## Layout

```
crates/core    agenttij-core — zero dependencies, all the decisions, unit-tested
crates/plugin  agenttij — the wasm plugin: lifecycle, rendering, navigation
hooks/         the shell hook agents run
layouts/       left and right sidebar layouts
scripts/       installer
docs/PLAN.md   design notes and the Zellij constraints they follow from
```

`core` deliberately does not depend on `zellij-tile`; the plugin crate adapts
`SessionInfo` into `core`'s own types at the boundary. That is what lets
`cargo test` run on the host instead of needing a WASM host.

```sh
cargo test -p agenttij-core
cargo clippy -p agenttij --target wasm32-wasip1
```

## Known limits

- **Switching sessions detaches and reattaches.** That is what switching a
  session *is* in Zellij; `p` exists so you rarely need it.
- **Peeking polls once a second** rather than streaming. `zellij subscribe`
  streams, but it is fed by the render pipeline, which skips tabs nobody is
  watching — so it goes silent for exactly the background agents you wanted to
  check on. `dump-screen` queries the pane directly and always works.
- **Cross-session pane detail depends on `session_serialization`.** With it
  off, Zellij never publishes other sessions' pane manifests, so agents there
  are tracked at session resolution. Jumping still lands on the right pane;
  discovery of non-reporting agents elsewhere does not work.
- **Read-only.** The sidebar navigates; it never types into an agent.
