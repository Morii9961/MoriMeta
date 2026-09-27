# MoriMeta — Claude Design Brief

> **Version:** v0.1  
> **Status:** Design Exploration  
> **Product:** MoriMeta  
> **Product Type:** Public-facing local-first desktop application  
> **Primary Platform:** Windows first; macOS/Linux considered later  
> **Audience:** Photographers, photography enthusiasts, power users working with large batches of photo metadata  
> **Purpose of this document:** Guide Claude Design through product UI/UX exploration before implementation begins

---

# 0. Design Mission

MoriMeta is a professional desktop tool for safely inspecting, editing, previewing, and automating photo metadata at scale.

The design must make users feel:

- **In control**
- **Safe**
- **Fast**
- **Precise**
- **Calm**
- **Professional**

The visual design should communicate:

> “I can safely drag thousands of files in here, understand exactly what will happen, and trust the application not to destroy my originals.”

MoriMeta is **not** a photo editor, not a DAM, not a social product, and not a SaaS dashboard.

It should feel like a serious desktop tool made for photographers.

---

# 1. Design Principles

## 1.1 Data First

Metadata is the core content.

The UI should prioritize:

- file lists
- metadata tables
- field values
- diffs
- rules
- warnings
- operation status

over decorative imagery.

Photo previews are useful, but they must not visually overpower metadata workflows.

---

## 1.2 Batch First

Every major interaction should work naturally for:

```text
1 file
20 files
200 files
2,000 files
5,000 files
```

The interface must avoid patterns that work beautifully for one item but collapse under large selection states.

---

## 1.3 Safety Must Be Visible

MoriMeta’s safety model should be visible in the UI rather than hidden in documentation.

Examples:

- Preview before Apply
- Clear “Before → After” differences
- Explicit RAW safety state
- Backup state
- Undo availability
- Number of files affected
- Number of fields affected
- Warnings for destructive or unsupported actions

The user should never need to guess whether an operation is safe.

---

## 1.4 Professional Density

MoriMeta should support high information density.

Avoid excessive whitespace that reduces productivity.

Use compact, well-aligned layouts inspired by professional creative and developer tools.

The goal is not “minimalism at all costs.”

The goal is:

> **Maximum clarity per pixel.**

---

## 1.5 Calm, Not Dramatic

Avoid:

- oversized hero typography
- marketing-style gradients
- glassmorphism
- decorative blur everywhere
- exaggerated shadows
- animated gimmicks
- giant rounded cards
- playful consumer-app visuals
- “AI SaaS dashboard” aesthetics

MoriMeta should feel quiet, stable, and trustworthy.

---

# 2. Visual Personality

Desired traits:

- restrained
- cool
- technical
- elegant
- contemporary
- precise
- unobtrusive

Suggested mood:

```text
Lightroom / Capture One
+
Linear-level polish
+
native desktop utility restraint
```

But do **not** copy any product directly.

---

# 3. Initial Visual Directions

For the first design exploration, produce **three clearly different directions**.

Do not immediately choose one.

## Direction A — Professional Darkroom

Characteristics:

- dark-first
- compact
- high-density
- desktop-native feeling
- strong table hierarchy
- restrained separators
- tool-like controls
- minimal decoration

Reference feeling:

> “A modern professional photography utility.”

---

## Direction B — Native Precision

Characteristics:

- cleaner and lighter
- macOS / Windows native utility influence
- neutral surfaces
- fine borders
- quieter visual hierarchy
- excellent keyboard-oriented usability
- extremely readable tables

Reference feeling:

> “A first-party desktop tool that happens to be cross-platform.”

---

## Direction C — Modern Technical

Characteristics:

- slightly more contemporary
- subtle Linear / Arc influence
- still dense and professional
- sharper component hierarchy
- elegant command surfaces
- restrained accent color
- carefully structured empty states

Reference feeling:

> “A modern engineering-grade creative tool.”

---

# 4. Important Design Constraint

Do **not** design MoriMeta as:

```text
Sidebar
+ gigantic page heading
+ 6 rounded statistic cards
+ oversized empty center panel
+ floating CTA
```

This is not a SaaS admin panel.

The primary surface is a working environment.

---

# 5. Main Application Structure

Suggested shell:

```text
┌─────────────────────────────────────────────────────────────┐
│ App Bar / Workspace / Search / Global Actions              │
├───────────────┬─────────────────────────────────────────────┤
│ Navigation    │                                             │
│               │            Main Workspace                   │
│ Library       │                                             │
│ Presets       │                                             │
│ Rules         │                                             │
│ History       │                                             │
│ Settings      │                                             │
│               │                                             │
├───────────────┴─────────────────────────────────────────────┤
│ Optional status / job / progress area                       │
└─────────────────────────────────────────────────────────────┘
```

