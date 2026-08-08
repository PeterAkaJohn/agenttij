# agenttij

A Zellij sidebar that tracks coding agents — Claude Code and friends — across
every session on the machine. It shows what each one is doing, sorts whatever
needs you to the top, holds the rest still, and lets you peek at an agent or
jump to it.

```
┌ agents ────────────┐
│ ⚠ agenttij     2m  │  ⚠ needs input     ◐ running
│ ◐ api-refactor 12s │  ✓ done            ○ idle
│ ✓ docs-fix   3 5m  │  ? unknown         · a pane, no agent
│ ○ scratch      1h  │
│                    │  "3" = the row owns
│ j/k ↵ n a v b d    │  three panes
└────────────────────┘
```

The sidebar is an ordinary pane: focus it with your usual pane navigation (or
`Alt s`), then. In the first column, `›` is the cursor and `▪` is the row on
screen — the same thing until you open a pane somewhere else.

| key | |
|---|---|
| `j` / `k`, `↓` / `↑` | move |
| `Enter` | show that row — switches session if the agent is elsewhere |
| `Tab` | open a row up: its panes listed underneath — or fold a project |
| `[`, `]` | jump between projects |
| `J`, `K` | move a project, or a row within its project |
| `r` | name a project — two with the same name are one |
| `b` | flip back to the previous row (`Alt b` anywhere) |
| `v` | cycle the panes *within* this row (`Alt v` anywhere) |
| `a` | add a pane to this row: an editor, a log, whatever (`Alt m`) |
| `n` | new agent pane — a new row (`Alt g`) |
| `d` `d` | close the row, the pane under it, or a whole project — asks first |
| `c` `c` | interrupt the agent, without going to it — asks first |
| `!` | only what needs you |
| `p` | peek at an agent without leaving your session |
| `q`, `Esc` | dismiss a peek |
| `?` | show every key and what it does |

Every one of those that makes sense away from the sidebar has a global binding,
installed for you: `Alt s` focuses the sidebar, `Alt v` cycles a row's panes,
`Alt b` flips rows, `Alt g` starts a new row, `Alt m` adds a pane to the row on
screen, `Alt a` summons a sidebar in any session, and `Alt ]` — Zellij's own
swap-layout key — folds the sidebar to a status rail. Full reference:
[docs/KEYBINDS.md](docs/KEYBINDS.md).

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
the plugin's permissions. Re-run it after updating: a version that asks for a
permission the old one did not will otherwise wait on a prompt too wide to fit in
a sidebar. Everything is backed up first, other tools' entries
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

The layouts are examples. Which side the sidebar sits on, how wide it is, what
else is in the tab — all of that is yours to write; ours only shows the sidebar
next to a working area, with Zellij's tab bar and status bar commented out
because your config probably supplies its own. Copy one and edit it, or lift the
`plugin` block into a layout you already have. The only part worth keeping
verbatim is the `rail` swap layout, which has to repeat the sidebar's
configuration exactly for `Alt ]` to fold the pane you have rather than launch a
second one.

### Workspace mode: a sidebar that never reloads

```sh
zellij --new-session-with-layout agenttij-workspace
```

This is the layout to use if you want the sidebar to *stay put* while the area
beside it changes. The sidebar owns a column; the rest of the tab is a single
slot holding exactly one pane. Picking a row puts that row's current pane in the
slot and **parks** whatever was there — Zellij calls it suppressed, and it keeps
running. The sidebar never moves, never re-renders from scratch, and there is no
detach: `Enter` is a pane swap.

```
┌ agents ────┬──────────────────────────┐
│ ⚠ bravo    │                          │
│ ◐ delta  2 │  delta's editor          │ ← the row on screen is showing
│    nvim    │                          │   its second pane; Tab opened
│ ✓ alpha    │                          │   the row to list it
│ · scratch  │  (every other pane is    │
│            │   parked, still running) │
└────────────┴──────────────────────────┘
     ▲            ▲
     │            └─ one pane, whichever member of the row you were last on
     └─ one row per agent; "2" means that row owns two panes
```

Start agents with `n` (or `Alt g`) and add panes to a row with `a` (or `Alt m`).
Both park what was on screen instead of splitting it. `d` `d` closes a row and
everything parked behind it — or one pane of a row, if you opened the row with
`Tab` — and puts the next row on screen rather than leaving it empty. Opening
panes any other way works too: the first swap parks the extras, and anything
ungrouped becomes a row of its own.

