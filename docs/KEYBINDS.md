# Keybinds

Two layers, handled by two different things: keys the sidebar reads while it is
focused, and Zellij keybinds that work from anywhere.

## In the sidebar

Reach it with your usual pane navigation, or `Alt s`. No modifiers: any key held
with Ctrl/Alt/Shift passes straight through to Zellij, so your own bindings keep
working while the sidebar has focus.

| Key | Does |
|---|---|
| `j`, `↓` | next row |
| `k`, `↑` | previous row |
| `Enter` | show the selected row, or the pane under it |
| `Tab` | open a row: its panes underneath it — or fold a project |
| `[`, `]` | previous / next project |
| `J`, `K` | move this project, or this row inside its project |
| `r` | name a project — two with the same name are one |
| `h` | the machines to watch, over ssh |
| `b` | flip back to the row you were on before |
| `B` | back to the session you came from |
| `o` | open a workspace here on that row's project |
| `v` | cycle to the next pane *within* the row on screen |
| `a` | add a pane to the row on screen |
| `n` | new agent pane — a new row |
| `d` `d` | close what the cursor is on — twice, it cannot be undone |
| `c` `c` | interrupt what it is running — twice, same reason |
| `!` | only what needs you |
| `/` | jump: everywhere you could go, filtered by typing |
| `p` | peek at a row's agent without leaving this session |
| `q`, `Esc` | dismiss a peek |
| `?` | this list, in a floating pane |

### Projects

A row is a group of panes; a project is a group of rows — everything working on
one codebase. Rows gather under the git root the hook records, so an agent
started in `crates/core` sits under the same project as one started at the top of
the repository.

Headers only appear once there is more than one project to tell apart: with one,
the header is a line taken from a list twenty columns wide. A header carries the
worst status inside it, so folding a project away never hides an agent that is
waiting for you. `Tab` folds and unfolds, `[` and `]` step between projects, and
`Enter` opens a folded one.

### Following work without moving it

`o` opens a row here on the selected row's project — the same code, in the
session you are already in, leaving the agent where it is. That is what people
usually mean by wanting a group in two places, and the only version of it that is
not a lie: a pane belongs to the session that owns it. On a project header it
uses the project's own directory, and on a project you have *named* it uses one
of its roots, since a name is not a place.

`B` goes back to the session you came from, which makes `Enter` into another
session a round trip rather than a one-way door. The two sidebars are different
plugin instances in different processes, so the one you leave writes the name
down and the one you arrive in reads it off the scan that was already running.

### The bar

A plugin pane with `bar "true"` is one line of counts rather than a column of
rows (see the README). It has no keys and never takes focus — a status bar you
have to dismiss is not a status bar — and it reads the same state files the
sidebar does, so it works on its own.

It takes `scope` like everything else, and the default is every agent on the
machine. A bar next to a `scope "session"` sidebar will therefore say more than
the sidebar does, which is usually the point; add `scope "session"` to it if you
would rather they agreed.

### Adding a pane where the work is

`a` and `Alt m` add a pane to the row on screen. When that row is a session on
another machine — a pane you opened with `Enter` on a `⇥` row — the new pane is
created *there*, inside that session, by Zellij's own CLI over ssh. It appears in
the view you are already looking at, and to anyone else attached to that session,
which a second ssh connection of our own would not.

It starts in the directory the agent over there is working in. That is done with
a `cd` rather than `new-pane --cwd`, which was measured being ignored for a
session started detached — the pane came up in `/` whatever was asked for.

And that session shows one pane at a time, like solo mode does here: arriving in
it and adding to it both gather its panes into a Zellij *stack*, so one is
expanded and the rest are title lines. There is no suppressing a pane through the
CLI, and no need — a stack is the same promise, kept by Zellij rather than by us,
and a new pane joins it on its own.

