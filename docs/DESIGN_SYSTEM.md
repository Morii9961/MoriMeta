# MoriMeta — Design System (canonical)

FROZEN · v1.0. Visual specimens: `MoriMeta A2 Design System.dc.html`. Values are px at 100% Windows scaling.

---

## 1. Color tokens

### Surfaces
| Token | Hex | Use |
|---|---|---|
| bg.chrome | #111315 | menu bar, status bar |
| bg.panel | #15171a | sidebar, inspector, popover body |
| bg.app | #17191c | toolbar, pane footers |
| bg.table | #191b1e | primary pane; row default #18191c |
| bg.row.alt | #1c1e21 | zebra row |
| bg.header | #1e2124 | table header, dialog, popover |
| bg.section | #1b1d20 | inspector section header |
| bg.input | #121416 | input fields |
| bg.hover | #23292e | hover row, active nav / source |

### Text
| Token | Hex | Use |
|---|---|---|
| text.strong | #eef1f4 | titles, counts, focused row |
| text.primary | #d9dde2 | body, values |
| text.secondary | #9aa0a7 | labels, column headers |
| text.muted | #7d848c | meta, descriptions |
| text.faint | #6b7178 | hints, legends |
| text.section | #7c838b | uppercase pane-section labels |
| text.empty | #4a4f55 | “—” empty value |
| text.disabled | #5d636a | disabled controls |

### Borders
| Token | Hex | Use |
|---|---|---|
| border.subtle | #1f2225 | row separators |
| border.pane | #26292d | pane edges, section dividers |
| border.cell | #2a2d31 | header cell dividers |
| border.control | #33373c | inputs, segmented controls |
| border.button | #3a3f45 | secondary buttons, popovers |
| border.strong | #4a4f55 | checkbox off, hover border |

### Accent (single hue)
| Token | Hex | Use |
|---|---|---|
| accent | #5ab4d0 | focus, selection, the one primary action |
| accent.text | #bfe6f2 | text on accent-outline buttons |
| accent.soft | #1f4250 | selected segment |
| accent.input | #3d6f80 | staged value border |
| select.row | #203846 | selected row |
| select.focus | #2a4a5c | focused + selected row (+ 1 px accent inset ring) |
| focus.ring | #8fd3e8 | keyboard focus: `0 0 0 1px bg, 0 0 0 3px #8fd3e8` |
| banner.preview | #1a2b33 / border #2a4552 | PREVIEW / APPLYING banner |

## 2. Semantic colors

### Change kinds (Preview, rules, batch impact)
| Glyph | Label | Token | Hex | Cell treatment |
|---|---|---|---|---|
| + | Added | change.add | #6cc58a | after value in green |
| ~ | Modified | change.mod | #7fa8ea | before struck through, after in primary text |
| − | Removed | change.rem | #e8866f | before struck through, after “removed” in red-orange |
| ! | Warning | change.warn | #e3b45c | amber tint rgba(227,180,92,.08) |
| ⊘ | Unsupported | change.uns | #8b9199 | 135° hatch `rgba(255,255,255,.04–.05)`; not toggleable |
| = | No change | change.none | #5d636a | value in text.faint |
| ↺ | Excluded | change.excluded | #7d848c | 1 px #3a3f45 outline, struck, 55% |

### Outcomes and states
| Glyph | Meaning | Hex |
|---|---|---|
| ✓ | succeeded / verified / OK | #6cc58a |
| × | failed / blocked (always paired with “original unchanged”) | #ef6a5e |
| – | skipped (read-only etc.) | #8b9199 |
| ≠ | conflict (sources disagree; changed since operation) | #e3b45c |
| i | info | #7fa8ea |
| ● / ◐ | RAW Safe on / off | #6cc58a / #e3b45c |

