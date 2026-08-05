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

# Appends a marker-delimited block, then asks Zellij whether it still likes the
# config and rolls back if it does not. Editing someone's hand-tuned config
# without a way out is not acceptable.
add_keybind() {
    [ -f "$zellij_config" ] || {
        echo "  no config.kdl — add the keybind yourself, see README"
        return 0
    }
    if grep -q "agenttij:begin" "$zellij_config"; then
        echo "  keybind already present"
        return 0
    fi

    cp "$zellij_config" "$zellij_config.agenttij.bak"
    cat >>"$zellij_config" <<EOF

// agenttij:begin — summon the sidebar in any session (safe to delete)
keybinds {
    shared_except "locked" {
        bind "$keybind" {
            LaunchOrFocusPlugin "$plugin_url" {
                floating true
                move_to_focused_tab true
            }
        }
    }
}
// agenttij:end
EOF

    if zellij --config "$zellij_config" setup --check >/dev/null 2>&1; then
        echo "  bound '$keybind' (backup: $zellij_config.agenttij.bak)"
    else
        mv "$zellij_config.agenttij.bak" "$zellij_config"
        echo "! config check failed, reverted — add the keybind yourself, see README"
    fi
}

remove_keybind() {
    [ -f "$zellij_config" ] || return 0
    grep -q "agenttij:begin" "$zellij_config" || return 0
    cp "$zellij_config" "$zellij_config.agenttij.bak"
    sed -i '/agenttij:begin/,/agenttij:end/d' "$zellij_config"
    echo "  keybind removed (backup: $zellij_config.agenttij.bak)"
}

if [ "$uninstall" -eq 1 ]; then
    echo "unregistering hooks in $settings"
    register_hooks uninstall
    set_permissions revoke
    remove_keybind
    rm -f "$hook_path" "$plugin_dir/agenttij.wasm" \
        "$layout_dir/agenttij-left.kdl" "$layout_dir/agenttij-right.kdl"
    echo "removed."
    exit 0
fi

echo "building..."
cargo build --release --target wasm32-wasip1 --manifest-path "$repo/Cargo.toml" -p agenttij

echo "installing plugin -> $plugin_dir/agenttij.wasm"
mkdir -p "$plugin_dir"
cp "$wasm" "$plugin_dir/agenttij.wasm"

echo "installing layouts -> $layout_dir"
mkdir -p "$layout_dir"
for side in left right; do
    sed "s|file:~/.config/zellij/plugins/agenttij.wasm|$plugin_url|" \
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
    echo "  ReadApplicationState, ChangeApplicationState, RunCommands"
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

'$keybind' summons the sidebar in any session — including one you jumped
into that has no sidebar of its own. To give every new session one, set
'default_layout "agenttij-left"' in your Zellij config.
EOF
