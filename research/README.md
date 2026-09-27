# research/ — Phase 0 reproducible experiments

Throwaway prototypes and scripts behind `docs/SPIKE_REPORT.md`. Nothing here is product code.
Downloads and generated files live in `research/.work/` (ignored); results are written to `research/results/`.

## Prerequisites

- Windows 10/11 (x64). No administrator rights needed.
- Python 3.11+ (standard library only).
- Rust stable (`x86_64-pc-windows-gnu` or `-msvc`), for the spikes.

## Order

```text
python research/scripts/fetch_exiftool.py      # pinned ExifTool (research/exiftool.lock.json), SHA-256 verified
python research/s0/repro.py both               # S0: re-run cited ExifTool findings (launcher and direct Perl)
python research/s0/perl_equiv.py               # S0: launcher vs perl.exe exiftool.pl equivalence
python research/s1/argfile_probe.py            # S1: argfile line decoding table

cd research/spikes
cargo run --release --example roundtrip -p exiftool-session -- 100000 xml launcher 20260926
cargo run --release --example roundtrip -p exiftool-session -- 100000 xml perl 20260926
cargo run --release --example robust    -p exiftool-session -- launcher
cargo run --release --example share_matrix     -p fs-txn -- E:/path/to/lab
cargo run --release --example replace_failures -p fs-txn -- E:/path/to/lab D:/other-volume-dir
cargo run --release --example crash -p fs-txn -- roundtrip E:/path/to/lab 50
cargo run --release --example crash -p fs-txn -- sweep     E:/path/to/lab backup
cargo run --release --example crash -p fs-txn -- random    E:/path/to/lab 300
cargo build --release --bin jpegseg -p jpeg-segments
cd ../..

# NEF samples (CC0): download the files listed in research/corpus.lock.json into research/.work/corpus/
python research/s3/corpus.py                   # derived JPEG corpus
python research/s3/fields.py                   # S3 (ExifTool side + Windows property system)
python research/s7/clean_export.py             # S7 (needs jpegseg built)
```

SMB runs use the loopback administrative share, e.g. `//localhost/E$/path/to/lab` (pass forward slashes: some shells collapse `\\`).

## Notes

- All ExifTool calls follow the hardening under test: `-config ""` first, `-charset filename=utf8` before `-@ -`, arguments only via stdin argfile, empty environment except `SystemRoot`/`TEMP`/`TMP`, private working directory.
- Write experiment scripts as files. Passing non-ASCII text or backslashes through a shell command line has twice produced misleading results in this project.