The exact layout may evolve, but the application should preserve:

- persistent workspace context
- fast navigation
- clear selection state
- visible batch state
- easy access to Preview / Apply

---

# 6. Core Screens to Design

Claude Design should create full designs for at least the following screens.

---

## 6.1 Library / Metadata Table

This is the primary daily-use screen.

Must include:

- imported file list
- thumbnail or compact image indicator
- filename
- file type
- capture time
- camera
- lens
- exposure metadata
- GPS state
- author / copyright
- rating
- selection state

Required interactions:

- multi-select
- Shift select
- Ctrl/Cmd select
- sort
- filter
- search
- resize columns
- reorder columns
- show/hide columns
- keyboard navigation

Important:

The table must remain readable with hundreds or thousands of rows.

Photo thumbnails should remain optional and compact.

---

## 6.2 Single File Metadata Inspector

When one file is selected, show a detailed metadata inspector.

Sections:

### Basic
- Title
- Description
- Rating
- Keywords

### Capture
- Date Taken
- Timezone
- Camera
- Lens
- ISO
- Aperture
- Shutter
- Focal Length

### Creator
- Artist
- Creator
- Copyright
- Credit
- Website

### Location
- Latitude
- Longitude
- Altitude
- Country
- Region
- City
- Location

### Advanced
- Raw ExifTool tags
- read-only / editable distinction

The inspector must make field provenance and editability clear.

---

## 6.3 Batch Edit Mode

When multiple files are selected, the inspector should transform into a batch editor.

Important concepts:

- Mixed values
- Same values
- Fields to overwrite
- Fields to remove
- Fields left unchanged

Suggested display:

```text
Artist
[ Mixed values ]

→ Set to:
Morii
```

or:

```text
GPS
[ Present in 128 / 248 files ]

Action:
Remove
```

Avoid making users think that opening the editor automatically changes all selected files.

---

## 6.4 Capture Time Tools

Design a dedicated high-quality interaction for date/time operations.

Must support:

### Absolute
```text
Set all to:
2026-09-04 12:27:00
```

### Shift
```text
+ 8 hours
- 3 minutes
+ 42 seconds
```

### Sequence
```text
Start:
12:27:00

Increment:
+2 min / file
```

### Random Interval
```text
2–3 minutes
```

### Range Distribution
```text
Start 12:27
End   12:45
```

### Preserve Relative Timing

The UI should visually explain the resulting sequence before committing.

---

## 6.5 Rule Builder

This is one of MoriMeta’s signature features.

Do not build a giant node graph for MVP.

Prefer a readable rule builder:

```text
IF
[ File Type ] [ is ] [ NEF ]

THEN
[ Write compatible metadata to XMP ]
```

Example:

```text
IF
[ Artist ] [ is empty ]

THEN
[ Set Artist ] [ Morii ]
```

Multiple rules should be:

- readable
- reorderable
- enable/disable-able
- previewable
- easy to understand without programming knowledge

Advanced logic can come later.

---

## 6.6 Preset Manager

Presets should feel lightweight and reusable.

Example presets:

- Copyright
- Web Safe
- Social Media Safe
- Moriium Safe
- Hokkaido 2027

Each preset should show:

- name
- short summary
- affected fields
- safety level
- last used
- edit / duplicate / delete

Applying a preset must lead to Preview, not directly mutate files.

---

## 6.7 Preview / Diff Screen

This is arguably the **most important screen in the entire product**.

Design it with exceptional clarity.

Must show:

```text
248 files
731 changes
```

Then field-level differences:

| File | Field | Before | After |
|---|---|---|---|
| DSC_0001.NEF | Date | 12:21 | 12:27 |
| DSC_0002.NEF | Date | 12:23 | 12:29 |
| DSC_0003.JPG | GPS | 35.68, 139.76 | Removed |

Changes should visually distinguish:

- Added
- Modified
- Removed
- Unsupported
- Warning

The design must allow users to:

- inspect all changes
- filter by change type
- filter by file
- exclude individual files
- exclude individual operations
- cancel
- return to edit
- apply changes

The primary CTA should only become visually dominant after the user understands the operation.

---

# 7.8 Apply / Progress State

During execution:

Show:

- files processed
- current file
- total operations
- success count
- warning count
- failure count
- progress
- cancel state if safe

Example:

```text
Processing 487 / 2,140 files

Success     482
Warnings      3
Failed        2
```

Avoid theatrical loading animations.

This is a work surface.

---

## 6.9 Completion Summary

After a batch operation:

```text
2,140 files processed
2,136 succeeded
2 warnings
2 failed
```