### Tone triplets (banners, inline panels)
| Tone | fg glyph | text | bg | border |
|---|---|---|---|---|
| Success | #6cc58a | #a9dcb8 | #161d19 | #2e4a37 |
| Warning | #e3b45c | #e8cf9c | #1f1c16 | #4a3d24 |
| Paused / RAW off | #e3b45c | #ecd9ae | #2a2414 | #5c4a24 |
| Error | #ef6a5e | #f0b3ab | #211816 (#261a19 banner) | #5a2e2a |
| Info | #7fa8ea | #b9cdf0 | #171c24 | #2e3a4c |
| Neutral / unsupported | #8b9199 | #c3c8ce | #1b1d20 | #33373c |
| Destructive button | fill #9c3329 · border #c94a3f · text #fff | | | |

Row tints: warning row #1f1c16, error row #211816. Colour never appears without its glyph and word.

## 3. Typography
Families:
- **UI:** `IBM Plex Sans`, then Microsoft YaHei UI / Yu Gothic UI.
- **Data:** `IBM Plex Mono`, same fallbacks, tabular figures.

| Token | Spec | Use |
|---|---|---|
| type.title | Sans 600 16/20 | dialog-less page titles (rare) |
| type.heading | Sans 600 13/18 | file name in inspector, summary headings |
| type.body | Sans 400 12/16 | default UI |
| type.label | Sans 400 11.5/16 | field labels, column headers (500) |
| type.meta | Sans 400 11/15 | descriptions, notes |
| type.section | Sans 600 10.5, uppercase, +0.07em | pane-section headers (zh: no caps/tracking) |
| mono.value | Mono 400 11.5/16 | values, dates, names in tables |
| mono.small | Mono 400 11/15 | tags, shortcuts, counts in facets |
| mono.tag | Mono 500 9.5–10.5 | row flags, provenance, badges |
| mono.count | Mono 600 14–16 | summary counts only |

Rules:
- Nothing is larger than 16 px.
- File names, times, numbers, tags, paths and shortcuts are always mono.
- Uppercase is used only for section labels and status tags.

## 4. Spacing (2 px base)
`sp.1 2 · sp.2 4 · sp.3 6 · sp.4 8 · sp.5 10 · sp.6 12 · sp.7 14 · sp.8 16 · sp.10 20`

| Context | Spacing |
|---|---|
| Cell padding x | 8 |
| Sidebar / toolbar padding x | 10 |
| Inspector / panel padding x | 12 |
| Dialog padding x | 14 |
| Settings page | 20 |

Nothing inside panes uses more than 16.

## 5. Borders, radii, elevation
**Borders:**
- hair `1px #1f2225`
- pane `1px #26292d`
- control `1px #33373c`
- focus `1px #5ab4d0`
- dashed `1px dashed #3a3f45` (empty state, same-value box)
- drop line `2px #5ab4d0`

**Radii:**

| Token | Value | Use |
|---|---|---|
| radius.0 | 0 | panes, tables, banners |
| radius.sm | 2 | inputs, checkboxes, badges, rule tokens |
| radius.md | 3 | buttons, segmented controls, popovers |
| radius.lg | 4 | dialogs |

**Elevation:** only floating layers have shadows.
- Popover: `0 8px 24px rgba(0,0,0,.45)`
- Dialog: `0 16px 40px rgba(0,0,0,.55)`, over scrim `rgba(8,9,10,.55–.62)`

## 6. Dimensions
| Element | 1440 | 1920 | 2560 | Notes |
|---|---|---|---|---|
| Menu bar | 28 | 28 | 28 | |
| Toolbar | 38 | 38 | 38 | controls 24 |
| Context banner | 28–32 | = | = | |
| Status bar | 24 | 24 | 24 | segments padding 0 10 |
| Preview action bar | 46 | 46 | 46 | |
| Sidebar | 232 | 232 | 264 | resize 200–320; rail 28 |
| Inspector | 344 | 344 | 400 | resize 280–480; rail 28 |
| Time tools panel | 440 | 440 | 500 | |
| History journal | 300 | 300 | 360 | |
| Primary pane min | 560 | 560 | 560 | |
| Table header | 24 | | | |
| Table toolbar | 30 | | | |
| Row compact / comfortable | 22 / 28 | | | |
| Diff row by file / by change / tag | 34 / 24 / 20 | | | |
| Preset row | 40 | | | |
| Button | 24 (dialog 26) | | | padding 0 12 |
| Input: inspector / batch, filter, rule / settings, dialog | 20 / 22 / 24 | | | |
| Checkbox | 11 (dialog 13) | | | |
| Search | 24×280 | 24×280 | 24×320 | |
| Dialog | 460–560 wide | | | never full screen |

