# Performance (S4, real corpus)

Measures scan throughput, per-file write cost (backup + ExifTool write + verification + commit),
undo cost, backup/journal size and peak memory, to confirm or revise PRODUCT_SPEC §7.1 (V-06, V-20, V-25).

## Tool

```text
cargo build --release -p mm-cli
python research/s4/bench.py CORPUS_DIR --work X:\mm-bench --label nvme-jpeg-1000 [--limit 1000]
python research/s4/bench.py --synthetic 200        # smoke test of the tool only (not real data)
```

The corpus directory is never modified; files are copied to `--work` first, and `--work` is the
volume being measured. Results go to `research/results/s4/<label>.json` (not committed: they contain
local paths). Summaries go into `docs/SPIKE_REPORT.md`.

## Matrix to run

| Corpus | Sizes | Storage |
|---|---|---|
| JPEG straight from camera (Nikon Z preferred), 10–40 MB each | 100 / 1,000 / 5,000 | NVMe SSD, SATA SSD, HDD, NAS (SMB) |
| NEF 25–80 MB (+ sidecar writes once implemented) | 100 / 1,000 | NVMe SSD, HDD |

Record per run: Windows build, CPU, RAM, disk model, antivirus on/off, MoriMeta commit, ExifTool version.

## Current limits of the tool

- mm-cli runs one ExifTool session and processes files one after another (no worker pool yet), so
  results are a lower bound for throughput; the pool (ARCHITECTURE §7.5) is measured when implemented.
- `scan` passes paths on the command line and is skipped above 800 files.
- Only JPEG + creator is writable in the current build.

## Smoke-test data point (synthetic, 2026-09-27; not a performance result)

200 copies of one 5.9 MB Z8-derived JPEG on a local NVMe NTFS volume: scan 121 files/s; apply
121 ms/file (48.7 MB/s) including backup, write, verification and commit; undo 43 ms/file; backup
store 2 × corpus size after apply + undo; journal 0.65 MB; peak working set ExifTool 62 MB, mm-cli 11 MB.
At this rate 5,000 files would take about 10 minutes sequentially.

## Resources needed (D-13)

- A real corpus: ≥ 5,000 camera JPEGs (10–40 MB) and ≥ 1,000 NEF (25–80 MB). The user's own photos
  (copies) are best; raw.pixls.us has single samples only.
- Access to an HDD, a SATA SSD and a NAS share for the storage matrix.
