#!/usr/bin/env python3
"""Drives throwaway Zellij sessions with real keystrokes, to test the plugin.

    harness.py start <session> <layout> [--group G] [--size 160x40] [--wasm W]
                                            (W defaults to $AGENTTIJ_WASM, or the build)
    harness.py keys <session> <bytes>...    '\\033h' or '\\eh' is Alt+h, '\\r' Enter
    harness.py state <session>
    harness.py expect <session> <check>...
    harness.py wait <session> <check>... [--timeout 10]
    harness.py reload <session>
    harness.py report <session> <pane> <status> [--cwd DIR]   what the hook says
    harness.py sh <session> <script>        runs in the session's directory
    harness.py pipe <session> <name> [payload]  `zellij pipe`, to no plugin in particular
    harness.py stop <session> [--keep]
    harness.py timeline <session> <layout> <delay>:<bytes>... [--group G]
    harness.py scenario <file>...

Keys go into a real client's stdin: `zellij action send-keys` bypasses keybind
resolution and `write-chars` never reaches a command pane, so nothing else tests
what a key does. The client holds its own end of the key pipe, so it never reads
end-of-file between keys - running out of keys used to take a pane with it.

Each session is sealed off: Zellij's default config with `sh` for a shell, no
background plugins and agenttij's binds carrying the layout's own `group`, plus
its own cache (permissions, resurrection, agenttij's files), log and sockets. So
`zellij ls` never shows it, nothing it writes reaches yours, and nothing here can
address a session of yours. It runs the plugin you built, not the one installed.

A check is <key>=<glob> or <key>!=<glob>:
    focus                          the focused pane
    visible                        tiled terminals on screen, comma separated
    floating                       floating pane titles, comma separated
    layer                          the floating layer: shown or hidden
    panes terminals parked procs   counts; procs is the session's processes
    running                        whether a process under the session matches
    title.P where.P width.P        where is tiled, parked, floating or gone
    read.P                         yes once a look covers P's last report
    drawn.P                        a line P has on screen (`!=`: none does);
                                   the sidebar's cursor is the `›` in column one
A pane P is terminal_N, plugin_N, `sidebar`, `slot` (the one terminal shown), or
`floating` (the first floating pane: a peek, the palette).

A scenario file is these commands one per line, without the session, starting
with `start`; `sleep <seconds>` waits, `$VARIABLES` are expanded, and
`needs VAR...` skips the scenario unless they are set; `$AGENTTIJ_SESSION_DIR`
is the scenario's own directory. The session is stopped at the end either
way, and a run the client broke is run once more before it counts as a failure.
"""

import argparse
import codecs
import fnmatch
import json
import os
import re
import shlex
import shutil
import signal
import subprocess
import sys
import time
import unicodedata
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
WASM = os.environ.get("AGENTTIJ_WASM", REPO / "target/wasm32-wasip1/release/agenttij.wasm")
# Not under /tmp: Zellij rewrites any path a plugin hands it that starts with
# /tmp into its own temporary directory (`translate_plugin_path`), so a row
# opened in a scenario's repository would open somewhere else.
ROOT = Path("/var/tmp/agenttij-harness")
# Where the hook and the plugin keep state, fixed in both: shared with the
# user's own agents, so a session cleans out exactly its own files on the way out.
STATE = Path("/tmp/agenttij")
# install.sh's defaults: summon, cycle, back, focus, new, add, jump.
BINDS = ("Alt a", "Alt v", "Alt b", "Alt s", "Alt g", "Alt m", "Alt t")
# What a headless client leaves in the log when it, not the plugin, broke a run.
CLIENT_TROUBLE = (
    "Input/output error",
    "Failed to apply cached resizes",
    "consecutive unknown messages",
)
# Logged by Zellij itself on ordinary operations.
NOISE = ("Vertical relayout of tab failed", "did not complete within 1s timeout")


class Failed(Exception):
    pass


def home(session):
    return ROOT / session


