"""Stage the ExifTool package that the installer ships (ARCHITECTURE ADR-03, D-17 B, SECURITY_MODEL §5).

1. The pinned package (research/scripts/fetch_exiftool.py, SHA-256 checked) is checked file by file
   against the manifest made from the official zip (research/exiftool-<version>.manifest.json).
2. Only what `perl.exe exiftool.pl` needs is copied: `exiftool_files/` (Perl, the script, its
   libraries and licences). The CC0 launcher is not shipped (D-17).
3. A manifest of exactly the staged files is written next to them (`exiftool.manifest`), and the
   staged package is checked against it, as the app does at every launch.
4. THIRD_PARTY_NOTICES.md for what the build ships (tools/third_party_notices.py, RELEASE_PLAN
   §7.2); About › Third-party notices shows it.

Usage: python apps/desktop/scripts/stage-exiftool.py
Output: apps/desktop/src-tauri/staging/exiftool/ and staging/THIRD_PARTY_NOTICES.md (not committed)
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
STAGE = ROOT / "apps" / "desktop" / "src-tauri" / "staging" / "exiftool"


def mm_cli(*args: str, pkg: Path) -> dict:
    with tempfile.TemporaryDirectory() as data:
        r = subprocess.run(
            ["cargo", "run", "-q", "-p", "mm-cli", "--", "--data", data, "--exiftool", str(pkg), *args],
            cwd=ROOT,
            capture_output=True,
            text=True,
            encoding="utf-8",
        )
    if r.returncode not in (0, 3):
        sys.exit(f"mm-cli {' '.join(args)} failed: {r.stderr or r.stdout}")
    return json.loads(r.stdout.strip().splitlines()[-1])


def main() -> None:
    version = json.loads((ROOT / "research" / "exiftool.lock.json").read_text(encoding="utf-8"))["version"]
    pkg = ROOT / "research" / ".work" / "exiftool" / version / "win64" / f"exiftool-{version}_64"
    official = ROOT / "research" / f"exiftool-{version}.manifest.json"
    if not (pkg / "exiftool_files" / "perl.exe").exists():
        sys.exit(f"{pkg}: not fetched; run python research/scripts/fetch_exiftool.py first")

    r = mm_cli("exiftool-check", "--full", "--manifest", str(official), pkg=pkg)
    if not r.get("intact"):
        sys.exit(f"the fetched package does not match the official manifest: {r.get('problem')}")

    if STAGE.exists():
        shutil.rmtree(STAGE)
    STAGE.mkdir(parents=True)
    shutil.copytree(pkg / "exiftool_files", STAGE / "exiftool_files")

    manifest = STAGE / "exiftool.manifest"
    r = mm_cli("exiftool-manifest", str(STAGE), "--version", version, "--out", str(manifest), pkg=pkg)
    files = r["files"]
    r = mm_cli("exiftool-check", "--full", pkg=STAGE)
    if not r.get("intact"):
        sys.exit(f"the staged package does not pass its own check: {r.get('problem')}")
    size = sum(p.stat().st_size for p in STAGE.rglob("*") if p.is_file())
    print(f"staged ExifTool {version}: {files} files, {size / 1e6:.1f} MB, manifest checked -> {STAGE}")

    notices = STAGE.parent / "THIRD_PARTY_NOTICES.md"
    r = subprocess.run(
        [sys.executable, str(ROOT / "tools" / "third_party_notices.py"), "--check", "--out", str(notices)],
        cwd=ROOT,
    )
    if r.returncode != 0:
        sys.exit("third-party notices could not be generated")


if __name__ == "__main__":
    main()