The one gap: only attachments made with `Enter` from the sidebar are remembered
this way. Jumping to a remote session from the palette (`Alt t`) opens the same
pane, but the sidebar did not open it and does not know what it is, so `a` there
adds a local pane.

### Machines

`h` opens the list of machines to watch, prefilled with the ones already being
watched, so adding and removing one are the same edit: type a comma-separated
list of ssh hosts and press `Enter`. It is remembered for *this session* — which boxes you care about is part of what
you are working on, so a session you open tomorrow for something else starts with
none. A machine you drop takes its rows with it immediately: they were only ever
its answer to a question that has stopped being asked.

A layout can name hosts too, and those are watched in every session that uses it
— that is the difference between the two ways of saying it. Both are watched; `scope "session"` does not
hide them, because how far to look on *this* machine is a different question from
which other machines you asked about.

There is a container for trying this without a second machine:
`scripts/testbox.sh up` prints a host string to paste into `h`, and
`scripts/testbox.sh down` removes it. It runs a real zellij session with two
panes and a state file each, so its rows peek at actual pane contents and `Enter`
attaches to something.

### Rows somewhere else

`⇢` marks a row in another session on this machine: `Enter` there detaches and
reattaches, which is what going there costs. `⇥` marks one on another machine
(see `hosts` in the README): `Enter` opens a pane here attached to that session,
since Zellij can only show panes it owns. That pane takes the slot and joins the
row you were on, the way `a` does — so `v` gets you back to what you were doing,
and it inherits that row's directory rather than arriving in no project at all. `p` peeks at either without moving.

### What the first column means

`›` is the cursor — where the next key lands. `▪` is the row that is actually on
screen. They are the same thing until you open a pane somewhere else, and then
they are not: `Alt g` puts a new row in front of you, and a sidebar that marked
only one of the two would be telling you the wrong one. The cursor now follows a
row you create, so the two agree again straight afterwards.

### Rows, not panes

In solo mode (`solo "true"`, which the workspace layout ships) a **row is a group
of panes**: an agent plus whatever you put beside it with `a` — an editor, a log —
of which exactly one is on screen. Companions get no row of their own; the row
*is* the agent session. Each row remembers which member you were last on, so
coming back to it puts you where you left off.

`Tab` opens a row up: its panes are listed underneath, indented and named after
whatever is running in them — `nvim`, `lazygit` — or after the row itself when
that is just a shell. `Enter` on one goes straight to that pane rather than to
whichever the row was last on. Rows owning a single pane have nothing to open, so `Tab` leaves them
alone.

`v` moves *inside* a row without opening it. `b` moves *between* rows, flipping
between the last two. Both matter more as `Alt v` and `Alt b` below, since the point is doing them
while you are typing at the agent.

Every pane belongs to exactly one row. A pane the sidebar does not recognise
becomes a row of its own, so a plugin reload costs you the grouping and never
access to a pane.

### Enter

| Situation | What happens |
|---|---|
| A row in this session, `solo "true"` | its current pane takes the slot; the previous one is parked (still running) |
| A row in this session, otherwise | plain pane focus |
| An agent elsewhere (`⇢` in the list) | detaches this client and reattaches to that session, landing on the agent |

### Peeking

`p` opens a floating pane mirroring the agent's pane once a second. It reaches
panes anywhere — another session, a background tab, a session nobody is attached
to — and does not disturb the target.

The peek is another instance of this plugin, which is what makes it work at all:
a floating pane is only on screen while it holds focus, and a *command* pane
cannot read a key, so a command-pane peek was either invisible or impossible to
dismiss. Any key dismisses a peek and then still does its own job; `q` and `Esc`
are the two that only dismiss.

`r` names the project the cursor is on: type, `Enter` keeps it, `Esc` drops it,
and while you are typing the keyboard belongs to the name — a `d` in a project
name is a letter, not a key. Enter with nothing typed gives the project its
directory back.

