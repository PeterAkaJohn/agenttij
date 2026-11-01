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

[ "$s" = gone ] && { rm -f "$f" "${f%.state}.seen"; exit 0; }

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

# What the agent left uncommitted, counted once when it finishes: the one moment
# the number means "this is what there is to review". New files count too - they
# are added, intent-to-add only, to a copy of the index, so the real one is never
# touched. Outside a repository, or with git before 2.31 (`--path-format`), there
# is no number.
c=""
if [ "$s" = done ] &&
    i=$(git -C "$w" rev-parse --path-format=absolute --git-path index 2>/dev/null) &&
    t=$(mktemp 2>/dev/null); then
    cp "$i" "$t" 2>/dev/null
    GIT_INDEX_FILE=$t git -C "$w" add -N :/ 2>/dev/null
    c=$(GIT_INDEX_FILE=$t git -C "$w" diff HEAD --shortstat 2>/dev/null | awk '
        { for (n = 2; n <= NF; n++) {
            if ($n ~ /^insertion/) a = $(n - 1)
            if ($n ~ /^deletion/) e = $(n - 1)
        } }
        END { if (a + e) printf "+%d-%d", a, e }')
    rm -f "$t"
fi

# The seventh field is left empty: it was the host, and a plugin from before
# would take anything there for one.
mkdir -p "$d" 2>/dev/null || exit 0
printf '%s\t%s\t%s\t%s\t%s\t%s\t\t%s\n' \
    "$s" "$ZELLIJ_SESSION_NAME" "$ZELLIJ_PANE_ID" "$(date +%s)" \
    "$w" "$r" "$c" >"$f" 2>/dev/null

exit 0
