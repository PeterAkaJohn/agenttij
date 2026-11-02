# Working on agenttij

A Zellij plugin (WASM) plus a shell hook. `docs/PLAN.md` holds the design and
the Zellij constraints behind it; `docs/KEYBINDS.md` holds every key. Read those
before changing behaviour — most of what looks like an arbitrary choice here is
a workaround for something measured.

## Commands

```sh
cargo test -p agenttij-core                        # the logic; runs on the host
cargo clippy -p agenttij-core --all-targets        # keep at zero warnings
cargo test -p agenttij-cli                         # the `agenttij` command
cargo clippy -p agenttij-cli --all-targets
cargo clippy -p agenttij --target wasm32-wasip1    # same for the plugin
cargo fmt --all
cargo build -p agenttij --target wasm32-wasip1 --release
./scripts/install.sh                               # build + install everything
python3 scripts/harness.py scenario scripts/e2e/*.txt   # the plugin, in real sessions
```

Fast loop when iterating on the plugin: build, copy the wasm over the installed
one, then reload it *with its configuration*:

```sh
zellij -s <session> action start-or-reload-plugin \
    -c "scope=session,solo=true" "file:$HOME/.config/zellij/plugins/agenttij.wasm"
```

Without `-c`, that command does not reload the sidebar you are looking at — it
starts a *second* one in a new pane, because a plugin's identity is its url plus
its configuration. Measured: `plugin_3`, titled "agents", beside the first. The
`-c` has to say everything the layout says, `group` included.
In a harness session, `harness.py reload <session>` does this with the plugin you
just built and the layout's whole configuration, and installs nothing.

## Structure

