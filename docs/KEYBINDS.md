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
| `n` | new agent pane in the slot, parking the current one |
| `b` | back to the session you came from |
| `q`, `Esc` | dismiss a peek |

**`Enter`** behaves differently depending on where the agent is and how the
sidebar is configured:

| Situation | What happens |
|---|---|
| Agent in this session, `solo "true"` | swaps it into the slot and parks the previous agent — no detach |
| Agent in this session, otherwise | plain pane focus |
| Agent in another session (`⇢` in the list) | detaches this client and reattaches to that session, landing on the agent's pane |

While a peek is open **any** key dismisses it, and then still does its own job —
peeking costs you no keystrokes, and there is no mode to remember. `q` and `Esc`
are the two that only dismiss.

In solo mode every pane in the session is listed, not just agents — a plain
shell appears as `·` with its pane title, sorted below the agents. That is what
makes a pane you have not started an agent in reachable after it is parked.

**`n`** opens a fresh terminal *in place of* whatever is in the slot, suspending
it rather than splitting the screen — this is the "managed pane" that keeps
workspace mode down to one agent on screen. Close the new pane and Zellij brings
the suspended one back, so the slot is never empty and starting an agent never
costs you the one you were looking at. Outside solo mode there is no slot to
manage, so `n` is an ordinary new pane.

Zellij's own `new pane` binding (`Ctrl p`, then `n`) splits instead, leaving two
panes on screen. That is why `n` exists.

**`p`** opens a floating pane polling `dump-screen` once a second. It reaches
panes anywhere — another session, a background tab, a session nobody is attached
to — and does not disturb the target.

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
| `Alt ]` | fold the sidebar to a status rail, and back (Zellij's own key) |

`Alt a` is the way out of a session that has no sidebar of its own — the one you
land in after jumping. It focuses an existing sidebar rather than stacking up
duplicates. Change it with the installer:

```sh
AGENTTIJ_KEYBIND="Alt s" ./scripts/install.sh
```

`Alt ]` is not ours — it is Zellij's default `NextSwapLayout`, and the shipped
layouts define a `rail` swap layout for it to switch to. Folding is a swap
layout rather than something the plugin does because it has to be *exact*: the
sidebar returns to the same side at the same percentage width. A plugin can only
resize itself in coarse asynchronous steps, so the width drifts a little further
on every fold; and hiding the pane is worse still, since a suppressed pane comes
back wherever Zellij decides to put it rather than where it was.

On the rail the sidebar shows one status glyph per agent and nothing else. `j`,
`k`, `Enter` and `p` all still work there.

Or edit the `// agenttij:begin … // agenttij:end` block in your Zellij config
directly. Re-running the installer regenerates that block, so edits inside it
are replaced — keep your own bindings outside it. `--no-keybind` skips the block
entirely, and `--uninstall` removes it, restoring the file byte for byte.

Two things to know:

- **`n` needs the `OpenTerminalsOrPlugins` permission.** The installer grants it,
  but a sidebar installed before `n` existed was granted three permissions
  rather than four — re-run `scripts/install.sh`, or `n` will be denied in
  silence.
- **Keybinds are read when a session starts.** Sessions already running when you
  installed will not have them — this is the usual reason a binding "does not
  work". Start a new session, or use the manual commands below.
- **A swap layout must repeat the plugin's configuration exactly.** Zellij
  identifies a plugin by its url *and* its configuration, so the `rail` block
  carries the same `scope`/`solo` values as the main layout. Change one, change
  both, or folding will start a second sidebar instead of moving the one you
  have.

## Without keybinds

Everything above is reachable from a shell, which is useful in a session that
predates the install.

Summon a sidebar — the `--` is required, Zellij will not take the url without
it:

```sh
zellij plugin --floating -- "file:$HOME/.config/zellij/plugins/agenttij.wasm"
```

Fold or unfold without the keybind:

```sh
zellij action next-swap-layout
```

## Not agenttij's, but worth knowing

| Key | Does |
|---|---|
| `Alt ]`, `Alt [` | next / previous swap layout — what folds the sidebar |
| Zellij's fullscreen toggle (`Ctrl p`, then `f` by default) | expands the focused agent over the whole tab, sidebar included, and back |

The shipped layouts size the sidebar as a percentage rather than a fixed number
of columns, because a pane declared `size=26` cannot be resized at all — not by
a plugin, and not by Zellij's own `resize` action.
