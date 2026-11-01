#!/bin/sh
# Drives a throwaway Zellij session with *real* keystrokes and logs what happens.
#
#   scripts/press-keys.sh <session> <layout> "<delay>:<keys>"...
#   scripts/press-keys.sh peektest layouts/x.kdl 8:'\033h' 2:p 4:x
#
# Keys go into the client's stdin, which is the only way to exercise anything
# input-related: `zellij action send-keys` bypasses keybind resolution, and
# `write-chars` does not reach command panes at all — a real keypress does reach
# their program's stdin, which is exactly what this proves and what nothing else
# can.
# Focus has to be moved by the keystream too — an external `focus-pane-id` gets
# overridden by whatever the layout does on startup. `\033h` is Alt+h, Zellij's
# default "focus left".
#
# Prints a second-by-second timeline of panes and focus, so you can see what a
# key did rather than guessing.
#
# Global keys (`Alt v`, `Alt m`, ...) come from your config and only reach a
# sidebar whose configuration they repeat, `group` included - aimed at a layout
# that says something else, they start a second sidebar. To test them without
# your template, point ZELLIJ_CONFIG_FILE at a copy rebound by
# zellij-keybinds.py with the layout's own `group`.

set -eu

[ $# -ge 3 ] || {
    sed -n '2,23p' "$0"
    exit 2
}

session=$1
layout=$2
shift 2

# The keystream: each argument is a delay in seconds, then the bytes to send.
# `printf %b` expands the escapes, so '\033' is a real Esc.
keystream() {
    for step in "$@"; do
        sleep "${step%%:*}"
        printf '%b' "${step#*:}"
    done
    sleep 4
}

cleanup() {
    pids="$(pgrep -f "^script -qec zellij -s $session" 2>/dev/null || true)
$(pgrep -f "^zellij -s $session" 2>/dev/null || true)"
    # shellcheck disable=SC2086
    [ -n "$(printf '%s' "$pids" | tr -d '[:space:]')" ] && kill $pids 2>/dev/null || true
    sleep 1
    zellij kill-session "$session" >/dev/null 2>&1 || true
}
trap cleanup EXIT INT TERM

echo "keys: $*"
keystream "$@" | script -qec "zellij -s $session -n $layout" /dev/null >/dev/null 2>&1 &

# Timeline. Runs a little past the last key so the effect is visible.
total=0
for step in "$@"; do
    total=$((total + ${step%%:*}))
done
total=$((total + 8))

second=0
while [ "$second" -lt "$total" ]; do
    # A session that is not up yet answers every action with the session list
    # on stdout and exit 0, so anything that is not a reply counts as nothing.
    clients=$(zellij -s "$session" action list-clients 2>/dev/null || true)
    case "$clients" in CLIENT_ID*) ;; *) clients= ;; esac
    focus=$(printf '%s\n' "$clients" | sed -n 2p | awk '{print $2}')
    # Background plugins (`load_plugins`, `zellij:link`) are listed as
    # suppressed plugin panes since 0.45, and nobody sees them.
    read -r panes peeks tab <<EOF
$(zellij -s "$session" action list-panes -j 2>/dev/null | python3 -c '
import json, sys
try:
    listed = json.load(sys.stdin)
except ValueError:
    print("- 0 -")
    sys.exit()
seen = [p for p in listed if not (p["is_plugin"] and p["is_suppressed"])]
peeks = sum(p["title"].startswith("peek ") for p in seen)
kind, _, number = sys.argv[1].partition("_")
tab = next((p["tab_id"] for p in listed
            if str(p["id"]) == number and p["is_plugin"] == (kind == "plugin")), "-")
print(len(seen), peeks, tab)
' "${focus:-none}")
EOF
    # Existence is not visibility: Zellij hides floating panes when a tiled pane
    # takes focus, so a pane can be listed and still be off screen. Since 0.45
    # this needs the tab (without one it answers "Tab not found"), and says
    # "false" on stderr with exit 2, whatever its --help claims.
    floating=-
    if [ "$tab" != - ]; then
        answer=$(zellij -s "$session" action are-floating-panes-visible -t "$tab" 2>&1 || true)
        case $answer in true | false) floating=$answer ;; esac
    fi
    printf '%3ds  panes=%-3s peeks=%-3s visible=%-6s focus=%s\n' \
        "$second" "$panes" "$peeks" "$floating" "${focus:--}"
    sleep 1
    second=$((second + 1))
done
