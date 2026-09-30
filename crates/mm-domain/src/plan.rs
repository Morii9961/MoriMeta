// SPDX-License-Identifier: GPL-3.0-or-later
//! Plan model (METADATA_MODEL §9). A Plan is immutable once created; the executable part of each
//! entry (`action`) is persisted when an Operation starts so that "continue" after a crash
//! executes exactly what was previewed (SAFETY_MODEL §9).

use serde::{Deserialize, Serialize};

use crate::snapshot::Snapshot;

/// Identity of a file at planning time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    /// Volume serial + 128-bit file ID, hex.
    pub file_id: String,
    /// Last-write time (100 ns units since 1601, as reported by Windows).
    pub mtime: u64,
}

/// One ExifTool tag operation. Tag names are family-1 qualified (`IFD0:Artist`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TagOp {
    /// Set a tag; several values write several list items.
    Set {
        tag: String,
        values: Vec<String>,
    },
    Delete {
        tag: String,
    },
    /// `-Photoshop:IPTCDigest=new` (digest of the IPTC written in the same command).
    UpdateIptcDigest,
}

impl TagOp {
    pub fn tag(&self) -> &str {
        match self {
            TagOp::Set { tag, .. } | TagOp::Delete { tag } => tag,
            TagOp::UpdateIptcDigest => "Photoshop:IPTCDigest",
        }
    }
}

/// The key under which ExifTool reads back what `write_tag` writes. A language-alternative
/// write names the language (`XMP-dc:Rights-x-default`, so that other languages are kept),
/// while the default language reads back without the suffix (`XMP-dc:Rights`).
pub fn read_key(write_tag: &str) -> &str {
    write_tag.strip_suffix("-x-default").unwrap_or(write_tag)
}

/// Result of planning one field for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPlan {
    pub status: EntryStatus,
    pub change: Option<FieldChange>,
    pub ops: Vec<TagOp>,
    pub expect: Vec<Expect>,
    pub notes: Vec<String>,
}

impl FieldPlan {
    pub fn blocked(why: String) -> FieldPlan {
        FieldPlan {
            status: EntryStatus::Blocked(why),
            change: None,
            ops: vec![],
            expect: vec![],
            notes: vec![],
        }
    }
}

/// One field's share of the write of an entry that changes several fields (a Preset). Kept so
/// that the user can leave that field out in Preview (INTERACTION_SPEC §9) and the write and its
/// verification are made again from the other fields alone.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldPart {
    pub field: String,
    pub ops: Vec<TagOp>,
    pub expect: Vec<Expect>,
}

/// The shares of the ready fields, when there are several (one field has nothing to split).
pub fn field_parts(fields: &[(&str, FieldPlan)]) -> Vec<FieldPart> {
    let ready: Vec<FieldPart> = fields
        .iter()
        .filter(|(_, f)| f.status == EntryStatus::Ready)
        .filter_map(|(_, f)| {
            f.change.as_ref().map(|c| FieldPart {
                field: c.field.clone(),
                ops: f.ops.clone(),
                expect: f.expect.clone(),
            })
        })
        .collect();
    if ready.len() > 1 { ready } else { vec![] }
}

/// Several fields planned for one file (a Preset), combined into what the file's entry gets.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergedPlan {
    pub status: EntryStatus,
    pub changes: Vec<FieldChange>,
    pub ops: Vec<TagOp>,
    pub expect: Vec<Expect>,
    pub notes: Vec<String>,
}

