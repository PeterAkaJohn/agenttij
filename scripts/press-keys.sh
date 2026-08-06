#!/bin/sh
# Drives a throwaway Zellij session with *real* keystrokes and logs what happens.
#
#   scripts/press-keys.sh <session> <layout> "<delay>:<keys>"...
#   scripts/press-keys.sh peektest layouts/x.kdl 8:'\033h' 2:p 4:x
#
# Keys go into the client's stdin, which is the only way to exercise anything
# input-related: `zellij action send-keys` bypasses keybind resolution, and
# `write-chars` does not reach command panes at all (their stdin is /dev/null).
# Focus has to be moved by the keystream too — an external `focus-pane-id` gets
# overridden by whatever the layout does on startup. `\033h` is Alt+h, Zellij's
# default "focus left".
#
# Prints a second-by-second timeline of panes and focus, so you can see what a
# key did rather than guessing.

set -eu

[ $# -ge 3 ] || {
    sed -n '2,17p' "$0"
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
    panes=$(zellij -s "$session" action list-panes 2>/dev/null | tail -n +2 | wc -l | tr -d ' ')
    peeks=$(zellij -s "$session" action list-panes 2>/dev/null | grep -ac 'peek ' || true)
    focus=$(zellij -s "$session" action list-clients 2>/dev/null | tail -n +2 | head -1 | awk '{print $2}')
    # Existence is not visibility: Zellij hides floating panes when a tiled pane
    # takes focus, so a pane can be listed and still be off screen.
    floating=$(zellij -s "$session" action are-floating-panes-visible 2>/dev/null | tr -d '[:space:]')
    printf '%3ds  panes=%-3s peeks=%-3s visible=%-6s focus=%s\n' \
        "$second" "${panes:-–}" "${peeks:-0}" "${floating:-–}" "${focus:-–}"
    sleep 1
    second=$((second + 1))
done
