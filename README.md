# MoriMeta

**A local-first batch photo-metadata tool for photographers — preview every change, then apply it, and undo it while the backup is kept.** Windows first. Built on [ExifTool](https://exiftool.org/).

[![ci](https://github.com/Morii9961/MoriMeta/actions/workflows/ci.yml/badge.svg)](https://github.com/Morii9961/MoriMeta/actions/workflows/ci.yml)

> **Status: pre-alpha.** The core engine (planning, the transactional writer, recovery, undo) exists and is driven by a development command-line tool; there is **no user interface yet**, no usable release, no installer and no signed binary. Documents in `docs/` are drafts (v0.3) and not approved specifications. Licensed under GPL-3.0-or-later (see [License](#license)).

## What it is meant to do

- Batch-edit capture time (Absolute, Shift, Sequence, Preserve Relative Timing), creator, copyright and GPS for tens to thousands of photos.
- Show an exact before/after preview of every field, and execute exactly the plan that was previewed.
- Keep NEF originals untouched by writing XMP sidecars; back up JPEG/TIFF files before changing them; record every operation so it can be undone while the backup is retained and the file has not been changed by other software since.
- Privacy tools; their MVP scope is still open (decision D-15).

It is not a DAM, a raw converter, a photo editor or a cloud service. No telemetry, no accounts.

## Safety, stated with its conditions

Safety claims are limited to what has been tested (see [`docs/SPIKE_REPORT.md`](docs/SPIKE_REPORT.md) and [`docs/SAFETY_MODEL.md`](docs/SAFETY_MODEL.md) §0):

- On local NTFS and SMB (tested over loopback), a prototype of the single-file transaction survived 90 injected crash points and 450 random process kills with no file left damaged after recovery.
- The product core is tested the same way on every change (CI): a crash at every step of every transaction kind, random process kills with four parallel workers, injected IO and journal-write failures, simulated disk full; afterwards recovery, resume and undo must leave every file byte-identical to its original. A real disk-full run on a small NTFS test volume passed (run manually; see [`docs/PHASE1B_FAULT_MATRIX.md`](docs/PHASE1B_FAULT_MATRIX.md)).
- The test photos are ExifTool's own sample files (and, in local runs, three real Nikon NEFs); that is not yet a broad camera corpus, and third-party software reading the written metadata (Lightroom, Capture One, NX Studio) has not been checked.
- exFAT/FAT32 cards, real cloud-sync clients, power loss and real NAS devices have **not** been tested yet.

## Repository layout

| Path | Contents |
|---|---|
| `docs/` | Product spec, architecture, metadata/safety/security models, development and release plans, spike report, backend interface map for the future UI adapter (`BACKEND_INTERFACE.md`) and the engineering review of the design (Chinese, drafts) |
| `crates/` | Rust workspace: `mm-exiftool` (ExifTool process protocol), `mm-fs` (Windows file primitives), `mm-domain` (fields, time tools, templates, rules; no IO), `mm-store` (SQLite journal, backup manifests), `mm-core` (planner, transactional executor, recovery, undo, retention, backend interface for the future UI), `mm-cli` (development driver and end-to-end tests) |
| `research/` | Reproducible Phase 0 experiments and throwaway prototypes (not product code) |
| `tools/` | Repository checks |

## Building and testing

Requirements: Windows 10/11 x64, Rust 1.88 or newer, Python 3.11+ (for research scripts).

```text
python research/scripts/fetch_exiftool.py     # pinned ExifTool, SHA-256 verified (needed by integration tests)
cargo test --workspace
```

Integration tests that need ExifTool are skipped when it has not been fetched.

The journal uses SQLite (bundled). With the MSVC toolchain nothing else is needed; with the GNU
toolchain (`x86_64-pc-windows-gnu`) a MinGW `gcc` must be on `PATH` to compile it.

`mm-cli` is a development driver for the core (not the product):

```text
cargo run --release -p mm-cli -- --data <dir> --exiftool <pinned ExifTool folder> plan-creator --set "Name" --out plan.json <photos...>
cargo run --release -p mm-cli -- --data <dir> --exiftool <folder> apply plan.json
cargo run --release -p mm-cli -- --data <dir> --exiftool <folder> plan-undo <op-id> --out undo.json
```

Only work on copies of photos: this is pre-alpha software.

## Contributing and security

Contributions are welcome under the Developer Certificate of Origin (`git commit -s`); see [`CONTRIBUTING.md`](CONTRIBUTING.md). Report vulnerabilities privately as described in [`SECURITY.md`](SECURITY.md).

## License

MoriMeta is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. It is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See [`LICENSE`](LICENSE) (SPDX: `GPL-3.0-or-later`).

ExifTool is © Phil Harvey and is distributed under the same terms as Perl; it is invoked as a separate program.

---

## 简体中文

MoriMeta 是面向摄影师的本地批量照片元数据工具：先完整预览，再执行；在备份保留期内、且文件没有被其他软件改动时可以撤销。目前处于预发布阶段：核心引擎（规划、事务写入、恢复、撤销）已实现并由开发用命令行驱动，尚无用户界面与可用版本；`docs/` 中是 v0.3 草案，尚未批准。许可证为 GPL-3.0-or-later（见 `LICENSE`）。
