# MoriMeta

**A local-first batch photo-metadata tool for photographers — preview every change, then apply it, and undo it while the backup is kept.** Windows first. Built on [ExifTool](https://exiftool.org/).

> **Status: pre-alpha, research and foundation phase.** There is no usable release, no installer, and no signed binary. Documents in `docs/` are drafts (v0.3) and not approved specifications. The license has not been chosen yet (see [License](#license)).

## What it is meant to do

- Batch-edit capture time (Absolute, Shift, Sequence, Preserve Relative Timing), creator, copyright and GPS for tens to thousands of photos.
- Show an exact before/after preview of every field, and execute exactly the plan that was previewed.
- Keep NEF originals untouched by writing XMP sidecars; back up JPEG/TIFF files before changing them; record every operation so it can be undone while the backup is retained and the file has not been changed by other software since.
- Privacy tools; their MVP scope is still open (decision D-15).

It is not a DAM, a raw converter, a photo editor or a cloud service. No telemetry, no accounts.

## Safety, stated with its conditions

Safety claims are limited to what has been tested (see [`docs/SPIKE_REPORT.md`](docs/SPIKE_REPORT.md) and [`docs/SAFETY_MODEL.md`](docs/SAFETY_MODEL.md) §0):

- On local NTFS and SMB (tested over loopback), a prototype of the single-file transaction survived 90 injected crash points and 450 random process kills with no file left damaged after recovery.
- exFAT/FAT32 cards, cloud-sync folders, power loss and real NAS devices have **not** been tested yet.

## Repository layout

| Path | Contents |
|---|---|
| `docs/` | Product spec, architecture, metadata/safety/security models, development and release plans, spike report (Chinese, drafts) |
| `crates/` | Rust workspace: `mm-exiftool` (ExifTool process protocol), `mm-fs` (Windows file primitives), `mm-domain` (time tools and value rules), more to come |
| `research/` | Reproducible Phase 0 experiments and throwaway prototypes (not product code) |
| `tools/` | Repository checks |

## Building and testing

Requirements: Windows 10/11 x64, Rust stable, Python 3.11+ (for research scripts).

```text
python research/scripts/fetch_exiftool.py     # pinned ExifTool, SHA-256 verified (needed by integration tests)
cargo test --workspace
```

Integration tests that need ExifTool are skipped when it has not been fetched.

## License

Not chosen yet. MoriMeta will be released as open source; the choice between Apache-2.0 and GPL-3.0-or-later (or a combination) is pending ([`docs/LICENSE_DECISION.md`](docs/LICENSE_DECISION.md)). Until a `LICENSE` file is added, no license is granted.

ExifTool is © Phil Harvey and is distributed under the same terms as Perl; it is invoked as a separate program.

---

## 简体中文

MoriMeta 是面向摄影师的本地批量照片元数据工具：先完整预览，再执行；在备份保留期内、且文件没有被其他软件改动时可以撤销。目前处于预发布前的研究与基础实现阶段，没有可用版本；`docs/` 中是 v0.3 草案，尚未批准。许可证尚未确定。
