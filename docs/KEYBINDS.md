# Keybinds

Two layers, because they are handled by two different things: keys the sidebar
itself reads while it is focused, and Zellij keybinds that work from anywhere.

## In the sidebar

These work when the sidebar pane has focus — reach it with your usual pane
navigation. No modifiers: any key held with Ctrl/Alt/Shift is passed straight
through to Zellij, so your own bindings keep working while the sidebar is
focused.

| Key | Does |
|---|---|
| `j`, `↓` | next agent |
| `k`, `↑` | previous agent |
| `Enter` | go to the selected agent |
| `p` | peek at it without leaving this session |
| `c` | park the sidebar off screen |

**`Enter`** behaves differently depending on where the agent is and how the
sidebar is configured:

| Situation | What happens |
|---|---|
| Agent in this session, `solo "true"` | swaps it into the slot and parks the previous agent — no detach |
| Agent in this session, otherwise | plain pane focus |
| Agent in another session (`⇢` in the list) | detaches this client and reattaches to that session, landing on the agent's pane |

**`p`** opens a floating pane polling `dump-screen` once a second. It reaches
panes anywhere — another session, a background tab, a session nobody is attached
to — and does not disturb the target.

**`c`** hides the sidebar (Zellij calls it suppressed: off screen, still
running). A hidden pane cannot be focused to press a key in, so bringing it back
is `Alt z` below.

## From anywhere in Zellij

Installed by `scripts/install.sh` into your Zellij config, as a
marker-delimited `shared_except "locked"` block placed **inside** your existing
`keybinds` block.

That placement is not cosmetic. Zellij reads keybindings with
`kdl_config.get("keybinds")`, which returns only the *first* matching node, so a
second top-level `keybinds` block parses cleanly, passes `zellij setup --check`,
and is then silently ignored. Children of the first block, by contrast, are all
iterated — which is why the bindings go in there.

| Key | Does |
|---|---|
| `Alt a` | summon a sidebar in whatever session you are in, as a floating pane |
| `Alt z` | park / unpark the workspace sidebar |

`Alt a` is the way out of a session that has no sidebar of its own — the one you
land in after jumping. It focuses an existing sidebar rather than stacking up
duplicates.

`Alt z` toggles the docked workspace sidebar, and is the counterpart to `c`.

Change either with the installer:

```sh
AGENTTIJ_KEYBIND="Alt s" AGENTTIJ_FOLD_KEYBIND="Alt x" ./scripts/install.sh
```

Or edit the `// agenttij:begin … // agenttij:end` block in your Zellij config
directly. Re-running the installer regenerates that block, so edits inside it
are replaced — keep your own bindings outside it. `--no-keybind` skips the block
entirely, and `--uninstall` removes it, restoring the file byte for byte.

Two things to know:

- **Keybinds are read when a session starts.** Sessions already running when you
  installed will not have them — this is the usual reason a binding "does not
  work". Start a new session, or use the manual commands below.
- **`Alt z` repeats the workspace layout's plugin configuration.** Zellij
  identifies a plugin by its url *and* its configuration, so a message that does
  not match launches a *second* sidebar instead of reaching the one you have. If
  you change `scope` or `solo` in a layout, change the keybind to match.

## Without keybinds

Everything above is reachable from a shell, which is useful in a session that
predates the install.

Summon a sidebar — the `--` is required, Zellij will not take the url without
it:

```sh
zellij plugin --floating -- "file:$HOME/.config/zellij/plugins/agenttij.wasm"
```

Park or unpark a running sidebar. `--name` may be `hide`, `show`, or anything
else to toggle; `-c` must match the layout's plugin configuration:

```sh
zellij pipe --plugin "file:$HOME/.config/zellij/plugins/agenttij.wasm" \
    -c 'scope=session,solo=true' --name toggle
```

## Not agenttij's, but worth knowing

| Key | Does |
|---|---|
| Zellij's fullscreen toggle (`Ctrl p`, then `f` by default) | expands the focused agent over the whole tab, sidebar included, and back |

A pane declared with a fixed width (`size=26`) cannot be resized — not by a
plugin and not by Zellij's own `resize` action — so there is no "narrow the
sidebar" key. Fullscreen and `c` are the two ways to give an agent the whole
width.
