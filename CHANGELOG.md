# Changelog

All notable changes are recorded here, following [Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow [Semantic Versioning](https://semver.org/). English is authoritative.

## [Unreleased]

No version has been released yet. The first public preview will be unsigned (DECISIONS D-2) and will list here what it contains and what has not been verified (DECISIONS D-13).

### Added

- Safe metadata writing core: every write is planned, previewed, backed up, verified (V1–V6) and recorded in a crash-safe Journal; interrupted operations are recovered at the next launch; every operation can be undone byte for byte.
- Fields: Creator, Copyright, capture time (Absolute, Shift, Sequence, Preserve Relative Timing), GPS (set, remove); template variables; Rules and Presets with import and export.
- JPEG written in place; NEF/NRW never written (changes go to XMP sidecars); TIFF, PNG, HEIC/HEIF, AVIF, WebP, DNG and other RAW formats read-only.
- Clean Export: JPEG copies with location, serial numbers and other private metadata removed as you choose, every removal listed before export; originals untouched.
- Desktop app (Tauri + React) in English and Simplified Chinese: Library, Inspector, batch editor, time tools, Preview with exclusions and acknowledgements, progress with safe Cancel, History with Undo, Retry and Restore to folder, Recovery, Settings, First Launch, backup management, optional signed updates.
- Settings › Advanced › Find history in a backup folder…, for a lost data folder.
- Help › About MoriMeta with the versions a bug report needs and the third-party notices.
- PRIVACY.md, docs/INSTALLATION.md, issue templates; dependency audit (cargo-deny, npm audit) in CI.

### Known limits

- Not yet verified: third-party readers (Lightroom, Capture One, NX Studio), power loss, real SD cards and NAS, cloud-sync clients, a 5,000-file camera corpus (DECISIONS D-13).
- Installer and update installation not yet tested on Windows 10/11 machines (S6).
