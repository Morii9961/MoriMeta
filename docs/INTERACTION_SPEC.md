# MoriMeta — Interaction Spec (canonical)

FROZEN · v1.0. Behaviour rules. When a mock and this file disagree, this file wins.

Core model: **Selection → Edit (stage) → Plan → Preview → Apply → Operation (journal, backup, undo).** Nothing touches disk before Apply.

---

## 1. Leave / Set / Clear semantics
| Action | Meaning | Result per file |
|---|---|---|
| **Leave** (default) | Don’t touch this field. | Not in plan. |
| **Set** *value* | Make the field equal *value*. | empty → **Added** · different → **Modified** · equal → **No change** (counted, not written) |
| **Clear** | Remove the field’s value. | present → **Removed** · absent → No change · can’t remove (e.g. GPS inside NEF via sidecar) → **Unsupported** |
| **Shift** (time) | Move local time by ±Δ. | Modified; the offset is unchanged; GPS UTC and MakerNotes times are untouched. |
| **Add / Remove** (keywords, 1.x) | List operations. | Per item. |

Rules:
- Set with an empty value is not allowed. Use Clear. The input shows an inline error.
- One field maps to its tag family and writes them together: e.g. Creator → EXIF:Artist + XMP-dc:creator (+ IPTC By-line only if IPTC exists); Capture time → DateTimeOriginal + CreateDate (+ XMP-exif). Tag-level results are visible in Preview → File detail.
- Template tokens `{year}`, `{creator}`, `{camera}` resolve **per file from its current (pre-plan) values**. `{token|fallback}` supplies a fallback. The rule builder warns when a token depends on a value another rule sets.

## 2. Mixed-value behaviour
- Aggregate states: `Same · v`, `Mixed · N values`, `Present in a / b`, `All empty`. Mixed is shown in amber and never as a value in an input. The input stays empty until the user types.
- Up to 3 distinct values + (empty) are listed with counts. **Select** narrows the table selection to those files. It doesn’t change edits.
- Setting a value on a mixed field previews impact before Preview: `+ added = unchanged ~ modified`.
- Same value: shown in a dashed non-input box. Choosing Set pre-fills it for editing.
- Protected fields (camera, lens, exposure, serials) aren’t in the batch editor except in a “Protected · read-only” group. 1.x fields are listed as “Coming in 1.x”.

## 3. Preview → Apply flow
1. **Stage.** Batch panel, inspector edits, time tools, rules or a preset put actions into the current plan. The toolbar Preview button turns accent. Staged counts are shown; “0 files written” is stated.
2. **Preview** (Ctrl ↵ or button). MoriMeta builds `Plan N v1`: it resolves every action per file and tag, runs pre-flight, and opens Preview in place. The banner says nothing has been written.
3. **Review.** Filter by kind, switch By file / By change, inspect tags, exclude things (§8, §9). Every exclusion creates `vN+1`; totals update live.
4. **Apply…** → ConfirmDialog when the plan is high-risk (§4); otherwise straight to progress.
5. **Operation.** Progress → Completion summary → History entry.
- **Back to edit** (Esc) returns to the staging surface with the plan intact. **Discard plan** clears staged actions (confirm if more than 20 actions).
- If files change on disk while Preview is open, the affected rows are flagged `RESCAN` and Apply is blocked until you rescan (automatic prompt).

## 4. Apply gating
Apply stays secondary (outline, not primary) until all of these are true:
- [ ] Pre-flight has no errors (backup location writable, enough space, ExifTool running, no rescan needed).
- [ ] Every present category of **Warnings**, **Unsupported** and **Removals** has been opened at least once (review checklist in the action bar: ○ → ✓).
- [ ] At least one change remains after exclusions.

When blocked, the button label and the action-bar note say why (“Apply blocked — no backup”, “Review warnings first”).

**High-risk plan** (any removal, any unsupported, >1,000 files, or any direct RAW write) → ConfirmDialog with counts by kind and where each type is written. Each removal category needs an acknowledgement checkbox; the confirm button stays disabled until all are ticked. Low-risk plans (only add/modify, ≤1,000 files) apply immediately after the button.

