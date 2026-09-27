"""Download the pinned ExifTool artifacts, verify SHA-256, and extract them.

Usage:  python research/scripts/fetch_exiftool.py
Output: research/.work/exiftool/<version>/win64/...   (Windows package)
        research/.work/exiftool/<version>/src/...     (source tarball, incl. t/images)

Only the Python standard library is used. Nothing is installed system-wide.
"""

from __future__ import annotations

import hashlib
import json
import shutil
import sys
import tarfile
import urllib.request
import zipfile
from pathlib import Path

RESEARCH = Path(__file__).resolve().parents[1]
WORK = RESEARCH / ".work"


def sha256_of(path: Path) -> str:
    h = hashlib.sha256()
    with path.open("rb") as f:
        for chunk in iter(lambda: f.read(1 << 20), b""):
            h.update(chunk)
    return h.hexdigest()


def download(urls: list[str], dest: Path, expected: str) -> None:
    if dest.exists() and sha256_of(dest) == expected:
        return
    last_err: Exception | None = None
    for url in urls:
        tmp = dest.with_suffix(dest.suffix + ".part")
        try:
            req = urllib.request.Request(url, headers={"User-Agent": "MoriMeta-research"})
            with urllib.request.urlopen(req, timeout=120) as r, tmp.open("wb") as f:
                shutil.copyfileobj(r, f)
            got = sha256_of(tmp)
            if got != expected:
                tmp.unlink()
                raise RuntimeError(f"SHA-256 mismatch for {url}: {got} != {expected}")
            tmp.replace(dest)
            return
        except Exception as e:  # try the next mirror
            last_err = e
            if tmp.exists():
                tmp.unlink()
    raise SystemExit(f"download failed for {dest.name}: {last_err}")


def main() -> int:
    lock = json.loads((RESEARCH / "exiftool.lock.json").read_text(encoding="utf-8"))
    ver = lock["version"]
    base = WORK / "exiftool" / ver
    dl = WORK / "downloads"
    dl.mkdir(parents=True, exist_ok=True)

    win = lock["artifacts"]["windows_x64"]
    src = lock["artifacts"]["source"]
    win_zip = dl / win["file"]
    src_tgz = dl / src["file"]
    download(win["urls"], win_zip, win["sha256"])
    download(src["urls"], src_tgz, src["sha256"])

    win_dir = base / "win64"
    if not (win_dir / ".extracted").exists():
        if win_dir.exists():
            shutil.rmtree(win_dir)
        with zipfile.ZipFile(win_zip) as z:
            z.extractall(win_dir)
        (win_dir / ".extracted").write_text(win["sha256"])

    src_dir = base / "src"
    if not (src_dir / ".extracted").exists():
        if src_dir.exists():
            shutil.rmtree(src_dir)
        with tarfile.open(src_tgz) as t:
            t.extractall(src_dir, filter="data")
        (src_dir / ".extracted").write_text(src["sha256"])

    print(json.dumps({"version": ver, "win64": str(win_dir), "src": str(src_dir)}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
