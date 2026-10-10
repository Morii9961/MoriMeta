#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""THIRD_PARTY_NOTICES for a MoriMeta build (RELEASE_PLAN §7.2).

Lists what the Windows build ships and reproduces each license text found in those packages:

- Rust crates linked into the app: the runtime dependency graph of `apps/desktop/src-tauri` for
  x86_64-pc-windows-msvc (build-only and dev-only crates are not shipped and not listed);
- npm packages bundled into the frontend (`npm ls --omit=dev`), fonts included;
- the ExifTool package, which is shipped unmodified with its own license files.

Identical license texts are printed once, followed by the packages that carry them. A package
that ships no license file is listed with its SPDX expression and repository. Nothing is
downloaded: crate sources come from the local Cargo registry (populated by any build), npm
packages from `apps/desktop/node_modules`.

    python tools/third_party_notices.py --out PATH [--check]

`--check` fails when a shipped package declares no license at all.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
ADAPTER = ROOT / "apps" / "desktop" / "src-tauri"
FRONTEND = ROOT / "apps" / "desktop"
TARGET = "x86_64-pc-windows-msvc"
LICENSE_NAME = re.compile(r"^(licen[cs]e|copying|notice|unlicense|copyright)", re.IGNORECASE)


def license_files(folder: Path) -> list[Path]:
    if not folder.is_dir():
        return []
    found = [
        p
        for p in sorted(folder.iterdir(), key=lambda p: p.name.lower())
        if p.is_file() and LICENSE_NAME.match(p.name) and p.suffix.lower() not in {".rs", ".js", ".ts", ".json"}
    ]
    return found


def read_text(p: Path) -> str:
    return p.read_bytes().decode("utf-8", errors="replace").replace("\r\n", "\n").strip("\n")


def norm_key(text: str) -> str:
    """Texts that differ only in whitespace are the same text."""
    return hashlib.sha256(" ".join(text.split()).encode()).hexdigest()


def cargo_graph() -> tuple[str, dict[str, dict], dict[str, list[str]]]:
    """The app crate's id, and every package it links (normal dependencies, Windows x64) with
    the edges between them."""
    cargo = os.environ.get("CARGO", "cargo")
    out = subprocess.run(
        [cargo, "metadata", "--format-version", "1", "--locked", "--filter-platform", TARGET],
        cwd=ADAPTER,
        check=True,
        capture_output=True,
    ).stdout
    meta = json.loads(out)
    by_id = {p["id"]: p for p in meta["packages"]}
    nodes = {n["id"]: n for n in meta["resolve"]["nodes"]}
    root = meta["resolve"]["root"]
    edges: dict[str, list[str]] = {}
    stack = [root]
    while stack:
        pid = stack.pop()
        if pid in edges:
            continue
        # only normal dependencies end up in the program
        edges[pid] = sorted(
            dep["pkg"] for dep in nodes[pid]["deps"] if any(k["kind"] is None for k in dep["dep_kinds"])
        )
        stack.extend(edges[pid])
    return root, {pid: by_id[pid] for pid in edges}, edges


def cargo_packages() -> list[dict]:
    _, packages, _ = cargo_graph()
    rows = []
    for p in packages.values():
        if not p["source"]:
            continue  # MoriMeta's own crates
        folder = Path(p["manifest_path"]).parent
        files = license_files(folder)
        if not files and p.get("license_file"):
            lf = folder / p["license_file"]
            if lf.is_file():
                files = [lf]
        rows.append(
            {
                "kind": "Rust crate",
                "name": p["name"],
                "version": p["version"],
                "license": p.get("license") or "",
                "url": p.get("repository") or p.get("homepage") or f"https://crates.io/crates/{p['name']}",
                "files": files,
            }
        )
    return sorted(rows, key=lambda r: (r["name"].lower(), r["version"]))