## 5. Warning acknowledgement
- **Warnings** (e.g. EXIF ≠ XMP: both will be set) are written as previewed. Reviewing = opening the Warnings filter; no per-item tick.
- **Removals** need the explicit acknowledgement in the confirm dialog (“Remove GPS location from 64 JPG files”).
- **Direct RAW writes** (1.x) need their own acknowledgement per plan (§14).
- Acknowledgements are recorded in the operation log.

## 6. Unsupported-field handling
- Unsupported items are **never silently dropped**. They appear in Preview as ⊘ cells (hatched), in counts, in the review checklist and in the operation log.
- Causes: sidecar can’t remove data embedded in a RAW; read-only format (HEIC, DNG, CR3… in 1.0); corrupted metadata (writing disabled for that file); field not writable in this format.
- Unsupported cells can’t be toggled. Excluding the whole file is allowed.
- Where an alternative exists, the note offers it (Clean export for GPS removal).

## 7. Read-only, skipped, conflicts
- Read-only files (attribute set) are listed as **Skipped · read-only** in Preview and the result. MoriMeta never clears the attribute by itself; the user can choose “Clear read-only attribute…” in the inspector (a separate, logged action).
- Source conflicts (EXIF ≠ XMP) are shown in the inspector. The effective value follows the precedence sidecar XMP > embedded XMP > EXIF > IPTC. Resolving a conflict = staging a Set, which goes through Preview.

## 8. File exclusion
- Preview row checkbox, or Space on the focused row. Excluded rows go to 42% opacity and their changes leave all totals.
- Sidebar “Excluded” summarises file, change and edit exclusions and offers Restore all.
- Exclusion never changes the Library selection.

## 9. Edit and change exclusion
- **Single change:** click a diff cell (or Enter on a focused cell in By change). The cell shows ↺, outlined and struck.
- **Whole edit:** untick it in “Edits in plan”. All of its cells dim and totals drop.
- Tags follow their field: excluding Capture time for a file drops all three date tags.

## 10. Safe cancellation
- **Cancel…** during Apply opens “Stop after the current file?”. The default button is **Keep going**.
- On stop:
  - finished files stay applied (and undoable)
  - the file in progress is rolled back from its temporary copy (the original was never touched)
  - queued files are untouched
- The operation is recorded as **Stopped** with those three counts.
- Closing the app during an operation always asks (locked setting). Updates never install during an operation.
- **Media removed / crash:** the operation pauses (banner + ErrorState) and offers Continue when back · Stop here · Undo finished. On the next launch after a crash, the **Recovery** dialog comes before any other write (§13).

## 11. Per-file write pipeline (why cancel and crash are safe)
Pre-flight → for each file: **backup** original (or sidecar) → write to **temp copy** → **re-read and verify** every planned tag and that the image data is unchanged → **atomic swap** → journal entry. Any failure before the swap leaves the original intact and marks the file Failed. Temp files are registered in the operation log; only registered temp files are ever cleaned up.

## 12. Undo, restore, retry
- **Undo operation…** (History or Completion): restores each file byte-for-byte from backup, and removes sidecars the operation created.
- **Conflict check first:** files changed since the operation (hash differs) are conflicts. You choose **Skip them** (default) or **Restore anyway** (the current version is backed up first).
- Undo is itself an operation (journal entry, backup, can be undone/redone).
- **Undo selected files…** is the same, for a chosen subset.
- **Restore backup to folder…** copies backed-up originals to a chosen folder without touching the current files.
- Backups expire by retention (default 30 days) or the size limit (oldest expired first). Expired entries stay in History as a record; Undo is disabled with “backup expired”.
- **Retry failed:** builds a new plan containing only the failed files’ original changes, re-runs pre-flight and opens Preview (skippable if nothing changed since). Skipped read-only files are included only if the attribute has been cleared.

## 13. Interrupted operation recovery
On launch, if the journal has an unfinished operation, show the Recovery dialog with counts: completed · not started · rolled back automatically · needs decision. Choices:
- **Continue remaining** (primary)
- **Undo the completed…**
- **Keep as is and close**

