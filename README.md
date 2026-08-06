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
| `c` | park the sidebar off screen (`Alt z` brings it back) |

Zellij-level, installed for you: `Alt a` summons a sidebar in any session,
`Alt z` parks and unparks it. Full reference, including how to rebind and how to
drive it without keybinds: [docs/KEYBINDS.md](docs/KEYBINDS.md).

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

The installer builds the plugin and drops it in `~/.config/zellij/plugins`,
installs the sidebar layouts, registers the Claude Code hook in
`~/.claude/settings.json`, binds `Alt a` in your Zellij config, and pre-grants
the plugin's permissions. Everything is backed up first, other tools' entries
are left alone, and re-running replaces rather than duplicates. The config edit
is checked with `zellij setup --check` and rolled back if Zellij dislikes it.
`--no-keybind` and `--no-grant` opt out; `--uninstall` reverses all of it. The
bindings go *inside* your existing `keybinds` block, because Zellij only reads
the first one it finds — see [docs/KEYBINDS.md](docs/KEYBINDS.md).

Keybindings are read when a session starts, so start a new session after
installing.

Permissions are pre-granted because Zellij asks by drawing a prompt *over* the
plugin's pane, and that prompt does not fit in 26 columns — the first launch
would otherwise be a blank pane waiting on a keypress you cannot see. Revoke by
deleting the entry from `~/.cache/zellij/permissions.kdl`. Note the cache is
keyed by plugin URL, so a rebuild installed elsewhere gets asked again.

Which side the sidebar sits on is a layout property, not a plugin setting —
use `agenttij-right`, or edit the layout and change `size=26` to taste.

### Workspace mode: a sidebar that never reloads

```sh
zellij --new-session-with-layout agenttij-workspace
```

This is the layout to use if you want the sidebar to *stay put* while the area
beside it changes. The sidebar owns a fixed column; the rest of the tab is a
single slot holding exactly one agent. Picking another agent puts it in the slot
and **parks** the previous one off screen — Zellij calls that suppressed, and it
keeps running. The sidebar never moves, never re-renders from scratch, and there
is no detach: `Enter` is a pane swap.

```
┌ agents ────┬──────────────────────────┐
│ ● bravo    │                          │
│ ◐ delta    │  bravo                   │  ← the only agent on screen
│ ✓ alpha    │                          │
│ ○ charlie  │  (the rest are parked,   │
│            │   still running)         │
└────────────┴──────────────────────────┘
```

Open agents however you like in this session — the first swap parks the extras.
The layout ships `scope "session"`, so the sidebar lists only this session's
agents and `Enter` can never throw you out of the workspace, and `solo "true"`,
which is what parks the others instead of leaving them on screen.

`c` parks the sidebar itself when you want the full width, and `Alt z` brings it
back. That is a keybind rather than another `c` because a hidden pane cannot be
focused to press a key in. Note that Zellij identifies a plugin by url *and*
configuration, so the keybind repeats the workspace layout's configuration — if
you change one, change both, or the keybind will launch a second sidebar
instead of talking to the one you have.

The catch is a hard one: **panes belong to the session that owns them.** Zellij
has no way to render another session's pane inside yours, so this only works for
agents running here. That is also why jumping to another session "reloads" the
sidebar — it is a real detach and reattach, with a different plugin instance on
the other side. If you want the persistent-sidebar feel, run your agents in one
workspace session rather than one session each.

Tabs cannot do this: switching tabs replaces the whole screen, sidebar
included. You can put a sidebar in every tab via `new_tab_template`, but that is
one plugin instance per tab with its own selection, not one that persists.

A pane stack (`stacked=true`) is the other way to arrange this, and it keeps
every agent on screen as a one-line title. Solo mode exists because that is not
always what you want. Two things to know if you try a stack anyway: a stack
declared with a single pane is not treated as a stack, and panes opened beside
it split instead of joining; and agenttij never calls `stack_panes` itself,
since new panes join a real stack on their own.

Outside workspace mode the sidebar lists agents everywhere, sorting this
session's agents first and marking the rest with `⇢` — so a row that will cost
you a detach always says so before you press `Enter`.

### Landing in a session with no sidebar

Jumping to a session that has no sidebar leaves you nothing to jump back with.
That is what `Alt a` is for: it summons the sidebar as a floating pane in
whatever session you are in, and focuses the existing one rather than stacking
up duplicates.

Keybindings are read when a session starts, so sessions that were already
running when you installed won't have it. From one of those, summon it by hand
— the `--` is required, Zellij will not take the URL without it:

```sh
zellij plugin --floating -- "file:$HOME/.config/zellij/plugins/agenttij.wasm"
```

To give every new session its own docked sidebar instead, set
`default_layout "agenttij-left"` in your Zellij config.

## Configuration

Three knobs:

```kdl
pane size=26 {
    plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
        // process names used to spot agents that are not reporting
        agents "claude,codex,aider,gemini,my-agent"
        // "all" (default) or "session" to list only this session's agents
        scope "session"
        // "true" to show one agent at a time, parking the others off screen
        solo "true"
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
layouts/       sidebar layouts: left, right, and workspace (persistent sidebar)
scripts/       installer
docs/KEYBINDS.md  every key, and how to change them
docs/PLAN.md      design notes and the Zellij constraints they follow from
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
  session *is* in Zellij; `p` exists so you rarely need it, and `Alt a` gets
  you a sidebar in whatever session you end up in.
- **Peeking polls once a second** rather than streaming. `zellij subscribe`
  streams, but it is fed by the render pipeline, which skips tabs nobody is
  watching — so it goes silent for exactly the background agents you wanted to
  check on. `dump-screen` queries the pane directly and always works.
- **Cross-session pane detail depends on `session_serialization`.** With it
  off, Zellij never publishes other sessions' pane manifests, so agents there
  are tracked at session resolution. Jumping still lands on the right pane;
  discovery of non-reporting agents elsewhere does not work.
- **Read-only.** The sidebar navigates; it never types into an agent.
- **A fixed-width pane cannot be resized.** A pane declared `size=26` is
  immovable, by Zellij's own `resize` action as much as by a plugin — which is
  why `c` parks the sidebar rather than folding it to a narrow rail.