There is a second way to say it, for when the answer belongs to the code rather
than to you: a `.agenttij` file. The hook looks for one above the working
directory and takes its first line as the project name — an empty file means
"named after this directory" — so `~/code/acme/.agenttij` puts every repository
beneath it in one project, on every machine and for everyone who checks it out.
Panes that never report are asked the same question, once per directory, so a
plain shell in one of those repositories lands there too. `r` still wins over a
marker, since it is the more specific thing to have said.

Naming is also how two repositories become one project. A front end and a back
end have different git roots and no shared parent worth grouping by, so nothing
can join them except you saying they are the same thing: name both `acme` and
they are one project, keeping their own row names inside it. Naming one the same
as a project that is already on screen joins that one too, even though it is
still keyed by its path — the name being compared is the one you can see, or you
would get two identical headers and no way to tell why. Every agent started
in either lands there afterwards without being told again, since the name is
remembered against the git root rather than against the agent. Naming a project
that is already several repositories renames all of them, so it stays together.

`J` and `K` move things rather than the cursor: a project among the projects, a
row among the rows of its project. Panes inside a row are left alone — their
order is the order they joined in, which is what `v` cycles through.

The sidebar sorts by what needs you until you have an opinion; from the first
time you move something, what you arranged stays arranged, and anything that
turns up later lands after it rather than in the middle of it. An arrangement
outlives the plugin: order and folds are written to
`${XDG_CACHE_HOME:-~/.cache}/agenttij/order` whenever you change either, and read
back when a sidebar starts, so a reload — or tomorrow — finds the projects where
you left them and the ones you folded away still folded. Rows are remembered per
session, since a pane id means nothing once its session is gone; projects are
remembered by path, so they keep their order everywhere. Shift is the only
modifier the sidebar reads — every other one still passes straight through to
your own Zellij bindings.

`!` shows only agents that need you — every project, every session, nothing else
— and the bottom line says so while it is on. Press it again for the whole list.

`c` interrupts: the byte Ctrl-C would send, so whatever the agent is running
stops and the pane stays. On a project it interrupts every agent in it. Like `d`
it asks first, and for the same reason. (Zellij's own "send SIGINT to pane" call
signals the pane's *shell*, which ignores it — measured; this is why the plugin
asks for `WriteToStdin`, and that one byte is the only thing it ever writes.)

`d` on a row closes the row: the pane you can see and every pane parked behind
it, agents included. `d` on a pane listed under an opened row closes only that
pane, and `d` on a project closes everything in it. Either way the first press only arms it — the bottom line names what is
about to go and how many panes go with it — and *any* other key cancels. Closing
the row you were looking at puts the row below it on screen (the one above, if it
was the last), rather than leaving the workspace empty. Rows in
another session are not ours to close, so `d` does nothing on them.

### Jumping

`/` in the sidebar, or `Alt t` from anywhere, opens a floating list of every
agent on the machine, every pane in this session, every live session, and every
session Zellij can bring back. Type to narrow it — letters have to appear in
order, and ones that arrive together or at the start of a word count for more, so
three letters usually gets there. A project name finds its agents even when they
are not called after it. `Enter` goes, `Esc` closes, `Up`/`Down` or `Tab` move.

Going somewhere in this session is a focus. Going to another session detaches and
reattaches, which is what jumping there means; a dead one comes back on the way.
It is the same plugin as the sidebar in another mode, so it needs no separate
install and no separate permissions — and `Alt t` finds the palette already open
rather than stacking a second one, as long as its configuration matches, which is
why the installer writes that configuration verbatim.

## From anywhere in Zellij

Installed by `scripts/install.sh` into your Zellij config, as a
marker-delimited `shared_except "locked"` block placed **inside** your existing
`keybinds` block.

That placement is not cosmetic. Zellij reads keybindings with
`kdl_config.get("keybinds")`, which returns only the *first* matching node, so a
second top-level `keybinds` block parses cleanly, passes `zellij setup --check`,
and is then silently ignored. Children of the first block are all iterated, which
is why the bindings go in there.

