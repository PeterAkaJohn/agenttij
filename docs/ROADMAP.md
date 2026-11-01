# Where this could go

A plan, not a promise.
Everything here was checked against Zellij 0.45.1's plugin API before being written down; where something is impossible it says so rather than proposing a shape for it.

The reader to keep in mind: someone with four projects open, two agents working, one of them stuck on a question, and a remote box with two more.

## What the walls are

Measured, not assumed, and every idea below is bent around them.

- **Panes belong to the session that owns them.**
  Nothing in the plugin API moves or renders a pane across sessions, so any feature phrased as "share a pane with another session" has to become something else.
- **The tick is the whole cost.**
  Idling beside a row of eight the sidebar burns about 24ms of CPU a second; with the timer off, 1ms.
  Anything that adds a poll adds the same again, which is why what the hook can count once - a project, what an agent changed - is counted there.
- **Filesystem events cannot help.**
  `watch_filesystem` starts one watcher per session rooted at the session's cwd, so it can never watch `/tmp/agenttij`.
  The poll stays; it can only get cheaper.
- **A plugin's identity is its url plus its configuration.**
  Two pieces that want to talk must agree on both, exactly.
- **Suppressed panes forget their geometry**, and closing one un-hides it.
  Every layout idea that "just moves panes around" pays for this.
- **Zellij keeps terminal notifications to itself.**
  0.45 parses OSC 9 and 777, then hands them only to the clients' terminals; no plugin event carries them.
  An agent without a hook stays invisible unless its pane is read.

## Built

- **Projects**: rows grouped under the repository they work in, folded to one line carrying the worst status inside; worktrees of one repository are one project.
- **The blocked queue** (`!`) and **interrupting an agent** (`c` `c`).
- **Status in the pane frame**, as a glyph in the title: `set_pane_color` would repaint the agent's own output.
- **Reach**: ssh hosts, a controller on the machines you watch, handoff (`o`), flipping back across sessions (`B`).
- **The palette** (`/`, `Alt t`): every row, project, session, host and resurrectable session; and the directory picker (`G`), which is the project launcher.
- **The bar** and **notify**: a bar with `notify` set is the notifier, so there is no separate plugin for it.
- **Workspaces that outlive the machine**: a snapshot per run, restored from the palette.
- **Unread finished work**: `●` until the row has been on screen with someone looking, or peeked at.
- **What an agent left uncommitted**, `+120−30`, counted by the hook when it finishes.
- **Worktree rows** (`w`, `Alt w`): a new row in a fresh git worktree, let go of when the row closes unless it holds work.
- **Status for agents without a hook**, read off their screens with `dump-screen` - no new permission - and found by what they run as well as by title.
- **A harness and e2e scenarios**: sealed Zellij sessions driven by real keystrokes, reading what the sidebar draws, run in CI against the Zellij the plugin is built for.

## Open, ranked

| | what | why it earns its place | cost |
|---|---|---|---|
| 1 | check in from a phone | 0.45 serves a session over its web server with a mobile UI; a toggle that shares this session and shows the URL, loudly visible while on | medium |
| 2 | reply to a blocked agent | answering "1" without going there, `write_chars_to_pane_id`; crosses "never types into an agent", so opt-in and needs-input rows only | small, and a judgement call |
| 3 | promote a project to its own tab (`P`) | real isolation and a tab bar that shows projects; `break_panes_to_new_tab` meets swap layouts and suppressed panes, where things have broken before | medium to large |

Replying needs `WriteToStdin`, which the plugin already asks for, for `c`.
Reading screens went through `dump-screen` rather than `ReadPaneContents` for the reason a new permission is costly: a session still running an older build rewrites the grant without it, and the next sidebar sits on an invisible prompt.

## A suite, if the pieces stay honest

The state directory is already a protocol, and more than one reader uses it: the sidebar, the controller, the palette, the bar.
They must not each pay for the poll.
A sidebar piping its snapshot to the others was written and taken out, on purpose: `pipe_message_to_plugin` needs `MessageAndLaunchOtherPlugins`, Zellij rewrote `permissions.kdl` without it on every run, and the send stayed denied - the same silent failure a user who never sees the prompt would get.
Against that, the feed saves one `cat` per second in the bar: about 4ms, measured.
Each piece reads the state files itself until a third and fourth reader appear, and then the permission is granted before any session starts.

## Not doing

- **Moving or mirroring a pane across sessions.**
  No API, and faking it with a second process pretending to be the same agent is a lie the sidebar would then have to maintain.
- **A knob for anything a layout can express.**
- **Sorting by recency.**
  Rows must not move under the cursor.
- **Dependencies in `core`.**
  They are the reason `cargo test` runs without a WASM host.