A file “needs decision” when a verified temp copy exists next to an intact original. It is resolved in History (use copy / discard copy).

## 14. RAW Safe Mode and direct-write confirmation
- **1.0:** RAW Safe Mode is always on. NEF/NRW edits go to `<name>.xmp` (Adobe naming). darktable `<name>.NEF.xmp` sidecars are read-only. GPS embedded in RAW can’t be cleared → Unsupported + Clean export suggestion.
- **Warning state:** notices (darktable sidecars, sidecar changed outside MoriMeta → rescan, orphan sidecars never edited). Still safe; amber, never red.
- **1.x, off:** enabled only in Settings › RAW for tested bodies. Persistent amber banner and status segment. Full NEF backups are mandatory.
- **Direct-write confirmation (every plan that writes into RAW):** destructive ConfirmDialog listing backup size and location, verification, undo window and tested bodies. Needs the acknowledgement “I understand the original NEF files will be modified.” It offers **Use sidecars for this plan** as a real alternative. Enter = Cancel.

## 15. Backup rules
- Backups can’t be disabled. Location must be a local, writable drive (network/removable refused).
- Backup unavailable (drive missing, not writable) → status segment red, Preview pre-flight error, Apply blocked with the reason. **No writes of any kind** (sidecar-only plans included) until resolved.
- Space: pre-flight estimates backup size; if it won’t fit within the limit or the free space, Apply is blocked with the numbers.

## 16. Rules and presets
- Rules run top to bottom against files **as they are now**. Rules don’t chain; when two rules set the same field, the later rule wins and the earlier one shows “overridden”.
- Disabled and incomplete rules are left out of the plan (the dry run says so).
- Reorder: drag the grip or Alt ↑/↓. The drop marker shows the position; the dry run updates on drop.
- Presets store rules as JSON (schema versioned). Applying a preset asks for the scope (selection / current filter / session), builds a plan and opens Preview.
- Duplicate inserts “<name> copy” and starts an inline rename. Delete moves the file to the Recycle Bin; History entries are unaffected.

## 17. Capture time tools
- All modes preview old → new per file in the table and on the timeline before staging.
- Order-dependent modes (Sequence, Range, Random) require the **Order by** choice to be confirmed. RAW+JPG pairs share one timestamp.
- Random uses a stored seed, so Apply writes exactly what Preview showed.
- Absolute warns that order by time is lost when it creates a shared timestamp.

## 18. Keyboard
| Scope | Keys |
|---|---|
| Global | Ctrl O add files · Ctrl ⇧ O add folder · Ctrl F search · Ctrl L add filter condition · Ctrl , settings · Ctrl B toggle sidebar · I toggle inspector · T time tools · Ctrl ⇧ K columns · F1 help |
| Table | ↑↓ move · Shift ↑↓ extend · Ctrl A select all · Space toggle selection · Home/End · PgUp/PgDn · Enter focus inspector · Ctrl C copy cell · header: click sort, Shift click add level |
| Inspector | Tab / Shift Tab between fields · Enter stage value · Esc revert field · Alt ↑↓ previous/next file |
| Batch / rules | Tab next part · ↵ set picker value · Alt ↑↓ move rule · Del delete focused rule (undoable in-editor) · Ctrl S save preset |
| Preview | Ctrl ↵ open Preview (from edit) · 1–6 filter by kind · 0 all · V toggle By file / By change · Space exclude focused file · Enter exclude focused change · → expand tags · Esc back to edit · Ctrl ↵ Apply… (only when the gate is satisfied) |
| Operation | Esc = Cancel… (opens dialog; default Keep going) · Ctrl R retry failed (completion) · Ctrl Z undo operation… (opens dialog) |
| Dialogs | Esc cancel · Enter default (safe option in destructive dialogs) · Space toggle acknowledgement |

All actions are reachable by keyboard, and focus is always visible (focus ring token). There are no hover-only actions: hover “Copy” in raw tags is also on Ctrl C.
