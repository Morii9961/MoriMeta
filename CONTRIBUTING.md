# Contributing to MoriMeta

Thank you for your interest. MoriMeta is pre-alpha: the core engine exists, the user interface does
not yet. Please open an issue before a larger change so that it can be discussed first.

## Developer Certificate of Origin (DCO)

MoriMeta is licensed under GPL-3.0-or-later. Contributions are accepted under the
[Developer Certificate of Origin 1.1](https://developercertificate.org/): by adding a
`Signed-off-by` line to a commit you certify that you wrote the change, or otherwise have the
right to submit it under the project's license.

Sign off every commit of a pull request with your real name and an e-mail address you can be
reached at:

```text
git commit -s
```

which adds

```text
Signed-off-by: Your Name <you@example.com>
```

A check on every pull request verifies that each commit carries a sign-off. There is no
Contributor License Agreement.

## Before you open a pull request

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `python research/scripts/fetch_exiftool.py` once (the pinned ExifTool, SHA-256 verified), then
  `cargo test --workspace` (tests that need ExifTool are skipped without it)
- `python tools/check_repo.py` on the files you add: the repository never contains photos, RAW
  files, raw research results, local paths or private metadata
  ([`docs/REPOSITORY_CHECKLIST.md`](docs/REPOSITORY_CHECKLIST.md))
- `python tools/check_architecture.py`: crate dependencies point one way, `mm-domain` stays
  pure, and the frontend can call only the app's own commands
- `python tools/check_ipc_types.py`: the UI's hand-written IPC types (`apps/desktop/src/ipc/types.ts`)
  match the Rust structs they mirror; a new IPC type needs a line in its table
- For the desktop app (`apps/desktop`): `npm ci`, `npm run build`, `npm test`; in `src-tauri`,
  `cargo fmt -- --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`. A new UI or
  backend text needs both languages (`src/i18n/messages.ts`; backend sentences in
  `src/i18n/backend.zh.json`, checked by `python tools/message_inventory.py --check
  apps/desktop/src/i18n/backend.zh.json`)
- New Rust source files start with `// SPDX-License-Identifier: GPL-3.0-or-later`

Where the UI repeats a backend rule to show a result while you type (GPS input, the time tools'
preview), both sides are tested against one case file in `crates/mm-domain/tests/`
(`gps_input_cases.json`, `time_text_cases.json`). Change the rule on both sides and extend the
case file; the backend's own test defines the expected results.

Changes that touch writing files — the transaction, recovery, undo, backups — need tests that
leave every file byte-identical to its original after recovery and undo
([`docs/SAFETY_MODEL.md`](docs/SAFETY_MODEL.md) §12). Test only on copies of photos.

## Security issues

Please do not open public issues for vulnerabilities; see [`SECURITY.md`](SECURITY.md).
