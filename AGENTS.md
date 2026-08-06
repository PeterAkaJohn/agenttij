# Working on agenttij

A Zellij plugin (WASM) plus a shell hook. `docs/PLAN.md` holds the design and
the Zellij constraints behind it; `docs/KEYBINDS.md` holds every key. Read those
before changing behaviour — most of what looks like an arbitrary choice here is
a workaround for something measured.

## Commands

```sh
cargo test -p agenttij-core                        # the logic; runs on the host
cargo clippy -p agenttij-core --all-targets        # keep at zero warnings
cargo clippy -p agenttij --target wasm32-wasip1    # same for the plugin
cargo fmt --all
cargo build -p agenttij --target wasm32-wasip1 --release
./scripts/install.sh                               # build + install everything
```

Fast loop when iterating on the plugin: build, copy the wasm over the installed
one, then `zellij -s <session> action start-or-reload-plugin "file:$HOME/.config/zellij/plugins/agenttij.wasm"`.

## Structure

```
crates/core    agenttij-core — zero dependencies, all the decisions, unit-tested
crates/plugin  agenttij — wasm: lifecycle, rendering, navigation
hooks/         the shell hook agents run — tool-agnostic: `$1` is the state
integrations/  per-tool glue that calls that hook (opencode plugin)
layouts/       left, right, workspace (+ a `rail` swap layout in each)
scripts/       installer and its three helpers
```

`core` must not gain dependencies — not even `zellij-tile`. The plugin crate
adapts `SessionInfo` into `core`'s own `PaneSnapshot` at the boundary, which is
what lets `cargo test` run without a WASM host. Put logic in `core` with a test;
keep `crates/plugin` to wiring, host calls and drawing.

## Conventions

- Comments explain *why*, especially where the code looks odd — it usually looks
  odd because a simpler version was tried and failed. Say what failed.
- No `unwrap`/`expect` on host calls; a plugin that panics leaves a dead pane.
- The sidebar is read-only: it navigates, and never types into an agent.
- No async runtime. Plugins run single-threaded on `wasmi`; the host *is* the
  runtime (`run_command` → `RunCommandResult`, `set_timeout` → `Timer`).

## Verifying changes

`cargo test` covers `core`. The plugin needs a real session, and there are traps:

- **Plugin pane content cannot be read.** `dump-screen` and `subscribe` both
  return nothing for plugin panes, including for third-party ones. Verify
  behaviour through side effects instead: `list-panes`, `list-clients`,
  `dump-layout`, or a file the plugin touches.
- **Test anything input-related with `scripts/press-keys.sh`.** It pipes real
  bytes into a throwaway client's stdin and prints a second-by-second timeline of
  panes and focus, which is the only way to see what a key actually did:
  `scripts/press-keys.sh test layouts/agenttij-workspace.kdl 7:'\033h' 3:p 6:q`.
  Two traps it exists to avoid: `zellij action send-keys` bypasses keybind
  resolution, and `write-chars` does not reach command panes at all — their
  stdin is `/dev/null`, and even a real keypress to a focused command pane is
  not readable from `/dev/tty`. Move focus from inside the keystream (`\033h` is
  Alt+h, focus left); an external `focus-pane-id` gets overridden on startup.
- **`zellij setup --check` proves a config parses, not that it works.** A second
  `keybinds` block passes the check and is then ignored.
- **`dump-layout` prints the live layout *and* the templates.** Cut at the first
  `swap_tiled_layout` / `new_tab_template` line before asserting on it.
- **`move-focus right` only lands on a visible pane**, which is a handy way to
  find out which pane currently holds the workspace slot.
- **Use throwaway session names and kill them afterwards.** The user has real
  sessions with real agents; never send keys or actions to a session you did not
  create, and check `zellij ls` before and after.
- **`pgrep`/`pkill` patterns match your own command line.** Anchor them
  (`pgrep -f '^zellij -s test'`) or you will kill your own shell.
- **The headless `script` harness kills panes sometimes.** Look for
  `Input/output error`, `Failed to apply cached resizes` or `consecutive unknown
  messages` in `/tmp/zellij-*/zellij-log/zellij.log` before concluding the code
  lost a pane.

## Zellij traps worth knowing

Each of these cost a debugging round already:

- **Every host command needs a permission**, and a missing one fails *silently*.
  `open_terminal*` needs `OpenTerminalsOrPlugins`, distinct from `RunCommands`.
  Adding one to `request_permission` means adding it to
  `scripts/grant-permissions.py` too, or users get a prompt they cannot see in a
  narrow pane.
- **A plugin's identity is its url *plus* its configuration.** A pipe, message or
  `LaunchOrFocusPlugin` whose configuration differs launches a *second* instance
  instead of reaching the one running. Layout configuration and anything
  addressing it must match exactly.
- **Only the first `keybinds` block in a config is read** (`kdl/mod.rs`, with a
  TODO about it). Bindings must be inserted inside it.
- **A pane with a fixed `size=` cannot be resized**, by a plugin or by Zellij's
  own `resize`. Layouts use percentages so the `rail` swap layout works.
- **Command panes cannot read the keyboard.** Their stdin is `/dev/null`, so a
  polling command pane cannot close itself on a keypress — the plugin has to do
  it. And a pane opened from a plugin steals focus *after* the call returns, so
  taking focus back has to wait for a later event.
- **Suppressed panes forget their geometry.** Unsuppressing puts the pane
  wherever Zellij likes, so hide/show is not a reversible collapse — that is
  what swap layouts are for.
- **`PaneManifest.panes` is a `HashMap`.** Sort anything derived from it, or rows
  shuffle between updates.
- **Resizing from a plugin is coarse and asynchronous.** Stepping towards a
  target width overshoots and drifts; don't try again.

## Don't

- Don't sort the list by recency: rows must not move under the cursor. Attention
  first, then pane id.
- Don't call `stack_panes` on panes that are already stacked; new panes join a
  stack on their own.
- Don't register Claude Code's `SubagentStop` hook — it can fire after the turn
  ended and would revive an idle pane.
- Don't add a config knob for something a layout can express.
