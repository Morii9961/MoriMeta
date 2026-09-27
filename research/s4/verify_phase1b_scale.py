"""Verify the Phase 1b 1,000-JPEG Creator apply/Undo exit condition.

Copies eight ExifTool fixture JPEGs into a new ignored work directory. This is
a scale and byte-round-trip test of mm-cli, not a real-camera corpus or a
third-party compatibility test. Nothing in the source corpus is modified.

Run after `cargo build --release -p mm-cli`:
    python research/s4/verify_phase1b_scale.py --count 1000

The complete local result is written under ignored research/results/. On any
failure, the work directory is deliberately retained for inspection.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import platform
import shutil
import subprocess
import sys
import time
from collections import Counter
from datetime import datetime, timezone
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / "scripts"))
import etlib as E  # noqa: E402

REPO = E.RESEARCH.parent
CLI = REPO / "target" / "release" / "mm-cli.exe"
SAMPLES = (
    "Writer.jpg",
    "Nikon.jpg",
    "Canon.jpg",
    "XMP.jpg",
    "Sony.jpg",
    "Olympus.jpg",
    "Pentax.jpg",
    "GPS.jpg",
)
CREATOR = "Scale Verification Creator"


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for chunk in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(chunk)
    return digest.hexdigest()


def mm(data: Path, *args: str) -> tuple[dict | list, float]:
    command = [str(CLI), "--data", str(data), "--exiftool", str(E.PKG), *args]
    start = time.perf_counter()
    proc = subprocess.run(command, capture_output=True)
    seconds = time.perf_counter() - start
    try:
        result = json.loads(proc.stdout.decode("utf-8"))
    except (UnicodeDecodeError, json.JSONDecodeError) as exc:
        raise RuntimeError(f"mm-cli {args[0]} returned invalid JSON ({proc.returncode})") from exc
    if proc.returncode != 0:
        detail = result.get("error", "unexpected result") if isinstance(result, dict) else "unexpected result"
        raise RuntimeError(f"mm-cli {args[0]} failed ({proc.returncode}): {detail}")
    return result, seconds


def states(report: dict) -> dict[str, int]:
    return dict(sorted(Counter(row["state"] for row in report["files"]).items()))


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--count", type=int, default=1000)
    parser.add_argument("--work", type=Path)
    parser.add_argument("--label", default="phase1b-scale")
    args = parser.parse_args()
    if args.count < 1:
        parser.error("--count must be positive")
    if not CLI.is_file() or not E.LAUNCHER.is_file():
        parser.error("build mm-cli and fetch the pinned ExifTool first")
    samples = [E.TIMAGES / name for name in SAMPLES]
    if not all(path.is_file() for path in samples):
        parser.error("pinned ExifTool source fixtures are missing")

    stamp = datetime.now(timezone.utc).strftime("%Y%m%dT%H%M%SZ")
    work = args.work or E.WORK / "s4" / f"{args.label}-{stamp}"
    if work.exists():
        parser.error("work directory already exists; choose a new path")
    photos = work / "photos"
    photos.mkdir(parents=True)
    data = work / "data"
    files: list[Path] = []
    source_counts: Counter[str] = Counter()
    before: dict[Path, str] = {}
    for index in range(args.count):
        source = samples[index % len(samples)]
        target = photos / f"{index:05d}.jpg"
        shutil.copyfile(source, target)
        files.append(target)
        source_counts[source.name] += 1
        before[target] = sha256(target)

    result = {
        "date_utc": stamp,
        "commit": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=REPO, text=True).strip(),
        "platform": f"{platform.system()} {platform.release()} build {platform.version()}",
        "exiftool_version": E.VERSION,
        "sample_type": "copies of eight ExifTool test fixture JPEGs; no unique camera corpus",
        "source_samples": [
            {"name": source.name, "count": source_counts[source.name], "bytes": source.stat().st_size,
             "sha256": sha256(source)} for source in samples
        ],
        "file_count": len(files),
        "input_bytes": sum(path.stat().st_size for path in files),
        "creator": CREATOR,
        "work": str(work),
    }
    print(f"Copied {len(files)} JPEGs; running plan/apply/scan/Undo on copies.", flush=True)

    try:
        paths_file = work / "files.txt"
        paths_file.write_text("\n".join(str(path) for path in files) + "\n", encoding="utf-8")
        names = work / "creator.txt"
        names.write_text(CREATOR + "\n", encoding="utf-8")
        plan_path = work / "plan.json"
        plan, result["plan_seconds"] = mm(
            data, "plan-creator", "--set-from", str(names), "--out", str(plan_path),
            "--files-from", str(paths_file),
        )
        result["plan_summary"] = plan["summary"]
        if len(plan["entries"]) != args.count or any(e["status"]["status"] != "ready" for e in plan["entries"]):
            raise AssertionError("plan did not contain exactly the requested ready entries")

        applied, result["apply_seconds"] = mm(data, "apply", str(plan_path))
        result["apply_states"] = states(applied)
        result["apply_status"] = applied["status"]
        result["apply_op_id"] = applied["op_id"]
        if result["apply_states"] != {"done": args.count}:
            raise AssertionError(f"apply states: {result['apply_states']}")
        result["changed_byte_hashes"] = sum(sha256(path) != before[path] for path in files)
        if result["changed_byte_hashes"] != args.count:
            raise AssertionError("not every JPEG changed at the byte level")

        scanned = 0
        scan_seconds = 0.0
        for offset in range(0, len(files), 100):
            rows, seconds = mm(data, "scan", *map(str, files[offset:offset + 100]))
            scan_seconds += seconds
            scanned += sum(row.get("creator") == [CREATOR] for row in rows)
        result["scan_seconds"] = scan_seconds
        result["creator_verified"] = scanned
        if scanned != args.count:
            raise AssertionError(f"Creator re-read matched for {scanned}/{args.count} files")

        checked, result["apply_fsck_seconds"] = mm(data, "fsck", applied["op_id"])
        result["apply_fsck_problems"] = checked["problems"]
        if checked["problems"]:
            raise AssertionError("apply fsck found problems")

        undo_path = work / "undo.json"
        undo_plan, result["undo_plan_seconds"] = mm(
            data, "plan-undo", applied["op_id"], "--out", str(undo_path)
        )
        result["undo_plan_summary"] = undo_plan["summary"]
        if len(undo_plan["entries"]) != args.count or any(e["status"]["status"] != "ready" for e in undo_plan["entries"]):
            raise AssertionError("Undo plan did not contain exactly the requested ready entries")
        undone, result["undo_seconds"] = mm(data, "apply", str(undo_path))
        result["undo_states"] = states(undone)
        result["undo_status"] = undone["status"]
        result["undo_op_id"] = undone["op_id"]
        if result["undo_states"] != {"done": args.count}:
            raise AssertionError(f"Undo states: {result['undo_states']}")
        result["byte_identical_after_undo"] = sum(sha256(path) == before[path] for path in files)
        if result["byte_identical_after_undo"] != args.count:
            raise AssertionError("Undo did not restore every original byte hash")

        checked, result["undo_fsck_seconds"] = mm(data, "fsck", undone["op_id"])
        result["undo_fsck_problems"] = checked["problems"]
        if checked["problems"]:
            raise AssertionError("Undo fsck found problems")
        result["temporary_files_left"] = sum(
            ".mmtmp-" in path.name or ".mmbak-" in path.name for path in photos.iterdir()
        )
        if result["temporary_files_left"]:
            raise AssertionError("temporary or bak files remain")
        result["passed"] = True
    except Exception as exc:
        result["passed"] = False
        result["failure"] = str(exc)
    finally:
        out = E.RESEARCH / "results" / "s4" / f"{args.label}-{stamp}.json"
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(result, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
        print(json.dumps({k: v for k, v in result.items() if k not in {"source_samples", "work"}},
                         ensure_ascii=False, indent=2), flush=True)
        print(f"Local result: {out}", flush=True)
    return 0 if result["passed"] else 1


if __name__ == "__main__":
    raise SystemExit(main())