/// Combine the plans of the fields of one file, each with its field name. The ready fields are
/// written together; a blocked or unsupported field is left out with a note, so that one missing
/// value does not stop the others (PRODUCT_SPEC §6.9). The file is blocked only when no field is
/// ready and one is blocked. A single field keeps its own status and notes unchanged.
pub fn merge(mut fields: Vec<(&str, FieldPlan)>) -> MergedPlan {
    if fields.len() == 1 {
        let (_, f) = fields.remove(0);
        return MergedPlan {
            status: f.status,
            changes: f.change.into_iter().collect(),
            ops: f.ops,
            expect: f.expect,
            notes: f.notes,
        };
    }
    let any_ready = fields.iter().any(|(_, f)| f.status == EntryStatus::Ready);
    let mut m = MergedPlan {
        status: EntryStatus::NoChange,
        changes: vec![],
        ops: vec![],
        expect: vec![],
        notes: vec![],
    };
    let mut blocked: Vec<String> = vec![];
    let mut unsupported: Vec<String> = vec![];
    for (label, f) in fields {
        m.notes.extend(f.notes);
        match f.status {
            EntryStatus::Ready => {
                m.changes.extend(f.change);
                for op in f.ops {
                    if !m.ops.contains(&op) {
                        m.ops.push(op); // the IPTC digest update is shared
                    }
                }
                for e in f.expect {
                    if !m.expect.contains(&e) {
                        m.expect.push(e);
                    }
                }
            }
            EntryStatus::NoChange => {}
            EntryStatus::Blocked(why) => {
                if any_ready {
                    m.notes.push(format!("{label} not changed: {why}"));
                }
                blocked.push(format!("{label}: {why}"));
            }
            EntryStatus::Unsupported(why) => {
                if any_ready {
                    m.notes.push(format!("{label} not changed: {why}"));
                }
                unsupported.push(format!("{label}: {why}"));
            }
        }
    }
    m.status = if any_ready {
        EntryStatus::Ready
    } else if !blocked.is_empty() {
        EntryStatus::Blocked(blocked.join("; "))
    } else if !unsupported.is_empty() {
        EntryStatus::Unsupported(unsupported.join("; "))
    } else {
        EntryStatus::NoChange
    };
    m
}

/// What verification (SAFETY_MODEL §5 V2) must find in the temporary output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "expect", rename_all = "snake_case")]
pub enum Expect {
    Equals {
        tag: String,
        values: Vec<String>,
    },
    Absent {
        tag: String,
    },
    /// A number within `within` of `value` (decimal strings; read numerically, e.g. GPS).
    Near {
        tag: String,
        value: String,
        within: String,
    },
    /// IPTCDigest present and matching the IPTC block (no "not current" warning).
    IptcDigestCurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Add,
    Modify,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    pub before: Option<Vec<String>>,
    pub after: Option<Vec<String>>,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "reason", rename_all = "snake_case")]
