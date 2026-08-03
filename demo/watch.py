#!/usr/bin/env python3
"""Polls all three deployment targets and shows the version each one serves.

Run this in a second terminal during the demo. As `spin up`, the SpinKube
rollout and `spin aka deploy` land, the rows update one at a time — and the
swatch changes colour, because the accent is hashed from the build id and so
is different for every build.

    ./demo/watch.py

Override any target with an env var: LOCAL_URL, KUBE_URL, AKA_URL, INTERVAL.
"""

import json
import os
import re
import sys
import time
import urllib.error
import urllib.request

TARGETS = [
    ("spin up", os.environ.get("LOCAL_URL", "http://127.0.0.1:3030")),
    ("spinkube", os.environ.get("KUBE_URL", "http://172.238.62.94")),
    ("spin aka", os.environ.get("AKA_URL", "https://e860adf5-c9c4-48df-b260-ea35c07b7ac0.fwf.app")),
]
INTERVAL = float(os.environ.get("INTERVAL", "5"))

DIM = "\033[2m"
RESET = "\033[0m"
BOLD = "\033[1m"


def probe(url):
    try:
        with urllib.request.urlopen(f"{url}/api/whereami", timeout=6) as resp:
            return json.load(resp)
    except (urllib.error.URLError, OSError, ValueError, TimeoutError):
        return None


def swatch(accent):
    """Draw the page's accent as a colour block, so the terminal and the browser
    visibly agree that they are showing the same build."""
    match = re.fullmatch(r"#([0-9a-fA-F]{6})", (accent or "").strip())
    if not match:
        return "   "
    r, g, b = (int(match.group(1)[i:i + 2], 16) for i in (0, 2, 4))
    return f"\033[38;2;{r};{g};{b}m███{RESET}"


def render():
    lines = [
        "\033[H\033[J",
        f"  {BOLD}spin-llm-chat — deploy status{RESET}   "
        f"{DIM}{time.strftime('%H:%M:%S')}, every {INTERVAL:g}s{RESET}",
        "",
        f"  {'TARGET':<10} {'RUNTIME':<18} {'VERSION':<22} {'GIT':<9} ACCENT",
        "  " + "─" * 72,
    ]
    for name, url in TARGETS:
        data = probe(url)
        if data is None:
            lines.append(f"  {name:<10} {DIM}{'—':<18} {'not reachable':<22} {'—':<9}{RESET}")
            continue
        lines.append(
            f"  {name:<10} {data.get('runtime', ''):<18} {data.get('version', ''):<22} "
            f"{data.get('gitSha', ''):<9} {swatch(data.get('accent'))} "
            f"{DIM}{data.get('accent', '')}{RESET}"
        )
    lines += ["", f"  {DIM}Ctrl-C to stop{RESET}"]
    sys.stdout.write("\n".join(lines) + "\n")
    sys.stdout.flush()


def main():
    once = "--once" in sys.argv
    try:
        while True:
            render()
            if once:
                return
            time.sleep(INTERVAL)
    except KeyboardInterrupt:
        print()


if __name__ == "__main__":
    main()
