#!/bin/sh
# Installs the agenttij plugin, its Claude Code hook, and the sidebar layouts.
#
# Safe to re-run: hook registration is replaced rather than appended, so an
# upgrade never leaves duplicates behind. Pass --uninstall to remove.

set -eu

repo="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
plugin_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zellij/plugins"
layout_dir="${XDG_CONFIG_HOME:-$HOME/.config}/zellij/layouts"
hook_dir="$HOME/.claude/hooks"
hook_path="$hook_dir/agenttij-state.sh"
wasm="$repo/target/wasm32-wasip1/release/agenttij.wasm"

# Overridable so the hook registration can be tested against a copy.
settings="${AGENTTIJ_SETTINGS:-$HOME/.claude/settings.json}"

register() {
    # $1 = install | uninstall
    if ! command -v python3 >/dev/null 2>&1; then
        echo "! python3 not found — register the hooks yourself, see README"
        return 0
    fi
    if [ -f "$settings" ]; then
        cp "$settings" "$settings.agenttij.bak"
        echo "  backup: $settings.agenttij.bak"
    fi
    python3 "$repo/scripts/register-hooks.py" "$1" "$settings" "$hook_path"
}

if [ "${1:-}" = "--uninstall" ]; then
    echo "unregistering hooks in $settings"
    register uninstall
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
    sed "s|file:~/.config/zellij/plugins/agenttij.wasm|file:$plugin_dir/agenttij.wasm|" \
        "$repo/layouts/agenttij-$side.kdl" >"$layout_dir/agenttij-$side.kdl"
done

echo "installing hook -> $hook_path"
mkdir -p "$hook_dir"
cp "$repo/hooks/agenttij-state.sh" "$hook_path"
chmod +x "$hook_path"

echo "registering hooks in $settings"
register install

cat <<EOF

done. start a session with the sidebar:

    zellij --new-session-with-layout agenttij-left

the sidebar is a normal pane: focus it as usual, then j/k to move,
Enter to jump to an agent, p to peek at one without leaving this session.
EOF
