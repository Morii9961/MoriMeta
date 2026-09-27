# S5 — UI technology spike (throwaway; not product UI)

Checks whether Tauri 2 + React + WebView2 can carry the Library table and IPC volumes
(ARCHITECTURE ADR-01 exit criteria, V-09, V-18). No visual design decisions are made here; the
product UI waits for the design direction chosen through the Design Brief process.

## Build and run

```text
npm ci
python src-tauri/icons/make_icon.py          # Tauri needs icons/icon.ico on Windows (generated, not committed)
cd src-tauri && cargo test                  # also regenerates src/bindings/*.ts via ts-rs
cd .. && npx tauri build --no-bundle
MM_S5_AUTORUN=1 MM_S5_OUT=<file.json> src-tauri/target/release/mm-s5-ui.exe   # automated benchmark, exits by itself
src-tauri/target/release/mm-s5-ui.exe                                          # manual checks
```

## Automated results (2026-09-27, 3 runs; i9-13900HX, RTX 4080 Laptop, 240 Hz, 32 GB, 150 % scaling, WebView2 153)

| Measure | Result | ADR-01 criterion |
|---|---|---|
| IPC `rows`: 5,000 rows × 30 text fields (2.3 MB JSON) | 34–50 ms | — |
| First render of 5,000 rows (virtualized) | 12 ms | — |
| Continuous scroll, 5 s | 236–237 fps, p95 frame 4.3 ms, 0 frames > 33 ms | ≥ 50 fps on a **mid-range laptop**: not yet measured on such hardware |
| Sort 5,000 rows (date asc/desc, text) | 18–44 ms | < 200 ms ✔ |
| Global filter | 20–22 ms | < 200 ms ✔ |
| Channel: 5,000 progress events | 30–33 k events/s, none lost | — |
| ARIA structure | `role=grid`, `aria-rowcount=5001`, ~44 rows rendered | structure present; screen-reader behaviour is a manual check |
| Least privilege | capability file lists only the four spike commands (app manifest); `plugin:window\|set_title` → "not allowed by ACL"; Channel works without any core permission | ✔ |
| Type generation | ts-rs 11: `Row.ts`, `Progress.ts` generated from Rust structs | works for plain structs; tauri-specta (command signatures) not evaluated |
| Toolchain | Tauri 2.12 builds on `x86_64-pc-windows-gnu` locally | production builds should use MSVC in CI |

## Manual checks (to be done by a person; record results in docs/SPIKE_REPORT.md)

| # | Check | How | Pass when |
|---|---|---|---|
| M1 | Chinese IME in the filter box | Microsoft Pinyin: type `sen` → choose 森 | text appears once; filtering happens on the committed text |
| M2 | Chinese IME in inline edit | select a cell, F2, type with Pinyin, press Enter to choose a candidate | Enter chooses the candidate and does not commit the cell; a second Enter commits |
| M3 | Narrator: grid navigation | Win+Ctrl+Enter, focus the grid, arrow keys | Narrator reads column header, row and cell value |
| M4 | NVDA | same with NVDA | same |
| M5 | Scaling 100 % / 200 % | change Windows display scale, restart | layout intact, text crisp |
| M6 | Mid-range laptop | run the automated benchmark on e.g. a 4-core/integrated-GPU laptop at 60 Hz | ≥ 50 fps, no long frames during scroll |
| M7 | Windows 10 22H2 | automated benchmark + M1–M3 | same as above |
