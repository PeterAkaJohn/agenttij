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
sh scripts/press-keys.sh test layouts/agenttij-workspace.kdl 8:'\033h' 4:p 6:q
```

Fast loop when iterating on the plugin: build, copy the wasm over the installed
one, then `zellij -s <session> action start-or-reload-plugin "file:$HOME/.config/zellij/plugins/agenttij.wasm"`.

## Structure

```
crates/core    agenttij-core — zero dependencies, all the decisions, unit-tested
               agent (status, rows), group (a row is a group of panes),
               panes (reconciliation, discovery), scan (state files),
               color (SGR), config, format (row layout)
crates/plugin  agenttij — wasm: lifecycle, rendering, navigation, peek mode
hooks/         the shell hook agents run — tool-agnostic: `$1` is the state
integrations/  per-tool glue that calls that hook (opencode plugin)
layouts/       left, right, workspace (+ a `rail` swap layout in each)
scripts/       installer, three helpers, and press-keys.sh
docs/          PLAN.md (design and constraints), KEYBINDS.md (every key)
```

The plugin has two modes in one binary. Without `peek` in its configuration it is
the sidebar; with it, the instance *is* a peek — it mirrors one pane and closes on
any key. A peek has to be a plugin pane, so it is this plugin (see the traps).

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
- **Permission grants are keyed by the plugin's path with no `file:` prefix.**
  Zellij writes `~/.cache/zellij/permissions.kdl` as bare paths, so an entry
  written as `"file:/path/x.wasm"` is never matched: the plugin loads, asks, and
  sits on a prompt too wide for the pane. `scripts/grant-permissions.py` takes
  whatever url you hand it, so hand it the path.
- **Adding a pane re-applies a swap layout, and the base layout stops fitting.**
  Zellij registers your base layout as swap position 0 with `ExactPanes` set to
  the pane count you wrote it with (`set_base_layout`, `tab/swap_layouts.rs`:
  "not intended to be progressive"). Open one more pane and it no longer fits, so
  the next swap layout wins — which is how a lone `rail` folded the sidebar every
  time a pane was added. Each layout therefore defines an unconstrained `sidebar`
  swap layout *before* `rail`. Re-applying keeps the *current* position rather
  than advancing (`add_tiled_pane` sets the damaged flag first, deliberately), so
  a rail you chose stays a rail. Constraining that first layout by pane count
  does not work: suppressing a pane relayouts too (`extract_pane`), and in solo
  mode that drops the count straight back down.
- **A plugin's identity is its url *plus* its configuration.** A pipe, message or
  `LaunchOrFocusPlugin` whose configuration differs launches a *second* instance
  instead of reaching the one running. Layout configuration and anything
  addressing it must match exactly.
- **Only the first `keybinds` block in a config is read** (`kdl/mod.rs`, with a
  TODO about it). Bindings must be inserted inside it.
- **A pane with a fixed `size=` cannot be resized**, by a plugin or by Zellij's
  own `resize`. Layouts use percentages so the `rail` swap layout works.
- **Command panes cannot read the keyboard.** Their stdin is `/dev/null`, and a
  real keypress to a focused command pane is not readable from `/dev/tty`
  either. Anything that must react to a key has to be a *plugin* pane.
- **A floating pane is only on screen while it holds focus.** Focus a tiled pane
  and Zellij sets the tab's `hide_floating_panes`; `pinned` does not override it.
  So a floating pane that needs to stay visible must be one that can hold focus
  usefully — which again means a plugin pane.
- **`get_session_environment_variables()` panics**, taking the plugin down with
  it, and `SessionInfo.plugins` is empty in practice. To learn your own plugin
  url, read your own pane's title from the manifest *before* renaming it — Zellij
  sets it to the url.
- **A pane opened from a plugin steals focus after the call returns**, so taking
  focus back has to wait for a later event, not the same handler.
- **Assert a swap *happened*, not just that panes survived.** Until grouping
  formed, `v` had no target and swapped nothing, so a pane-count test passed
  while proving only that idleness is safe. Check that the visible pane changed —
  `move-focus right` lands on it.
- **Never swap panes with `replace_pane_with_existing_pane`.** Zellij files
  suppressed panes in a `HashMap` keyed by *the pane that replaced them*
  (`tab/mod.rs`, `SuppressedPanes`), so a pane that is already someone's value is
  orphaned the moment it becomes a key — a third pane joining a row destroyed the
  second. `hide_pane_with_id` files a pane under its own id (`suppress_pane`,
  same file, with a comment saying so), so hide-the-old / show-the-new chains
  safely.
- **Replacing a pane that is itself a replacement destroys the pane in the
  middle.** Zellij stacks suppressed panes behind a replacement, and
  `open_*_in_place_of_pane_id` on top of that stack drops the middle one
  (measured: three panes in, two out). Open a pane normally, *then* swap it into
  place.
- **Reconcile against fresh pane data only.** `PaneUpdate`/`SessionUpdate` is the
  only moment the pane list is true; reconciling group membership on a state-file
  tick deletes whatever was added since the last update.
- **Suppressed panes forget their geometry.** Unsuppressing puts the pane
  wherever Zellij likes, so hide/show is not a reversible collapse — that is
  what swap layouts are for.
- **Host calls are round-trips; do not make them per row per render.** Asking
  `get_pane_cwd` / `get_pane_running_command` for every row on every rebuild made
  navigation lag, because rebuilds burst exactly when you are moving around.
  Cache per pane and refresh on a slow tick.
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
