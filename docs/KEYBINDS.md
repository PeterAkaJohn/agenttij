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
| `Tab` | show a row's other panes underneath it, indented |
| `b` | flip back to the row you were on before |
| `v` | cycle to the next pane *within* the row on screen |
| `a` | add a pane to the row on screen |
| `n` | new agent pane — a new row |
| `d` `d` | close what the cursor is on — twice, it cannot be undone |
| `p` | peek at a row's agent without leaving this session |
| `q`, `Esc` | dismiss a peek |
| `?` | this list, in a floating pane |

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

`d` on a row closes the row: the pane you can see and every pane parked behind
it, agents included. `d` on a pane listed under an opened row closes only that
pane. Either way the first press only arms it — the bottom line names what is
about to go and how many panes go with it — and *any* other key cancels. Rows in
another session are not ours to close, so `d` does nothing on them.

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
