#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Is there an ExifTool release newer than the pinned one? (RELEASE_PLAN §10, step 1)

Reads https://exiftool.org/ver.txt and https://exiftool.org/history.html and lists every version
newer than research/exiftool.lock.json, with its date and whether ExifTool's history marks it as a
security update ("Security update", "Security update (Windows only)") or a production release.
A security update starts the 14-day update SLA even when it is not a production release.

    python tools/exiftool_watch.py [--pinned X.YY] [--history FILE] [--latest X.YY] [--issue OUT.md]

Prints one line per newer version. With `--issue`, writes the text of a tracking issue to OUT.md
and prints its title as the last line (nothing is written when the pin is current). `--history` and
`--latest` read saved copies instead of the network. Only the standard library is used.
"""

from __future__ import annotations

import argparse
import html
import json
import re
import sys
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
BASE = "https://exiftool.org"
ENTRY = re.compile(
    r"<a name='v(?P<v>\d+\.\d+)'><b>(?P<date>[^<]*?) - Version \d+\.\d+</b>(?P<head>.*?)<ul>(?P<body>.*?)\n</ul>",
    re.DOTALL,
)


def fetch(url: str) -> str:
    req = urllib.request.Request(url, headers={"User-Agent": "MoriMeta-exiftool-watch"})
    with urllib.request.urlopen(req, timeout=60) as r:
        return r.read().decode("utf-8", errors="replace")


def key(v: str) -> tuple[int, ...]:
    return tuple(int(p) for p in v.split("."))


def entries(history: str) -> list[dict]:
    out = []
    for m in ENTRY.finditer(history):
        items = [html.unescape(re.sub(r"<[^>]+>", "", li)).strip() for li in m.group("body").split("<li>")[1:]]
        items = [" ".join(i.split()) for i in items]
        security = next((i for i in items if i.lower().startswith("security update")), None)
        out.append(
            {
                "version": m.group("v"),
                "date": m.group("date").strip(),
                "security": security,
                "production": "production release" in m.group("head"),
                "changes": items,
            }
        )
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--pinned")
    ap.add_argument("--history", type=Path)
    ap.add_argument("--latest")
    ap.add_argument("--issue", type=Path)
    a = ap.parse_args()

    pinned = a.pinned or json.loads((ROOT / "research" / "exiftool.lock.json").read_text(encoding="utf-8"))["version"]
    latest = (a.latest or fetch(f"{BASE}/ver.txt")).strip()
    history = a.history.read_text(encoding="utf-8") if a.history else fetch(f"{BASE}/history.html")
    if not re.fullmatch(r"\d+\.\d+", latest):
        sys.exit(f"unexpected ver.txt content: {latest[:40]!r}")
    found = entries(history)
    if not found:
        sys.exit("history.html: no version entries found; the page format may have changed")

    newer = [e for e in found if key(e["version"]) > key(pinned)]
    if key(latest) > key(pinned) and not any(e["version"] == latest for e in newer):
        newer.insert(0, {"version": latest, "date": "?", "security": None, "production": False, "changes": []})
    print(f"pinned {pinned}, latest {latest}")
    for e in newer:
        flags = [f for f in (e["security"], "production release" if e["production"] else None) if f]
        print(f"{e['version']} ({e['date']}){': ' + ', '.join(flags) if flags else ''}")
    if not newer or not a.issue:
        return 0

    security = [e for e in newer if e["security"]]
    top = newer[0]["version"]
    title = f"ExifTool {top} is available (pinned {pinned})" + (" - security update" if security else "")
    lines = [
        f"ExifTool's [version history]({BASE}/history.html) lists releases newer than the pinned {pinned} "
        "(`research/exiftool.lock.json`). Opened by `.github/workflows/exiftool-watch.yml`.",
        "",
    ]
    if security:
        lines += [
            "**Security update:** "
            + ", ".join(e["version"] + e["security"][len("Security update") :] for e in security)
            + ". "
            "RELEASE_PLAN §10 starts the 14-day SLA for a PATCH release, even when it is not a production release.",
            "",
        ]
    for e in newer:
        prod = " - production release" if e["production"] else ""
        lines.append(f"### {e['version']} ({e['date']}){prod}")
        lines.append("")
        lines += [f"- {c}" for c in e["changes"]] or ["- (not in history.html yet)"]
        lines.append("")
    lines += [
        "Steps (RELEASE_PLAN §10): pick the version by the lock rule (newest release with every known "
        "security fix); update the version, URLs and SHA-256 from "
        f"{BASE}/checksums.txt in `research/exiftool.lock.json`; fetch it "
        "(`research/scripts/fetch_exiftool.py`) and make `research/exiftool-<version>.manifest.json` from the "
        "checked package with `mm-cli exiftool-manifest`; let CI run the full regression; "
        "spot-check the compatibility lab; release a PATCH version naming the ExifTool version and why.",
    ]
    a.issue.write_text("\n".join(lines) + "\n", encoding="utf-8", newline="\n")
    print(title)
    return 0


if __name__ == "__main__":
    sys.exit(main())
