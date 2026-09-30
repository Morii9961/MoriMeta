# SPDX-License-Identifier: GPL-3.0-or-later
"""Pre-commit inclusion check (docs/REPOSITORY_CHECKLIST.md §2).

Checks the files staged for commit (default) or the paths given on the command line:
  * path rules      - build output, downloads, generated results, photos/raw, design-session files
  * size / binary   - > 1 MiB or containing NUL bytes
  * content rules   - local absolute paths, user-profile paths, this machine's name, e-mail
                      addresses, private keys and common token formats

A line containing `repo-check: allow` is exempt from content rules (use sparingly, explain why).
Exit status 1 if anything is blocked.

Usage:  python tools/check_repo.py            # staged files
        python tools/check_repo.py FILE...    # explicit files
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path, PurePosixPath

ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 1 << 20

PATH_DENY = [
    (re.compile(r"(^|/)target/"), "build output"),
    (re.compile(r"^research/\.work/"), "downloaded tools / corpora / lab files"),
    (re.compile(r"^research/results/"), "generated results (local paths, sample metadata)"),
    (re.compile(r"\.(jpe?g|nef|nrw|dng|cr2|cr3|arw|raf|rw2|orf|heic|heif|avif|tiff?|webp|png|psd)$", re.I), "image / raw file"),
    (re.compile(r"\.(zip|7z|tar|gz|exe|dll|pdb|msi)$", re.I), "archive / binary"),
    (re.compile(r"(^|/)\.env($|\.)"), "environment file"),
]

EMAIL_ALLOW = {"noreply@anthropic.com"}
EMAIL_ALLOW_DOMAINS = {"example.com", "example.org"}

CONTENT_RULES = [
    # placeholders such as C:\Users\<name> or %USERNAME% are not real paths
    (re.compile(r"[A-Za-z]:[\\/]+Users[\\/]+(?![<%{])[^\\/\s\"'<>]+", re.I), "Windows user-profile path"),
    (re.compile(r"(?<![\w.])/(home|Users)/[a-z0-9._-]+/", re.I), "Unix user-profile path"),
    (re.compile(r"[A-Za-z]:[\\/]+MoriMeta[\\/]", re.I), "absolute local project path"),
    (re.compile(r"-----BEGIN [A-Z ]*PRIVATE KEY-----"), "private key"),
    (re.compile(r"\b(ghp|gho|ghu|ghs|github_pat)_[A-Za-z0-9_]{20,}"), "GitHub token"),
    (re.compile(r"\bAKIA[0-9A-Z]{16}\b"), "AWS access key"),
    (re.compile(r"\bsk-[A-Za-z0-9_-]{20,}"), "API secret key"),
]
EMAIL = re.compile(r"[A-Za-z0-9._%+-]+@([A-Za-z0-9.-]+\.[A-Za-z]{2,})")


def staged() -> list[str]:
    out = subprocess.run(["git", "diff", "--cached", "--name-only", "--diff-filter=ACMR", "-z"],
                         cwd=ROOT, capture_output=True, check=True).stdout
    return [p for p in out.decode("utf-8").split("\0") if p]


def check(rel: str) -> list[str]:
    reasons = []
    posix = PurePosixPath(rel.replace("\\", "/"))
    s = str(posix)
    for rx, why in PATH_DENY:
        if rx.search(s):
            reasons.append(f"path: {why}")
    f = ROOT / rel
    if not f.is_file():
        return reasons
    data = f.read_bytes()
    if len(data) > MAX_BYTES:
        reasons.append(f"size {len(data)} bytes > 1 MiB")
    if b"\0" in data[:8192]:
        reasons.append("binary content")
        return reasons
    text = data.decode("utf-8", errors="replace")
    machine = os.environ.get("COMPUTERNAME", "")
    for n, line in enumerate(text.splitlines(), 1):
        if "repo-check: allow" in line:
            continue
        for rx, why in CONTENT_RULES:
            if rx.search(line):
                reasons.append(f"line {n}: {why}")
        for m in EMAIL.finditer(line):
            addr, dom = m.group(0), m.group(1).lower()
            if addr.lower() not in EMAIL_ALLOW and dom not in EMAIL_ALLOW_DOMAINS:
                reasons.append(f"line {n}: e-mail address {addr}")
        if machine and len(machine) >= 4 and machine.lower() in line.lower():
            reasons.append(f"line {n}: this machine's name")
    return reasons


def main(argv: list[str]) -> int:
    files = argv or staged()
    blocked = 0
    for rel in files:
        r = check(rel)
        if r:
            blocked += 1
            print(f"BLOCK {rel}")
            for x in r[:8]:
                print(f"      {x}")
    print(f"{len(files)} file(s) checked, {blocked} blocked")
    return 1 if blocked else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
