#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Collect the files of a MoriMeta release (RELEASE_PLAN §7.2, §8, §11; DECISIONS D-2).

After `npx tauri build`, puts into OUT:

- `MoriMeta_<version>_x64-setup.exe`, the installer;
- `THIRD_PARTY_NOTICES.md`, as staged into the installer (apps/desktop/scripts/stage-exiftool.py);
- `MoriMeta_<version>_sbom.cdx.json` (tools/sbom.py);
- `Image-ExifTool-<version>.tar.gz`, the source of the shipped ExifTool, checked against
  research/exiftool.lock.json;
- `SHA256SUMS.txt` over all of them (`sha256sum -c` format);
and next to OUT `release-notes-<version>.md`: the version's CHANGELOG section followed by the
files, their SHA-256 and how to verify them, the text of the draft release.

The version must be the same in tauri.conf.json, the desktop Cargo.toml and package.json, and match
`--tag` when given. With `--tag` CHANGELOG.md and CHANGELOG.zh-CN.md must both have a section for the version; the
Chinese one follows the English in the notes.

    python tools/release_assets.py --out DIR [--tag vX.Y.Z]
"""

from __future__ import annotations

import argparse
import hashlib
import json
import re
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DESKTOP = ROOT / "apps" / "desktop"
ADAPTER = DESKTOP / "src-tauri"
REPO = "Morii9961/MoriMeta"


def sha256(p: Path) -> str:
    h = hashlib.sha256()
    with p.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def version(tag: str | None) -> str:
    def read(p: Path) -> str:
        return p.read_text(encoding="utf-8")

    found = {
        "tauri.conf.json": json.loads(read(ADAPTER / "tauri.conf.json"))["version"],
        "desktop Cargo.toml": tomllib.loads(read(ADAPTER / "Cargo.toml"))["package"]["version"],
        "package.json": json.loads(read(DESKTOP / "package.json"))["version"],
    }
    if tag is not None:
        if not tag.startswith("v"):
            sys.exit(f"tag {tag}: release tags are v<version>")
        found["tag"] = tag[1:]
    if len(set(found.values())) != 1:
        sys.exit("versions differ: " + ", ".join(f"{k} {v}" for k, v in found.items()))
    return next(iter(found.values()))


def changelog_section(ver: str, name: str = "CHANGELOG.md") -> str | None:
    """The version's section of CHANGELOG.md, or of its Chinese counterpart CHANGELOG.zh-CN.md."""
    path = ROOT / name
    if not path.is_file():
        return None
    text = path.read_text(encoding="utf-8")
    m = re.search(rf"^## \[{re.escape(ver)}\][^\n]*\n(.*?)(?=^## \[|\Z)", text, re.MULTILINE | re.DOTALL)
    return m.group(1).strip() if m else None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--out", required=True, type=Path)
    ap.add_argument("--tag")
    a = ap.parse_args()
    ver = version(a.tag)
    notes = changelog_section(ver)
    notes_zh = changelog_section(ver, "CHANGELOG.zh-CN.md")
    # release notes in English and Chinese (RELEASE_PLAN §11)
    for name, section in (("CHANGELOG.md", notes), ("CHANGELOG.zh-CN.md", notes_zh)):
        if a.tag and section is None:
            sys.exit(f"{name} has no section ## [{ver}]")

    out = a.out
    if out.exists() and any(out.iterdir()):
        sys.exit(f"{out} is not empty")
    out.mkdir(parents=True, exist_ok=True)

    installer = ADAPTER / "target" / "release" / "bundle" / "nsis" / f"MoriMeta_{ver}_x64-setup.exe"
    notices = ADAPTER / "staging" / "THIRD_PARTY_NOTICES.md"
    for p in (installer, notices):
        if not p.is_file():
            sys.exit(f"{p}: missing; stage ExifTool and build the installer first")
    shutil.copy2(installer, out / installer.name)
    shutil.copy2(notices, out / notices.name)

    sbom = out / f"MoriMeta_{ver}_sbom.cdx.json"
    subprocess.run(
        [sys.executable, str(ROOT / "tools" / "sbom.py"), "--check", "--out", str(sbom)], cwd=ROOT, check=True
    )

    lock = json.loads((ROOT / "research" / "exiftool.lock.json").read_text(encoding="utf-8"))
    src = lock["artifacts"]["source"]
    tarball = ROOT / "research" / ".work" / "downloads" / src["file"]
    if not tarball.is_file() or sha256(tarball) != src["sha256"]:
        sys.exit(f"{tarball}: missing or not the pinned file; run python research/scripts/fetch_exiftool.py")
    shutil.copy2(tarball, out / tarball.name)

    files = sorted(p for p in out.iterdir() if p.is_file())
    sums = {p.name: sha256(p) for p in files}
    lines = "".join(f"{h}  {n}\n" for n, h in sums.items())
    (out / "SHA256SUMS.txt").write_text(lines, encoding="ascii", newline="\n")

    rows = "\n".join(f"| `{n}` | `{h}` |" for n, h in sums.items())
    body = notes if notes is not None else "_No CHANGELOG section for this version: a test build._"
    if notes_zh is not None:
        body += f"\n\n## 中文\n\n{notes_zh}"
    text = f"""{body}

## Files

| File | SHA-256 |
|---|---|
{rows}

`SHA256SUMS.txt` lists the same values. This build is not code-signed (DECISIONS D-2), so Windows
SmartScreen may warn; check the SHA-256 first (docs/INSTALLATION.md). The installer and the SBOM
were built by GitHub Actions from the tagged commit; to check that:

```
gh attestation verify {installer.name} -R {REPO}
```

ExifTool {lock["version"]} is included unmodified; `{tarball.name}` is its source. Strawberry Perl
source: https://strawberryperl.com. Licenses: `THIRD_PARTY_NOTICES.md`.
"""
    (out.parent / f"release-notes-{ver}.md").write_text(text, encoding="utf-8", newline="\n")
    for n, h in sums.items():
        print(f"{h}  {n}")
    print(f"notes: {out.parent / f'release-notes-{ver}.md'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
