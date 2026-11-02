#!/usr/bin/env python3
"""Register (or remove) agenttij's Claude Code hooks in a settings.json.

Kept separate from install.sh so it can be exercised against a copy of a real
settings file - it edits a file the user cares about, and other tools keep
their hooks in there too.

`plugin` prints the same hooks as the Claude Code plugin carries them, which is
what plugins/agenttij/hooks/hooks.json is generated from: one table of events
for both ways of installing them.

usage: register-hooks.py <install|uninstall> <settings.json> [hook-path]
       register-hooks.py plugin > plugins/agenttij/hooks/hooks.json
"""

import json
import sys

# The state each hook event reports.
#
# SubagentStop is deliberately absent: it can fire after the main turn has
# already ended, which would revive an idle pane.
EVENTS = {
    "SessionStart": "idle",
    "UserPromptSubmit": "running",
    "Notification": "needs-input",
    "Stop": "done",
    "SessionEnd": "gone",
}

MARKER = "agenttij-state.sh"


def is_ours(entry):
    return any(MARKER in hook.get("command", "") for hook in entry.get("hooks", []))


def entries(command):
    return {
        event: [
            {
                "matcher": "*",
                "hooks": [{"type": "command", "command": command(state), "timeout": 5}],
            }
        ]
        for event, state in EVENTS.items()
    }


def register(settings, hook_path, install):
    hooks = settings.setdefault("hooks", {})

    # Drop our own entries first, so re-running upgrades instead of
    # duplicating. Other tools' hooks are matched on nothing and left alone.
    for event in list(hooks):
        hooks[event] = [entry for entry in hooks[event] if not is_ours(entry)]
        if not hooks[event]:
            del hooks[event]

    if install:
        for event, entry in entries(lambda state: f"sh '{hook_path}' {state}").items():
            hooks.setdefault(event, []).extend(entry)

    if not hooks:
        settings.pop("hooks", None)
    return settings


def main(argv):
    if argv[1:] == ["plugin"]:
        hooks = entries(
            lambda state: f'sh "${{CLAUDE_PLUGIN_ROOT}}/hooks/{MARKER}" {state}'
        )
        print(json.dumps({"hooks": hooks}, indent=2))
        return
    if len(argv) < 3 or argv[1] not in ("install", "uninstall"):
        sys.exit(__doc__)

    mode, path = argv[1], argv[2]
    hook_path = argv[3] if len(argv) > 3 else ""

    try:
        with open(path) as handle:
            settings = json.load(handle)
    except FileNotFoundError:
        settings = {}

    settings = register(settings, hook_path, install=mode == "install")

    with open(path, "w") as handle:
        json.dump(settings, handle, indent=2)
        handle.write("\n")


if __name__ == "__main__":
    main(sys.argv)
