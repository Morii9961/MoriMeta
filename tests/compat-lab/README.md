# Compatibility lab (S3 third-party part)

Checks whether other applications read what MoriMeta writes the way the Preview promises
(RESEARCH_NOTES V-03, V-05, V-07, V-14, V-15, V-19; DEVELOPMENT_PLAN §5.3). Results decide the
frozen field registry v1 and D-18 (time-zone correction).

## Generate the test files

```text
python research/scripts/fetch_exiftool.py
python research/s3/corpus.py                 # needs the CC0 NEFs from research/corpus.lock.json
cargo build --release -p mm-cli
python research/s3/compat_corpus.py          # -> research/.work/compat-lab/<date>/
```

Output: `files/` (test files), `cases.csv` (what was written and why), `checklist.csv` (one row per
case × application, columns to fill in: app_version, observed, pass, notes). Source column:
`mm-cli` = real product path; `candidate` = mapping planned in METADATA_MODEL but not yet product
code; `manual` = needs a step inside the application.

| Case | Purpose |
|---|---|
| C1 | CJK creator written by mm-cli (EXIF UTF-8 + XMP) |
| C2 | creator in a file with Latin IPTC + IPTCDigest (mm-cli) |
| C4–C6 | capture time with kept / changed / absent UTC offset (D-18, V-07) |
| C7 | IPTC converted to UTF-8 with CJK By-line (METADATA_MODEL §6 option ①) |
| C8 | Nikon MakerNotes serial set to empty: does lens identification survive? (V-19) |
| N1–N5 | NEF + minimal `<basename>.xmp`: creator, time fields (all / photoshop only / exif only), GPS (V-03, V-07) |
| N6 | LR 15 `.acr` coexistence (manual) |
| E1 | Clean Export output vs original: orientation and colour in browsers |

## Procedure

1. Copy `files/` to the test machine; never test on the only copy of anything.
2. For each application, import or open the files with default settings; for Lightroom Classic also
   run "Read Metadata from Files" for the raw cases and note the catalog settings used.
3. Fill in `checklist.csv`; add screenshots where the display is ambiguous.
4. Commit only the filled-in checklist and a short summary (no photos): `tests/compat-lab/results/<date>-<app>.csv`.
   Run `python tools/check_repo.py` before committing (application paths in screenshots/notes can leak user names).

## Automated part

Windows Explorer / Photos reads through the Windows Property System; `research/s3/shellprops.ps1`
records Author, Copyright and Date taken for any list of files (S3 already did this for C1-type
files: CJK author/copyright display correctly; Date taken ignores OffsetTimeOriginal).

## Resources needed (D-13) — not available in this environment

| Resource | Needed for |
|---|---|
| Lightroom Classic 15.x (+ Bridge/ACR) licence and a test machine | C1–C8, N1–N6 |
| Capture One | C1–C8, N1–N5 |
| Nikon NX Studio | C8 (serial/lens), N1–N5 |
| darktable, digiKam (free) | N1–N5 (can be installed on request) |
| Photo Mechanic | C1–C7, N1–N5 (optional) |
| Nikon Z-series camera JPEG/NEF files from the user's own cameras | realistic MakerNotes/MPF; replaces derived samples |
| A real C2PA-signed JPEG (e.g. from a camera or Adobe export) | V-14 beyond the synthetic JUMBF test |
| Windows 10 22H2 machine | WIN row on the older OS |
