# MoriMeta — Screen Spec (canonical)

FROZEN · v1.0. Each state has a mock ID: `file#screen`. The file number refers to `MoriMeta A2 N ….dc.html`, and the screen is the review-bar button or the `screen` prop. Every state is validated at 1440×900, 1920×1080 and 2560×1440.

Shared chrome on every screen: MenuBar 28 · Toolbar 38 · StatusBar 24 (RAW safety · backup · undo · scan · ExifTool).

---

## 1. Library (file 1)
| State | ID | Content / rules |
|---|---|---|
| Default | 1#default | Nothing is selected, so the inspector slot shows a **Session summary**: where edits are written (JPG in file, NEF → sidecar, sidecars to be created), Needs attention (read-only, conflicts, changed since import, cloud placeholders, darktable sidecars, C2PA) each with an action, recent operations. |
| Large dataset | 1#large | 5,860 files streaming in. The progress strip under the table toolbar shows count, %, ETA, “read-only, nothing is written” and Cancel scan (Esc). Rows appear immediately; unread values show `…`. The scrollbar marks the scan line and the viewport tooltip. Facets say “reading…”. Inspector collapsed to its rail. |
| Filters | 1#filters | Facets (left) AND a condition row above the table (`Where [Camera is NIKON Z 8] and [GPS is present] and [Copyright is empty] + Add condition · Clear all · Save as smart filter…`). The add-condition popover shows field → operator → value with a live match count before adding. The search term appears in the toolbar field. |
| Sorting | 1#sort | Multi-sort `1▲ Camera`, `2▲ Capture time`, grouped by Camera (group rows 24 px with count). Header context menu open. |
| Columns | 1#columns | Column chooser popover (show/hide, drag, locked File name, protected Serial, saved layouts). Live resize readout “Lens · 210 → 248 px”. |
| Empty | 1#empty | EmptyState in the centre with safety lines and supported formats; sidebar “No folders yet”; inspector “Select a file…”. |
| Stress: narrow | 1#stress | Sidebar rail (28), inspector at 280 (provenance hidden), long zh/ja names middle-truncated at 230 px, backup unavailable (red status segment), RAW notices, horizontal scrollbar + frozen-column shadow. |
| Stress: CJK | 1#stress-cjk | Chinese and Japanese folder names in Sources and the title bar, a lens facet with three long models, long names. At 2560 the table fills the width. |

## 2. Metadata Inspector (file 1)
Header: name · type/size · writes to · path · n / N. Layer toggle **Effective / In file / Sidecar**. Section strip. One section expanded; the others collapsed with field counts.

| Section | ID | Fields | Notes |
|---|---|---|---|
| Basic | 1#i-basic | Title, Description (multi-line), Rating, Label, Keywords | Embedded preview 112 px. Sidecar-visibility note. |
| Capture | 1#i-capture | Date taken, Sub-second (lock), Offset, Created, Modified (lock), GPS time UTC (lock) | Note: tools move Date taken + Created, never GPS UTC or MakerNotes. Actions: Time tools… (T), Copy time to selection. |
| Camera / Lens | 1#i-camera | Make, Model, Body serial (masked · show), Firmware, Lens, Lens serial, Focal, Aperture, Shutter, ISO, EC, Metering, Flash, Shutter count | All protected in 1.0. Copy all as text. |
| Creator | 1#i-creator | Artist (conflict ≠), Creator, Copyright, Credit, Website, Email | Conflict panel: each source (EXIF:Artist in file, XMP-dc:Creator sidecar ← used, IPTC not present) + **Use “Morii” everywhere** (goes through Preview) / Keep as is. |
| Location | 1#i-location | Lat/Lon (decimal + DMS), Altitude, Country, Region, City, Location | Amber note: GPS inside the NEF can’t be removed via sidecar → Unsupported; use Clean export. |
| Advanced | 1#i-adv | Raw tag groups: EXIF (in file), XMP-dc, XMP-xmp (sidecar), MakerNotes (protected), Composite | Filter + group dropdown, read-only, hover row shows Copy. |

## 3. Batch Edit (file 2)
Inspector slot = batch panel: selection header (count, write-target split, “Nothing is written from here”) → MixedValueFields → 1.x and protected groups (listed, not editable) → footer (staged count, 0 written, Discard, **Preview changes Ctrl ↵**).