```
crates/core    agenttij-core — zero dependencies, all the decisions, unit-tested
               agent (status, rows), group (a row is a group of panes),
               project (rows sharing a git root), order (what you rearranged,
               and its file), panes (reconciliation, discovery),
               scan (state files), color (SGR), config, format (row layout),
               api (what an agent may ask the sidebar, both ends of it)
crates/plugin  agenttij — wasm: lifecycle, rendering, navigation, peek mode
crates/cli     agenttij-cli - the `agenttij` command agents run, and
               `agenttij mcp`; serde_json lives here and nowhere else
hooks/         the shell hook agents run — tool-agnostic: `$1` is the state.
               Also resolves the project (`.agenttij`, git root, cwd), because
               the sidebar cannot afford that per row per tick
integrations/  per-tool glue that calls that hook (opencode plugin)
plugins/       the Claude Code plugin (MCP server + skill); the marketplace
               that lists it is .claude-plugin/ at the root
layouts/       left, right, workspace (+ `sidebar` and `rail` swap layouts)
scripts/       installer, three helpers, testbox.sh, and harness.py with its
               e2e/ scenarios
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
- The sidebar is read-only: it navigates, and never types into an agent. Two
  exceptions, both asked for twice over. `c` writes a single interrupt byte
  (`0x03`) and asks twice before it does - Zellij's own `send_sigint_to_pane_id`
  signals the pane's shell, which ignores it, so there is no other way to stop a
  runaway. `R` answers an agent that is waiting on you, and only where a layout
  set `reply "true"` (off otherwise): the answer is typed into the sidebar and
  sent with Enter, to a row of this session whose status is needs-input, checked
  again as it is sent. The argument for it: a question you can read in a peek
  and answer in two keys is the commonest reason to leave the sidebar at all.
  Nothing else may write, and adding anything that does means arguing with this
  line first.
- No async runtime. Plugins run single-threaded on `wasmi`; the host *is* the
  runtime (`run_command` → `RunCommandResult`, `set_timeout` → `Timer`).

## Verifying changes

`cargo test` covers `core`.
The plugin is covered by `scripts/harness.py`, which drives real Zellij sessions with real keystrokes, and by its scenarios in `scripts/e2e/`, which CI runs.
Anything a key does gets a scenario; `harness.py --help` lists the commands and the checks.

- **Drive the plugin through `scripts/harness.py`, never `zellij action`.**
  `send-keys` bypasses keybind resolution and `write-chars` never reaches a command pane, so only bytes in a real client's stdin test what a key does.
  `start` gives the session its own config (Zellij's defaults, `sh`, no background plugins, the binds carrying the layout's own `group`), cache, log and socket directory, and runs the plugin you built rather than the one installed.
  So it never shows in `zellij ls`, writes nothing into your caches, and cannot address one of your sessions.
  The shared config and cache it replaced left a workspace snapshot per test session in `~/.cache/agenttij/sessions`, which a palette would have offered to restore after the next boot.
  Then `keys`, `state`, `expect` or `wait` on checks like `focus=slot` and `title.slot=*1/3`, and `stop`, which reads the session's own log for a verdict.
  Step by step from a shell, or as a scenario file.
- **A test passes only if it failed first.** Run a new scenario against the build before the fix (`AGENTTIJ_WASM` points the harness at another wasm) and watch it fail.
  The scenarios in `scripts/e2e/` were checked that way against `b09de72`.
- **Check titles and focus, not counts.** The bugs fixed alongside the harness (a palette relabelling the pane on screen, `Alt b` with nowhere to go) left every count right; only a title and the focus were wrong.
- **Read what the sidebar draws off the client, not off Zellij.** Zellij will not hand out a plugin pane's content (next), so the harness replays the client's raw output through just enough of a terminal and cuts the pane out of it: `drawn.sidebar=›*` checks the line the cursor is on (`›` in column one), and `state` prints every line drawn.
  That is how a row on this machine turned out never to show how many panes it owns - every count and title was right, and the `3` was simply not on screen.
- **Plugin pane content cannot be read.** `dump-screen` and `subscribe` both return nothing for plugin panes, including third-party ones.
  Verify behaviour through side effects instead - `list-panes`, `list-clients`, `dump-layout`, a file the plugin touches - or through what the client draws, as the harness does.
  A pane *frame* is not pane content either - the client draws it, so `dump-screen` never shows it.
  The harness keeps each client's raw output in `client.out` in the session's directory, and the frame line is in there to grep.
  Capturing one by hand (`script -qfc "zellij ..." out >/dev/null`), keep the `>/dev/null`: a 0.45 client announces itself on stdout, and the Zellij pane that receives it marks itself a nested guest and swallows keys.
  Since 0.45 the default frame (`pane_frame_style` titles) is one title row with the name centred, not a box.
- **Keys are written as in a shell:** `\033h` or `\eh` is Alt+h, `\r` is Enter, `\x27` a quote.
  The harness reads `\0333` as Esc then 3; `printf %b`, which the old shell harness used, read it as one octal byte.
  Move focus from inside the keystream; an external `focus-pane-id` gets overridden on startup.
  The harness's panes run `sh`, because a lone Esc typed into zsh makes the next key a Meta key: `echo` ran as `cho`.
- **`zellij setup --check` proves a config parses, not that it works.** A second `keybinds` block passes the check and is then ignored.
- **`dump-layout` prints the live layout *and* the templates.** Cut at the first `swap_tiled_layout` / `new_tab_template` line before asserting on it.
- **`move-focus right` only lands on a visible pane**, which is a handy way to find out which pane currently holds the workspace slot.
- **Outside the harness, use throwaway session names and kill them afterwards.** The user has real sessions with real agents; never send keys or actions to a session you did not create, and check `zellij ls` before and after.
- **`pgrep`/`pkill` patterns match your own command line.** Anchor them (`pgrep -f '^zellij -s test'`) or you will kill your own shell.
- **A client that dies takes panes with it.** Letting a keystream end used to do it: the client hit EOF, spent a second sending the server nonsense until it was logged out, and a pane went with it, which cost a whole chase after a pane `Alt m` had opened and Zellij had confirmed by id.
  The harness's client holds its own end of the key pipe, so it never reads EOF.
  If one dies anyway, `stop` says `log: client` and a scenario runs once more; outside the harness, look for `Input/output error`, `Failed to apply cached resizes` or `consecutive unknown messages` in the log before believing a pane count.

## Zellij traps worth knowing

Each of these cost a debugging round already. Measured on 0.44; all were
rechecked against 0.45.1's source when the plugin moved to it, and the ones
0.45 changed say so.

- **Every host command needs a permission**, and a missing one fails *silently*.
  `open_terminal*` needs `OpenTerminalsOrPlugins`, distinct from `RunCommands`.
  Adding one to `request_permission` means adding it to
  `scripts/grant-permissions.py` too, or users get a prompt they cannot see in a
  narrow pane.
- **A command issued in `load` is dropped, silently.** The permission grant has
  not landed yet, so `run_command` there does nothing at all — measured with the
  directory picker: it asked twice in `load` and zoxide was never started, no
  error, no output, no clue. The first moment a command of yours can run is
  `PermissionRequestResult`; the tick is the backstop.
- **A plugin's command does not inherit `$HOME`.** zoxide could not find its own
  database and printed nothing; paths came back unshortened because the home to
  shorten against was empty too. Read it out of the passwd entry instead
  (`getent passwd "$(id -u)" | cut -d: -f6`) and hand it to whatever needs it.
  The command otherwise runs fine with no environment at all — verified with
  `env -i`. Zellij's own source (0.44.3 and 0.45.1, `background_jobs.rs`) passes
  the server's environment on, so this depends on how the server was started;
  keep reading the passwd entry either way.
- **A denied permission is remembered as an empty grant, and everything then
  fails silently.** Zellij's prompt is answered with `y`/`n` — so a keystroke
  test that types `n` into a session whose plugin is *waiting* on that prompt
  answers "deny", and `~/.cache/zellij/permissions.kdl` keeps the plugin with an
  empty `{}` block forever after. The symptom is a sidebar that draws but does
  nothing: no new panes, no floating panes, no scan. Check that file first when a
  plugin stops acting, and re-grant before testing anything that adds a
  permission.
- **A running session rewrites `permissions.kdl` with what it asks for.** Every
  permission request re-reads the file and replaces the plugin's entry with
  exactly the set that instance asked for (`cache_plugin_permissions`,
  `wasm_bridge.rs`), granted or not. So a session still running an older build -
  whose next peek is a new instance asking again - shrinks a grant you just
  widened, the next session asks for something the cache no longer has, gets no
  answer, and the command fails silently. Grant before starting sessions, and
  check the file again afterwards, because it may not say what you just wrote.
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
  a rail you chose stays a rail. The base layout stays in the cycle too, and with
  two panes on screen it always fits, so `NextSwapLayout` past the rail wraps to
  it and the next one lands on `sidebar`: a press that changes nothing. Unfold
  with `PreviousSwapLayout` (`Alt [`) and the base never comes round again;
  every reordering or constraint tried instead either kept that dud or lost the
  rail when a pane was added. Constraining that first layout by pane count
  does not work: suppressing a pane relayouts too (`extract_pane`), and in solo
  mode that drops the count straight back down.
- **A second client repaints the frames and is not our doing.** Zellij colours
  the focused pane's frame per *client* as soon as two are attached and the
  session is not mirrored (`frame_color`, `ui/pane_contents_and_ui.rs`): client 1
  gets magenta, client 2 blue, and the theme's `frame_selected` is not used at
  all. The same condition adds `MY FOCUS AND:` plus a block per other client to
  the title (`render_my_and_others_focus`, `ui/pane_boundaries_frame.rs`): in the
  middle with the name on the left in the `full` frame style, on the left with
  the name centred in 0.45's default `titles` style. Measured on 0.44 with
  two captured clients: `[38;2;255;94;241m` for client 1 and
  `[38;2;94;161;255m` for client 2, both titled `· agenttij`. So a blue border
  and a title nobody set means something else is attached - switching to a
  session that is already open in another terminal is the usual way in.
- **A pane keeps the session name it was born with.** `ZELLIJ_SESSION_NAME` is
  set when the pane spawns and a `rename-session` does not reach it - measured
  through `/proc/<pid>/environ`, before and after. The hook reads that variable,
  so renaming a session orphans the state files of every agent already in it, and
  `panes::reconcile` drops an agent whose session is not live. That is why a
  rename records the old name as an alias (`a` lines, per session) and `rebuild`
  maps agents reporting under one onto this session: with it, a running agent
  keeps its status across a rename; without it, the row degrades to a discovered
  pane. Anything that renames a session goes through `Sidebar::rename_session`
  for that reason.
- **Zellij does not serialize a parked pane.** `get_layout_metadata`
  (`screen.rs`) walks tiled and floating panes and only touches the suppressed
  ones to swap a scrollback editor back in, so a pane hidden with
  `hide_pane_with_id` is absent from `session-layout.kdl`. Measured: a row of
  three came back as its head alone (and the template refilled the rest).
  Resurrected command panes carry `start_suspended true`, so an agent waits for
  Enter rather than relaunching itself.
- **One file rewritten by every sidebar loses what belongs to a session.** The
  arrangement is saved whole from whichever sidebar saved last, which is right
  for an opinion about project order and wrong for a record *belonging* to a
  session: measured with three sidebars up, a session's rows never survived to
  the file at all. Shared things (`p`, `f`, `n`) stay in `order`; a session's own
  (`b`, `g`, `w`, `h`) go to `sessions/<name>`, which only it writes. One `cat`
  reads both.
- **A session restarts inside a boot, and that is the case the boot id misses.**
  A session killed and resurrected comes back holding one row - Zellij does not
  serialize a parked pane - and its new sidebar replaced the snapshot describing
  every row it was there to bring back, because the snapshot was keyed on the
  machine boot and the boot had not changed. Measured: two rows of three before
  the kill, one row and no workspace after it, with nothing left to restore from.
  A sidebar now replaces only the snapshot *it* wrote (`Sidebar::snapshot_at`)
  and a fresh run writes a new one, so the last run's survives; the palette
  offers it, and restoring it rebuilds the rows (measured: 3 panes after the
  resurrection, 6 in two rows after the restore).
- **Keeping a snapshot per run means saying *which* one to restore.** The
  newest is not the best: a run whose grouping had come apart records rows of one
  pane, and restoring that rebuilds a session with nothing tied together - which
  is what "the rows and the hidden panes are not tied together" turned out to be,
  from a real arrangement whose newest snapshot was six rows of one pane and
  whose older one held the same work as six rows of three. The palette entry
  carries the stamp (`Target::Workspace { session, stamp }`, asked for as
  `<session>@<stamp>`), so each entry restores the snapshot it names and its row
  counts and ages are how you tell them apart.
- **Everything a resurrection brings back is a stranger.** The grouping is
  written in pane ids and the ids start again, so a session that comes back
  holding more than one pane comes back with none of them tied together - which
  is the whole of "the rows and the hidden panes are not tied together" after an
  attach. Nothing can infer which row a pane belonged to; the workspace snapshot
  can, though, so a restore treats each such pane as standing in for one
  remembered row and opens that row's companions behind it
  (`Sidebar::filling`). Only a pane that came back *alone*: one the layout's
  template has already filled out is a row of three, and filling it again made
  it a row of five. Measured: two rows of three, killed, resurrected as two
  loose panes, restored to `0,4,5` and `1,2,3`.
- **A resurrected pane has no working directory until you start it.** Zellij
  brings command panes back with `start_suspended true`, and a command that has
  not run has nothing in `/proc` for `get_pane_cwd` to read - so every row is
  unplaceable, and a restore that skips directories already open skipped none of
  them and rebuilt the row already on screen (nine panes where six were wanted).
  Rows whose directory cannot be read count against the remembered ones anyway.
- **A session's own sidebar overwrites the thing it should restore.** The rows a
  session has are written to `~/.cache/agenttij/order` whenever they change — so
  after a restart the sidebar in `work` replaces `work`'s four remembered rows
  with the one row Zellij brought back, before anyone can ask for the other
  three. Snapshots are therefore stamped with `/proc/sys/kernel/random/boot_id`:
  this boot's entry for a session is live and replaceable, an earlier boot's is a
  snapshot, and only earlier ones are offered for restoring. The same stamp is
  what makes it safe to keep pane ids in that file at all.
- **A reload keeps the panes and loses nothing else either, now.** Grouping used
  to go with it — a reload built a `Groups` from nothing, every pane became a row
  of its own, and a row of five came apart into five. The rows are written into
  `~/.cache/agenttij/order` (`g` lines, per session) and put back around the panes
  that are still there. Which also means the arrangement file must not be written
  before it has been read: a sidebar that started a second ago has a blank one.
- **A plugin's identity is its url *plus* its configuration.** A pipe, message or
  `LaunchOrFocusPlugin` whose configuration differs launches a *second* instance
  instead of reaching the one running. Layout configuration and anything
  addressing it must match exactly — including a `group` template, which is why
  `scripts/install.sh` takes `AGENTTIJ_GROUP` and writes it into the binds.
  Measured: `Alt g` at a sidebar whose layout carried `group` and whose binding
  did not put `plugin_3` in the pane list and opened an untemplated row.
- **Only the first `keybinds` block in a config is read** (`kdl/mod.rs`, with a
  TODO about it). Bindings must be inserted inside it.
- **A pane with a fixed `size=` cannot be resized**, by a plugin or by Zellij's
  own `resize`. Layouts use percentages so the `rail` swap layout works.
- **A command pane's program *does* read the keyboard** — this note used to say
  the opposite, and it was wrong. Measured both ways, layout `pane command=` and
  the runtime path a plugin uses (`open_command_pane`, same as `zellij run`): a
  real keypress lands on the program's stdin, which is what makes `group "claude"`
  as usable as typing `claude` into a shell. What never arrives is `zellij action
  write-chars` / `send-keys` — that is where the `/dev/null` belief came from, and
  it is why `scripts/harness.py` pipes into a client instead. A peek still has
  to be a plugin pane, but for the floating-focus reason below, not this one.
- **A floating pane in the tab is not the workspace slot.** Zellij's own `Ctrl p`
  then `e` (`TogglePaneEmbedOrFloating`) floats the focused pane, command and
  all, and `get_layout_metadata` *does* serialize floating panes - so a session
  resurrected after that brings a floating agent back for ever, and re-serializes
  it each time. Measured in a user's cache: one session with
  `floating_panes { pane command="claude" name="○ contentchef-serverless" }`.
  `PaneInfo.is_floating` is what tells them apart; `visible_terminal` skips them,
  or the sidebar parks and shows against a pane that is not in the tiled layer at
  all. They stay rows, because a floating pane is still somewhere to go.
- **A floating pane is only on screen while it holds focus.** Focus a tiled pane
  and Zellij sets the tab's `hide_floating_panes`; `pinned` does not override it.
  So a floating pane that needs to stay visible must be one that can hold focus
  usefully — which again means a plugin pane. Hidden is not closed: before 0.45
  a peek hidden with `Alt f` lived on unseen, re-reading its pane every second.
  0.45 sends floating plugins `Visible(false)` when their layer hides, and a peek
  or the keybind list closes itself on it.
- **`get_session_environment_variables()` panics**, taking the plugin down with
  it, and `SessionInfo.plugins` is empty in practice. Your own plugin url is your
  pane's `PaneInfo.plugin_url`. It used to be read from the pane's title, which
  is the url only until something renames it: a sidebar reloaded after naming
  itself never learned it (no peek, no palette, no help), and one launched at
  runtime is titled `(<cwd>) - file:/…`.
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
- **Closing a hidden pane un-hides it instead.** `close_pane` returns early for
  a suppressed id and hands it to `replace_pane_with_suppressed_pane`
  (`tab/mod.rs`) — the plugin API always passes `ignore_suppressed_panes: false`,
  so there is no way to ask for the other behaviour. Show the pane first, then
  close it; `delete` in the sidebar shows even panes it believes are visible,
  because a pane list one second stale is exactly how you hit the bad path.
- **`list-panes` counts suppressed panes; `dump-layout` does not.** So a pane
  count from `list-panes` says nothing about what is on screen — in solo mode it
  reads exactly like a pane that refused to park, which cost a long chase after a
  bug that was not there. Since 0.45 it also lists background plugins
  (`load_plugins`, `zellij:link`) as suppressed plugin panes, so count terminals,
  or leave out plugins that are suppressed. Use `dump-layout` for what is visible.
  Before 0.45 it printed an empty tab when only plugin panes were left; 0.45.1
  prints a lone sidebar (measured). Neither
  proves a *hidden* pane was closed rather than merely parked; for that count the
  processes the session's server owns
  (`ps --ppid "$(pgrep -f "^[^ ]*zellij --server .*/<session>$")"`).
- **0.45 turns a stack into suppressed panes.** `stacked_pane_list` is on by
  default: on every render the collapsed members of a stack leave the tiled
  panes and are filed as suppressed, under their own ids (`groupify_all`,
  `tab/mod.rs`). In a `PaneManifest` they look exactly like a pane the sidebar
  parked, and `hide_pane_with_id` on one does nothing. The sidebar never stacks,
  so this only reaches a pane the user stacked. And `stack-panes` leaves a grid
  alone: on a session of three panes, `new-pane` then `stack-panes` left four
  tiled, while `stack-panes` then `new-pane` left one stack of four (measured,
  0.45.1) - which is why the remote fallback stacks first.
- **Focusing a scrolled pane enters Scroll mode** (0.45, #5299, read in source):
  `focus_pane_with_id` included, so showing a row whose pane was scrolled up
  leaves the client in Scroll mode. `scroll_mode_sync false` turns it off.
- **An attach pane is a nested session since 0.45.** `Enter` on a remote row
  opens `ssh -t host zellij attach`, and that client announces itself; the
  session here then asks what to do (`nested_session_handling`, default `ask`),
  and every answer, `Esc` included, ends with the pane sending its keys to the
  session over there while it has focus - our own binds too. Measured against
  the testbox. Per-pane control does not exist, and the option is global to the
  user's Zellij, so it is documented rather than set.
- **A command sent over ssh runs in the login shell over there.** The scripts
  `scan.rs` sends are POSIX, so `over_ssh` and `host_command` wrap them in
  `sh -c`. Under zsh an unquoted `$ids` stayed one word, so `stack-panes` got
  every id as one malformed argument, and `cat dir/*.state dir/rows` with no
  state files aborted on the glob and lost the rows. Measured over ssh into
  `scripts/testbox.sh`, whose login shell is zsh for that reason.
- **A CLI pipe reaches each plugin once per attached client, and only
  `ReadCliPipes` may answer it.** A pipe naming no plugin goes to every
  (plugin, client) pair (`pipe_to_all_plugins`), so with two clients attached
  the sidebar hears it twice - which is why the agent API claims a request with
  a `mkdir` before acting. Writing back (`cli_pipe_output`) and
  `unblock_cli_pipe_input` need `ReadCliPipes`, which this plugin does not ask
  for; the unblock sat in `pipe()` refused, logged on every pipe.
  Zellij releases the pipe on its own once every plugin has returned
  (`PendingPipes`), so answers go in a file instead.
- **`zellij pipe` without a payload reads stdin, and can stay blocked.** Feed it
  `</dev/null`. Even then, on 0.45.1 a broadcast pipe stayed blocked twice in one
  session (five minutes, then until killed) and could not be made to again; the
  remote fallback therefore gives it two seconds and lets it go.
- **Replacing a pane that is itself a replacement destroys the pane in the
  middle.** Zellij stacks suppressed panes behind a replacement, and
  `open_*_in_place_of_pane_id` on top of that stack drops the middle one
  (measured: three panes in, two out). Open a pane normally, *then* swap it into
  place.
- **The pane list is up to a second old, and every key that shows a pane changes
  the screen now.** Trusting `PaneManifest` one keypress later hides a pane that
  is already parked — a no-op — while the show un-suppresses its neighbour beside
  what you are actually looking at, and the workspace ends up split between two
  rows. Measured: `Enter` then `Alt v` in a single write left two panes at 50%
  each, and it stayed that way. What the plugin put on screen itself is not a
  guess, so `panes::slot` prefers that (`Sidebar::showing`) until a list confirms
  it, with a countdown so a show that never landed cannot wedge the slot for ever.
  The same staleness swallowed the second of two quick `Alt v`s. Which is why
  nothing else may work the slot out for itself: `actions::open_row` used to, and
  `Alt v` then `Alt m` in one write hid the pane already parked and left the new
  one split beside what was on screen. It takes the slot from the caller now.
  `add_to_row` goes further and prefers the *focused* terminal, because a screen
  split by Zellij's own `Alt n` has two of them and the list names whichever
  comes first.
- **A row of the sidebar can blink out on the same staleness.** The pane list
  regressing (below) reaches `panes::reconcile` too, which reaps an agent the
  moment its pane is missing - so someone else's keypress in another session can
  take a row out of the list for one frame and put it back. Reconciling against
  `panes::still_alive` (this list and the one before) is what stops that.
- **A grace counted in updates needs updates to arrive.** `SessionUpdate` is
  skipped early when the pane list has not moved, so a quiet session produced no
  reconcile at all - and a member that never appeared (Zellij answered with an id
  and made no pane) sat in its row for ever, with `v` cycling onto a pane that is
  not there: the show does nothing, the hide parks what you were looking at, and
  the workspace is empty. Reconcile therefore runs on every update, moved or not,
  and only the *redraw* is skipped. `walk` also refuses to show a pane no list has
  ever named.
- **The pane list also goes *backwards*.** A pane that has been listed alive can
  be absent from the next list and back in the one after - it happens as the
  pane is suppressed. Traced through a burst of `Alt m`: `[0,1,2,3,4,5]`, then
  `[0,1,2,3,4]`, then `[0,1,2,3,4,5]` again. One update was enough for
  `Groups::reconcile` to take pane 5 out of its row and hand it back as a row of
  its own, which is what "panes escape the group" was. So *every* member missing
  from a list gets a grace, not only the ones that have never been seen
  (`GRACE_MISSING`, three updates; a pane still being born gets `GRACE_NEW`,
  ten). The cost is that a pane you close leaves its row a few updates late.
- **An action that takes over a second answers with no pane id, and does it
  anyway.** `route_action` waits `ACTION_COMPLETION_TIMEOUT` (one second,
  `route.rs`) for the id and then returns `affected_pane_id: None`, while the
  pane is still opened - so `open_terminal` can hand back `None` for a pane that
  exists, which under a burst leaves a pane in no row at all with no id to put it
  in one. `Sidebar::orphan` remembers the row instead, and `Groups::adopt` takes
  in whatever stranger the next list names. Read in Zellij's source rather than
  measured firing, so treat it as the belt and not the braces.
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
- **The tick is the whole cost.** Idling beside a row of eight panes the sidebar
  burns about 24ms of CPU a second; with the timer off, 1ms. Reconciling,
  sorting and drawing are noise next to forking the scan and carrying its output
  back through the host, so anything that wants to be cheaper has to happen less
  often, not faster. `SessionUpdate` arrives every second whether or not a pane
  moved, and rebuilding and redrawing on all of them doubled the bill for
  nothing — compare the pane list and return early.
- **A path a plugin hands Zellij is read in the plugin's filesystem, not
  yours.** `translate_plugin_path` (`plugins/zellij_exports.rs`) rewrites
  anything starting with `/tmp`, `/host`, `/data` or `/cache` — so opening a pane
  in `/tmp/x` lands it in `$ZELLIJ_TMP_DIR/x`. Measured with the directory
  picker: `/tmp` became `/tmp/zellij-1000`. Every other absolute path passes
  through untouched.
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