Actions:

- View failed files
- Retry failed
- View operation
- Undo
- Export log
- Done

Success should feel calm, not celebratory.

---

## 6.10 History / Undo

History should resemble an operation journal.

Example:

```text
Today · 14:42
Applied “Moriium Safe”
248 files · 731 changes

Today · 14:57
Shifted Capture Time +8h
248 files
```

Each entry supports:

- Inspect
- Undo
- Restore backup
- Retry failures

The user should understand that history is functional, not merely informational.

---

## 6.11 RAW Safe Mode

Design a clear but non-alarming RAW safety state.

Example:

```text
RAW Safe Mode
● Enabled

RAW metadata edits will prefer XMP sidecars.
Original RAW files will not be modified when possible.
```

If disabled:

```text
RAW Safe Mode
○ Disabled

Direct RAW metadata writing may modify original files.
Backup required.
```

Use warning hierarchy carefully.

Do not show red destructive warnings everywhere.

Reserve strong red states for genuinely dangerous actions.

---

## 6.12 Settings

Minimum sections:

### General
- theme
- language
- startup behavior

### Metadata
- ExifTool
- date/time defaults
- default author
- RAW Safe Mode

### Backup
- backup location
- retention
- cleanup

### Privacy
- telemetry
- crash reporting
- log privacy

### Updates
- stable / beta
- check for updates

### Advanced
- ExifTool path
- worker limits
- debug logging

---

# 7. Interaction Principles

## 7.1 Preview Before Mutation

No batch edit should directly modify files from the editing surface.

Expected flow:

```text
Edit
→ Preview
→ Confirm
→ Apply
→ Summary
```

---

## 7.2 Mixed Values Must Be Explicit

If 200 selected files contain different values:

Do not display a misleading blank field.

Use:

```text
Mixed
```

or:

```text
Multiple values
```

and clearly distinguish:

- unchanged
- replace
- remove

---

## 7.3 Destructive Actions Require Intent

Actions such as:

- remove metadata
- overwrite technical metadata
- direct RAW write
- restore old backup over current metadata

must visually require intentional confirmation.

Avoid generic modal spam for safe actions.

---

## 7.4 Keyboard First-Class Support

Professional users should be able to work quickly.

Design for:

- arrow keys
- tab navigation
- multi-select shortcuts
- search shortcut
- preview shortcut
- apply shortcut where safe
- undo

Do not make mouse-only interactions essential.

---

# 8. Component Language

Recommended UI vocabulary:

- Tables
- Inspectors
- Split panes
- Toolbars
- Search fields
- Compact dropdowns
- Segmented controls
- Tabs when appropriate
- Inline validation
- Small status badges
- Context menus
- Command palette, possibly later

Avoid overusing:

- Cards
- Pills
- Floating buttons
- Huge toggles
- Decorative badges

---

# 9. Color Strategy

MoriMeta should use restrained color.

Base colors:

- neutral cool grays
- muted surfaces
- high readability

Accent:

- one primary accent color
- subtle and consistent

Semantic colors:

- Add
- Modify
- Remove
- Warning
- Error
- Success

must remain accessible and distinguishable without relying solely on color.

Do not specify a final palette until the three exploration directions are compared.

---

# 10. Typography

Priorities:

1. legibility
2. numeric alignment
3. metadata readability
4. compact density

Important values such as:

```text
1/320
f/1.8
ISO 64
2027-01-20 14:42:05
```

must be easy to scan.

Consider tabular numerals where appropriate.

Avoid oversized marketing typography inside the application.

---

# 11. Spacing

Spacing should feel deliberate but compact.

Avoid:

- oversized 32–48px card padding
- giant empty gaps
- excessive vertical whitespace

Professional desktop tools should allow more data on screen without feeling cramped.

---

# 12. Responsive / Window Behavior

This is a desktop app, not a mobile-first website.

Design primarily for:

```text
1440 × 900
1920 × 1080
2560 × 1440
```

But support smaller windows gracefully.

Required behavior:

- resizable sidebars
- collapsible inspector
- adaptive table columns
- persistent usable area
- no broken layouts at minimum supported size

Do not redesign the app into mobile cards when the window becomes narrower.

---

# 13. Accessibility

The public release should account for:

- keyboard navigation
- visible focus states
- sufficient contrast
- large text scaling
- semantic states not encoded only by color
- screen-reader-friendly controls where realistic
- sensible tab order

---

# 14. Internationalization

Architecture must support:

- English
- Simplified Chinese

Avoid UI designs that only work with short English labels.

Test controls using both compact English and longer Chinese strings.

---

# 15. Empty States

Empty states should be quiet and useful.

Example:

