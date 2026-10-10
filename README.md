# MoriMeta

**A local-first batch photo-metadata tool for photographers — preview every change, then apply it, and undo it while the backup is kept.** Windows first. Built on [ExifTool](https://exiftool.org/).

[![ci](https://github.com/Morii9961/MoriMeta/actions/workflows/ci.yml/badge.svg)](https://github.com/Morii9961/MoriMeta/actions/workflows/ci.yml)

> **Status: pre-alpha.** The core engine (planning, the transactional writer, recovery, undo) and a first desktop app (`apps/desktop`, Tauri + React, English and 简体中文) exist. There is no public release and no signed binary yet; a local unsigned installer can be built (below). The specifications in `docs/` (v0.3) are approved, with the decisions recorded in [`docs/DECISIONS.md`](docs/DECISIONS.md). Licensed under GPL-3.0-or-later (see [License](#license)).

## What it is meant to do

- Batch-edit capture time (Absolute, Shift, Sequence, Preserve Relative Timing), creator, copyright and GPS for tens to thousands of photos.
- Show an exact before/after preview of every field, and execute exactly the plan that was previewed.
- Keep NEF originals untouched by writing XMP sidecars; back up JPEG/TIFF files before changing them; record every operation so it can be undone while the backup is retained and the file has not been changed by other software since.
- Privacy: remove GPS in place, and Clean Export — JPEG copies that keep only what you choose, each copy checked before it is written (D-15).

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
| `apps/desktop/` | The desktop app: Tauri adapter over `mm-core` (`src-tauri/`) and the React UI (`src/`), following the frozen design (`docs/DESIGN*.md`, `SCREEN_SPEC.md`, `INTERACTION_SPEC.md`) |
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

The desktop app (needs Node.js 20+):

```text
cd apps/desktop
npm ci
npm run dev                                   # the UI in a browser, with a mock backend (development only)
cargo build --manifest-path src-tauri/Cargo.toml   # then run src-tauri/target/debug/morimeta.exe with npm run dev running
node scripts/ui-smoke.mjs <copies-folder> <work-folder>   # drives the real app: edit, apply, verify on disk, undo
python scripts/stage-exiftool.py              # stage the checked ExifTool package and THIRD_PARTY_NOTICES.md for the installer
npx tauri build --config src-tauri/tauri.bundle.conf.json   # unsigned per-user NSIS installer
```

Only work on copies of photos: this is pre-alpha software.

## Code signing policy

Builds are not code-signed yet; releases come with SHA-256 checksums and GitHub build attestations. The roles, the signing process and the privacy statement for signed releases are in [`docs/CODE_SIGNING.md`](docs/CODE_SIGNING.md).

## Contributing and security

Contributions are welcome under the Developer Certificate of Origin (`git commit -s`); see [`CONTRIBUTING.md`](CONTRIBUTING.md). Report vulnerabilities privately as described in [`SECURITY.md`](SECURITY.md).

## License

MoriMeta is free software: you can redistribute it and/or modify it under the terms of the GNU General Public License as published by the Free Software Foundation, either version 3 of the License, or (at your option) any later version. It is distributed in the hope that it will be useful, but WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See [`LICENSE`](LICENSE) (SPDX: `GPL-3.0-or-later`).

ExifTool is © Phil Harvey and is distributed under the same terms as Perl; it is invoked as a separate program.

---

## 简体中文

MoriMeta 是面向摄影师的本地批量照片元数据工具：先完整预览，再执行；在备份保留期内、且文件没有被其他软件改动时可以撤销。目前处于预发布阶段：核心引擎（规划、事务写入、恢复、撤销）与第一版桌面应用（`apps/desktop`，中英文界面）已实现，尚无公开发布版本与签名安装包，可以在本地构建未签名的安装包（见上文）。`docs/` 中的 v0.3 规格已批准，各项决定见 `docs/DECISIONS.md`。许可证为 GPL-3.0-or-later（见 `LICENSE`）。

代码签名政策（Code signing policy）：目前的构建尚未签名，发布时附 SHA-256 校验和与 GitHub 构建证明；角色、签名流程与隐私声明见 [`docs/CODE_SIGNING.md`](docs/CODE_SIGNING.md)。