## 7. States
- **Hover:** row → #23292e; secondary button → border #4a4f55, bg #23272b; tab → bg #1e2124. No hover on disabled items.
- **Focus (keyboard):** 2 px outer ring #8fd3e8 with 1 px gap. Focused row → 1 px accent inset ring. Focused input → accent border + `0 0 0 2px rgba(90,180,208,.25)`.
- **Selection:**
  - selected row #203846
  - focused + selected #2a4a5c + ring
  - selected segment #1f4250 / text #bfe6f2
  - Leave segment selected: neutral #2a2e33
- **Disabled:**
  - text #5d636a, border #2c3035, no fill.
  - Disabled primary buttons show their reason in adjacent text.
  - Excluded rows/files: 42% opacity.
  - Disabled rules: 42%.
- **Staged (plan):**
  - batch field bg #1a2227 + ● accent dot
  - value input border #3d6f80
  - impact line in change colours
- **Warning:** amber tone triplet; row tint #1f1c16; field border #6b5530 with ≠.
- **Error:** red tone triplet; invalid input border #8a3a33 bg #1f1716. Every error states “your files are safe / unchanged” when true.
- **Success:** green glyphs and text only. There are no green fills except toggled-on switches.
- **Protected:** plain text with no input box; provenance says “· lock”.

