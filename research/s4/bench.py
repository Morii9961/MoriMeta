"""S4 benchmark driver: metadata scan throughput and per-file write cost through mm-cli on a
corpus directory, with peak memory of mm-cli and ExifTool sampled once per second.

Usage:
  python research/s4/bench.py CORPUS_DIR [--work DIR] [--limit N] [--label TEXT]
  python research/s4/bench.py --synthetic N [--work DIR]      # tool smoke test, NOT real-corpus data

CORPUS_DIR is never modified: files are copied to --work (default research/.work/s4/<label>)
on the volume under test before anything is written. Put --work on the storage you want to
measure (NVMe, SATA SSD, HDD, NAS via \\\\server\\share).

Writes research/results/s4/<label>.json.
"""

from __future__ import annotations

import json
import os
import shutil
import statistics
import subprocess
import sys
import threading
import time
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

REPO = E.RESEARCH.parent
CLI = REPO / "target" / "release" / "mm-cli.exe"
EXTS = {".jpg", ".jpeg"}


class PeakMemory(threading.Thread):
    """Samples the working set of mm-cli.exe and exiftool/perl processes (bytes)."""

    def __init__(self) -> None:
        super().__init__(daemon=True)
        self.stop = threading.Event()
        self.peak: dict[str, int] = {}

    def run(self) -> None:
        cmd = ["powershell", "-NoProfile", "-Command",
               "Get-Process mm-cli,exiftool,perl -ErrorAction SilentlyContinue | ForEach-Object { $_.ProcessName + ' ' + $_.WorkingSet64 }"]
        while not self.stop.is_set():
            out = subprocess.run(cmd, capture_output=True, text=True).stdout
            for line in out.splitlines():
                parts = line.split()
                if len(parts) == 2 and parts[1].isdigit():
                    self.peak[parts[0]] = max(self.peak.get(parts[0], 0), int(parts[1]))
            time.sleep(1.0)


def mm(data: Path, *args: str) -> tuple[float, dict]:
    t0 = time.perf_counter()
    p = subprocess.run([str(CLI), "--data", str(data), "--exiftool", str(E.PKG), *args], capture_output=True)
    dt = time.perf_counter() - t0
    try:
        out = json.loads(p.stdout.decode("utf-8"))
    except Exception:
        out = {"raw": p.stdout.decode("utf-8", "replace")[:500], "stderr": p.stderr.decode("utf-8", "replace")[:500]}
    return dt, out


def main() -> int:
    args = sys.argv[1:]
    synthetic = int(args[args.index("--synthetic") + 1]) if "--synthetic" in args else 0
    label = args[args.index("--label") + 1] if "--label" in args else ("synthetic" if synthetic else "corpus")
    work = Path(args[args.index("--work") + 1]) if "--work" in args else E.WORK / "s4" / label
    limit = int(args[args.index("--limit") + 1]) if "--limit" in args else None
    if not CLI.exists():
        raise SystemExit("build first: cargo build --release -p mm-cli")
    if work.exists():
        shutil.rmtree(work)
    photos = work / "photos"
    photos.mkdir(parents=True)

    if synthetic:
        src = E.WORK / "corpus" / "jpeg" / "z8.jpg"
        sources = [src] * synthetic
        corpus_desc = f"SYNTHETIC: {synthetic} copies of {src.name} ({src.stat().st_size} bytes) — tool smoke test only"
    else:
        corpus = Path(args[0])
        sources = sorted(p for p in corpus.rglob("*") if p.suffix.lower() in EXTS)[:limit]
        corpus_desc = f"{corpus} ({len(sources)} JPEG files)"
    t0 = time.perf_counter()
    files = []
    for i, s in enumerate(sources):
        d = photos / f"{i:05d}{s.suffix.lower()}"
        shutil.copyfile(s, d)
        files.append(d)
    copy_s = time.perf_counter() - t0
    total_bytes = sum(f.stat().st_size for f in files)

    data = work / "data"
    mem = PeakMemory()
    mem.start()
    list_file = work / "files.txt"
    list_file.write_text("\n".join(str(f) for f in files), encoding="utf-8")
    # scan (single ExifTool session, chunks of 100)
    scan_s, _ = mm(data, "scan", *map(str, files)) if len(files) <= 800 else (None, None)
    names = work / "names.txt"
    names.write_text("Benchmark Creator\n", encoding="utf-8")
    plan = work / "plan.json"
    plan_s, plan_out = mm(data, "plan-creator", "--set-from", str(names), "--out", str(plan), *map(str, files))
    apply_s, apply_out = mm(data, "apply", str(plan))
    op = apply_out.get("op_id")
    undo_plan = work / "undo.json"
    uplan_s, _ = mm(data, "plan-undo", op, "--out", str(undo_plan)) if op else (None, None)
    undo_s, undo_out = mm(data, "apply", str(undo_plan)) if op else (None, {})
    mem.stop.set()
    mem.join(timeout=3)

    done = sum(1 for f in apply_out.get("files", []) if f.get("state") == "done")
    backup_bytes = sum(p.stat().st_size for p in (data / "backups").rglob("*") if p.is_file())
    db_bytes = sum(p.stat().st_size for p in (data / "db").glob("*") if p.is_file())
    result = {
        "label": label,
        "corpus": corpus_desc,
        "files": len(files),
        "total_bytes": total_bytes,
        "work_dir_volume": os.path.splitdrive(str(work))[0] or str(work)[:2],
        "copy_seconds": round(copy_s, 2),
        "scan_seconds": None if scan_s is None else round(scan_s, 2),
        "scan_files_per_s": None if scan_s is None else round(len(files) / scan_s, 1),
        "plan_seconds": round(plan_s, 2),
        "plan_summary": plan_out.get("summary"),
        "apply_seconds": round(apply_s, 2),
        "apply_done": done,
        "apply_ms_per_file": round(apply_s * 1000 / max(done, 1), 1),
        "apply_mb_per_s": round(total_bytes / 1e6 / apply_s, 1) if apply_s else None,
        "undo_plan_seconds": uplan_s and round(uplan_s, 2),
        "undo_seconds": undo_s and round(undo_s, 2),
        "undo_status": undo_out.get("status"),
        "backup_bytes": backup_bytes,
        "journal_db_bytes": db_bytes,
        "peak_working_set_bytes": mem.peak,
        "notes": "single ExifTool session, one file at a time (no worker pool yet); scan skipped above 800 files (command-line length)",
    }
    out = E.RESEARCH / "results" / "s4" / f"{label}.json"
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(json.dumps(result, indent=2), encoding="utf-8")
    print(json.dumps(result, indent=2))
    return 0


if __name__ == "__main__":
    sys.exit(main())