def npm_packages() -> list[dict]:
    npm = "npm.cmd" if os.name == "nt" else "npm"
    out = subprocess.run(
        [npm, "ls", "--omit=dev", "--all", "--parseable"],
        cwd=FRONTEND,
        check=True,
        capture_output=True,
        text=True,
    ).stdout
    rows = []
    seen = set()
    for line in out.splitlines():
        folder = Path(line.strip())
        if not line.strip() or folder.resolve() == FRONTEND.resolve():
            continue
        pj = folder / "package.json"
        if not pj.is_file():
            continue
        info = json.loads(pj.read_text(encoding="utf-8"))
        key = (info.get("name"), info.get("version"))
        if key in seen:
            continue
        seen.add(key)
        lic = info.get("license") or ""
        if isinstance(lic, dict):
            lic = lic.get("type", "")
        repo = info.get("repository") or ""
        if isinstance(repo, dict):
            repo = repo.get("url", "")
        rows.append(
            {
                "kind": "npm package",
                "name": info.get("name", folder.name),
                "version": info.get("version", ""),
                "license": lic,
                "url": repo or info.get("homepage") or f"https://www.npmjs.com/package/{info.get('name')}",
                "files": license_files(folder),
            }
        )
    return sorted(rows, key=lambda r: (r["name"].lower(), r["version"]))


EXIFTOOL = """\
## ExifTool package

MoriMeta runs ExifTool by Phil Harvey as a separate program (`exiftool_files\\perl.exe
exiftool_files\\exiftool.pl`, DECISIONS D-17). The package is the official Windows package of the
pinned version, shipped unmodified in the `exiftool` folder of the installation; its integrity is
checked against a manifest before it runs.

- ExifTool: free software; you can redistribute it and/or modify it under the same terms as Perl
  itself (the Artistic License or the GNU General Public License). https://exiftool.org
- Strawberry Perl and its components: their licenses are in
  `exiftool\\exiftool_files\\Licenses_Strawberry_Perl.zip`; the GNU GPL text is in
  `exiftool\\exiftool_files\\LICENSE`.
- Source code: https://exiftool.org (Image-ExifTool distribution) and https://strawberryperl.com.
"""


def render(rows: list[dict], version: str) -> tuple[str, list[str]]:
    problems = []
    texts: dict[str, dict] = {}
    for r in rows:
        r["keys"] = []
        for f in r["files"]:
            t = read_text(f)
            if not t:
                continue
            k = norm_key(t)
            texts.setdefault(k, {"text": t, "users": []})["users"].append(f"{r['name']} {r['version']}")
            r["keys"].append(k)
        if not r["license"] and not r["keys"]:
            problems.append(f"{r['kind']} {r['name']} {r['version']}: no license declared or shipped")
    out = [
        "# Third-party notices",
        "",
        f"MoriMeta {version} is free software under the GNU General Public License, version 3 or",
        "(at your option) any later version. It includes the following third-party software, each",
        "under its own license. This file is generated by `tools/third_party_notices.py`.",
        "",
        EXIFTOOL,
        "## Packages",
        "",
        "| Package | Version | License | Source |",
        "|---|---|---|---|",
    ]
    for r in rows:
        lic = r["license"] or "see license text"
        out.append(f"| {r['name']} ({r['kind']}) | {r['version']} | {lic} | {r['url']} |")
    out += ["", "## License texts", ""]
    for i, t in enumerate(sorted(texts.values(), key=lambda t: t["users"][0].lower()), start=1):
        users = sorted(set(t["users"]), key=str.lower)
        out.append(f"### Text {i}")
        out.append("")
        out.append("Used by: " + ", ".join(users))
        out.append("")
        out.append("```text")
        out.append(t["text"].replace("```", "'''"))
        out.append("```")
        out.append("")
    without = [r for r in rows if not r["keys"]]
    if without:
        out += [
            "## Packages without a license file",
            "",
            "These packages ship no license file; their license is the SPDX expression shown, whose",
            "standard text applies, and their source is at the address given.",
            "",
        ]
        for r in without:
            out.append(f"- {r['name']} {r['version']}: {r['license']} ({r['url']})")
        out.append("")
    return "\n".join(out), problems


def app_version() -> str:
    conf = json.loads((ADAPTER / "tauri.conf.json").read_text(encoding="utf-8"))
    return conf.get("version", "")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--check", action="store_true")
    a = ap.parse_args()
    rows = cargo_packages() + npm_packages()
    text, problems = render(rows, app_version())
    a.out.parent.mkdir(parents=True, exist_ok=True)
    a.out.write_text(text + "\n", encoding="utf-8", newline="\n")
    crates = sum(1 for r in rows if r["kind"] == "Rust crate")
    print(f"{a.out}: {crates} crates, {len(rows) - crates} npm packages, {len(text) // 1024} KB")
    for p in problems:
        print("problem:", p, file=sys.stderr)
    return 1 if a.check and problems else 0


if __name__ == "__main__":
    sys.exit(main())