def env(session):
    d = home(session)
    clean = {k: v for k, v in os.environ.items() if not k.startswith("ZELLIJ")}
    clean.update(
        ZELLIJ_CONFIG_FILE=str(d / "config.kdl"),
        # Not only tidier: with no Zellij config directory in $HOME, as on a CI
        # runner, `-n <layout>` came up with the default layout instead, config
        # file or not. Measured on 0.45.1 with an empty $HOME.
        ZELLIJ_CONFIG_DIR=str(d),
        ZELLIJ_SOCKET_DIR=str(d / "sock"),
        XDG_CACHE_HOME=str(d / "cache"),
        TMPDIR=str(d / "tmp"),
        TERM="xterm-256color",
    )
    clean.setdefault("LANG", "C.UTF-8")
    return clean


def action(session, *args):
    return subprocess.run(
        ["zellij", "-s", session, "action", *args],
        env=env(session),
        capture_output=True,
        text=True,
    )


def meta(session):
    try:
        return json.loads((home(session) / "meta.json").read_text())
    except FileNotFoundError:
        raise Failed(f"{session} is not running here")


# --- state ---------------------------------------------------------------


def processes():
    table = {}
    for entry in Path("/proc").iterdir():
        if not entry.name.isdigit():
            continue
        try:
            ppid = int((entry / "stat").read_text().rsplit(")", 1)[1].split()[1])
            cmd = (entry / "cmdline").read_bytes().replace(b"\0", b" ").decode(errors="replace")
        except (OSError, IndexError, ValueError):
            continue
        table[int(entry.name)] = (ppid, cmd.strip())
    return table


def snapshot(session):
    """Everything a check can ask about, or None when the session is gone."""
    url = meta(session)["url"]
    try:
        listed = json.loads(action(session, "list-panes", "-j", "-a").stdout)
    except ValueError:
        return None

    panes = []
    for p in sorted(listed, key=lambda p: (not p["is_plugin"], p["id"])):
        if p["is_plugin"] and p["is_suppressed"]:
            continue  # a background plugin, which nobody sees
        where = "floating" if p["is_floating"] else "parked" if p["is_suppressed"] else "tiled"
        panes.append(
            {
                "id": f"{'plugin' if p['is_plugin'] else 'terminal'}_{p['id']}",
                "where": where,
                "cols": p["pane_columns"],
                "rows": p["pane_rows"],
                "title": p["title"],
                "tab": p["tab_id"],
                "ours": p["is_plugin"] and p.get("plugin_url") == url,
            }
        )

    clients = action(session, "list-clients").stdout.splitlines()
    focus = clients[1].split()[1] if len(clients) > 1 else "-"
    tab = next((p["tab"] for p in panes if p["id"] == focus), None)
    layer = "-"
    if tab is not None:
        answer = action(session, "are-floating-panes-visible", "-t", str(tab))
        # "false" arrives on stderr with exit 2, whatever --help says.
        layer = {"true": "shown", "false": "hidden"}.get((answer.stdout + answer.stderr).strip(), "-")

    table = processes()
    sock = f"{home(session)}/sock/"
    server = next((pid for pid, (_, cmd) in table.items() if "--server" in cmd and sock in cmd), None)
    children = [pid for pid, (ppid, _) in table.items() if ppid == server]
    under, frontier = [], list(children)
    while frontier:
        pid = frontier.pop()
        under.append(table[pid][1])
        frontier += [child for child, (ppid, _) in table.items() if ppid == pid]

    shown = [p["id"] for p in panes if p["where"] == "tiled" and p["id"].startswith("terminal")]
    return {
        "session": session,
        "panes": panes,
        "focus": focus,
        "layer": layer,
        "procs": len(children),
        "running": under,
        "visible": shown,
        "slot": shown[0] if len(shown) == 1 else None,
        "floating": next((p["id"] for p in panes if p["where"] == "floating"), None),
        "sidebar": next((p["id"] for p in panes if p["ours"] and p["where"] == "tiled"), None),
    }


