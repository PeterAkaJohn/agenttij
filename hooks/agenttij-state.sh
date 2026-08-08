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

w="${CLAUDE_PROJECT_DIR:-$PWD}"

# The project this belongs to, resolved here because the sidebar cannot: it
# would need a git call per row per tick, and it redraws every second.
#
# A `.agenttij` file above us wins: two repositories that are one project — a
# front end and a back end — have no shared git root to be found, so this is the
# only way the filesystem can say they belong together. Its first line is the
# name; an empty file means "named after this directory". Walking up uses shell
# expansion rather than `dirname` so it costs no processes.
r=""
p="$w"
while [ -n "$p" ]; do
    if [ -f "$p/.agenttij" ]; then
        IFS= read -r r <"$p/.agenttij" || r=""
        r=$(printf '%s' "$r" | tr -d '[:cntrl:]')
        [ -n "$r" ] || r=${p##*/}
        break
    fi
    p=${p%/*}
done
[ -n "$r" ] || r=$(git -C "$w" rev-parse --show-toplevel 2>/dev/null) || r="$w"

mkdir -p "$d" 2>/dev/null || exit 0
printf '%s\t%s\t%s\t%s\t%s\t%s\n' \
    "$s" "$ZELLIJ_SESSION_NAME" "$ZELLIJ_PANE_ID" "$(date +%s)" \
    "$w" "$r" >"$f" 2>/dev/null

exit 0
