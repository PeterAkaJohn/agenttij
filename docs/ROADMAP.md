# Where this could go

A plan, not a promise. Everything here was checked against Zellij 0.44.3's
actual plugin API before being written down; where something is impossible it
says so rather than proposing a shape for it.

The reader to keep in mind: someone with four projects open, two agents working,
one of them stuck on a question, and a remote box with two more.

## What the walls are

These are measured, not assumed, and every idea below is bent around them.

- **Panes belong to the session that owns them.** Nothing in the ~200-function
  plugin API moves or renders a pane across sessions. Any feature phrased as
  "share a pane with another session" has to become something else.
- **The tick is the whole cost.** Idling beside a row of eight the sidebar burns
  ~24ms of CPU a second; with the timer off, 1ms. Anything that adds a poll adds
  the same again — which is why a suite of four polling plugins would cost 4×
  and needs the design in *A suite* below.
- **Filesystem events cannot help.** `watch_filesystem` starts one watcher per
  session rooted at the session's cwd (`plugins/watch_filesystem.rs`), so it can
  never watch `/tmp/agenttij`. The poll stays; it can only get cheaper.
- **A plugin's identity is url + configuration.** Two plugins in the suite that
  want to talk must agree on both, exactly.
- **Suppressed panes forget their geometry**, and closing one un-hides it. Every
  layout idea that "just moves panes around" pays for this.

## The three asks

### Super groups — yes, as projects

A row is a group of panes. A **project** is a group of rows: the agents and
panes that belong to one codebase.

```
▾ agenttij              2      ▾ open, "2" = two rows inside
  ⚠ agent          2m
    nvim
▸ api-refactor    ⚠   3        ▸ folded, and something in it needs you
▸ dotfiles            1
```

Keyed by the git root, not the directory: `~/code/api` and `~/code/api/crates/x`
are one project. The hook already writes the cwd, so it should write the root
beside it (`git rev-parse --show-toplevel`, once, when the agent starts) — that
keeps the resolution out of the plugin, which cannot afford a fork per row.
Panes with no agent get theirs from the cached cwd on the slow tick.

Folding is display state, so this is core plus renderer: no host calls, no new
Zellij surface, nothing that can destroy a pane. Folded projects collapse to one
line carrying the worst status inside them, which is the point — four projects
in ten lines instead of forty.

Keys: `Tab` folds a project the way it opens a row, `[` and `]` jump project to
project, `Enter` on a folded project shows the row you last used in it.

*The other design* — a project **is** a Zellij tab, moved there with
`break_panes_to_new_tab` — buys real isolation and a tab bar that shows projects,
and costs pane moves interacting with swap layouts and suppressed panes, which is
the exact area that has broken three times already. Worth having later as an
action (`P` promotes a project to its own tab), not as the model.

### Sharing a group with another session — not as stated

There is no way to render a pane that belongs to another session, and no way to
move one. What is actually available, in order of how much it helps:

1. **Handoff** (`o`): open a row here with that project's directory. The agent
   stays where it is, you get a workspace on the same code in the session you
   are already sitting in. This is what people mean nine times in ten.
2. **Flip back across sessions**: `b` flips rows within a session; it should flip
   back to the session you jumped from too, so `Enter` into another session stops
   being a one-way trip.
3. **Watch** (`p`, already): a peek is the only way to see another session's pane,
   and it stays. Locally it can stop forking `dump-screen` and use
   `get_pane_scrollback`.
4. **Real sharing** (`share_current_session`): Zellij 0.44 can serve a session
   over its own web server with login tokens. That is genuine sharing — with a
   browser, a phone, or a colleague. A `S` toggle showing the URL is small. It
   also exposes a live terminal over HTTP, so: off by default, explicit, and
   loudly visible in the sidebar while it is on.

### SSH — yes, and it fits the existing shape

The state files are the whole protocol, so a remote host is a `cat` away:

- **Scan**: `ssh -o BatchMode=yes host 'cat /tmp/agenttij/*.state'`, folded into
  the existing script, each line tagged with the host.
- **Peek**: the same `zellij action dump-screen` we already run, over ssh.
- **Jump**: you cannot switch to a session on another machine, so open a row here
  running `ssh -t host zellij attach <session>`. Same keystroke, honest mechanics.

The catch is the tick budget: an ssh fork is 10–100× a local one. So remote hosts
scan on their own slow cadence (5s, backing off to a minute after a failure), and
the docs tell people to set `ControlMaster auto` / `ControlPersist` — with a
shared connection a remote scan is a few milliseconds, without one it is a TCP
handshake and a key exchange every time.

Hosts belong in the layout beside the other plugin configuration
(`hosts "dev1,build2"`), because a layout is where "this workspace watches these
machines" is already expressed.

## A suite, if the pieces stay honest

The state directory is already a protocol. Documented as one, it supports more
than one reader:

- **agenttij** — the sidebar. What exists.
- **agenttij-jump** — a floating fuzzy switcher over every row, project, session,
  host and dead session. Three letters and Enter, from anywhere, without looking
  at a sidebar. For someone with four projects this is probably worth more than
  everything else here.
- **agenttij-bar** — one status line: `⚠2 ◐1 ✓5 · api-refactor needs input`. For
  people who want the information without the column.
- **agenttij-notify** — headless; only turns a blocked agent into a desktop
  notification. Today that is a knob on the sidebar, which means no sidebar, no
  notifications.

They must not each pay for the poll. The rule: **whoever has the sidebar open
publishes.** The sidebar pipes its merged snapshot to the others
(`pipe_message_to_plugin`); the others poll only if nothing has arrived for a few
seconds. Each still works alone — that is the independence requirement — but
together they cost one tick, not four.

## Everything else worth building, ranked

| | what | why it earns its place | cost |
|---|---|---|---|
| 1 | `get_session_list()` instead of forking `zellij list-sessions` | a host call replaces the most expensive fork we make; also hands us dead sessions for free | small |
| 2 | blocked queue (`!`) | filters to what needs you, across sessions and hosts — the sidebar's whole reason, one key | small |
| 3 | interrupt an agent (`c`, two presses like `d`) | `send_sigint_to_pane_id`. Runaway agents are why people watch them | small |
| 4 | pane frame colour by status (`set_pane_color`) | a blocked agent's pane goes yellow — awareness with no sidebar and no keystroke | small |
| 5 | dead-session graveyard | `resurrectable_sessions` comes back in the same call as item 1: yesterday's session, one key to resurrect | small |
| 6 | project launcher (`N`) | pick a project root you have used before, start an agent there | medium |
| 7 | reply to a blocked agent | `write_chars_to_pane_id` — answering "1" without leaving the sidebar. Crosses the "never types into an agent" line, so: opt-in, needs-input rows only, never a default | medium, and a judgement call |

## Not doing

- **Moving or mirroring a pane across sessions.** No API, and faking it with a
  second process pretending to be the same agent is a lie the sidebar would then
  have to maintain.
- **A knob for anything a layout can express.** Still true.
- **Sorting by recency.** Rows must not move under the cursor.
- **Dependencies in `core`.** The reason `cargo test` runs without a WASM host.

## Order

- **First, the plumbing**: item 1, the state format gaining `root` and `host`,
  and local peeks off `get_pane_scrollback`. Small, and everything else leans on
  it.
- **Then the sidebar people actually asked for**: projects, `!`, `c`.
- **Then reach**: ssh hosts, handoff, flip-back, graveyard.
- **Then the suite**: jump first — it is the one that changes a working day.