```text
No files loaded

Drag photos or folders here
or

[ Add Files ] [ Add Folder ]
```

Avoid decorative illustrations that occupy most of the screen.

---

# 16. Error States

Errors should explain:

- what failed
- which file
- why
- whether other files are safe
- what the user can do next

Example:

```text
DSC_3921.NEF

Could not write metadata.

Reason:
File is read-only.

[ Retry ] [ Show File ] [ Ignore ]
```

---

# 17. Public Release Considerations

Because MoriMeta will be distributed publicly, design must include polished states for:

- first launch
- ExifTool unavailable
- update available
- unsupported file
- permission denied
- read-only file
- corrupted metadata
- operation interrupted
- recovery available
- no backup configured
- RAW Safe Mode disabled
- app upgrade / config migration

These should not feel like developer-only edge cases.

---

# 18. First Launch

Keep onboarding short.

Suggested flow:

### Screen 1
MoriMeta

> Safe batch metadata editing for photographers.

### Screen 2
RAW safety

> RAW Safe Mode is enabled by default.

### Screen 3
Privacy

> Photos are processed locally.

### Done

Do not create a long tutorial carousel.

The app should teach itself through the interface.

---

# 19. Design System Deliverables

Claude Design should establish:

- color tokens
- typography scale
- spacing scale
- radii
- border hierarchy
- elevation hierarchy
- semantic colors
- input sizes
- toolbar sizes
- table density
- focus styles
- disabled styles
- warning styles
- error styles

Also define reusable components:

```text
AppShell
Sidebar
Toolbar
MetadataTable
MetadataInspector
FieldEditor
MixedValueField
RuleRow
PresetRow
DiffTable
OperationSummary
ProgressPanel
WarningBanner
RawSafetyStatus
EmptyState
ErrorState
ConfirmDialog
```

---

# 20. Design Anti-Patterns

Claude Design must actively avoid:

## Card Hell

Do not put every field inside a separate rounded card.

## Dashboardification

Do not turn the app into charts and statistics.

## Marketing UI

No hero sections inside the working application.

## Excessive Rounded Corners

Professional density should not be buried in giant soft containers.

## Huge Typography

Titles should support navigation, not dominate the workspace.

## Decorative Glass

No blur-heavy glassmorphism.

## Floating Everything

Actions should live where users expect them.

## Hidden Safety

Preview, backup, and RAW behavior must be discoverable.

## Destructive Ambiguity

Never hide the real consequences of an operation behind vague labels.

---

# 21. First Claude Design Assignment

Use this brief together with the MoriMeta Product Spec.

## Phase 1 — Exploration

Produce **three distinct visual concepts**:

1. Professional Darkroom
2. Native Precision
3. Modern Technical

For each concept, design:

- App Shell
- Library / Metadata Table
- Metadata Inspector
- Batch Edit state
- Preview / Diff

Do not create implementation code yet.

Do not select a winner automatically.

Present the three directions for human review.

---

# 22. Phase 2 — Selected Direction

After one direction is chosen:

Expand it into:

1. Library
2. Single Metadata Inspector
3. Batch Editor
4. Capture Time Tool
5. Rule Builder
6. Preset Manager
7. Preview / Diff
8. Apply Progress
9. Completion Summary
10. History / Undo
11. Settings
12. First Launch
13. Error States
14. RAW Safe Mode states

Then create a consistent Design System.

---

# 23. Phase 3 — Interaction Review

Before handoff to implementation, review:

- batch workflows
- mixed values
- destructive operations
- keyboard usage
- large file counts
- error recovery
- undo
- crash recovery
- resizing
- Chinese localization
- accessibility

Identify any interaction that is visually attractive but operationally unclear.

Replace it.

---

# 24. Design Handoff Requirements

After the UI direction is approved, produce:

```text
DESIGN.md
```

containing:

- design philosophy
- layout system
- tokens
- component inventory
- interaction principles
- screen specifications
- empty states
- error states
- responsive rules
- accessibility rules
- localization rules

The final design should be usable as the canonical reference for Claude Code or another implementation agent.

---

# 25. Product Priority

Whenever visual beauty conflicts with workflow clarity:

```text
Safety
>
Clarity
>
Speed
>
Information Density
>
Consistency
>
Aesthetics
>
Novelty
```

MoriMeta should look beautiful because it is precise and coherent, not because it is decorative.

---

# 26. Final Design Goal

The target experience is:

> A photographer imports 2,000 photos, immediately understands the metadata state, constructs a complex batch operation, previews every change, trusts the result, applies it, and can undo it without anxiety.

The interface should disappear behind that workflow.

If a user says:

> “This feels like the metadata tool that should already have existed.”

the design has succeeded.
