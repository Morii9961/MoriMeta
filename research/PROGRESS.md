# Progress (2026-09-26)

docs/ are **v0.3 drafts** (not approved); evidence in `docs/SPIKE_REPORT.md`; raw results in `research/results/`.

## Done

- **S0** ExifTool pinned to 13.59; cited findings reproduced by script (F-28 corrected, F-38/F-11 revised); direct `perl.exe exiftool.pl` equivalent to the launcher.
- **S1** Rust stay_open protocol: 100,000/100,000 exact (`-ex` + JSONQ); injection, forged terminators, crash, hang, stderr flood, orphan prevention pass.
- **S2** File transaction (NTFS + SMB loopback): lock handle + ReplaceFileW; 90 crash-point cases + 450 random kills, 0 invariant violations; ReplaceFileW is not atomic under process kill (recovered); existing file at bak name is overwritten (guard added).
- **S3** ExifTool side: explicit mapping over MWG (MWG writes `?` silently); IPTC byte preflight; list-tag + tagsFromFile rule; time-field locations; sidecar preservation; MakerNotes serial behaviour; JUMBF detection; Windows property display; NEF ImageDataHash stable.
- **S7** Clean Export JPEG: 53 sources, preview removal set = actual 53/53, all outputs pass segment + tag whitelist + ImageDataHash; 4/4 negative controls blocked.
- **Docs v0.3**: all eight documents revised + SPIKE_REPORT.
- **Phase 1a foundation** (`crates/`): mm-exiftool, mm-fs, mm-domain — 33 tests, clippy/fmt clean.

## Not done / needs the user

- S3 third-party software (LR Classic 15, Capture One, NX Studio, …), S4 performance on real corpus, S5 UI tech check, S6 packaging/updater rehearsal.
- exFAT, cloud-sync folders, power-loss (VM) testing.
- Decisions: D-1 license, D-2 signing route, D-15 privacy scope, D-16, D-17, D-18 (see docs/DEVELOPMENT_PLAN.md §8).
- Design: three visual directions for selection (Design Brief §21) before any product UI.

Toolchain: Rust 1.98.1 `x86_64-pc-windows-gnu`, per-user, PATH not modified: `%USERPROFILE%\.cargo\bin\cargo`.