pub enum EntryStatus {
    Ready,
    NoChange,
    Blocked(String),
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum EntryAction {
    /// Metadata write through ExifTool (Embedded target).
    Write {
        ops: Vec<TagOp>,
        expect: Vec<Expect>,
    },
    /// Undo: put back the verified backup `backup` (content hash `h0`), only if the file still
    /// has the post-operation hash `h1` (otherwise Conflict).
    Restore {
        backup: String,
        h0: String,
        h1: String,
    },
    /// Undo of a file that was deleted or moved after the operation (SAFETY_MODEL §7.2): create
    /// it again at its path from the verified backup `backup` (content hash `h0`, `size` bytes).
    /// Never replaces a file that exists at the path by then.
    Recreate {
        backup: String,
        h0: String,
        size: u64,
    },
    /// Undo of a file that the undone Operation created (SAFETY_MODEL §4.3, §7.2): move it into
    /// the backup store instead of deleting it, only if its content is still `h`.
    MoveToBackupStore { h: String },
    /// A new file written from nothing by ExifTool (a new XMP sidecar, SAFETY_MODEL §4.3): it
    /// holds only these tags and never replaces a file that exists at the path.
    CreateFile {
        ops: Vec<TagOp>,
        expect: Vec<Expect>,
    },
}

/// Where a field is written (SAFETY_MODEL §3 FormatPolicy).
#[derive(Debug, Clone, Copy)]
pub enum Target<'a> {
    /// In the file itself (JPEG).
    Embedded(&'a Snapshot),
    /// In the XMP sidecar of a read-only RAW (`raw`, empty for a sidecar selected on its own);
    /// `sidecar` is the existing sidecar, if any. Only XMP tags are written.
    Sidecar {
        raw: &'a Snapshot,
        sidecar: Option<&'a Snapshot>,
    },
}

/// Notes that Preview counts as Warnings (INTERACTION_SPEC §5: e.g. EXIF ≠ XMP, both will be
/// set) start with this; the rest are plain notes.
pub const WARNING: &str = "warning: ";

pub fn warning(text: impl std::fmt::Display) -> String {
    format!("{WARNING}{text}")
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub seq: u32,
    /// The file that is written (for a RAW: its XMP sidecar).
    pub path: String,
    /// The read-only RAW whose sidecar `path` is.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub raw: Option<String>,
    pub fingerprint: Fingerprint,
    pub status: EntryStatus,
    pub changes: Vec<FieldChange>,
    pub action: Option<EntryAction>,
    pub notes: Vec<String>,
    /// Left out by the user in Preview (a new Plan version); keeps the status it would have had.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub excluded: bool,
    /// Each field's share of `action` when several fields change ([`FieldPart`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub parts: Vec<FieldPart>,
    /// Changes the user left out in Preview (INTERACTION_SPEC §9), shown struck through; not
    /// written, not verified, not counted. `changes` holds only what is written.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub excluded_changes: Vec<FieldChange>,
}

/// What a Plan was made from (METADATA_MODEL §9 `created_from`), so that the same edit can be
/// planned again for files that could not be written. Times and shifts are kept as the text the
/// user typed (`time::format_local` / `format_shift`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "from", rename_all = "snake_case")]
pub enum PlanSource {
    /// `None` clears.
    Creator {
        set: Option<Vec<String>>,
    },
    Copyright {
        set: Option<String>,
    },
    /// `lat,lon[,alt]`; `None` removes.
    Gps {
        set: Option<String>,
    },
    CaptureTime {
        tool: TimeSpec,
        digitized: bool,
    },
    Preset {
        id: Option<String>,
        preset: crate::rules::Preset,
    },
    Undo {
        of: String,
    },
    /// Several edits staged together in the batch editor: the field edits as an unconditional
    /// Preset, and a capture-time tool over the whole selection.
    Batch {
        preset: crate::rules::Preset,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        time: Option<TimeSpec>,
        #[serde(default = "yes")]
        digitized: bool,
    },
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case")]
pub enum TimeSpec {
    Absolute {
        to: String,
    },
    Shift {
        by: String,
    },
    /// `order`: `time` or `name`.
    Sequence {
        start: String,
        step: String,
        order: String,
    },
    PreserveRelative {
        anchor: String,
        to: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanKind {
    Apply,
    Undo { of: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub version: u32,
    pub kind: PlanKind,
    pub title: String,
    pub registry_version: u32,
    pub exiftool_version: String,
    pub entries: Vec<PlanEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source: Option<PlanSource>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct WriteTargets {
    /// Into the file itself (a JPEG).
    pub in_file: usize,
    /// Into an existing XMP sidecar (or an XMP file chosen on its own).
    pub sidecar: usize,
    /// A new XMP sidecar next to a RAW.
    pub new_sidecar: usize,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub files: usize,
    pub ready: usize,
    pub no_change: usize,
    pub blocked: usize,
    pub unsupported: usize,
    /// Left out by the user; not counted in the other groups.
    pub excluded: usize,
    /// Ready entries with at least one warning.
    pub warnings: usize,
    /// Where the executable entries write (PRODUCT_SPEC §6.12 "counts by write target").
    pub targets: WriteTargets,
    pub changes: usize,
}

impl PlanEntry {
    /// Left out: the whole file, or every one of its changes.
    pub fn left_out(&self) -> bool {
        self.excluded || (self.changes.is_empty() && !self.excluded_changes.is_empty())
    }

    /// Leave the change of `field` out (or take it in again). The write and what verification
    /// expects are made again from the fields that stay, so every tag of the field goes with it
    /// ("excluding Capture time drops all three date tags"). Returns false when the entry has no
    /// change of that field; an error when it cannot be split.
    pub fn set_field_excluded(&mut self, field: &str, excluded: bool) -> Result<bool, String> {
        let from = if excluded {
            &self.changes
        } else {
            &self.excluded_changes
        };
        let Some(i) = from.iter().position(|c| c.field == field) else {
            return Ok(false);
        };
        // one field: leaving it out leaves the entry nothing to write (left_out)
        if self.parts.is_empty() && self.changes.len() + self.excluded_changes.len() > 1 {
            return Err(format!("entry {} cannot leave out {field} alone", self.seq));
        }
        if excluded {
            let c = self.changes.remove(i);
            self.excluded_changes.push(c);
        } else {
            let c = self.excluded_changes.remove(i);
            self.changes.push(c);
        }
        if !self.parts.is_empty() && !self.changes.is_empty() {
            let mut ops: Vec<TagOp> = vec![];
            let mut expect: Vec<Expect> = vec![];
            for p in &self.parts {
                if !self.changes.iter().any(|c| c.field == p.field) {
                    continue;
                }
                for o in &p.ops {
                    if !ops.contains(o) {
                        ops.push(o.clone()); // the IPTC digest update is shared
                    }
                }
                for e in &p.expect {
                    if !expect.contains(e) {
                        expect.push(e.clone());
                    }
                }
            }
            match &mut self.action {
                Some(EntryAction::Write { ops: o, expect: x })
                | Some(EntryAction::CreateFile { ops: o, expect: x }) => {
                    *o = ops;
                    *x = expect;
                }
                _ => return Err(format!("entry {} has no write to split", self.seq)),
            }
        }
        // keep the order the Preview showed
        let order = |f: &str| self.parts.iter().position(|p| p.field == f);
        self.changes.sort_by_key(|c| order(&c.field));
        Ok(true)
    }

    /// The warnings among the notes, without the marker.
    pub fn warnings(&self) -> impl Iterator<Item = &str> {
        self.notes.iter().filter_map(|n| n.strip_prefix(WARNING))
    }
}

impl Plan {
    /// `apply` or `undo`, for logs.
    pub fn kind_name(&self) -> &'static str {
        match self.kind {
            PlanKind::Apply => "apply",
            PlanKind::Undo { .. } => "undo",
        }
    }

    pub fn summary(&self) -> PlanSummary {
        let mut s = PlanSummary {
            files: self.entries.len(),
            ..Default::default()
        };
        for e in &self.entries {
            if e.left_out() {
                s.excluded += 1;
                continue;
            }
            if e.status == EntryStatus::Ready && e.warnings().next().is_some() {
                s.warnings += 1;
            }
            if e.status == EntryStatus::Ready && e.action.is_some() {
                let xmp = e.path.to_ascii_lowercase().ends_with(".xmp");
                match e.action {
                    Some(EntryAction::CreateFile { .. }) => s.targets.new_sidecar += 1,
                    _ if xmp => s.targets.sidecar += 1,
                    _ => s.targets.in_file += 1,
                }
            }
            match e.status {
                EntryStatus::Ready => s.ready += 1,
                EntryStatus::NoChange => s.no_change += 1,
                EntryStatus::Blocked(_) => s.blocked += 1,
                EntryStatus::Unsupported(_) => s.unsupported += 1,
            }
            s.changes += e.changes.len();
        }
        s
    }

    /// What the user must acknowledge before this Plan runs (INTERACTION_SPEC §4–5): `remove:<field>`
    /// for every field it removes somewhere, `unsupported` when some file cannot take a change,
    /// `large` above 1,000 files. Sorted; empty for a low-risk Plan.
    pub fn required_acks(&self) -> Vec<String> {
        let mut out = std::collections::BTreeSet::new();
        let mut files = 0usize;
        for e in self.executable() {
            files += 1;
            for c in &e.changes {
                if c.kind == ChangeKind::Remove {
                    out.insert(format!("remove:{}", c.field));
                }
            }
        }
        if self
            .entries
            .iter()
            .any(|e| !e.excluded && matches!(e.status, EntryStatus::Unsupported(_)))
        {
            out.insert("unsupported".into());
        }
        if files > 1000 {
            out.insert("large".into());
        }
        out.into_iter().collect()
    }

    /// Leave the change of `field` out (or take it in again) in the entries `seqs`, or in every
    /// entry that has it ("the whole edit", INTERACTION_SPEC §9). Returns how many entries
    /// changed; an unknown sequence number is an error.
    pub fn set_field_excluded(
        &mut self,
        seqs: Option<&[u32]>,
        field: &str,
        excluded: bool,
    ) -> Result<usize, String> {
        if let Some(seqs) = seqs
            && let Some(q) = seqs
                .iter()
                .find(|q| !self.entries.iter().any(|e| e.seq == **q))
        {
            return Err(format!("plan has no entry {q}"));
        }
        let mut n = 0;
        for e in &mut self.entries {
            if seqs.is_none_or(|s| s.contains(&e.seq)) && e.set_field_excluded(field, excluded)? {
                n += 1;
            }
        }
        Ok(n)
    }

    /// Entries that an Operation will execute.
    pub fn executable(&self) -> impl Iterator<Item = &PlanEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == EntryStatus::Ready && e.action.is_some() && !e.left_out())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn field(name: &str, tag: &str, value: &str) -> (String, FieldPlan) {
        (
            name.to_owned(),
            FieldPlan {
                status: EntryStatus::Ready,
                change: Some(FieldChange {
                    field: name.into(),
                    before: None,
                    after: Some(vec![value.into()]),
                    kind: ChangeKind::Add,
                }),
                ops: vec![
                    TagOp::Set {
                        tag: tag.into(),
                        values: vec![value.into()],
                    },
                    TagOp::UpdateIptcDigest,
                ],
                expect: vec![
                    Expect::Equals {
                        tag: tag.into(),
                        values: vec![value.into()],
                    },
                    Expect::IptcDigestCurrent,
                ],
                notes: vec![],
            },
        )
    }

    /// INTERACTION_SPEC §9: one field of a multi-field entry is left out and taken in again; the
    /// write and what verification expects follow, the shared IPTC digest update stays once.
    #[test]
    fn a_field_is_left_out_and_taken_in_again() {
        let owned = [
            field("creator", "IPTC:By-line", "Morii"),
            field("copyright", "IPTC:CopyrightNotice", "(c) Morii"),
        ];
        let fields: Vec<(&str, FieldPlan)> =
            owned.iter().map(|(n, f)| (n.as_str(), f.clone())).collect();
        let parts = field_parts(&fields);
        let m = merge(fields);
        let mut e = PlanEntry {
            seq: 0,
            path: "x.jpg".into(),
            raw: None,
            fingerprint: Fingerprint {
                size: 1,
                file_id: "f".into(),
                mtime: 0,
            },
            status: EntryStatus::Ready,
            changes: m.changes,
            action: Some(EntryAction::Write {
                ops: m.ops,
                expect: m.expect,
            }),
            notes: vec![],
            excluded: false,
            parts,
            excluded_changes: vec![],
        };
        let whole = e.clone();
        assert_eq!(e.set_field_excluded("copyright", true), Ok(true));
        assert_eq!(e.changes.len(), 1);
        assert_eq!(e.excluded_changes[0].field, "copyright");
        let Some(EntryAction::Write { ops, expect }) = &e.action else {
            panic!()
        };
        assert_eq!(ops.len(), 2, "{ops:?}"); // By-line + the digest
        assert!(!ops.iter().any(|o| o.tag() == "IPTC:CopyrightNotice"));
        assert!(expect.contains(&Expect::IptcDigestCurrent));
        assert!(!e.left_out());
        assert_eq!(e.set_field_excluded("gps", true), Ok(false));
        // both out: nothing is written; both in again: exactly the previewed write
        e.set_field_excluded("creator", true).unwrap();
        assert!(e.left_out());
        e.set_field_excluded("copyright", false).unwrap();
        e.set_field_excluded("creator", false).unwrap();
        assert_eq!(e, whole);
    }

    #[test]
    fn a_single_field_entry_is_left_out_whole() {
        let (n, f) = field("creator", "XMP-dc:Creator", "Morii");
        let m = merge(vec![(n.as_str(), f)]);
        let mut plan = Plan {
            id: "p".into(),
            version: 1,
            kind: PlanKind::Apply,
            title: String::new(),
            registry_version: 1,
            exiftool_version: String::new(),
            entries: vec![PlanEntry {
                seq: 0,
                path: "x.jpg".into(),
                raw: None,
                fingerprint: Fingerprint {
                    size: 1,
                    file_id: "f".into(),
                    mtime: 0,
                },
                status: EntryStatus::Ready,
                changes: m.changes,
                action: Some(EntryAction::Write {
                    ops: m.ops,
                    expect: m.expect,
                }),
                notes: vec![],
                excluded: false,
                parts: vec![],
                excluded_changes: vec![],
            }],
            source: None,
        };
        assert_eq!(plan.executable().count(), 1);
        assert_eq!(plan.set_field_excluded(None, "creator", true), Ok(1));
        assert_eq!(plan.executable().count(), 0);
        let s = plan.summary();
        assert_eq!((s.excluded, s.ready, s.changes), (1, 0, 0));
        assert!(plan.required_acks().is_empty());
        assert!(
            plan.set_field_excluded(Some(&[7]), "creator", false)
                .is_err()
        );
        assert_eq!(plan.set_field_excluded(Some(&[0]), "creator", false), Ok(1));
        assert_eq!(plan.executable().count(), 1);
    }
}