| State | ID | Rules shown |
|---|---|---|
| Just opened · mixed | 2#b-open | All fields on Leave. Distributions with Select. Preview disabled: “No actions staged · Preview needs at least one”. |
| Staged | 2#b-staged | Shift time +3m 42s (~248), Set Creator (+136 =112), Set Copyright template `© {year} Morii` (+50 ~189 =9), Clear GPS (−64 JPG ⊘64 NEF, hatched bar + explanation). |
| Same values | 2#b-same | 36 JPG. Same values as dashed boxes; Set replaces for all; Set GPS on empty adds (accepts decimal or DMS paste). |
| Partial + unsupported | 2#b-partial | 312 files incl. 40 HEIC + 28 DNG (read-only formats, dimmed rows). Amber banner: they stay selected and appear as Unsupported. Each action shows reach; GPS Clear only partly applicable (64 of 190). Keywords shown as 1.x, Leave only. |

## 4. Capture Time Tools (file 2)
Replaces the inspector (440/500 wide). Mode grid (8 radios), parameters, **Result** summary, fixed notes (GPS UTC not changed; NEF sidecar visibility), footer Reset · Add to plan · Preview changes. The centre table shows `#`, File, Writes to, now, **new** (tinted column), change, order status. The bottom timeline (128 px) shows now vs after marks, with pairs highlighted.

| Mode | ID | Parameters | Result |
|---|---|---|---|
| Absolute | 2#t-abs | date, time, .ss, offset keep/set | warns “shared timestamp — order lost” |
| Shift | 2#t-shift | +/−, d h m s, offset unchanged, “compute from reference pair…” | ~248 · order preserved |
| Change time zone | (listed) | keep instant / relabel | — |
| Sequence | 2#t-seq | start, step per pair, **order by** (must be confirmed) | first→last, collisions none |
| Preserve relative | 2#t-rel | reference file (◆ row), is now, should be | computed shift, spacing exact |
| Sync from reference | (listed) | two-camera offset | — |
| Range distribution (1.x) | 2#t-range | start, end, even / keep proportions, order | step per pair |
| Random interval (1.x) | 2#t-rand | start, gap min–max, **seed** + reroll, order | reproducible (seed saved in plan) |

## 5. Rule Builder (file 3)
Header: preset name, “N rules · M enabled · runs top to bottom”, EDITED · NOT SAVED badge, Test against ▾, Revert, Save (Ctrl S). RuleRows. Add rule + the evaluation rule text. Right pane **Dry run**: files / changes estimate, kind counts, By rule, Checks (errors, warnings, unmatched, protected fields untouched), Sample files, footer “Rule 06 incomplete, left out” + Preview plan.

| State | ID |
|---|---|
| Rules with validation (warning override, template variable warning, disabled rule, incomplete error) | 3#rules |
| Editing a condition (field picker popover: type-to-filter, grouped, 1.x disabled, key hints) | 3#rules-edit |
| Reorder (lifted rule, drop line “position 02”) | 3#rules-drag |

## 6. Preset Manager (file 3)
Left: Kind / Source facets. Centre: PresetRows (sort Last used ▼). Right: Details (plain-sentence rules in order, Safety, Last used with Inspect, actions Edit rules · Duplicate · Rename · Export… · Delete…, **Apply to N selected files…**, “Applying builds a plan and opens Preview”).

| State | ID |
|---|---|
| List + details | 3#presets |
| Row menu + inline rename of a duplicated preset | 3#p-menu |
| Apply dialog: scope radios (selection / filter / session), facts, Build preview | 3#p-apply |
| Delete confirm (light; history unaffected; Export first) | 3#p-delete |

## 7. Preview / Diff (file 4) — most important
Banner `PREVIEW · Plan 0142 · vN — nothing has been written to disk · From … · Back to edit Esc`.
Sidebar: Show changes (6 kinds, with ○/✓ review marks on Removed, Warnings, Unsupported) · Edits in plan (toggle) · Write target counts · Excluded summary.
Centre: summary strip (files, changes, kind counts, filter files, By file | By change).
Right: **Checks** (pre-flight, RAW safety, backup) | **File detail** (tags that will be written).
Action bar: `Review before applying: ○ Warnings 21 ○ Unsupported 64 ○ Removals 64` · note · Discard plan · Back to edit · **Apply N changes…**

| State | ID | Notes |
|---|---|---|
| By file | 4#pv-file | File-level + field-level diff in one grid. Exclude file / change / edit. |
| By change | 4#pv-change | One row per change, expandable to tag level. |
| Apply confirm | 4#pv-confirm | High-risk dialog: counts by kind, where and how each type is written, acknowledgement for removals. |
| Stress: backup unavailable | 4#pv-stress | 2,139 files / 5,904 changes, long names, RAW notices. Pre-flight × Backup location; Backup section → ErrorState (Choose location…, Retry); Apply **blocked** with the reason in the action bar. |

