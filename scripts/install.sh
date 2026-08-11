#!/bin/sh
# Installs the agenttij plugin, its Claude Code hook, the sidebar layouts, a
# keybinding to summon the sidebar, and the plugin's permissions.
#
# Safe to re-run: everything it writes is replaced rather than appended to, so
# an upgrade never leaves duplicates behind. Pass --uninstall to remove it all.
#
#   --no-keybind   skip the keybinding
#   --no-grant     skip pre-granting permissions (expect an invisible prompt)

set -eu

repo="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
config_home="${XDG_CONFIG_HOME:-$HOME/.config}"
plugin_dir="$config_home/zellij/plugins"
layout_dir="$config_home/zellij/layouts"
zellij_config="$config_home/zellij/config.kdl"
permissions="${XDG_CACHE_HOME:-$HOME/.cache}/zellij/permissions.kdl"
hook_dir="$HOME/.claude/hooks"
hook_path="$hook_dir/agenttij-state.sh"
wasm="$repo/target/wasm32-wasip1/release/agenttij.wasm"
plugin_url="file:$plugin_dir/agenttij.wasm"

# Overridable so the settings patching can be tested against a copy.
settings="${AGENTTIJ_SETTINGS:-$HOME/.claude/settings.json}"
keybind="${AGENTTIJ_KEYBIND:-Alt a}"
cycle_keybind="${AGENTTIJ_CYCLE_KEYBIND:-Alt v}"
back_keybind="${AGENTTIJ_BACK_KEYBIND:-Alt b}"
focus_keybind="${AGENTTIJ_FOCUS_KEYBIND:-Alt s}"
new_keybind="${AGENTTIJ_NEW_KEYBIND:-Alt g}"
add_keybind="${AGENTTIJ_ADD_KEYBIND:-Alt m}"
# Alt t because Zellij's defaults already claim j, k, l, n, p and the rest of
# the obvious ones — see docs/KEYBINDS.md.
jump_keybind="${AGENTTIJ_JUMP_KEYBIND:-Alt t}"
# The `group` template — the panes every new row starts with — taken from the
# layouts themselves, so it is written in one place and that place is a layout.
#
# The keybinds have to repeat it: a plugin's identity is its url *plus* its
# configuration, so `Alt g` can only reach a sidebar whose configuration it says
# back exactly. That is also why there is one template per machine rather than
# one per layout — the binds are machine-wide, so every layout installed here is
# rewritten to carry the same one, and two layouts asking for different templates
# is a thing this cannot honour.
#
#   AGENTTIJ_GROUP="; nvim ."   overrides whatever the layouts say
#   AGENTTIJ_GROUP=""           installs no template at all
layouts_group="$(sed -n 's/^ *group "\(.*\)"$/\1/p' "$repo"/layouts/*.kdl | sort -u)"
if [ "$(printf '%s\n' "$layouts_group" | grep -c .)" -gt 1 ]; then
    echo "! the layouts disagree on \`group\`, and the keybinds can only match one."
    echo "  set AGENTTIJ_GROUP to the one you want; installing none."
    layouts_group=""
fi
# `-` and not `:-`, so an empty AGENTTIJ_GROUP means "none" rather than "default".
group_template="${AGENTTIJ_GROUP-$layouts_group}"

want_keybind=1
want_grant=1
uninstall=0
for arg in "$@"; do
    case "$arg" in
    --uninstall) uninstall=1 ;;
    --no-keybind) want_keybind=0 ;;
    --no-grant) want_grant=0 ;;
    *) echo "unknown option: $arg" >&2 && exit 2 ;;
    esac
done

need_python() {
    command -v python3 >/dev/null 2>&1 || {
        echo "! python3 not found — $1 skipped, see README"
        return 1
    }
}

register_hooks() {
    need_python "hook registration" || return 0
    if [ -f "$settings" ]; then
        cp "$settings" "$settings.agenttij.bak"
        echo "  backup: $settings.agenttij.bak"
    fi
    python3 "$repo/scripts/register-hooks.py" "$1" "$settings" "$hook_path"
}

set_permissions() {
    need_python "permissions" || return 0
    mkdir -p "$(dirname "$permissions")"
    python3 "$repo/scripts/grant-permissions.py" "$1" "$permissions" \
        "$plugin_dir/agenttij.wasm"
}

