#!/bin/sh
# Installs into a throwaway home, checks everything that should have landed,
# uninstalls, and checks it is all gone. Nothing of yours is touched: $HOME and
# the XDG directories point into a temporary directory for the whole run.
#
#   scripts/check-install.sh

set -eu

repo="$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT

# Cargo and rustup live in the real home; everything else in the fake one.
export CARGO_HOME="${CARGO_HOME:-$HOME/.cargo}"
export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
export HOME="$work/home"
export XDG_CONFIG_HOME="$HOME/.config"
export XDG_CACHE_HOME="$HOME/.cache"
unset AGENTTIJ_SETTINGS AGENTTIJ_GROUP AGENTTIJ_BIN_DIR 2>/dev/null || true

config="$XDG_CONFIG_HOME/zellij/config.kdl"
plugin="$XDG_CONFIG_HOME/zellij/plugins/agenttij.wasm"
settings="$HOME/.claude/settings.json"
permissions="$XDG_CACHE_HOME/zellij/permissions.kdl"
mkdir -p "$(dirname "$config")" "$(dirname "$settings")"
zellij setup --dump-config >"$config"
printf '{}\n' >"$settings"

failed=0
check() {
    if eval "$2"; then
        echo "  ok    $1"
    else
        echo "  FAIL  $1"
        failed=1
    fi
}

echo "install"
sh "$repo/scripts/install.sh" >"$work/install.out" 2>&1 || {
    cat "$work/install.out"
    exit 1
}
check "the plugin" '[ -f "$plugin" ]'
check "the command, executable" '[ -x "$HOME/.local/bin/agenttij" ]'
check "the command runs" '"$HOME/.local/bin/agenttij" --help 2>&1 | grep -q "group new"'
check "the hook, executable" '[ -x "$HOME/.claude/hooks/agenttij-state.sh" ]'
check "every layout" '[ "$(ls "$XDG_CONFIG_HOME/zellij/layouts"/agenttij-*.kdl | wc -l)" = "$(ls "$repo"/layouts/agenttij-*.kdl | wc -l)" ]'
check "layouts point at the installed plugin" '! grep -q "file:~/" "$XDG_CONFIG_HOME"/zellij/layouts/agenttij-*.kdl'
for state in idle running needs-input done gone; do
    check "hook registered for $state" 'grep -q "agenttij-state.sh'"'"' $state" "$settings"'
done
for permission in ReadApplicationState ChangeApplicationState RunCommands OpenTerminalsOrPlugins WriteToStdin; do
    check "granted $permission" 'grep -A6 "^\"$plugin\"" "$permissions" | grep -q "$permission"'
done
check "keybinds inside the first keybinds block" 'grep -q "agenttij:begin" "$config"'
check "Alt w bound" 'grep -q "bind \"Alt w\"" "$config"'
check "Alt ? bound" 'grep -q "bind \"Alt ?\"" "$config"'
check "the config still parses" 'zellij --config "$config" setup --check 2>&1 | grep -q "Well defined"'

echo "install again"
sh "$repo/scripts/install.sh" >"$work/again.out" 2>&1
check "keybinds written once" '[ "$(grep -c "agenttij:begin" "$config")" = 1 ]'
check "hooks registered once" '[ "$(grep -c "agenttij-state.sh'"'"' done" "$settings")" = 1 ]'

echo "install with Claude Code's plugin installed"
mkdir -p "$work/bin"
cat >"$work/bin/claude" <<'EOF'
#!/bin/sh
echo '[{"id":"agenttij@agenttij","scope":"user","enabled":true}]'
EOF
chmod +x "$work/bin/claude"
PATH="$work/bin:$PATH" sh "$repo/scripts/install.sh" >"$work/plugin.out" 2>&1
check "no hooks of its own: the plugin's run instead" '! grep -q agenttij-state.sh "$settings"'
check "the hook script, for other tools" '[ -x "$HOME/.claude/hooks/agenttij-state.sh" ]'

echo "uninstall"
sh "$repo/scripts/install.sh" --uninstall >"$work/uninstall.out" 2>&1
check "the plugin is gone" '[ ! -e "$plugin" ]'
check "the command is gone" '[ ! -e "$HOME/.local/bin/agenttij" ]'
check "the hook is gone" '[ ! -e "$HOME/.claude/hooks/agenttij-state.sh" ]'
check "the layouts are gone" '! ls "$XDG_CONFIG_HOME"/zellij/layouts/agenttij-*.kdl >/dev/null 2>&1'
check "the keybinds are gone" '! grep -q "agenttij:begin" "$config"'
check "the hooks are unregistered" '! grep -q agenttij-state.sh "$settings"'
check "the grant is revoked" '! grep -q "^\"$plugin\"" "$permissions"'

exit "$failed"