def read(session, pane):
    """Whether a look the sidebar wrote down covers the pane's last report."""
    number = (pane or "").rpartition("_")[2]

    def field(suffix, index):
        try:
            line = (STATE / f"{session}.{number}.{suffix}").read_text()
            return int(line.removeprefix("seen=").split("\t")[index])
        except (OSError, IndexError, ValueError):
            return None

    reported, seen = field("state", 3), field("seen", 2)
    return reported is not None and seen is not None and seen >= reported


def report(a):
    """Runs the real hook as an agent in that pane would."""
    hook_env = {k: v for k, v in os.environ.items() if k not in ("PWD", "CLAUDE_PROJECT_DIR")}
    hook_env.update(ZELLIJ_SESSION_NAME=a.session, ZELLIJ_PANE_ID=a.pane.rpartition("_")[2])
    subprocess.run(["sh", str(REPO / "hooks/agenttij-state.sh"), a.status], env=hook_env,
                   cwd=home(a.session) / a.cwd, stdin=subprocess.DEVNULL, check=True)


def action_pipe(session, name, payload):
    """What a machine watching this one sends: a pipe that names no plugin."""
    subprocess.run(["zellij", "-s", session, "pipe", "--name", name, *([payload] if payload else [])],
                   env=env(session), stdin=subprocess.DEVNULL, capture_output=True, timeout=10)


def shell(a):
    """A script for setting a scene - a repository to work in, say - kept in the
    session's directory so it goes when the session does."""
    subprocess.run(["sh", "-c", a.script], cwd=home(a.session), check=True)


def show(state):
    if state is None:
        print("  (no session)")
        return
    print(f"  focus {state['focus']}  floating layer {state['layer']}  procs {state['procs']}")
    for p in state["panes"]:
        mark = "*" if p["id"] == state["focus"] else " "
        role = "sidebar" if p["id"] == state["sidebar"] else "slot" if p["id"] == state["slot"] else ""
        size = "-" if p["where"] == "parked" else f"{p['cols']}x{p['rows']}"
        print(f"  {mark} {p['id']:<12} {p['where']:<9} {role:<8} {size:<8} {p['title']}")
    if state["sidebar"]:
        print("  the sidebar draws:")
        for number, line in enumerate(drawn(state["session"], state["sidebar"])):
            if line.strip():
                print(f"    {number:>2} {line}")


def value(state, key):
    if state is None:
        return "<no session>"
    counts = {
        "panes": len(state["panes"]),
        "terminals": sum(p["id"].startswith("terminal") for p in state["panes"]),
        "parked": sum(p["where"] == "parked" for p in state["panes"]),
    }
    if key in counts:
        return str(counts[key])
    if key in ("focus", "layer", "procs"):
        return str(state[key])
    if key == "visible":
        return ",".join(state["visible"])
    if key == "floating":
        return ",".join(p["title"] for p in state["panes"] if p["where"] == "floating")
    field, _, pane = key.partition(".")
    if field in ("title", "where", "width", "read") and pane:
        pane = state.get(pane, pane) if pane in ("sidebar", "slot", "floating") else pane
        if field == "read":
            return "yes" if read(state["session"], pane) else "no"
        found = next((p for p in state["panes"] if p["id"] == pane), None)
        if found is None:
            return "gone" if field == "where" else "<gone>"
        return str({"title": found["title"], "where": found["where"], "width": found["cols"]}[field])
    raise Failed(f"no such check: {key}")


def check(state, text):
    """(passed, what was there) for one check."""
    match = re.fullmatch(r"([\w.]+)(!?=)(.*)", text)
    if not match:
        raise Failed(f"not a check: {text}")
    key, op, want = match.groups()
    if key == "running":
        hit = state is not None and any(fnmatch.fnmatchcase(c, want) for c in state["running"])
        return hit == (op == "="), "a match" if hit else "no match"
    if key.startswith("drawn."):
        pane = key.partition(".")[2]
        pane = (state or {}).get(pane) or pane
        lines = drawn(state["session"], pane) if state else []
        hit = any(fnmatch.fnmatchcase(line, want) for line in lines)
        return hit == (op == "="), " | ".join(line for line in lines if line.strip())
    if key == "focus" and want in ("sidebar", "slot") and state is not None:
        want = state[want] or f"<no {want}>"
    got = value(state, key)
    return fnmatch.fnmatchcase(got, want) == (op == "="), got


