#!/bin/sh
# A machine to watch, without a second machine: a container running sshd with
# two agents' worth of state files in it.
#
#   scripts/testbox.sh up      # build, start, print the host string
#   scripts/testbox.sh down    # remove it
#
# Add the printed host in the sidebar with `h`. Your own public keys are copied
# in, so the ssh the plugin runs — BatchMode, no password — just works.

set -eu
name=agenttij-testbox
image=agenttij-testbox

case "${1:-up}" in
up)
    work=$(mktemp -d)
    trap 'rm -rf "$work"' EXIT
    cat ~/.ssh/*.pub >"$work/key.pub" 2>/dev/null || {
        echo "no public key in ~/.ssh — ssh-keygen first" >&2
        exit 1
    }
    cat >"$work/Dockerfile" <<'EOF'
FROM alpine:3.20
RUN apk add --no-cache openssh-server && ssh-keygen -A
# Unlocked, or sshd refuses the key: adduser leaves the account with a locked
# password and public-key auth is denied for locked accounts.
RUN adduser -D -s /bin/sh dev && passwd -u dev
RUN mkdir -p /home/dev/.ssh /tmp/agenttij
COPY key.pub /home/dev/.ssh/authorized_keys
RUN chown -R dev:dev /home/dev/.ssh && chmod 700 /home/dev/.ssh \
 && chmod 600 /home/dev/.ssh/authorized_keys
# Two agents: one blocked, one working, in two different projects.
CMD sh -c 'printf "needs-input\tbox\t3\t$(date +%s)\t/srv/api\t/srv/api\n" >/tmp/agenttij/box.3.state; \
           printf "running\tbox\t5\t$(date +%s)\t/srv/web\t/srv/web\n" >/tmp/agenttij/box.5.state; \
           /usr/sbin/sshd -D -e'
EOF
    docker build -q -t "$image" "$work" >/dev/null
    docker rm -f "$name" >/dev/null 2>&1 || true
    docker run -d --name "$name" "$image" >/dev/null
    sleep 2
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
    sed -n '2,10p' "$0"
    exit 2
    ;;
esac