Rows gather into **projects**: everything working on one codebase, under the git
root the hook records, so an agent started in `crates/core` sits with one started
at the top of the repo. A front end and a back end in separate repositories are
one project the moment you say so — `r`, name both `acme`, and they are together
from then on, including agents you start in either later. When the grouping
belongs to the code rather than to you, put a `.agenttij` file above them holding
the name (or empty, to use its directory's): it travels with the checkout, works
on every machine, and needs nobody to press anything. A project folds to a line that still shows the worst
status inside it, `[` and `]` step between them, `J` and `K` put them in the
order you want, and both the order and which projects you folded are remembered
in `~/.cache/agenttij/order`, so a reload or a new day finds them as you left
them — and headers only appear once there is more
than one project to tell apart. Four repositories become four
lines you can open, instead of fifteen you have to read.

In solo mode a **row is a group of panes**, not a single pane. `a` adds a pane to
the row you are on — an editor next to the agent, a log next to that — and
exactly one member is on screen at a time. Companions never get rows of their
own: the row *is* the agent session. `Alt v` cycles through the row's panes from
anywhere, including while you are typing at the agent, which is the point of it.

Each row remembers which member you were last looking at. Every pane belongs to
exactly one row, and a pane the sidebar does not recognise becomes a row of its
own — so a reload costs you the grouping, never access to a pane.
The layout ships `scope "session"`, so the sidebar lists only this session's
agents and `Enter` can never throw you out of the workspace, and `solo "true"`,
which is what parks the others instead of leaving them on screen.

When you want the width back, `Alt ]` folds the sidebar to a status rail and
folds it out again — same side, same percentage width, because it is a swap
layout defined alongside the main one rather than something the plugin does to
itself. Each layout's `rail` block repeats that layout's plugin configuration,
since Zellij identifies a plugin by url *and* configuration; change one, change
both.

There is a second swap layout, `sidebar`, listed before the rail and identical to
the layout you start in. It is there because Zellij re-applies a swap layout
every time a pane is added and registers your base layout with an exact pane
count, so with only a rail to fall through to, opening a pane folded the sidebar
by itself. It costs one press: in a session still holding the panes its layout
was written with, the first `Alt ]` lands on `sidebar` and looks like nothing
happened, and the second folds. After that `Alt ]` folds on the first press, and
a rail you chose stays a rail when you open panes.

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

The knobs:

```kdl
pane size="20%" {
    plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
        // process names used to spot agents that are not reporting
        agents "claude,codex,opencode,aider,gemini,my-agent"
        // "all" (default) or "session" to list only this session's agents
        scope "session"
        // "true" makes a row a group of panes, one on screen at a time
        solo "true"
        // what the pane frame calls itself (not `title`: Zellij keeps that one)
        pane_title "agents"
        // per-status colours: a name, a 256-colour index, or #rrggbb.
        // statuses are needs-input, running, done, idle, unknown, pane
        colors "needs-input=yellow,running=blue,done=green,idle=bright-black"
        // run something when an agent becomes blocked on you
        notify "notify-send -u critical"
        // "false" to stop naming a row's panes "<row> 2/3"
        position "true"
    }
}
```

A percentage width rather than `size=26`, because a fixed-width pane cannot be
resized — which is what `Alt ]` needs in order to fold it to a rail.

## How it works

State flows one way. The agent reports, a file records, the sidebar reads.

```
Claude Code hook ──> /tmp/agenttij/<session>.<pane>.state
                          │
                     sh + cat, 1/tick
                          v
   SessionUpdate ──> [ sidebar plugin ] ──> Enter: switch_session_with_focus
   (this session)        ^             └──> p:     dump-screen, polled
                         │
              get_session_list() — which sessions are alive
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

### Other agents

The hook is not Claude-specific: it takes a state word as its argument and reads
`$ZELLIJ_SESSION_NAME` / `$ZELLIJ_PANE_ID` from the environment, so anything that
can run a command on an event can feed the sidebar.

**opencode** ships a plugin system, and `integrations/opencode/agenttij.js` uses
it. Copy that file to `~/.config/opencode/plugins/`:

| opencode event | Status |
|---|---|
| `session.created` | idle |
| `tool.execute.before` | running |
| `permission.asked` | needs input |
| `session.idle` | done |
| `session.deleted` | removed |

For anything else, call `agenttij-state.sh <state>` from whatever event
mechanism it has. Without one, a tool still appears — by pane title if its name
is in `agents`, and always in a solo workspace, just without a live status.

A row is named after the folder it is working in — the last component, not the
whole path. An agent reports its own through the hook; a plain pane is asked for
its cwd, since its title is a shell prompt and says whatever your prompt says.

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
hooks/         the shell hook agents run — tool-agnostic
integrations/  per-tool glue that calls it (an opencode plugin)
layouts/       left, right, workspace, each with `sidebar` and `rail` swaps
scripts/       installer, its helpers, and a real-keystroke test harness
docs/KEYBINDS.md  every key, and how to change them
docs/ROADMAP.md   what could come next, and what Zellij will not allow
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
  immovable, by Zellij's own `resize` action as much as by a plugin, which is why
  the layouts use a percentage width and fold via a swap layout.