# --- commands ------------------------------------------------------------


def helper(script, *args):
    subprocess.run([sys.executable, str(REPO / "scripts" / script), *map(str, args)], check=True,
                   stdout=subprocess.DEVNULL)


def start(a):
    d = home(a.session)
    if (d / "meta.json").exists():
        raise Failed(f"{a.session} is already running; stop it first")
    wasm = Path(a.wasm).resolve()
    if not wasm.exists():
        raise Failed(f"no {wasm}: cargo build -p agenttij --target wasm32-wasip1 --release")
    shutil.rmtree(d, ignore_errors=True)
    for sub in ("cache/zellij", "tmp", "sock"):
        (d / sub).mkdir(parents=True)
    url = f"file:{wasm}"

    # The plugin url, and `group` on both sides, as install.sh writes them.
    source = Path(a.layout)
    if not source.exists() and (REPO / source).exists():
        source = REPO / source
    layout = re.sub(r"file:[^\"]*agenttij\.wasm", url, source.read_text())
    if a.group is not None:
        layout = re.sub(r'(?m)^ *group ".*"\n', "", layout)
        if a.group:
            layout = re.sub(r'(?m)^( *)solo "true"$', lambda m: f'{m[0]}\n{m[1]}group "{a.group}"', layout)
    groups = set(re.findall(r'(?m)^ *group "(.*)"$', layout))
    if len(groups) > 1:
        raise Failed("the layout disagrees with itself about `group`")
    (d / "layout.kdl").write_text(layout)

    config = subprocess.run(["zellij", "setup", "--dump-config"], capture_output=True, text=True,
                            check=True).stdout
    config = re.sub(r"(?ms)^load_plugins \{.*?^\}", "load_plugins {\n}", config)
    config += 'default_shell "sh"\nshow_startup_tips false\nshow_release_notes false\n'
    (d / "config.kdl").write_text(config)
    helper("zellij-keybinds.py", "install", d / "config.kdl", url, *BINDS, next(iter(groups), ""))
    helper("grant-permissions.py", "grant", d / "cache/zellij/permissions.kdl", wasm)

    fifo = d / "keys"
    os.mkfifo(fifo)
    reader = os.open(fifo, os.O_RDWR)
    cols, rows = a.size.split("x")
    command = (f"stty cols {cols} rows {rows}; exec zellij -s {shlex.quote(a.session)} "
               f"-n {shlex.quote(str(d / 'layout.kdl'))}")
    client = subprocess.Popen(
        ["script", "-qfc", command, str(d / "client.out")],
        stdin=reader,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
        env=env(a.session),
        start_new_session=True,
    )
    os.close(reader)
    (d / "meta.json").write_text(json.dumps({"client": client.pid, "url": url, "size": a.size}))

    if not settle(a.session, ["where.sidebar=tiled", "title.sidebar!=*.wasm*"], 20, quiet=True):
        show(snapshot(a.session))
        print("  the client said, last:")
        for line in said_by_client(d / "client.out")[-12:]:
            print(f"    {line}")
        stop_session(a.session, keep=True)
        raise Failed(f"{a.session}: the sidebar never came up")
    print(f"{a.session} up, raw client output in {d / 'client.out'}")
    show(snapshot(a.session))


# An escape sequence, or a run of text between them.
TOKEN = re.compile(
    r"\x1b(?:\[([0-9;?<>=]*)[ -/]*([@-~])|\][^\x07\x1b]*(?:\x07|\x1b\\)|[P_^X].*?\x1b\\"
    r"|[()][0-9A-Za-z]|.)|([^\x1b]+)",
    re.S,
)


