#!/bin/sh
# agenttij: report this pane's agent state to the sidebar.
# usage: agenttij-state.sh <idle|running|needs-input|done|gone>
#
# Never fails loudly: a broken hook must not disturb the agent it reports on.

s="${1:-}"
cat >/dev/null 2>&1 || true # drain hook stdin so the agent never blocks on us

[ -n "$s" ] || exit 0
[ -n "${ZELLIJ_SESSION_NAME:-}" ] || exit 0
[ -n "${ZELLIJ_PANE_ID:-}" ] || exit 0

d=/tmp/agenttij
f="$d/$ZELLIJ_SESSION_NAME.$ZELLIJ_PANE_ID.state"

[ "$s" = gone ] && { rm -f "$f"; exit 0; }

mkdir -p "$d" 2>/dev/null || exit 0
printf '%s\t%s\t%s\t%s\t%s\n' \
    "$s" "$ZELLIJ_SESSION_NAME" "$ZELLIJ_PANE_ID" "$(date +%s)" \
    "${CLAUDE_PROJECT_DIR:-$PWD}" >"$f" 2>/dev/null

exit 0