**RAW safety states** (status-bar segment + popover):
- **On:** `● RAW Safe Mode · NEF → XMP sidecar` (green).
- **Warning:** `● … · N notices` (amber dot, bg #221f18). Still safe.
- **Off (1.x):** `◐ RAW Safe Mode off · Direct NEF writes · backup required` (bg #2e2716), plus the persistent amber banner.
- **Backup unavailable:** backup segment `× Backup unavailable · … — writes blocked` (#f0b3ab on #261a19). All writes are blocked.
- **Backup nearly full:** backup segment amber.

---

## 8. Components

For each component: purpose · anatomy · props · states · rules.

### AppShell
Owns the chrome and three panes. Props: `module`, `banner?`, `sidebar: expanded|rail`, `inspector: expanded|narrow|rail`, `actionBar?`, `locked` (operation running). Panes resize via 1 px splitters (4 px hit area). Collapse order when the primary pane is under 560: sidebar → inspector 280 → inspector rail. Rails are 28 px with a vertical label and an active-filter dot. While `locked`, toolbar tools are disabled and an amber “Editing is locked while the operation runs” note appears.

### Sidebar
Anatomy:
- Group header: 26 px, uppercase label, right slot “any” (muted) or “clear” (accent).
- Source rows: 22 px, indented 16 per level.
- Facet rows: 21 px, checkbox + label + mono count, amber count = attention.

Modes: `library` (Sources + facets) · `preview` (Show changes + Edits in plan + Write target + Excluded) · `operation` (info + filter) · `settings` (nav) · `presets` (kind/source) · `rules` (presets list + fields touched).

Logic: facets are OR within a group and AND across groups. Long paths ellipsize at the end, with the full path in a tooltip.

### Toolbar
38 px. Module segmented control (24 px, radius 3) · 1 px divider · text tools (Add ▾, Time tools, Apply preset ▾, Clean export…) · spacer · search 280 · Preview button.

Preview button states:
- disabled grey outline: no staged action
- accent outline: plan staged, shortcut Ctrl ↵

The table toolbar is 30 px: mono count · plain-language filter · Group ▾ · Density ▾ · Columns ▾.

### MetadataTable
Primary surface, virtualised. Props: `columns[]`, `sort[]` (multi-level), `groupBy?`, `density`, `selection`, `focusRow`, `frozen = [flags, name]`.
- **Header:** 24 px, cells divided by 1 px #2a2d31; sorted column in text.strong with `▲` or `1▲ 2▲`.
- **Rows:** 22 px zebra.
- **Cells:** mono for data; empty `—` (#4a4f55); not-yet-read `…`.
- **Flags column (40):** mono caps tags (CONF, RO, C2PA, RESCAN, CLOUD, DENIED, BAD META).
- **Writes-to column:** Embedded / XMP sidecar / Read-only.
- **Names:** middle-truncated (keep the last ≈45% incl. number + extension; CJK counts as width 2). Full name in tooltip.
- **Width:** the last column flexes so the table always fills its pane. When columns exceed the pane, a horizontal scrollbar plus an 8 px shadow edge on the frozen columns.
- **Large datasets:** rows appear immediately and values stream in. The vertical scrollbar marks the scanned-up-to line and shows the “Rows a–b of N” tooltip while dragging.
- **Column chooser:** popover 270 wide: search, drag-reorder, show/hide, locked name, saved layouts, reset.
- **Header context menu:** sort asc/desc, add sort level, group by, clear sort, size to fit, hide, Columns….

### MetadataInspector
One file. Anatomy:
1. Header: mono 13 name (ellipsized, full in tooltip), type · size, write target, path, position n / N.
2. Layer toggle: Effective / In file / Sidecar.
3. Embedded preview, 112 px (Basic only, loaded on demand).
4. Section strip: Basic · Capture · Camera · Creator · Location · Advanced.
5. The expanded section.
6. Collapsed sections, each with a field count.
7. Footer legend + Preview.

Advanced shows raw ExifTool tags grouped (EXIF, XMP-dc, XMP-xmp, MakerNotes (protected), Composite), read-only, filterable, with copy on hover.

At the 280 px minimum the provenance column drops out (goes into the tooltip) and the label column becomes 78.

### FieldEditor
Grid `92 label | 1fr value | 62–66 provenance`, min-height 22, input 20 (multi-line 50).

States: editable (boxed) · hover · focus · staged (accent inset left edge) · empty · conflict (≠ + inline line “In file: … · sidecar value is used”) · invalid (red, message below) · protected (plain, “· lock”).

Editing in the inspector stages the change into the plan. It never writes on blur or Enter.

### MixedValueField
Batch field block. Anatomy:
- ● staged dot + label + aggregate (mono 11, amber if Mixed)
- detail or distribution list: ≤ 3 values + (empty); count, 56 px bar, **Select** link
- action segmented (Leave first)
- value input (accent-bordered), or a dashed same-value box
- impact bar (5 px; hatched = can’t apply)
- impact line (mono, change colours)
- note

Aggregates: `Same · v` · `Mixed · N values` · `Present in a / b` · `All empty` · `In a / b`.

### RuleRow
Anatomy:
- Left rail (64): grip ⋮⋮ · enable checkbox · number.
- Body: lines led by `IF` / `AND` / `THEN` (mono 10.5, IF blue #8fc3d4, THEN green). Tokens are 22 px pickers: field ▾, operator ▾, value input (mono, accent-input border), keyword text (“to”, “by”).
- Right (176): live “a / b match”, effect in change colours, tag (descriptive / removal · backed up / time).
- ⋯ menu.

States:
- disabled: 42%
- editing: accent inset left edge, picker popover open
- dragging: lifted, shadow + accent ring, 2 px drop line with the position
- incomplete: red dashed placeholder tokens

Inline message: amber warning (override, template variable reads the original value) or red error (incomplete), each with a one-click fix link.

### PresetRow
40 px. Name (500) + source line (Built-in / Yours / Imported · uses 1.x fields) · summary · fields · kind badge (✎ Descriptive, − Removal, ~ Time, ⇲ Export copy) · rules count · last used · ⋯. F2 renames in place (text selected). Duplicate inserts “… copy” directly under the original and starts a rename.

### DiffTable
Two views:
- **By file:** 34 px rows. Columns: ✓ · File · Writes to · one column per edited field · Δ. Each cell has a glyph + before (10.5, struck for mod/rem) + after.
- **By change:** 24 px rows. Columns: ✓ ▸ · File (shown once per group) · Field · Change · Before · After · Writes to · Note. Expanding shows 20 px tag sub-rows (└ EXIF:DateTimeOriginal …).

Interaction:
- Row checkbox → exclude file.
- Cell click → exclude a single change.
- Sidebar edit checkbox → exclude an edit everywhere.

Rules:
- Unsupported and skipped cells aren’t toggleable.
- Kind filter dims non-matching cells to 30% in By file and filters rows in By change.
- Focused file → select.focus row; the right pane’s File detail lists every tag that will be written or deleted.

### OperationSummary
Tally line (glyph + mono 15 count + word) → 4 px segmented bar → “Needs your attention · N files”. Then IssueRows: glyph · file + status · what happened + why (and whether it is safe) · actions. Then an all-files table. The “re-read after writing: every value matches the preview” confirmation is green.

Right pane actions:
- View failed + skipped
- **Retry failed** (accent outline)
- Inspect in History
- Undo operation…
- Export log…
- Done

### ProgressPanel
Large mono count `187 / 247 files` + %, 6 px bar, changes + rate + ETA. Running counts (succeeded / warning / failed / skipped / queued). Current-file steps: backup → temp write → verify → swap, with glyphs ✓ ● =. Pinned footer “If you cancel” with three consequences + Cancel….

### WarningBanner
Full-width, 28–32 px, directly under the toolbar. glyph · bold statement · explanation · spacer · 1–2 outlined actions. Only one at a time; the most severe wins. Kinds: `preview` · `applying` · `paused` · `rawOff` · `blocking` (red) · `attention` (amber). Session- or operation-wide only; per-file issues go to flags and the inspector.

### RawSafetyStatus
Status bar segment (first segment, always visible) + 430 px popover anchored bottom-left.

Popover sections:
- title with state tag
- lead sentence
- this session’s write targets
- who sees sidecar edits
- per-format behaviour
- notices, each with an action

States: on · warning · off (1.x). The backup segment beside it has normal / nearly-full / unavailable.

### EmptyState
Dashed box (#3a3f45), max 480 wide, left-aligned. Title 14/600, one sentence, 1–2 actions with shortcuts, and optional safety lines. Variants:
- **session empty:** full box
- **filter has no results:** one line + Clear filter
- **pane empty:** plain muted text, no box

No illustrations.

### ErrorState
Tone panel with header (glyph + title + one-sentence summary) + four rows **What failed · Why · Your files · Next**, optional mono diagnostics, actions. Placement: centre pane (session: ExifTool, paused op), inspector (file: permission, read-only, damaged, unsupported), dialog (recovery before any write). Tone: red blocked/failed · amber paused/read-only · grey unsupported.

### ConfirmDialog
460–560 wide, radius 4, scrim, 36 px title bar (glyph + the question with counts), body of consequence rows (glyph · text · mono count), optional radios, optional acknowledgement checkbox, footer (left: small note; right: buttons).

| Weight | Used for | Buttons |
|---|---|---|
| Light | preset delete, settings reset | Cancel · confirm (primary) |
| High-risk | Apply with removals / unsupported / >1,000 files | Back to preview · Apply N (disabled until every ack is ticked) |
| Destructive | direct RAW write (1.x) | Cancel · safe alternative (accent outline) · destructive (red, disabled until ack) |

Keys:
- Esc always cancels.
- Enter triggers the safe default in destructive dialogs.
- Never stack two dialogs.