def screen(session):
    """What the client has on screen, as lines: its raw output replayed through
    just enough of a terminal - Zellij draws by placing the cursor and writing,
    and the rest (colour, links, modes) changes nothing a check can read."""
    d = home(session)
    cols, rows = map(int, meta(session)["size"].split("x"))
    grid = [[" "] * cols for _ in range(rows)]
    row = col = 0
    raw = (d / "client.out").read_bytes().decode(errors="replace")
    for params, final, text in TOKEN.findall(raw):
        if text:
            for char in text:
                if char == "\r":
                    col = 0
                elif char == "\n":
                    row = min(row + 1, rows - 1)
                elif char == "\b":
                    col = max(col - 1, 0)
                elif ord(char) < 32 or unicodedata.combining(char):
                    continue
                else:
                    wide = unicodedata.east_asian_width(char) in "WF"
                    if col < cols:
                        grid[row][col] = char
                        if wide and col + 1 < cols:
                            grid[row][col + 1] = ""
                    col += 2 if wide else 1
            continue
        if not final:
            continue  # not a CSI sequence: links, titles, modes
        numbers = [int(n) if n.isdigit() else 0 for n in params.split(";")] if params else []
        first = (numbers or [0])[0]
        if final in "Hf":
            row = min(max((numbers + [1])[0], 1), rows) - 1
            col = min(max((numbers[1:] + [1])[0], 1), cols) - 1
        elif final == "J" and first == 2:
            grid = [[" "] * cols for _ in range(rows)]
        elif final == "K":
            grid[row][col:] = [" "] * (cols - col)
        elif final in "ABCD":
            step = max(first, 1)
            row = min(max(row + {"A": -step, "B": step}.get(final, 0), 0), rows - 1)
            col = min(max(col + {"D": -step, "C": step}.get(final, 0), 0), cols - 1)
        elif final == "G":
            col = min(max(first, 1), cols) - 1
        elif final == "d":
            row = min(max(first, 1), rows) - 1
    return ["".join(line) for line in grid]


def drawn(session, pane):
    """The lines a pane's content occupies on the client's screen."""
    listed = json.loads(action(session, "list-panes", "-j", "-a").stdout or "[]")
    kind, _, number = pane.partition("_")
    found = next((p for p in listed if str(p["id"]) == number
                  and p["is_plugin"] == (kind == "plugin")), None)
    if found is None:
        return []
    x, y = found["pane_content_x"], found["pane_content_y"]
    width, height = found["pane_content_columns"], found["pane_content_rows"]
    lines = [line[x:x + width].rstrip() for line in screen(session)[y:y + height]]
    while lines and not lines[-1]:
        lines.pop()
    return lines


def said_by_client(path):
    """The client's own output as plain lines: escapes and blank lines gone."""
    try:
        raw = path.read_bytes().decode(errors="replace")
    except OSError:
        return []
    plain = re.sub(r"\x1b(\[[0-9;?<>=]*[ -/]*[@-~]|\][^\x07\x1b]*(\x07|\x1b\\)|[P_^X].*?\x1b\\|[()][0-9A-Za-z]|.)",
                   "", raw, flags=re.S)
    lines = (line.strip() for line in re.split(r"[\r\n]+", plain))
    return [line for line in lines if line]


def unescape(text):
    # Python reads at most three octal digits, so `\0333` is Esc then 3 - which
    # `printf %b` got wrong - and `\e` is Esc as in a shell.
    text = re.sub(r"(?<!\\)\\e", r"\\x1b", text)
    return codecs.decode(text.encode("latin-1", "backslashreplace"), "unicode_escape").encode()


def send(session, chunks, gap=0.2):
    try:
        fd = os.open(home(session) / "keys", os.O_WRONLY | os.O_NONBLOCK)
    except OSError:
        raise Failed(f"{session}: no client is reading keys")
    with os.fdopen(fd, "wb", buffering=0) as stream:
        for i, chunk in enumerate(chunks):
            if i:
                time.sleep(gap)
            stream.write(unescape(chunk))


