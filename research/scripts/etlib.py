"""Shared helpers for MoriMeta research scripts.

Every ExifTool invocation here follows the hardening rules under test:
  * first argument is `-config ""` (no config file is loaded),
  * `-charset filename=utf8` precedes `-@ -`,
  * all other arguments travel as UTF-8 lines on stdin (argfile),
  * the child environment is emptied except SystemRoot/TEMP/TMP,
  * the working directory is an empty private directory.
"""

from __future__ import annotations

import hashlib
import json
import os
import shutil
import subprocess
import time
from dataclasses import dataclass
from pathlib import Path

RESEARCH = Path(__file__).resolve().parents[1]
WORK = RESEARCH / ".work"
LOCK = json.loads((RESEARCH / "exiftool.lock.json").read_text(encoding="utf-8"))
VERSION = LOCK["version"]
PKG = WORK / "exiftool" / VERSION / "win64" / f"exiftool-{VERSION}_64"
SRC = WORK / "exiftool" / VERSION / "src" / f"Image-ExifTool-{VERSION}"
TIMAGES = SRC / "t" / "images"
LAUNCHER_ORIG = PKG / "exiftool(-k).exe"
LAUNCHER = PKG / "exiftool.exe"  # renamed copy: brackets in the name are parsed as options
PERL = PKG / "exiftool_files" / "perl.exe"
SCRIPT = PKG / "exiftool_files" / "exiftool.pl"
CWD = WORK / "run" / "exiftool-cwd"
TMPDIR = WORK / "run" / "tmp"

CREATE_NO_WINDOW = 0x08000000


def ensure_layout() -> None:
    if not PKG.exists():
        raise SystemExit("run research/scripts/fetch_exiftool.py first")
    if not LAUNCHER.exists():
        shutil.copy2(LAUNCHER_ORIG, LAUNCHER)
    CWD.mkdir(parents=True, exist_ok=True)
    TMPDIR.mkdir(parents=True, exist_ok=True)


def clean_env() -> dict[str, str]:
    return {
        "SystemRoot": os.environ.get("SystemRoot", r"C:\Windows"),
        "TEMP": str(TMPDIR),
        "TMP": str(TMPDIR),
    }


def base_cmd(mode: str = "launcher") -> list[str]:
    if mode == "launcher":
        return [str(LAUNCHER)]
    if mode == "perl":
        return [str(PERL), str(SCRIPT)]
    raise ValueError(mode)


@dataclass
class Result:
    code: int
    stdout: bytes
    stderr: bytes
    seconds: float

    @property
    def out(self) -> str:
        return self.stdout.decode("utf-8", "replace")

    @property
    def err(self) -> str:
        return self.stderr.decode("utf-8", "replace")

    def json(self):
        return json.loads(self.stdout.decode("utf-8"))


def run(lines: list[str], mode: str = "launcher", env: dict[str, str] | None = None,
        extra_cli: list[str] | None = None, timeout: float = 120) -> Result:
    """Run one exiftool command with `lines` passed as a UTF-8 argfile on stdin."""
    ensure_layout()
    cmd = base_cmd(mode) + ['-config', '', '-charset', 'filename=utf8'] + (extra_cli or []) + ['-@', '-']
    data = ("\n".join(lines) + "\n").encode("utf-8")
    t0 = time.perf_counter()
    p = subprocess.run(cmd, input=data, capture_output=True, cwd=CWD,
                       env=clean_env() if env is None else env,
                       timeout=timeout, creationflags=CREATE_NO_WINDOW)
    return Result(p.returncode, p.stdout, p.stderr, time.perf_counter() - t0)


def run_raw_stdin(data: bytes, mode: str = "launcher", timeout: float = 120) -> Result:
    ensure_layout()
    cmd = base_cmd(mode) + ['-config', '', '-charset', 'filename=utf8', '-@', '-']
    t0 = time.perf_counter()
    p = subprocess.run(cmd, input=data, capture_output=True, cwd=CWD, env=clean_env(),
                       timeout=timeout, creationflags=CREATE_NO_WINDOW)
    return Result(p.returncode, p.stdout, p.stderr, time.perf_counter() - t0)


def read_tags(path: Path | str, tags: list[str] | None = None, mode: str = "launcher",
              numeric: bool = True, extra: list[str] | None = None) -> dict:
    lines = ["-json", "-G1", "-a", "-struct"] + (["-n"] if numeric else []) + (extra or [])
    lines += [f"-{t}" for t in (tags or [])]
    lines.append(str(Path(path).resolve()).replace("\\", "/"))
    r = run(lines, mode=mode)
    if not r.stdout.strip():
        raise RuntimeError(f"no output for {path}: {r.err}")
    return r.json()[0]


def p(path: Path | str) -> str:
    """Absolute forward-slash path for argfiles."""
    return str(Path(path).resolve()).replace("\\", "/")


def sha256(path: Path | str) -> str:
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def fresh_dir(name: str) -> Path:
    d = WORK / "lab" / name
    if d.exists():
        shutil.rmtree(d)
    d.mkdir(parents=True)
    return d
