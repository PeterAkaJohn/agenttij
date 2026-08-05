#!/usr/bin/env python3
"""Pre-grant agenttij's Zellij permissions, or revoke them.

Zellij asks for plugin permissions by drawing its own prompt over the plugin's
pane. In a 26-column sidebar that prompt does not fit, so the first launch is a
blank pane waiting on a keypress you cannot see. Installing the plugin is the
consent; this records it so the sidebar works on first run.

Permissions are cached per plugin URL, so a plugin moved or rebuilt elsewhere
gets asked again.

usage: grant-permissions.py <grant|revoke> <permissions.kdl> <plugin-url>
"""

import re
import sys

PERMISSIONS = (
    "ReadApplicationState",  # session and pane metadata
    "ChangeApplicationState",  # focus a pane, switch session
    "RunCommands",  # read state files, open a preview
)


def without_entry(text, url):
    """Strips any existing block for this URL, leaving the rest untouched."""
    pattern = re.compile(
        r'^"' + re.escape(url) + r'"\s*\{.*?^\}\n?',
        re.MULTILINE | re.DOTALL,
    )
    return pattern.sub("", text)


def entry(url):
    body = "".join(f"    {name}\n" for name in PERMISSIONS)
    return f'"{url}" {{\n{body}}}\n'


def main(argv):
    if len(argv) != 4 or argv[1] not in ("grant", "revoke"):
        sys.exit(__doc__)

    mode, path, url = argv[1], argv[2], argv[3]

    try:
        with open(path) as handle:
            text = handle.read()
    except FileNotFoundError:
        text = ""

    text = without_entry(text, url)
    if mode == "grant":
        if text and not text.endswith("\n"):
            text += "\n"
        text += entry(url)

    with open(path, "w") as handle:
        handle.write(text)


if __name__ == "__main__":
    main(sys.argv)