def settle(session, checks, timeout, quiet=False):
    deadline = time.monotonic() + timeout
    while True:
        state = snapshot(session)
        results = [(text, *check(state, text)) for text in checks]
        if all(ok for _, ok, _ in results):
            return True
        if time.monotonic() > deadline:
            if not quiet:
                for text, ok, got in results:
                    if not ok:
                        print(f"  FAILED {text} (got {got!r})")
                show(state)
            return False
        time.sleep(0.25)


def reload(a):
    url = meta(a.session)["url"]
    layout = (home(a.session) / "layout.kdl").read_text()
    block = re.search(re.escape(url) + r'"\s*\{(.*?)\}', layout, re.S)
    pairs = re.findall(r'(?m)^\s*(\w+) "(.*)"$', block[1]) if block else []
    # The configuration has to be the layout's, all of it, or this starts a
    # second sidebar instead of reloading the first.
    args = ["-c", ",".join(f"{k}={v}" for k, v in pairs)] if pairs else []
    action(a.session, "start-or-reload-plugin", *args, url)


def verdict(session):
    """('clean' | 'panic' | 'client', what to say), from the session's own log."""
    log = next(home(session).glob("tmp/zellij-*/zellij-log/zellij.log"), None)
    text = log.read_text(errors="replace") if log else ""
    entries = re.split(r"(?m)^(?=(?:ERROR|WARN|INFO|DEBUG|TRACE) +\|)", text)
    panics = [e for e in entries if "PANIC IN PLUGIN" in e or "panicked" in e]
    client = [e for e in entries if e.startswith("ERROR") and any(t in e for t in CLIENT_TROUBLE)]
    other = [e for e in entries if e.startswith("ERROR") and e not in panics and e not in client
             and not any(n in e for n in NOISE)]

    def gist(entry):
        lines = entry.strip().splitlines()
        found = re.search(r"\.rs:\d+: (.*)", lines[0])
        said = found[1].strip() if found else lines[0]
        if said == "a non-fatal error occurred" and len(lines) > 1:
            said = re.sub(r"^\s*\d+: ", "", lines[-1]).strip()  # the innermost cause
        return said[:160]

    lines = [f"  panic: {gist(e)}" for e in panics] + [f"  client: {gist(e)}" for e in client[:3]]
    lines += [f"  error: {g}" for g in sorted({gist(e) for e in other})]
    status = "panic" if panics else "client" if client else "clean"
    return status, "\n".join([f"log: {status}", *lines])


def glob_escape(text):
    return re.sub(r"([*?\[])", r"[\1]", text)


def stop_session(session, keep=False):
    d = home(session)
    status, said = verdict(session)  # first: killing a session logs errors of its own
    print(said)
    try:
        client = meta(session)["client"]
    except Failed:
        client = None
    subprocess.run(["zellij", "kill-session", session], env=env(session), capture_output=True)
    for left in STATE.glob(f"{glob_escape(session)}.*"):
        left.unlink(missing_ok=True)
    if client:
        try:
            os.killpg(client, signal.SIGTERM)
        except ProcessLookupError:
            pass
    time.sleep(0.5)
    subprocess.run(["zellij", "delete-session", "--force", session], env=env(session),
                   capture_output=True)
    if keep or status != "clean":
        # Out of the way, so the same session can start again - a retry used to
        # find this one and refuse.
        kept = d.with_name(f"{d.name}.kept-{int(time.time())}")
        d.rename(kept)
        print(f"kept {kept}")
    else:
        shutil.rmtree(d, ignore_errors=True)
    return status


def timeline(a):
    start(a)
    due, at = [], 0.0
    for step in a.steps:
        delay, _, data = step.partition(":")
        at += float(delay)
        due.append((at, data))
    began, sample, end = time.monotonic(), 0, at + 8
    try:
        while sample <= end:
            now = time.monotonic() - began
            while due and due[0][0] <= now:
                send(a.session, [due.pop(0)[1]])
            if now >= sample:
                state = snapshot(a.session)
                print(f"{sample:3d}s  " + "  ".join(
                    f"{key}={value(state, key) or '-'}"
                    for key in ("panes", "parked", "floating", "layer", "focus", "visible")))
                sample += 1
            time.sleep(0.05)
    finally:
        stop_session(a.session)


