#!/bin/sh
# A machine to watch, without a second machine: a container running sshd and a
# zellij session with two panes, each with a state file so the sidebar has
# something to show.
#
#   scripts/testbox.sh up             # build, start, print the host string
#   scripts/testbox.sh up --sidebar   # ... with a full sidebar running in it
#   scripts/testbox.sh up --controller # ... with a hidden controller only
#   scripts/testbox.sh down           # remove it
#
# With `--sidebar` the box runs the plugin as well, which is what makes adding a
# pane over there use suppressed panes rather than a stack: the sidebar in that
# session answers the pipe and does it the way it would locally. Without it, the
# fallback is what you see.
#
# Add the printed host in the sidebar with `h`. Your own public keys are copied
# in, so the ssh the plugin runs — BatchMode, no password — just works.

set -eu
name=agenttij-testbox
image=agenttij-testbox

case "${1:-up}" in
up)
    sidebar=""
    controller=""
    [ "${2:-}" = "--sidebar" ] && sidebar=yes
    [ "${2:-}" = "--controller" ] && { sidebar=yes; controller=yes; }
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    cat ~/.ssh/*.pub >"$work/key.pub" 2>/dev/null || {
        echo "no public key in ~/.ssh — ssh-keygen first" >&2
        exit 1
    }

    cat >"$work/Dockerfile" <<'EOF'
FROM alpine:3.20
RUN apk add --no-cache openssh-server ca-certificates && ssh-keygen -A
# A real zellij, so Enter on one of its rows attaches to something.
ARG ZELLIJ=v0.44.3
RUN wget -qO- "https://github.com/zellij-org/zellij/releases/download/$ZELLIJ/zellij-x86_64-unknown-linux-musl.tar.gz" \
    | tar xz -C /usr/local/bin && chmod +x /usr/local/bin/zellij
# Unlocked, or sshd refuses the key: adduser leaves the account with a locked
# password, and public-key auth is denied for locked accounts.
RUN adduser -D -s /bin/sh dev && passwd -u dev
# /srv/api and /srv/web are the projects its state files claim, so a pane opened
# there by the sidebar has somewhere to land.
RUN mkdir -p /home/dev/.ssh /tmp/agenttij /home/dev/.config/zellij /srv/api /srv/web
# Without a config, zellij greets the session with its first-run wizard, which
# is the first thing you would see on attaching.
RUN printf 'show_startup_tips false\nshow_release_notes false\n' \
    >/home/dev/.config/zellij/config.kdl
COPY key.pub /home/dev/.ssh/authorized_keys
COPY box.kdl /home/dev/box.kdl
COPY start.sh /start.sh
RUN chown -R dev:dev /home/dev /tmp/agenttij && chmod 700 /home/dev/.ssh \
 && chmod 600 /home/dev/.ssh/authorized_keys && chmod +x /start.sh
CMD ["/start.sh"]
EOF

    cat >"$work/box.kdl" <<'EOF'
layout {
    pane command="sh" {
        args "-c" "echo 'api: may I write to src/main.rs? (y/n)'; exec sleep 100000"
    }
    pane command="sh" {
        args "-c" "echo 'web: building...'; exec sleep 100000"
    }
}
EOF

    if [ -n "$sidebar" ]; then
        wasm=target/wasm32-wasip1/release/agenttij.wasm
        [ -f "$wasm" ] || {
            echo "build it first: cargo build -p agenttij --target wasm32-wasip1 --release" >&2
            exit 1
        }
        cp "$wasm" "$work/agenttij.wasm"
        if [ -n "$controller" ]; then
            # Two panes and a controller, which takes itself off the screen: what
            # a dev box would run. It has to be in the layout — `launch-plugin`
            # is refused for a session with no client attached ("No connected
            # clients found"), and a background session has none.
            cat >"$work/box.kdl" <<'EOF'
layout {
    pane command="sh" {
        args "-c" "echo 'api: may I write to src/main.rs? (y/n)'; exec sleep 100000"
    }
    pane command="sh" {
        args "-c" "echo 'web: building...'; exec sleep 100000"
    }
    // Floating, and it can never take focus — so it is never on screen: Zellij
    // shows floating panes only while one of them is focused.
    floating_panes {
        pane {
            plugin location="file:/home/dev/.config/zellij/plugins/agenttij.wasm" {
                remote "true"
            }
        }
    }
}
EOF
        else
            # Its own solo workspace, the same shape as agenttij-workspace.kdl.
            cat >"$work/box.kdl" <<'EOF'
layout {
    pane split_direction="vertical" {
        pane size="20%" {
            plugin location="file:/home/dev/.config/zellij/plugins/agenttij.wasm" {
                scope "session"
                solo "true"
            }
        }
        pane command="sh" {
            args "-c" "echo 'api: may I write to src/main.rs? (y/n)'; exec sleep 100000"
        }
    }
}
EOF
        fi
        # Pre-granted, or the plugin sits on a prompt nobody can answer.
        cat >"$work/permissions.kdl" <<'EOF'
"/home/dev/.config/zellij/plugins/agenttij.wasm" {
    ReadApplicationState
    ChangeApplicationState
    RunCommands
    OpenTerminalsOrPlugins
    WriteToStdin
}
EOF
        cat >>"$work/Dockerfile" <<'EOF'
RUN mkdir -p /home/dev/.config/zellij/plugins /home/dev/.cache/zellij
COPY agenttij.wasm /home/dev/.config/zellij/plugins/agenttij.wasm
COPY permissions.kdl /home/dev/.cache/zellij/permissions.kdl
RUN chown -R dev:dev /home/dev
EOF
    fi

    cat >"$work/start.sh" <<'EOF'
#!/bin/sh
# sshd, plus a zellij session called `box` with two panes, plus a state file per
# pane — so the sidebar has rows, `p` has a pane to dump, and `Enter` has a
# session to attach to.
set -eu
/usr/sbin/sshd -e

su dev -c 'HOME=/home/dev TERM=xterm-256color \
    zellij --layout /home/dev/box.kdl attach box --create-background' || true
sleep 2

panes=$(su dev -c 'HOME=/home/dev zellij -s box action list-panes' 2>/dev/null |
    awk '$1 ~ /^terminal_/ { sub(/terminal_/, "", $1); print $1 }' | sort -n)
first=$(echo "$panes" | sed -n 1p)
second=$(echo "$panes" | sed -n 2p)

# Written against the panes that actually exist, so a peek reads the pane its
# row claims to be about.
[ -n "$first" ] && printf 'needs-input\tbox\t%s\t%s\t/srv/api\t/srv/api\n' \
    "$first" "$(date +%s)" >/tmp/agenttij/box.$first.state
[ -n "$second" ] && printf 'running\tbox\t%s\t%s\t/srv/web\t/srv/web\n' \
    "$second" "$(date +%s)" >/tmp/agenttij/box.$second.state

tail -f /dev/null
EOF

    [ -n "$controller" ] && printf 'yes\n' >"$work/controller" || : >"$work/controller"
    cat >>"$work/Dockerfile" <<'EOF'
COPY controller /home/dev/.controller.maybe
RUN if [ -s /home/dev/.controller.maybe ]; then mv /home/dev/.controller.maybe /home/dev/.controller;     else rm -f /home/dev/.controller.maybe; fi
EOF
    docker build -q -t "$image" "$work" >/dev/null
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" "$image" >/dev/null
    sleep 4
    ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' "$name")
    # Without this the plugin's BatchMode ssh fails on an unknown host key.
    ssh-keyscan -H "$ip" 2>/dev/null >>~/.ssh/known_hosts
    echo "watching host: dev@$ip"
    echo "press h in the sidebar and type it"
    ;;
down)
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker rmi "$image" >/dev/null 2>&1 || true
    echo "gone (its host key is still in ~/.ssh/known_hosts)"
    ;;
*)
    sed -n '2,11p' "$0"
    exit 2
    ;;
esac