| Key | Does |
|---|---|
| `Alt s` | focus the sidebar |
| `Alt v` | cycle to the next pane in the row on screen |
| `Alt b` | flip back to the row you were on before |
| `Alt g` | new row — a new agent pane |
| `Alt m` | add a pane to the row on screen |
| `Alt a` | summon a sidebar in whatever session you are in, floating |
| `Alt t` | jump, from anywhere — no sidebar needed |
| `Alt ]` | fold the sidebar to a status rail, and back — Zellij's own key |

`Alt g` and `Alt m` are `n` and `a` without going through the sidebar first —
`Alt m` adds to the row *on screen*, which is why `a` does the same rather than
using the cursor: the two would otherwise disagree.

`Alt a` is the way out of a session with no sidebar of its own, the one you land
in after jumping. It focuses an existing sidebar rather than stacking duplicates.

`Alt ]` is `NextSwapLayout`, a Zellij default; the shipped layouts define a
`rail` variant for it to switch to. Folding is a swap layout rather than
something the plugin does because it has to be *exact*: the sidebar returns to
the same side at the same percentage width. A plugin can only resize itself in
coarse asynchronous steps, so the width drifts; and hiding the pane is worse,
since a suppressed pane comes back wherever Zellij decides to put it.

In a session still holding the panes its layout was written with, the first
`Alt ]` lands on `sidebar` — the same arrangement under another name — and the
second folds to the rail. That layout exists so that opening a pane does not fold
the sidebar on its own (see the swap-layout trap in AGENTS.md); once you have
opened one, `Alt ]` folds on the first press.

On the rail the sidebar shows one status glyph per row, centred, and every key
still works.

Change any of ours with the installer:

```sh
AGENTTIJ_KEYBIND="Alt w" AGENTTIJ_FOCUS_KEYBIND="Alt e" \
AGENTTIJ_CYCLE_KEYBIND="Alt r" AGENTTIJ_BACK_KEYBIND="Alt t" \
AGENTTIJ_NEW_KEYBIND="Alt y" AGENTTIJ_ADD_KEYBIND="Alt u" \
    ./scripts/install.sh
```

Or edit the `// agenttij:begin … // agenttij:end` block directly. Re-running the
installer regenerates that block, so edits inside it are replaced — keep your own
bindings outside it. `--no-keybind` skips it, and `--uninstall` removes it,
restoring the file byte for byte.

Two things to know:

- **Keybinds are read when a session starts.** Sessions already running when you
  installed will not have them — the usual reason a binding "does not work".
  Start a new session, or use the commands below.
- **Every `MessagePlugin` binding repeats the layout's plugin configuration.** Zellij
  identifies a plugin by its url *and* its configuration, so a message that does
  not match launches a *second* sidebar instead of reaching yours. Change `scope`
  or `solo` in a layout and you must change the bindings to match.

## Without keybinds

Useful in a session that predates the install.

Summon a sidebar — the `--` is required, Zellij will not take the url without it:

```sh
zellij plugin --floating -- "file:$HOME/.config/zellij/plugins/agenttij.wasm"
```

Fold to the rail and back:

```sh
zellij action next-swap-layout
```

Cycle a row's panes, or flip rows. `--name` is the action; `-c` must match the
layout's plugin configuration:

```sh
zellij pipe --plugin "file:$HOME/.config/zellij/plugins/agenttij.wasm" \
    -c 'scope=session,solo=true' --name cycle
```

## Not ours, but worth knowing

| Key | Does |
|---|---|
| Zellij's fullscreen toggle (`Ctrl p`, then `f` by default) | expands the focused pane over the whole tab, sidebar included, and back |

The shipped layouts size the sidebar as a percentage rather than a fixed number
of columns, because a pane declared `size=26` cannot be resized at all — not by a
plugin, and not by Zellij's own `resize`.