# Adds the bindings inside the existing `keybinds` block, then asks Zellij
# whether it still likes the config and rolls back if it does not. Editing
# someone's hand-tuned config without a way out is not acceptable.
add_keybind() {
    [ -f "$zellij_config" ] || {
        echo "  no config.kdl — add the keybinds yourself, see docs/KEYBINDS.md"
        return 0
    }
    need_python "keybinds" || return 0

    cp "$zellij_config" "$zellij_config.agenttij.bak"
    python3 "$repo/scripts/zellij-keybinds.py" install "$zellij_config" \
        "$plugin_url" "$keybind" "$cycle_keybind" "$back_keybind" "$focus_keybind" \
        "$new_keybind" "$add_keybind" "$jump_keybind" "$group_template"

    if zellij --config "$zellij_config" setup --check >/dev/null 2>&1; then
        echo "  bound $keybind, $focus_keybind, $cycle_keybind, $back_keybind,"
        echo "  $new_keybind, $add_keybind, $jump_keybind"
        echo "  (backup: $zellij_config.agenttij.bak)"
    else
        mv "$zellij_config.agenttij.bak" "$zellij_config"
        echo "! config check failed, reverted — add them yourself, see docs/KEYBINDS.md"
    fi
}

remove_keybind() {
    [ -f "$zellij_config" ] || return 0
    grep -q "agenttij:begin" "$zellij_config" || return 0
    need_python "keybind removal" || return 0
    cp "$zellij_config" "$zellij_config.agenttij.bak"
    python3 "$repo/scripts/zellij-keybinds.py" uninstall "$zellij_config" "" "" "" "" "" "" "" "" ""
    echo "  keybinds removed (backup: $zellij_config.agenttij.bak)"
}

if [ "$uninstall" -eq 1 ]; then
    echo "unregistering hooks in $settings"
    register_hooks uninstall
    set_permissions revoke
    remove_keybind
    rm -f "$hook_path" "$plugin_dir/agenttij.wasm" \
        "$layout_dir"/agenttij-*.kdl
    echo "removed."
    exit 0
fi

echo "building..."
cargo build --release --target wasm32-wasip1 --manifest-path "$repo/Cargo.toml" -p agenttij

echo "installing plugin -> $plugin_dir/agenttij.wasm"
mkdir -p "$plugin_dir"
cp "$wasm" "$plugin_dir/agenttij.wasm"

echo "installing layouts -> $layout_dir"
[ -n "$group_template" ] && echo "  every row starts as: $group_template"
mkdir -p "$layout_dir"
# The plugin url, and the group template on both sides of the fence.
#
# A plugin's identity is its url *plus* its configuration, so a keybinding that
# does not say the same `group` as the layout is addressing a sidebar that does
# not exist — and Zellij obliges by starting one, which is a pane appearing out
# of nowhere and every existing pane becoming a row of its own. Rewriting the
# layouts from the same variable that writes the binds is what keeps that from
# happening: with a template both sides carry it, without one neither does.
layout_edit="s|file:~/.config/zellij/plugins/agenttij.wasm|$plugin_url|
/^ *group \"/d"
if [ -n "$group_template" ]; then
    # A real newline, continued with a backslash: `\n` in a replacement is a GNU
    # extension and this is not.
    layout_edit="$layout_edit
s|^\\( *\\)solo \"true\"|\\1solo \"true\"\\
\\1group \"$group_template\"|"
fi
for side in left right workspace everything remote template; do
    sed "$layout_edit" \
        "$repo/layouts/agenttij-$side.kdl" >"$layout_dir/agenttij-$side.kdl"
done

echo "installing hook -> $hook_path"
mkdir -p "$hook_dir"
cp "$repo/hooks/agenttij-state.sh" "$hook_path"
chmod +x "$hook_path"

echo "registering hooks in $settings"
register_hooks install

if [ "$want_grant" -eq 1 ]; then
    echo "granting plugin permissions in $permissions"
    echo "  ReadApplicationState, ChangeApplicationState, RunCommands,"
    echo "  OpenTerminalsOrPlugins"
    set_permissions grant
fi

if [ "$want_keybind" -eq 1 ]; then
    echo "adding keybind to $zellij_config"
    add_keybind
fi

cat <<EOF

done. start a session with the sidebar:

    zellij --new-session-with-layout agenttij-left

the sidebar is a normal pane: focus it as usual, then j/k to move,
Enter to jump to an agent, p to peek at one without leaving this session.

'$keybind' summons a sidebar in any session, '$focus_keybind' focuses the one
you have, '$cycle_keybind' cycles the panes in the row you are on and
'$back_keybind' flips back to the previous row. '$new_keybind' starts a new row
and '$add_keybind' adds a pane to the row on screen. Zellij's own swap-layout key
('Alt ]' by default) folds the sidebar to a status rail.

To give every new session a sidebar, set 'default_layout "agenttij-left"' in
your Zellij config.
EOF