def run_scenario(path, parser):
    session = re.sub(r"[^a-z0-9-]", "-", f"e2e-{Path(path).stem}".lower())[:24]
    if home(session).exists():
        stop_session(session)
    status = "pass"
    os.environ["AGENTTIJ_SESSION_DIR"] = str(home(session))
    print(f"== {path}")
    try:
        for number, line in enumerate(Path(path).read_text().splitlines(), 1):
            words = shlex.split(os.path.expandvars(line), comments=True)
            if not words:
                continue
            print(f"  > {line.strip()}")
            if words[0] == "needs":
                missing = [name for name in words[1:] if not os.environ.get(name)]
                if missing:
                    print(f"  skipped: {' '.join(missing)} not set")
                    return "skip"
                continue
            if words[0] == "sleep":
                time.sleep(float(words[1]))
                continue
            a = parser.parse_args([words[0], session, *words[1:]])
            if a.run(a) is False:
                status = "fail"
                print(f"  {path}:{number}: failed")
                break
    except (Failed, subprocess.CalledProcessError, SystemExit) as error:
        status = "fail"
        print(f"  {error}")
    finally:
        if home(session).exists():
            # A failed run keeps its directory whatever the log says: the log,
            # the client's output and the layout are what explain it.
            logged = stop_session(session, keep=status == "fail")
            if logged == "panic":
                status = "fail"
            elif logged == "client" and status == "fail":
                status = "client"
    return status


def scenario(a, parser):
    failed = []
    for path in a.files:
        status = run_scenario(path, parser)
        if status == "client":
            print("  the client broke that run, not the plugin; once more")
            status = run_scenario(path, parser)
        print(f"== {path}: {status}\n")
        if status not in ("pass", "skip"):
            failed.append(path)
    if failed:
        print("failed: " + " ".join(failed))
    return not failed


def build_parser():
    parser = argparse.ArgumentParser(description=__doc__,
                                     formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="command", required=True)

    def command(name, run, *positionals, starts=False):
        p = sub.add_parser(name)
        p.add_argument("session")
        for positional, nargs in positionals:
            p.add_argument(positional, nargs=nargs)
        if starts:
            p.add_argument("--group", help="the template every row starts with")
            p.add_argument("--size", default="160x40", help="COLSxROWS of the client")
            p.add_argument("--wasm", default=str(WASM), help="the plugin to run")
        p.set_defaults(run=run)
        return p

    command("start", start, ("layout", None), starts=True)
    k = command("keys", lambda a: send(a.session, a.bytes, a.gap), ("bytes", "+"))
    k.add_argument("--gap", type=float, default=0.2, help="seconds between arguments")
    command("state", lambda a: show(snapshot(a.session)))
    command("expect", lambda a: settle(a.session, a.checks, 0), ("checks", "+"))
    w = command("wait", lambda a: settle(a.session, a.checks, a.timeout), ("checks", "+"))
    w.add_argument("--timeout", type=float, default=10)
    command("reload", reload)
    r = command("report", report, ("pane", None), ("status", None))
    r.add_argument("--cwd", default=str(REPO),
                   help="where the agent works, relative to the session's directory")
    command("sh", shell, ("script", None))
    p = command("pipe", lambda a: action_pipe(a.session, a.name, a.payload), ("name", None))
    p.add_argument("payload", nargs="?")
    s = command("stop", lambda a: stop_session(a.session, a.keep) == "clean")
    s.add_argument("--keep", action="store_true", help="keep its directory and log")
    command("timeline", timeline, ("layout", None), ("steps", "+"), starts=True)
    files = sub.add_parser("scenario")
    files.add_argument("files", nargs="+")
    files.set_defaults(run=lambda a: scenario(a, parser))
    return parser


def main():
    parser = build_parser()
    a = parser.parse_args()
    try:
        ok = a.run(a)
    except Failed as error:
        sys.exit(str(error))
    sys.exit(1 if ok is False else 0)


if __name__ == "__main__":
    main()
