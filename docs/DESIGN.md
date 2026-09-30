# MoriMeta — Design (canonical)

**Status:** FROZEN · v1.0 · Direction A, Professional Darkroom
**Audience:** implementers (Claude Code / Sol). This file and the three companion specs are the implementation reference.

| Document | Covers |
|---|---|
| `DESIGN.md` (this) | Intent, principles, layout architecture, file map, source-of-truth order |
| `DESIGN_SYSTEM.md` | Tokens, states, dimensions, the 17 components |
| `SCREEN_SPEC.md` | Every screen and state, with its mock ID |
| `INTERACTION_SPEC.md` | Behaviour rules: batch semantics, Preview → Apply, gating, cancel, undo, keyboard |

## 1. What MoriMeta is

A local-first Windows desktop tool for photographers. It inspects, batch-edits, previews and safely applies EXIF / IPTC / XMP metadata changes across thousands of files. It is a working instrument, not a DAM, catalogue, SaaS dashboard or Lightroom clone.

Priority order, applied to every decision:
**Safety > Clarity > Speed > Information density > Consistency > Aesthetics > Novelty**

## 2. Design principles

1. **The table is the product.** The metadata table (or the diff table in Preview) is always the widest pane and the visual centre. Everything else is a side panel, a bar or a popover.
2. **Nothing is written without Preview.** Editing builds a *plan*. Only Preview can apply it. There is no “save” on any field.
3. **Say what will happen, where, and how to undo it.** Every write shows its target (in file / new sidecar / updated sidecar), its backup and its undo path before it happens.
4. **Unsafe is never silent.** Unsupported, skipped, read-only and conflicting items are always listed and counted, never hidden or quietly dropped.
5. **Glyph + colour + word.** Semantic state always uses all three (`+ Added`, `⊘ Unsupported`). Colour alone never carries meaning.
6. **Density with restraint.** 22 px rows, 11.5–12 px type, one accent hue, no decoration: no cards, gradients, hero areas, large headings, illustrations or glass.
7. **Red means blocked or failed, and says the files are safe.** Amber means attention, grey means “can’t, by design”. The strong red fill is reserved for one action: writing directly into RAW files (1.x).

## 3. Layout architecture (AppShell)

```
┌ MenuBar 28 ─────────────────────────────────────────────────────────────┐
├ Toolbar 38 — Library | Presets | Rules | History · tools · search · Preview ┤
├ ContextBanner 28–32 (optional: PREVIEW / APPLYING / RAW off / paused / blocking) ┤
├ Sidebar 232/264 ┬ Primary pane (table, min 560) ┬ Inspector 344/400 ┤
│ sources + facets│ 30 px table toolbar           │ fields / batch / │
│ or mode panel   │ MetadataTable / DiffTable     │ checks / progress│
├─────────────────┴──────────────────────────────┴───────────────────┤
├ ActionBar 46 (Preview only) — review checklist · Discard · Back · Apply ┤
└ StatusBar 24 — RAW safety · backup · undo · scan · ExifTool ───────────┘
```

- **Modules:** Library (default) · Presets · Rules · History, as a segmented control in the toolbar. Settings is a full window mode opened from the menu (Ctrl ,).
- **Preview replaces the primary pane in place.** It is not a new window or a modal. The table pivots into a diff; the sidebar becomes “Show changes” + edits in plan; the inspector becomes Checks / File detail.
- **Apply / progress / completion** reuse the same shell. The primary pane lists per-file status, and the inspector slot becomes the ProgressPanel, then the OperationSummary.
- **Status bar is permanent:** RAW Safe state, backup location and space, undo availability, scan progress and ExifTool state are visible on every screen.

## 4. Mock files (reference implementation of the look)

All mocks are Design Components with a review bar on top. That bar switches the screen/state and the frame (1440×900 · 1920×1080 · 2560×1440), with Fit or 100% zoom.

| File | Contents |
|---|---|
| `MoriMeta A2 1 Library.dc.html` | Library states, 6 inspector sections, stress: narrow / collapsed / long names / CJK |
| `MoriMeta A2 2 Batch and Time.dc.html` | Batch edit (mixed, staged, same, partial/unsupported), 6 capture-time modes |
| `MoriMeta A2 3 Rules and Presets.dc.html` | Rule builder (validation, edit, reorder), preset manager, apply, delete |
| `MoriMeta A2 4 Preview and Apply.dc.html` | Preview by file / by change, confirm, progress, cancel, completion, stress: backup unavailable |
| `MoriMeta A2 5 History Safety Settings.dc.html` | Journal, undo with conflicts, interrupted op, RAW Safe on / warn / off / confirm, 7 Settings pages |
| `MoriMeta A2 6 Launch and Errors.dc.html` | First launch (3 steps), 7 error states incl. recovery |
| `MoriMeta A2 Design System.dc.html` | Tokens, states, dimensions, component specimens, responsive + i18n rules |
| `morimeta-data.js`, `mm2-kit.js` | Mock data and mock helpers only. Not production code. |
| `baseline/…Approved Baseline v1.dc.html` | Frozen Phase 1 direction snapshot. Never edit. |

## 5. Source-of-truth order

1. `INTERACTION_SPEC.md`, for behaviour.
2. `DESIGN_SYSTEM.md` + the Design System page, for tokens and components.
3. `SCREEN_SPEC.md`, for which states exist and their content.
4. Mock files, for exact visual composition.

If a mock and a spec disagree, the spec wins; log the difference.

## 6. Scope notes

- **1.0 writes:** JPEG and TIFF (in file), NEF/NRW (XMP sidecar only). Reads: JPEG, TIFF, NEF/NRW, DNG, HEIC, CR2/CR3, ARW, RAF, PNG, WebP.
- **Shown as 1.x in the mocks** (visible, disabled or badged):
  - RAW Safe Mode off / direct NEF writes
  - random-interval and range-distribution time modes
  - keywords, title and description batch editing
  - custom ExifTool path
  - HEIC / DNG / other-RAW writing
- **Localisation:** English and 简体中文 UI. Metadata values display in any script. CJK fallback fonts: Microsoft YaHei UI, Yu Gothic UI.
- **Theme:** dark only in 1.0. A light theme is a later token remap.
