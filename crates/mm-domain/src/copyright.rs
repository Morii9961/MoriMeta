// SPDX-License-Identifier: GPL-3.0-or-later
//! `copyright` field: read reconciliation and write planning for Embedded JPEG/TIFF targets
//! (METADATA_MODEL §6, §8; PRODUCT_SPEC §6.7). Explicit mapping (ADR-08): EXIF `IFD0:Copyright`,
//! XMP `dc:rights` default language, IPTC `CopyrightNotice` only when the file already has IPTC,
//! and the already-present `XMP-tiff:Copyright`.
//!
//! `dc:rights` is a language alternative. It is written as `…-x-default`: ExifTool 13.59 deletes
//! every other language when the tag is written without a language code (checked 2026-09-27),
//! so other languages are kept and only reported. The default language reads back as
//! `XMP-dc:Rights` ([`crate::plan::read_key`]).
//!
//! Registry status: provisional (`creator::REGISTRY_VERSION`) until S3 freezes v1.

use crate::iptc;
use crate::plan::{ChangeKind, EntryStatus, Expect, FieldChange, FieldPlan, TagOp, Target};
use crate::snapshot::Snapshot;
use crate::value::{TextKind, validate_text};

pub const FIELD: &str = "copyright";

pub const EXIF: &str = "IFD0:Copyright";
/// Read key of the default language; written as [`DC_RIGHTS_WRITE`].
pub const DC_RIGHTS: &str = "XMP-dc:Rights";
pub const DC_RIGHTS_WRITE: &str = "XMP-dc:Rights-x-default";
pub const IPTC_NOTICE: &str = "IPTC:CopyrightNotice";
pub const TIFF_COPYRIGHT: &str = "XMP-tiff:Copyright";
pub const TIFF_COPYRIGHT_WRITE: &str = "XMP-tiff:Copyright-x-default";
/// IPTC.pm: `CopyrightNotice => string[0,128]`
pub const NOTICE_MAX_BYTES: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CopyrightEdit {
    Set(String),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CopyrightState {
    /// Effective value: XMP dc:rights (default language) > IFD0:Copyright > IPTC CopyrightNotice.
    pub effective: Option<String>,
    /// Every location that holds a value, with its value.
    pub sources: Vec<(String, String)>,
    pub conflicting: bool,
    /// `dc:rights` entries in other languages (kept as they are).
    pub other_languages: Vec<(String, String)>,
}

pub fn read(snap: &Snapshot) -> CopyrightState {
    let mut sources = Vec::new();
    for tag in [DC_RIGHTS, EXIF, IPTC_NOTICE, TIFF_COPYRIGHT] {
        if let Some(v) = snap.text(tag) {
            sources.push((tag.to_owned(), v));
        }
    }
    let prefix = format!("{DC_RIGHTS}-");
    let other_languages = snap
        .keys()
        .filter(|k| k.starts_with(&prefix))
        .filter_map(|k| snap.text(k).map(|v| (k.clone(), v)))
        .collect();
    let effective = sources.first().map(|(_, v)| v.clone());
    let conflicting = sources.windows(2).any(|w| w[0].1 != w[1].1);
    CopyrightState {
        effective,
        sources,
        conflicting,
        other_languages,
    }
}

/// Validation of user input for the copyright field.
pub fn validate(v: &str) -> Result<(), String> {
    validate_text(v, TextKind::SingleLine).map_err(|e| format!("{v:?}: {e}"))?;
    if v.trim() != v {
        return Err(format!("{v:?}: leading or trailing spaces"));
    }
    Ok(())
}

fn set(ops: &mut Vec<TagOp>, expect: &mut Vec<Expect>, write: &str, value: &str) {
    ops.push(TagOp::Set {
        tag: write.into(),
        values: vec![value.into()],
    });
    expect.push(Expect::Equals {
        tag: crate::plan::read_key(write).into(),
        values: vec![value.into()],
    });
}

pub fn plan(snap: &Snapshot, edit: &CopyrightEdit) -> FieldPlan {
    let before = read(snap);
    let iptc = iptc::present(snap);
    if iptc && iptc::has_extra_records(snap) {
        return FieldPlan::blocked("file contains more than one IPTC record; not written".into());
    }
    let mut ops = Vec::new();
    let mut expect = Vec::new();
    let mut notes = Vec::new();
    let after = match edit {
        CopyrightEdit::Set(v) => {
            if let Err(e) = validate(v) {
                return FieldPlan::blocked(e);
            }
            set(&mut ops, &mut expect, EXIF, v);
            set(&mut ops, &mut expect, DC_RIGHTS_WRITE, v);
            if snap.contains(TIFF_COPYRIGHT) {
                set(&mut ops, &mut expect, TIFF_COPYRIGHT_WRITE, v);
            }
            if iptc {
                if let Some(why) = iptc::refuse(snap, "CopyrightNotice", v, NOTICE_MAX_BYTES) {
                    return FieldPlan::blocked(why);
                }
                set(&mut ops, &mut expect, IPTC_NOTICE, v);
                notes.push("IPTC CopyrightNotice updated because the file already has IPTC".into());
            }
            Some(v.clone())
        }
        CopyrightEdit::Clear => {
            for (write, read) in [
                (EXIF, EXIF),
                (DC_RIGHTS_WRITE, DC_RIGHTS),
                (TIFF_COPYRIGHT_WRITE, TIFF_COPYRIGHT),
                (IPTC_NOTICE, IPTC_NOTICE),
            ] {
                if snap.contains(read) {
                    ops.push(TagOp::Delete { tag: write.into() });
                    expect.push(Expect::Absent { tag: read.into() });
                }
            }
            None
        }
    };
    if !before.other_languages.is_empty() {
        notes.push(format!(
            "copyright in other languages is kept unchanged: {:?}",
            before.other_languages
        ));
    }
    if ops.iter().any(|o| o.tag() == IPTC_NOTICE) && snap.contains(iptc::DIGEST) {
        ops.push(TagOp::UpdateIptcDigest);
        expect.push(Expect::IptcDigestCurrent);
    }
    let unchanged = match edit {
        CopyrightEdit::Set(v) => {
            before.sources.iter().all(|(_, x)| x == v)
                && snap.contains(EXIF)
                && snap.contains(DC_RIGHTS)
        }
        CopyrightEdit::Clear => before.sources.is_empty(),
    };
    if unchanged {
        return FieldPlan {
            status: EntryStatus::NoChange,
            change: None,
            ops: vec![],
            expect: vec![],
            notes,
        };
    }
    let kind = match (&before.effective, &after) {
        (None, Some(_)) => ChangeKind::Add,
        (Some(_), None) => ChangeKind::Remove,
        _ => ChangeKind::Modify,
    };
    if before.conflicting {
        notes.push(crate::plan::warning(format!(
            "locations disagreed before the change, all are set now: {:?}",
            before.sources
        )));
    }
    FieldPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.effective.map(|v| vec![v]),
            after: after.map(|v| vec![v]),
            kind,
        }),
        ops,
        expect,
        notes,
    }
}

