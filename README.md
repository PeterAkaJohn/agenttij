# agenttij

A Zellij sidebar for running coding agents: Claude Code, Codex, opencode and friends, across every session on the machine.
It shows what each agent is doing, puts whatever needs you at the top, holds the rest still, and gets you to any of them in a key or two.

```
┌ agents ────────────┐
│ ▾ agenttij      2  │  ⚠ needs input     ◐ running
│ ⚠ agenttij  3  2m  │  ● done, unread    ✓ done, read
│   ├ nvim        -  │  ○ idle            ? unknown
│   └ lazygit     -  │  · a pane, no agent
│ ● docs-fix     5m  │  ▾ ▸ a project, open or folded
│ ▸ dotfiles      4  │  ├ └ panes of the group above
│                    │  "3" = the group owns
│ j/k ↵ n a v b d    │  three panes
└────────────────────┘
```

It thinks in **projects**, made of **groups**, made of **panes**.
A group is one row of the sidebar: an agent and whatever you keep beside it, an editor, a log, a shell, with one of them on screen at a time and the rest parked, still running.
A project is every group working on one codebase.

## Contents

- [Quick start](#quick-start)
- [Reading the sidebar](#reading-the-sidebar)
- [What you can do with it](#what-you-can-do-with-it)
- [Keys](#keys)
- [Layouts](#layouts)
- [Configuration](#configuration)
- [Installing, in detail](#installing-in-detail)
- [Building and developing](#building-and-developing)
- [How it works](#how-it-works)
- [Troubleshooting](#troubleshooting)
- [Known limits](#known-limits)

## Quick start

You need:

- Zellij 0.45 or newer.
- Rust, with the `wasm32-wasip1` target.
- `python3`, which the installer's helpers run on.

And, for the parts that use them: `git` 2.31 or newer (worktrees, and counting what an agent changed), `zoxide` (ranking the directory picker), `ssh` (watching other machines), something like `notify-send` (notifications), and Claude Code or another agent.
It is developed and tested on Linux.

```sh
git clone https://github.com/PeterAkaJohn/agenttij
cd agenttij
rustup target add wasm32-wasip1
./scripts/install.sh
zellij -n agenttij-workspace
```

The installer builds the plugin and the `agenttij` command, installs the layouts, adds keybindings to your Zellij config, pre-grants the plugin's permissions, and registers the hook that lets Claude Code report its status.
Keybindings are read when a session starts, so use a new session after installing.

Then, in that session:

1. Press `n` in the sidebar (or `Alt g` from anywhere) to start a new group, and run your agent in it.
2. Press `a` (or `Alt m`) to add a pane to that group, an editor, say.
   It opens on screen, and the agent is parked behind it.
3. Press `v` (or `Alt v`) to cycle between them, and `Enter` on any row to put that group on screen.
4. Press `?` (or `Alt ?`) for every key.

If you use Claude Code, also install its plugin, which carries the status hooks, the tools that let agents open panes, and two skills:

```
/plugin install agenttij --marketplace PeterAkaJohn/agenttij
```

Then run `./scripts/install.sh` once more, so it takes its own copy of the hooks out of `~/.claude/settings.json`.

## Reading the sidebar

Each row is a group, or a project header above its groups.
From left to right: a status glyph, the name, how many panes the group owns (when more than one), and how long ago its agent last reported (`now`, `12s`, `5m`, `2h`, `3d`).

| Glyph | Means |
|---|---|
| `⚠` | needs input: the agent is waiting on you |
| `◐` | running |
| `●` | done, and you have not looked yet |
| `✓` | done, and read |
| `○` | idle |
| `?` | unknown |
| `·` | a pane with no agent in it |
| `▾` `▸` | a project, open or folded; folded, it shows the worst status inside it |
| `├` `└` | the panes of a group you opened with `Tab` |
| `⇢` | a group in another session on this machine; `Enter` switches session |
| `⇥` | a group on another machine |

In the first column, `›` is the cursor and `▪` marks the group on screen: the same thing, until you move.

Whatever needs you sorts first, then the rest in a fixed order, so rows never move under the cursor because something else got busy.

A finished agent stays `●` until its group has been on screen with someone looking at it, or you peeked at it; then it turns `✓`.
One that finishes while you watch it is read as it finishes.

A finished group also shows what its agent left uncommitted, `+120−30`, new files included: on the row when there is room next to the name, and on the pane's frame.
It is counted once, when the agent finishes.

## What you can do with it

### Run several agents in one workspace

The `agenttij-workspace` layout gives the sidebar a column and the rest of the tab a single slot holding one pane.
Picking a group puts that group's current pane in the slot and **parks** whatever was there: Zellij calls it suppressed, and it keeps running.
The sidebar never moves and never reloads; `Enter` is a pane swap, not a detach.

```
┌ agents ────┬──────────────────────────┐
│ ⚠ bravo    │                          │
│ ◐ delta  2 │  delta's editor          │ ← the group on screen is showing
│    nvim    │                          │   its second pane; Tab opened
│ ✓ alpha    │                          │   the group to list it
│ · scratch  │  (every other pane is    │
│            │   parked, still running) │
└────────────┴──────────────────────────┘
```

- `n` (`Alt g`) starts a new group; `a` (`Alt m`) adds a pane to the group you are on.
  Both park what was on screen instead of splitting it.
- `v` and `V` (`Alt v`, `Alt V`) cycle through a group's panes, `1`-`9` (`Alt 1`-`Alt 9`) go straight to one by the number its frame shows (`· api 3/5`), and `'` (`Alt '`) flips between the two you were alternating.
- `b` (`Alt b`) flips back to the previous group.
- `Tab` lists a group's panes underneath it, named after what runs in them, so you can pick one.
- `d` `d` closes a group and every pane parked behind it, or just the pane under the cursor in an opened group, and puts the next group on screen.
  It asks first.
- `m` moves the pane or group under the cursor into the group on screen, for a pane that ended up on its own.

Each group remembers which pane you were last on.
A pane the sidebar does not recognise, one you opened with Zellij's own keys, say, becomes a group of its own, so nothing is ever out of reach.

### Start every group the same way

If every group you build comes out the same, say so once in the layout:

```kdl
plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
    scope "session"
    solo "true"
    group "claude; nvim .; lazygit"
}
```

Now `n` and `Alt g` open all three at once: `claude` on screen, the editor and `lazygit` parked behind it.
`;` separates panes, the first is the one you see, and an empty entry is a plain shell: `group "; nvim ."` is a shell with an editor behind it.
The group the session starts with is filled out the same way.
`a` stays a single plain pane on purpose: it means "one more", not "one more group".

`agenttij-template` is that layout, ready to run.

Words are split on whitespace, so an argument with a space in it belongs in a script whose path goes in the template.
The template is part of the sidebar's configuration, and Zellij identifies a plugin by its configuration, so `Alt g` in your keybindings has to carry the same one.
The installer reads it from the layouts and writes it into the bindings: edit the layout, re-run `./scripts/install.sh`, and start a new session.
That makes the template machine-wide; `AGENTTIJ_GROUP="..."` overrides the layouts, and `AGENTTIJ_GROUP=""` installs none.

### Keep work organised by project

Groups gather into projects: everything under one git root, so an agent started in `crates/core` sits with one started at the top of the repository, and worktrees of one repository are one project.

- `r` names a project.
  Two projects given the same name are one: a front end and a back end in separate repositories, say.
- A `.agenttij` file above a directory does the same from the filesystem: its first line is the project's name (empty means the directory's own), and it travels with the checkout.
- `Tab` on a project header folds it to one line that still shows the worst status inside it.
- `[` and `]` jump between projects, and `J` and `K` move a project, or a group within its project.

The order and what you folded are remembered in `~/.cache/agenttij/order`, so a reload or a new day finds them where you left them.
Headers only appear once there is more than one project to tell apart.

### Check on an agent without going there

- `p` peeks: a floating, live view of an agent's pane wherever it is, in another session, a background tab, or a session nobody is attached to.
  Any key closes it.
- `/` in the sidebar, or `Alt t` from anywhere, opens the jump palette: every agent, pane, session and resurrectable session on the machine.
  Three letters and `Enter` take you there.
- `!` shows only what needs you.
- `B` goes back to the session you came from, so jumping to another session is a round trip.
- `o` opens a new group here on the selected group's project, when you want to work on the same code without moving the agent.

Jumping to another session is a detach and reattach, because that is what switching sessions is in Zellij.
Peeking exists so you rarely need it.

### Answer or stop an agent from the sidebar

- `R` answers an agent that is waiting on you: type the answer in the sidebar and `Enter` sends it, after checking the agent is still waiting.
  It is off unless the layout sets `reply "true"`.
- `c` `c` interrupts an agent without going to it, by sending it a Ctrl-C.
  It asks first.

Apart from those two, the sidebar never types into an agent.

### Run agents side by side on one repository

`w` on a group, or `Alt w` from anywhere for the group on screen, makes a git worktree beside that group's repository and starts a new group in it, template and all: `api.worktrees/api-2` on a branch `api-2`, from the commit the group is on.
Two agents on one repository then never write over each other's files.
Closing the group removes the worktree with `git worktree remove`, which refuses one holding uncommitted work: that one stays, and the sidebar says so.

### Start work in another directory

`n` starts a group where the group on screen already is.
`G` (`Alt G`) opens a directory picker first, ranked by `zoxide` when it is installed, or takes a path you type.
Which project the new group files under is still decided by `.agenttij` or the git root.

### Get told when an agent needs you

```kdl
notify "notify-send -u critical"
```

`notify` runs a command when an agent becomes blocked on you, and when one finishes a turn nobody was watching.
It is given `agenttij` and a message such as `api needs input`.

The same plugin also comes as a one-line bar, counts first, then whoever most wants you:

```kdl
pane size=1 borderless=true {
    plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
        bar "true"
        notify "notify-send -u critical"
    }
}
```

```
⚠2 ◐1 ✓5 · api-refactor 2m
```

A bar counts every agent on the machine unless you add `scope "session"`, never takes focus, and works with or without a sidebar, so it is also how a session with no sidebar gets notifications.

### Name sessions after their folder

Zellij names a new session `quadratic-donkey`, which is fine until four of them are `EXITED` and you have to guess which was which.
`session_name "folder"` in the sidebar's configuration renames a session after the directory it was started in, once, and only when Zellij made the name up; a folder whose name is taken gets `-2`.
`S` in the sidebar renames it by hand.

Renaming is safe for agents already running: they keep reporting under the old name, which the sidebar keeps as an alias.

Or name it when you create it, so attaching finds it again and resurrects it if it died:

```sh
zj() {
    name=${PWD##*/}
    zellij attach "$name" 2>/dev/null || zellij -s "$name" -n agenttij-template
}
```

### Come back after a restart

With `session_serialization true` in your Zellij config, a reboot leaves your sessions listed as `EXITED - attach to resurrect`.
`zellij attach <name>` brings back every pane that was on screen, in its directory, with its command, waiting for `Enter`.

What Zellij cannot bring back is what was parked: it does not save suppressed panes.
So agenttij keeps its own record of each session's groups, the directory each worked in and the programs its panes ran, in `~/.cache/agenttij/sessions/<name>`.
In the jump palette, type `restore`:

```
⊞ contentchef      4 groups · 2h
⊞ agenttij         2 groups
```

Picking one rebuilds those groups, one a second, each in its own directory with its own panes.
Groups already open are skipped, counted rather than merely matched, so three groups on one project restore two when one is already there.
Program arguments are not kept, because a pane only reports the program it runs: `nvim +42 file` comes back as `nvim`.

The palette also says what each old session was working on, so you are not guessing from animal names:

```
⊙ contentchef-serverless   contentchef · session
⊘ adept-petunia            agenttij · 21d
⊗ someone-elses            resurrect · 4d
```

`⊙` is a live session, `⊘` a dead one that had a sidebar in it (named after its projects, and sorted first), and `⊗` any other dead one.
The names come from the pane titles in Zellij's own saved layouts, so this works even for sessions older than the snapshots.

### Watch agents on other machines

Agents on a dev box can show up next to the local ones, marked `⇥` with the host they are on:

```kdl
plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
    hosts "dev1,build2"
}
```

Or press `h` in the sidebar and type them; that list is remembered for the session you typed it in.

Install agenttij on the watched machine too, since its hook is what writes the state, and give each host key-based ssh login.
Starting the session there with `zellij -n agenttij-remote` adds a *controller*: the same plugin, invisible, which keeps that session to one pane at a time and does what your sidebar asks.
Its groups then appear here as groups, `Tab` lists their panes, and `v`, `a`, `d` and `Enter` on them act over there.

`Enter` on a remote group opens a pane here attached to that session, because Zellij cannot show a pane it does not own; `p` peeks without attaching.
Since Zellij 0.45 that pane is a nested session: while it has focus your keys go to the machine over there, and `Ctrl o` then `]` brings them back.

Hosts are asked every five seconds, and one that fails is left alone for a minute.
Set up connection sharing for them, or every one of those asks pays for an ssh handshake:

```
Host dev1 build2
    ControlMaster auto
    ControlPath ~/.ssh/control-%r@%h:%p
    ControlPersist 10m
```

No second machine handy?
`scripts/testbox.sh up` starts a Docker container running sshd and a Zellij session with two panes, and prints the host string to give `h`.

### Follow along from a phone

`s` opens Zellij's own web-sharing screen, where the session is shared and Zellij's web server started.
While it is shared, the sidebar's first line says so, with how many are watching.
For a phone, the server has to listen on an address the phone can reach, with a certificate: `web_server_ip`, `web_server_cert` and `web_server_key` in your Zellij config.
It serves a live terminal, so mind who can reach it.

### Let agents use the sidebar

An agent can ask the sidebar for panes the way you do with `a` and `n`: a dev server beside it, a log to tail, a second agent in a fresh worktree.

| Command | MCP tool | Does |
|---|---|---|
| `agenttij list` | `list_groups` | projects, their groups and panes, what is on screen, each group's status |
| `agenttij pane add [--run CMD] [--cwd DIR] [--group N] [--show]` | `add_pane` | a pane in the caller's group, or group `N` |
| `agenttij group new [--run CMD] [--cwd DIR] [--worktree] [--show]` | `new_group` | a group from the template, its first pane running `CMD` |
| `agenttij show PANE` | `show_pane` | puts a pane on screen, as `Enter` would |
| `agenttij read PANE [--lines N]` | `read_pane` | what a pane shows, or the last `N` lines of its scrollback |
| `agenttij pane close PANE` | `close_pane` | closes a pane an agent opened, never one of yours |
| `agenttij wait GROUP [--timeout S]` | `wait_group` | waits until the group's agent is done or waiting on you (300 seconds by default) |

What an agent opens is **parked**: it runs and joins the group, and your screen and focus stay where they were.
`--show` puts it on screen, for when you asked to look at it.
"The caller's group" is the one holding the pane the command runs in (`$ZELLIJ_PANE_ID`), so an agent's dev server lands next to that agent.
Answers are one line of JSON, `{"ok":false,"error":"..."}` with exit code 1 when the sidebar says no; `read` prints the pane's text.
The sidebar remembers which panes agents opened and refuses to close any other; a plugin reload forgets, and they become yours.

Handing a task to a second agent is the same pieces: a group in a fresh worktree running `claude '<task>'`, `wait` until it is done, `read` what it said.
The Claude Code plugin's `delegate` skill does exactly that, `/agenttij:delegate <task>`, and leaves the worktree for you to review.

**Claude Code**: the plugin adds the MCP tools, a skill saying when to use them, the `delegate` skill and the status hooks.

```
/plugin install agenttij --marketplace PeterAkaJohn/agenttij
```

From a shell: `claude plugin marketplace add PeterAkaJohn/agenttij`, then `claude plugin install agenttij@agenttij`.

**Other agents that speak MCP** run the same server, `agenttij mcp`; in Codex's `~/.codex/config.toml`:

```toml
[mcp_servers.agenttij]
command = "agenttij"
args = ["mcp"]
```

**Any other agent** can run the command: a line in its `AGENTS.md` saying "start long-running processes with `agenttij pane add --run '<command>'`" is enough.

Outside Zellij the MCP server lists no tools, so it can be configured everywhere.
It needs a sidebar with `solo "true"` in the session, which every layout here has except `agenttij-left` and `agenttij-right`; `list` works without one.

### Use agents other than Claude Code

The hook is not tied to Claude Code.
It takes a status word as its argument and reads `$ZELLIJ_SESSION_NAME` and `$ZELLIJ_PANE_ID` from the environment, so anything that can run a command on an event can report:

```sh
~/.claude/hooks/agenttij-state.sh running
```

The statuses are `idle`, `running`, `needs-input`, `done` and `gone`.

**opencode** has a plugin for it: copy `integrations/opencode/agenttij.js` to `~/.config/opencode/plugins/`.

| opencode event | Status |
|---|---|
| `session.created` | idle |
| `tool.execute.before` | running |
| `permission.asked` | needs input |
| `session.idle` | done |
| `session.deleted` | removed |

An agent with no hook at all is still found, by its pane title or by the program the pane runs (any name in `agents`), and its status is read off its screen, one agent every other second.
A screen that keeps changing is working (`◐`), one that went still after working has finished (`✓`), and a question at the bottom, `(y/n)` or "Do you want...", is waiting on you (`⚠`).
That is a reading, not a report, so a hook stays the better way for any tool that has one.

## Keys

In the sidebar:

| Key | Does |
|---|---|
| `j` `k`, `↓` `↑` | move |
| `Enter` | show that group; switches session if it is elsewhere |
| `Tab` | open a group to list its panes, or fold a project |
| `[` `]` | previous / next project |
| `J` `K` | move a project, or a group within its project |
| `r` | name a project; the same name joins two |
| `S` | rename this session |
| `h` | machines to watch, over ssh |
| `b` | flip back to the previous group |
| `B` | back to the session you came from |
| `o` | a new group here, on that group's project |
| `v` `V` | cycle the panes of this group, either way |
| `1`-`9` | straight to that pane of the group |
| `'` | flip to the pane before this one |
| `a` | add a pane to this group |
| `m` | move what the cursor is on into the group on screen |
| `n` | new group |
| `G` | new group in a directory you pick |
| `w` | new group in a git worktree of that group's repository |
| `s` | share this session on the web (Zellij's own screen) |
| `R` | answer a waiting agent (with `reply "true"`) |
| `/` | jump: everywhere you could go |
| `p` | peek without leaving |
| `d` `d` | close this group, this pane, or a project; asks first |
| `c` `c` | interrupt the agent; asks first |
| `!` | only what needs you |
| `q`, `Esc` | dismiss a peek |
| `?` | every key |

From any pane, installed into your Zellij config:

| Key | Does |
|---|---|
| `Alt ?` | every key |
| `Alt s` | focus the sidebar |
| `Alt a` | summon a sidebar in any session, floating |
| `Alt v` `Alt V` | cycle the panes of the group on screen |
| `Alt 1`-`Alt 9` | straight to that pane of the group |
| `Alt '` | flip to the pane before this one |
| `Alt b` | flip back to the previous group |
| `Alt g` | new group |
| `Alt G` | new group in a directory you pick |
| `Alt w` | new group in a git worktree |
| `Alt m` | add a pane to the group on screen |
| `Alt t` | jump; type `restore` for a workspace |
| `Alt ]` `Alt [` | fold the sidebar to a one-column status rail, and back |

[docs/KEYBINDS.md](docs/KEYBINDS.md) has every key in full, how to change them, and how to do the same without keybindings.

## Layouts

The installer copies these to `~/.config/zellij/layouts/`; start one with `zellij -n <name>`.

| Layout | What it is |
|---|---|
| `agenttij-workspace` | the sidebar and one slot: groups of panes, one on screen at a time, this session's agents only. Start here. |
| `agenttij-template` | the workspace, with every new group built from `group "claude; nvim .; lazygit"` |
| `agenttij-machine` | the template, listing the agents of every session on the machine |
| `agenttij-everything` | the workspace sidebar with a bar under it, to see it all at once |
| `agenttij-left`, `agenttij-right` | a plain sidebar beside Zellij's usual panes, listing every agent on the machine |
| `agenttij-remote` | for a machine you watch from elsewhere: your panes and an invisible controller |

They are examples: which side the sidebar sits on, how wide it is and what else is in the tab are yours to change.
Copy one, or lift its `plugin` block into a layout you already have.
Two things to keep when you do:

- **A percentage width.**
  A pane declared with a fixed `size=26` cannot be resized, by a plugin or by Zellij itself, and folding to the rail needs to.
- **The `sidebar` and `rail` swap layouts, with the same configuration.**
  `Alt ]` folds by switching swap layout, and Zellij identifies a plugin by its url *and* its configuration: a swap layout that says anything different starts a second sidebar instead of folding the one you have.
  The `sidebar` swap layout, listed before the rail, keeps Zellij from folding the sidebar by itself when a pane is added.

To give every new session a sidebar, set `default_layout "agenttij-workspace"` in your Zellij config.
For a session started without one, `Alt a` summons a floating sidebar, or by hand (the `--` is required):

```sh
zellij plugin --floating -- "file:$HOME/.config/zellij/plugins/agenttij.wasm"
```

## Configuration

Everything is set in the layout, inside the plugin block:

```kdl
pane size="20%" {
    plugin location="file:~/.config/zellij/plugins/agenttij.wasm" {
        scope "session"
        solo "true"
        group "claude; nvim .; lazygit"
        notify "notify-send -u critical"
    }
}
```

| Key | Default | Does |
|---|---|---|
| `scope` | `all` | `session` lists only this session's agents, plus any machines you watch |
| `solo` | `false` | `true` makes a group several panes with one on screen, the others parked |
| `group` | one plain shell | the panes every new group starts with, `;` between them |
| `agents` | `claude,codex,opencode,aider,gemini` | names that mark a pane as an agent when it has no hook |
| `notify` | nothing | a command to run when an agent needs you or finishes unwatched |
| `reply` | `false` | `true` lets `R` type an answer into a waiting agent |
| `position` | `true` | with `solo`, names the pane on screen after its group's status, name and place, `◐ api 2/3`; `false` stops it |
| `session_name` | nothing | `folder` names a session after its directory, once, if Zellij made the name up |
| `hosts` | nothing | ssh hosts to watch, comma-separated |
| `pane_title` | `agents` | what the sidebar's own pane frame says |
| `colors` | built in | per-status colours: `needs-input=yellow,running=blue,done=green`, by name, 256-colour index or `#rrggbb`; the statuses are `needs-input`, `unread`, `running`, `done`, `idle`, `unknown` and `pane` |
| `bar` | `false` | `true` draws one line of counts instead of a column |
| `remote` | `false` | `true` is a controller: no column, no keys, for `agenttij-remote` |

A setting is part of the plugin's identity, so a layout's `sidebar` and `rail` swap layouts must repeat it, and a `group` template must match your keybindings (the installer takes care of that).

## Installing, in detail

```sh
./scripts/install.sh               # install, or update: re-running replaces rather than duplicates
./scripts/install.sh --uninstall   # remove everything it installed
```

What it does:

- Builds the plugin into `~/.config/zellij/plugins/agenttij.wasm` and the `agenttij` command into `~/.local/bin`.
- Copies the layouts to `~/.config/zellij/layouts/`.
- Copies the hook to `~/.claude/hooks/agenttij-state.sh` and registers it in `~/.claude/settings.json`, unless Claude Code's agenttij plugin is installed, which carries the hooks itself.
- Adds the keybindings inside your Zellij config's existing `keybinds` block (Zellij only reads the first one), checks the result with `zellij setup --check`, and rolls back if Zellij rejects it.
- Pre-grants the plugin's permissions in `~/.cache/zellij/permissions.kdl`.
  Zellij asks by drawing a prompt over the plugin's pane, and that prompt does not fit in a sidebar: the first launch would otherwise be a blank pane waiting on a key you cannot see.

Everything is backed up first, and other tools' entries are left alone.
Re-run it after updating: a version that asks for a new permission would otherwise wait on that invisible prompt.

| Option | |
|---|---|
| `--no-keybind` | skip the keybindings |
| `--no-grant` | skip pre-granting permissions |
| `--uninstall` | reverse all of it |
| `AGENTTIJ_BIN_DIR` | where the `agenttij` command goes, `~/.local/bin` by default |
| `AGENTTIJ_GROUP` | the group template for every layout and binding; empty for none |
| `AGENTTIJ_KEYBIND`, `AGENTTIJ_FOCUS_KEYBIND`, `AGENTTIJ_CYCLE_KEYBIND`, `AGENTTIJ_BACK_KEYBIND`, `AGENTTIJ_NEW_KEYBIND`, `AGENTTIJ_ADD_KEYBIND`, `AGENTTIJ_JUMP_KEYBIND` | other keys for `Alt a`, `Alt s`, `Alt v`, `Alt b`, `Alt g`, `Alt m` and `Alt t` |

## Building and developing

```sh
rustup target add wasm32-wasip1

cargo build -p agenttij --target wasm32-wasip1 --release   # the plugin
cargo build -p agenttij-cli --release                      # the agenttij command

cargo test -p agenttij-core                                # the logic, on the host
cargo test -p agenttij-cli
cargo clippy -p agenttij-core --all-targets -- -D warnings
cargo clippy -p agenttij-cli --all-targets -- -D warnings
cargo clippy -p agenttij --target wasm32-wasip1 -- -D warnings
cargo fmt --all
```

The plugin is tested in real Zellij sessions by `scripts/harness.py`, which types real keystrokes into a real client and reads back what the sidebar drew:

```sh
python3 scripts/harness.py scenario scripts/e2e/*.txt     # every scenario
python3 scripts/harness.py --help                         # driving a session step by step
sh scripts/check-install.sh                               # the installer, in a throwaway home
scripts/testbox.sh up                                     # a second machine, in Docker
```

Each harness session is sealed off, with its own config, cache and sockets, and runs the plugin you built rather than the one installed, so it never touches your own sessions.
CI runs all of the above, plus the remote scenarios against the testbox.

```
crates/core    agenttij-core: zero dependencies, all the decisions, unit-tested
crates/plugin  agenttij: the wasm plugin, lifecycle, rendering, navigation
crates/cli     agenttij-cli: the agenttij command and its MCP server
plugins/       the Claude Code plugin: MCP tools, skills, and the shell hook
integrations/  per-tool glue that calls the hook (an opencode plugin)
layouts/       the layouts above, each with sidebar and rail swap layouts
scripts/       installer and its helpers, the harness and its scenarios, testbox
docs/          KEYBINDS.md (every key), PLAN.md (design), ROADMAP.md (what next)
```

`core` deliberately does not depend on `zellij-tile`: the plugin crate adapts Zellij's types into `core`'s own at the boundary, which is what lets `cargo test` run without a WASM host.
[AGENTS.md](AGENTS.md) holds the conventions and the Zellij traps behind most of the odd-looking code; read it before changing behaviour.

## How it works

State flows one way: the agent reports, a file records it, the sidebar reads it.

```
agent hook ──> /tmp/agenttij/<session>.<pane>.state
                         │
                    one cat a second
                         v
  SessionUpdate ──> [ sidebar plugin ] ──> Enter: switch session, show pane
  (panes, clients)       ^            └──> p:     dump-screen, once a second
                         │
             get_session_list(): which sessions are alive
```

| Claude Code event | Status |
|---|---|
| `SessionStart` | idle |
| `UserPromptSubmit` | running |
| `Notification` | needs input |
| `Stop` | done, with what it left uncommitted |
| `SessionEnd` | removed |

State lives in files rather than in the plugin, so a sidebar in a session started later still sees every running agent, and a reload loses nothing.
The hook also works out the project (`.agenttij`, then the git root), once per report, because the sidebar cannot afford a `git` call per group per second.
Nothing stays listed without something alive behind it: every update reconciles against the live sessions and their panes, which covers `kill -9`, closed panes and dead sessions, where no hook ever fires.

| File | Holds |
|---|---|
| `/tmp/agenttij/<session>.<pane>.state` | an agent's status, written by its hook |
| `/tmp/agenttij/<session>.<pane>.seen` | when a group was last looked at, for read and unread |
| `/tmp/agenttij/api/` | answers to agents' requests |
| `~/.cache/agenttij/order` | project order, names and folds |
| `~/.cache/agenttij/sessions/<name>` | a session's groups, hosts and snapshots for restoring |

An agent's request is a `zellij pipe` that only the sidebar holding the groups acts on, and the answer comes back in a file under `/tmp/agenttij/api/`; [docs/PLAN.md](docs/PLAN.md) explains why not over the pipe.

Zellij plugins are single-threaded WASI modules run by an interpreter, so there is no async runtime: anything slow is handed to the host and comes back as an event.
The sidebar polls once a second; idle beside five groups of three, it costs about 8ms of CPU a second.

## Troubleshooting

- **The sidebar draws but does nothing**: no new panes, no statuses.
  A denied permission is remembered as an empty grant in `~/.cache/zellij/permissions.kdl`.
  Re-run `./scripts/install.sh`, which grants them again, and start a new session.
- **A blank pane where the sidebar should be.**
  It is waiting on a permission prompt too wide to show, usually after installing with `--no-grant` or building the plugin somewhere else.
  Re-run the installer.
- **The `Alt` keys do nothing.**
  Keybindings are read when a session starts: start a new one.
- **`Alt g` opened a second sidebar, or every pane became its own group.**
  The binding's configuration does not match the layout's, usually a `group` template edited in one place only.
  Re-run the installer and start a new session.
- **Every Claude Code event reports twice.**
  The Claude Code plugin was installed after the installer ran, so both carry the hooks.
  Re-run the installer.
- **Keys go to another machine.**
  A pane attached to a remote session is a nested session; `Ctrl o` then `]` brings the keys back.
- **The client is in Scroll mode after showing a pane.**
  Zellij 0.45 enters Scroll mode when focusing a pane that was scrolled up; `scroll_mode_sync false` in your Zellij config turns that off.

## Known limits

- **Switching sessions detaches and reattaches.**
  That is what switching a session is in Zellij; `p` exists so you rarely need to, and `Alt a` gets you a sidebar wherever you end up.
- **Panes belong to the session that owns them.**
  Zellij cannot show another session's pane inside yours, so the one-slot workspace only works for agents running in it; keep the agents you want side by side in one session.
- **Peeking polls once a second** rather than streaming: `zellij subscribe` goes silent for exactly the background panes you want to check on, while `dump-screen` always answers.
- **Pane detail in other sessions depends on `session_serialization`.**
  With it off, Zellij publishes no other session's panes, so agents there are tracked by session, and agents with no hook there are not found.
- **Parked panes do not survive a restart on their own**, since Zellij does not save them; restoring from the palette rebuilds them, without program arguments.
- **One group template per machine**, because the keybindings that carry it are machine-wide.

## License

[MIT](LICENSE)