## 8. Apply Progress (file 4)
Banner `APPLYING · Operation · plan · N files · started · elapsed`. Toolbar locked. Sidebar: operation info + status filter. Centre: tally + bar, per-file table (#, file, writes to, status, changes, detail, time), current row highlighted. Right: ProgressPanel.

| State | ID |
|---|---|
| Running | 4#run |
| Safe cancel dialog (“Stop after the current file?”; primary = Keep going) | 4#cancel |

## 9. Completion Summary (file 4)
| State | ID |
|---|---|
| 243 succeeded · 2 warnings · 1 failed · 1 skipped. IssueRows with Retry / Show file / Inspect; verified line; right pane actions (View failed, Retry failed, Inspect in History, Undo…, Export log…, Done); backup + undo facts | 4#done |

## 10. History / Undo (file 5)
Left: operation journal grouped by day. Each entry: time, glyph, title, counts, backup status (● kept / ○ expired / recovery available). Centre: selected operation header with actions (Undo operation…, Undo selected files…, Retry failed, Restore backup to folder…, Export log…), tabs by status, per-file table with **Now vs. after operation** (= unchanged / ≠ changed by another app). Right: operation facts + how undo works.

| State | ID |
|---|---|
| Journal + operation detail | 5#h-journal |
| Undo dialog with 12 conflicts (skip vs restore anyway) | 5#h-undo |
| Interrupted operation (completed / not started / rolled back / needs you) | 5#h-failed |

## 11. RAW Safe Mode (file 5)
| State | ID |
|---|---|
| Enabled: status segment + popover (session targets, who sees sidecar edits, per-format) | 5#r-on |
| Warning: amber dot, notices with actions (darktable sidecars, changed outside, orphans) | 5#r-warn |
| Disabled (1.x): amber banner + segment, Writes-to column “NEF · in file ◐”, popover limits | 5#r-off |
| Direct RAW write confirm (1.x): destructive dialog, ack, safe alternative | 5#r-confirm |

## 12. Settings (file 5)
Left nav, form rows `210 label | control + help`, LOCKED / 1.x tags.

| Page | ID | Key content |
|---|---|---|
| General | 5#s-general | Language, theme, startup, density, scale, close-during-operation (locked) |
| Metadata / ExifTool | 5#s-meta | Default creator, copyright template, time display, file-modified behaviour, ExifTool status + path (1.x custom) |
| RAW | 5#s-raw | RAW Safe Mode (locked on in 1.0), sidecar naming, darktable read-only, other formats, direct NEF (1.x, disabled) |
| Backup | 5#s-backup | Location, retention 30 d, size limit, usage bar, clean up, “can’t be switched off” |
| Privacy | 5#s-privacy | Local-only statement, log detail (paths anonymised), log folder |
| Updates | 5#s-updates | Update available banner (installs on next launch, never mid-operation), frequency, channel, what the check sends |
| Advanced | 5#s-adv | Workers, verify (locked), debug logging warning, config migration record, diagnostics, reset |

## 13. First Launch (file 6)
Three dialog steps over the real empty app, with a progress indicator and Skip setup.
- 6#l-1 what it does + language
- 6#l-2 safety model + backup location
- 6#l-3 privacy + update check choice (no pre-selection bias; weekly shown selected as default)

## 14. Error States (file 6)
| State | ID | Placement | Tone |
|---|---|---|---|
| ExifTool unavailable | 6#e-exif | centre ErrorState; table lists names only; status segment red | red |
| Permission denied | 6#e-perm | banner (14 files) + inspector ErrorState; DENIED flags | red |
| Read-only file | 6#e-ro | inspector; Clear read-only attribute… is explicit | amber |
| Corrupted metadata | 6#e-corrupt | inspector; writing disabled for the file; readable fields dimmed; diagnostics | red |
| Unsupported format (HEIC) | 6#e-unsup | inspector; what works, what happens in batch | grey |
| Interrupted operation (card removed) | 6#e-int | paused banner + centre ErrorState; flags ✓ ↺ ○ | amber |
| Recovery available (next launch) | 6#e-recover | dialog before any write; Keep as is · Undo completed · **Continue remaining** | amber |

## 15. Responsive validation
An automated check ran over every state of files 1–6 at all three frames. It found no text spilling out of its box, no clipped labels without ellipsis, and no primary action hidden by overflow. It found one issue, the “no value needed” filter placeholder, which is now fixed. Pane widths follow `DESIGN_SYSTEM.md §6`, and the table stays the widest pane in every state. At 1440 the library table scrolls horizontally with frozen name columns; at 1920 all 14 default columns fit.