/// Effective copyright for a target: for a RAW, the sidecar's default-language `dc:rights` wins.
pub fn read_target(t: &Target) -> CopyrightState {
    match t {
        Target::Embedded(s) => read(s),
        Target::Sidecar { raw, sidecar } => match sidecar.and_then(|s| s.text(DC_RIGHTS)) {
            Some(v) => CopyrightState {
                effective: Some(v.clone()),
                sources: vec![(format!("sidecar {DC_RIGHTS}"), v)],
                conflicting: false,
                other_languages: sidecar.map(|s| read(s).other_languages).unwrap_or_default(),
            },
            None => read(raw),
        },
    }
}

pub fn plan_target(t: &Target, edit: &CopyrightEdit) -> FieldPlan {
    match t {
        Target::Embedded(s) => plan(s, edit),
        Target::Sidecar { raw, sidecar } => plan_sidecar(raw, *sidecar, edit),
    }
}

/// Sidecar mode (METADATA_MODEL §8 W-S): only the default language of `dc:rights` in the
/// sidecar; the RAW is never written, so a copyright the RAW itself holds cannot be cleared.
fn plan_sidecar(raw: &Snapshot, sidecar: Option<&Snapshot>, edit: &CopyrightEdit) -> FieldPlan {
    let before = read_target(&Target::Sidecar { raw, sidecar }).effective;
    let in_sidecar = sidecar.and_then(|s| s.text(DC_RIGHTS));
    let unchanged = FieldPlan {
        status: EntryStatus::NoChange,
        change: None,
        ops: vec![],
        expect: vec![],
        notes: vec![],
    };
    let mut ops = Vec::new();
    let mut expect = Vec::new();
    let after = match edit {
        CopyrightEdit::Set(v) => {
            if let Err(e) = validate(v) {
                return FieldPlan::blocked(e);
            }
            if in_sidecar.as_deref() == Some(v.as_str()) {
                return unchanged;
            }
            set(&mut ops, &mut expect, DC_RIGHTS_WRITE, v);
            Some(v.clone())
        }
        CopyrightEdit::Clear => {
            if let Some(v) = read(raw).effective {
                return FieldPlan::blocked(format!(
                    "the RAW file itself holds copyright {v:?}; a sidecar can override it but not remove it"
                ));
            }
            if in_sidecar.is_none() {
                return unchanged;
            }
            ops.push(TagOp::Delete {
                tag: DC_RIGHTS_WRITE.into(),
            });
            expect.push(Expect::Absent {
                tag: DC_RIGHTS.into(),
            });
            None
        }
    };
    let mut notes = vec!["written to the XMP sidecar; the RAW file is not modified".to_string()];
    if let Some(v) = read(raw).effective.filter(|v| Some(v) != after.as_ref()) {
        notes.push(format!(
            "the RAW file itself keeps {v:?}; programs that ignore sidecars show that value"
        ));
    }
    let kind = match (&before, &after) {
        (None, Some(_)) => ChangeKind::Add,
        (Some(_), None) => ChangeKind::Remove,
        _ => ChangeKind::Modify,
    };
    FieldPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.map(|v| vec![v]),
            after: after.map(|v| vec![v]),
            kind,
        }),
        ops,
        expect,
        notes,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn snap(v: serde_json::Value) -> Snapshot {
        Snapshot::from_json(&v)
    }

    fn tags(p: &FieldPlan) -> Vec<&str> {
        p.ops.iter().map(|o| o.tag()).collect()
    }

    #[test]
    fn set_writes_exif_and_default_language_only() {
        let p = plan(
            &snap(json!({"XMP-dc:Rights-de": "Rechte"})),
            &CopyrightEdit::Set("© 森 2026".into()),
        );
        assert_eq!(p.status, EntryStatus::Ready);
        assert_eq!(tags(&p), vec![EXIF, DC_RIGHTS_WRITE]);
        // verification reads the default language back without the suffix
        assert!(p.expect.contains(&Expect::Equals {
            tag: DC_RIGHTS.into(),
            values: vec!["© 森 2026".into()]
        }));
        assert!(p.notes.iter().any(|n| n.contains("other languages")));
        assert_eq!(p.change.unwrap().kind, ChangeKind::Add);
    }

    #[test]
    fn read_prefers_xmp_default_language_and_reports_conflicts() {
        let s = read(&snap(json!({
            "IFD0:Copyright": "EXIF", "XMP-dc:Rights": "XMP", "XMP-dc:Rights-fr": "FR"
        })));
        assert_eq!(s.effective.as_deref(), Some("XMP"));
        assert!(s.conflicting);
        assert_eq!(s.other_languages.len(), 1);
    }

    #[test]
    fn latin_iptc_blocks_non_latin_and_long_values() {
        let s = snap(json!({"IPTC:CopyrightNotice": "Old", "Photoshop:IPTCDigest": "x"}));
        assert!(matches!(
            plan(&s, &CopyrightEdit::Set("© 森".into())).status,
            EntryStatus::Blocked(_)
        ));
        let ok = plan(&s, &CopyrightEdit::Set("© Zoë 2026".into()));
        assert_eq!(ok.status, EntryStatus::Ready);
        assert!(tags(&ok).contains(&IPTC_NOTICE));
        assert!(ok.ops.contains(&TagOp::UpdateIptcDigest));
        let utf8 = snap(json!({"IPTC:CopyrightNotice": "x", "IPTC:CodedCharacterSet": "UTF8"}));
        assert_eq!(
            plan(&utf8, &CopyrightEdit::Set("森".repeat(42))).status,
            EntryStatus::Ready
        ); // 126 bytes
        assert!(matches!(
            plan(&utf8, &CopyrightEdit::Set("森".repeat(43))).status,
            EntryStatus::Blocked(_)
        )); // 129 bytes
    }

    #[test]
    fn tiff_copy_is_updated_and_clear_removes_only_present_locations() {
        let s = snap(
            json!({"IFD0:Copyright": "A", "XMP-tiff:Copyright": "A", "XMP-dc:Rights-de": "R"}),
        );
        let p = plan(&s, &CopyrightEdit::Set("B".into()));
        assert!(tags(&p).contains(&TIFF_COPYRIGHT_WRITE));
        let c = plan(&s, &CopyrightEdit::Clear);
        assert_eq!(tags(&c), vec![EXIF, TIFF_COPYRIGHT_WRITE]);
        assert_eq!(c.change.unwrap().kind, ChangeKind::Remove);
    }

    #[test]
    fn sidecar_mode_writes_only_the_default_language() {
        let raw = snap(json!({"IFD0:Copyright": "Nikon owner"}));
        let t = Target::Sidecar {
            raw: &raw,
            sidecar: None,
        };
        let p = plan_target(&t, &CopyrightEdit::Set("© Morii".into()));
        assert_eq!(tags(&p), vec![DC_RIGHTS_WRITE]);
        assert_eq!(p.change.unwrap().before, Some(vec!["Nikon owner".into()]));
        assert!(matches!(
            plan_target(&t, &CopyrightEdit::Clear).status,
            EntryStatus::Blocked(_)
        ));
        let side = snap(json!({"XMP-dc:Rights": "© Morii"}));
        let t2 = Target::Sidecar {
            raw: &raw,
            sidecar: Some(&side),
        };
        assert_eq!(
            plan_target(&t2, &CopyrightEdit::Set("© Morii".into())).status,
            EntryStatus::NoChange
        );
    }

    #[test]
    fn unchanged_and_invalid_values() {
        let s = snap(json!({"IFD0:Copyright": "C", "XMP-dc:Rights": "C"}));
        assert_eq!(
            plan(&s, &CopyrightEdit::Set("C".into())).status,
            EntryStatus::NoChange
        );
        assert_eq!(
            plan(&snap(json!({})), &CopyrightEdit::Clear).status,
            EntryStatus::NoChange
        );
        for bad in ["", " x", "a\nb"] {
            assert!(matches!(
                plan(&snap(json!({})), &CopyrightEdit::Set(bad.into())).status,
                EntryStatus::Blocked(_)
            ));
        }
    }
}
