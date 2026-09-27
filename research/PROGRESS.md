# Progress (2026-09-27)

Local Git repository (no remote). docs/ are v0.3 drafts (not approved). Evidence: `docs/SPIKE_REPORT.md`, `docs/PHASE1_REPORT.md`.

## Done

- **Git baseline** + `tools/check_repo.py` (inclusion check) + `docs/REPOSITORY_CHECKLIST.md`, `README.md`, `docs/LICENSE_DECISION.md`.
- **S0/S1/S2/S3(ExifTool side)/S7**: see SPIKE_REPORT. S2 remains partial (no exFAT, cloud-sync, power loss, real NAS).
- **Phase 1a/1b**: mm-exiftool, mm-fs, mm-domain, mm-store (SQLite journal), mm-core (planner, verification, transaction, recovery, resume, undo, fsck), mm-cli. JPEG + Creator end to end; crash (20 points + random kills) and injected IO errors (10 steps) all recover; undo byte-identical (53 tests at that point).
- **S3/S4 preparation**: `research/s3/compat_corpus.py` + `tests/compat-lab/`; `research/s4/bench.py` + `tests/perf/`.
- **S5 (automated part)**: `research/spikes/s5-ui` — Tauri 2.12 + React; numbers in SPIKE_REPORT §5a.
- **Phase 1b scale (limited)**: 1,000 copies of 8 ExifTool fixtures, Creator apply → undo byte-identical (`docs/PHASE1B_SCALE_VALIDATION.md`); not a camera corpus or S4 result.
- **G-1 fault matrix** (`docs/PHASE1B_FAULT_MATRIX.md`): journal write failures (real SQLITE_BUSY, once/persistent), simulated disk full → Operation pauses and resumes, undo-path crashes/IO errors, space pre-check (§6.2). 62 tests. Real disk full on a 64 MB NTFS VHDX (photo volume; backup/journal volume with real SQLITE_FULL): passed, run separately with `MM_E2E_SMALL_VOLUME`. Round 2: manifest / recover / resume write failures, undo random kills; fixed resume stranding re-registered files in a finished Operation. 66 tests.
- **G-6**: undo recreates a deleted/moved file at its path from the backup (no-overwrite rename, journal role `recreate`); missing folder → Blocked; undoing a recreate → Blocked (needs move-to-backup-store). 69 tests.

## Waiting / needs the user

- Design session files (`docs/DESIGN.md`, `DESIGN_SYSTEM.md`, `SCREEN_SPEC.md`, `INTERACTION_SPEC.md`): not touched, not committed; review against the architecture after handoff.
- Decisions: D-1 license + repository owner (then public repo), D-2 signing, D-15 (interim direction (c)), D-18 (after S3 third-party results).
- Resources (D-13): LR/C1/NX Studio etc., real 5,000-file corpus, mid-range laptop / Win10 for S5 manual items, VM for power loss, exFAT media, a safe cloud-sync folder.
- Commit identity for the public push (docs/REPOSITORY_CHECKLIST.md P-3).

## Next engineering steps (no design dependency)

- Worker pool + per-volume IO limits (ARCHITECTURE §7.5/§8), then S4 on real data.
- More real disk-full positions (after commit, during undo; volume needs remounting); wider fault positions; manifest per-file append (SAFETY_MODEL §6.1) + rewrite after a failed write, with manifest-only recovery (G-7); move-to-backup-store transaction (undo of a recreate; new sidecars in Phase 3).
- Time tools (Absolute/Shift/Sequence/Preserve) through the same Plan/transaction path.
