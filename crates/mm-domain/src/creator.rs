// SPDX-License-Identifier: GPL-3.0-or-later
//! `creator` field: read reconciliation and write planning for Embedded JPEG/TIFF targets
//! (METADATA_MODEL §2.2, §6, §8). Explicit mapping (ADR-08): EXIF `IFD0:Artist` (items joined
//! with "; "), XMP `dc:creator` (Seq), IPTC `By-line` only when the file already has IPTC, and
//! any other already-present location (`XMP-tiff:Artist`).
//!
//! Registry status: provisional (`REGISTRY_VERSION = 0`) until the S3 third-party checks freeze v1.

use crate::iptc;
use crate::plan::{ChangeKind, EntryStatus, Expect, FieldChange, FieldPlan, TagOp, Target};
use crate::snapshot::Snapshot;
use crate::value::{TextKind, validate_text};

pub const REGISTRY_VERSION: u32 = 0;
pub const FIELD: &str = "creator";

pub const ARTIST: &str = "IFD0:Artist";
pub const DC_CREATOR: &str = "XMP-dc:Creator";
pub const IPTC_BYLINE: &str = "IPTC:By-line";
pub const TIFF_ARTIST: &str = "XMP-tiff:Artist";
pub const IPTC_DIGEST: &str = iptc::DIGEST;
/// IPTC.pm: By-line => string[0,32]
pub const BYLINE_MAX_BYTES: usize = 32;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreatorEdit {
    Set(Vec<String>),
    Clear,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatorState {
    /// Effective value: XMP dc:creator > IFD0:Artist > IPTC By-line.
    pub effective: Option<Vec<String>>,
    /// Every location that holds a value, with its value.
    pub sources: Vec<(String, Vec<String>)>,
    pub conflicting: bool,
}

pub fn read(snap: &Snapshot) -> CreatorState {
    let mut sources = Vec::new();
    if let Some(v) = snap.list(DC_CREATOR) {
        sources.push((DC_CREATOR.to_owned(), v));
    }
    if let Some(v) = snap.text(ARTIST) {
        sources.push((ARTIST.to_owned(), split_artist(&v)));
    }
    if let Some(v) = snap.list(IPTC_BYLINE) {
        sources.push((IPTC_BYLINE.to_owned(), v));
    }
    if let Some(v) = snap.text(TIFF_ARTIST) {
        sources.push((TIFF_ARTIST.to_owned(), split_artist(&v)));
    }
    let effective = sources.first().map(|(_, v)| v.clone());
    let conflicting = sources.windows(2).any(|w| w[0].1 != w[1].1);
    CreatorState {
        effective,
        sources,
        conflicting,
    }
}

fn split_artist(s: &str) -> Vec<String> {
    s.split("; ")
        .filter(|x| !x.is_empty())
        .map(str::to_owned)
        .collect()
}

/// Validation of user input for the creator field.
pub fn validate(items: &[String]) -> Result<(), String> {
    if items.is_empty() {
        return Err("creator list is empty (use Clear to remove the field)".into());
    }
    for it in items {
        validate_text(it, TextKind::SingleLine).map_err(|e| format!("{it:?}: {e}"))?;
        if it.contains(';') {
            return Err(format!(
                "{it:?}: ';' is reserved as the EXIF Artist separator"
            ));
        }
        if it.trim() != it {
            return Err(format!("{it:?}: leading or trailing spaces"));
        }
    }
    Ok(())
}

/// Result of planning the creator field for one file.
pub type CreatorPlan = FieldPlan;

pub fn plan(snap: &Snapshot, edit: &CreatorEdit) -> CreatorPlan {
    let before = read(snap);
    let blocked = FieldPlan::blocked;
    let iptc = iptc::present(snap);
    if iptc && iptc::has_extra_records(snap) {
        return blocked("file contains more than one IPTC record; not written".into());
    }
    let mut ops = Vec::new();
    let mut expect = Vec::new();
    let mut notes = Vec::new();
    let after: Option<Vec<String>> = match edit {
        CreatorEdit::Set(items) => {
            if let Err(e) = validate(items) {
                return blocked(e);
            }
            let joined = items.join("; ");
            ops.push(TagOp::Set {
                tag: ARTIST.into(),
                values: vec![joined.clone()],
            });
            expect.push(Expect::Equals {
                tag: ARTIST.into(),
                values: vec![joined.clone()],
            });
            ops.push(TagOp::Set {
                tag: DC_CREATOR.into(),
                values: items.clone(),
            });
            expect.push(Expect::Equals {
                tag: DC_CREATOR.into(),
                values: items.clone(),
            });
            if snap.contains(TIFF_ARTIST) {
                ops.push(TagOp::Set {
                    tag: TIFF_ARTIST.into(),
                    values: vec![joined.clone()],
                });
                expect.push(Expect::Equals {
                    tag: TIFF_ARTIST.into(),
                    values: vec![joined],
                });
            }
            if iptc {
                for it in items {
                    if let Some(why) = iptc::refuse(snap, "By-line", it, BYLINE_MAX_BYTES) {
                        return blocked(why);
                    }
                }
                ops.push(TagOp::Set {
                    tag: IPTC_BYLINE.into(),
                    values: items.clone(),
                });
                expect.push(Expect::Equals {
                    tag: IPTC_BYLINE.into(),
                    values: items.clone(),
                });
                notes.push("IPTC By-line updated because the file already has IPTC".into());
            }
            Some(items.clone())
        }
        CreatorEdit::Clear => {
            for tag in [ARTIST, DC_CREATOR, TIFF_ARTIST, IPTC_BYLINE] {
                if snap.contains(tag) {
                    ops.push(TagOp::Delete { tag: tag.into() });
                    expect.push(Expect::Absent { tag: tag.into() });
                }
            }
            None
        }
    };
    let iptc_touched = ops.iter().any(|o| o.tag() == IPTC_BYLINE);
    if iptc_touched && snap.contains(IPTC_DIGEST) {
        ops.push(TagOp::UpdateIptcDigest);
        expect.push(Expect::IptcDigestCurrent);
    }
    // no change when every location already holds exactly the target value
    let unchanged = match edit {
        CreatorEdit::Set(items) => {
            !before.sources.is_empty()
                && before.sources.iter().all(|(_, v)| v == items)
                && snap.contains(ARTIST)
                && snap.contains(DC_CREATOR)
        }
        CreatorEdit::Clear => before.sources.is_empty(),
    };
    if unchanged {
        return CreatorPlan {
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
        notes.push(format!(
            "locations disagreed before the change: {:?}",
            before.sources
        ));
    }
    CreatorPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.effective,
            after,
            kind,
        }),
        ops,
        expect,
        notes,
    }
}

/// Effective creator for a target: for a RAW, the sidecar's `dc:creator` wins over the RAW's
/// own values (as Lightroom and Camera Raw read them).
pub fn read_target(t: &Target) -> CreatorState {
    match t {
        Target::Embedded(s) => read(s),
        Target::Sidecar { raw, sidecar } => match sidecar.and_then(|s| s.list(DC_CREATOR)) {
            Some(v) => CreatorState {
                effective: Some(v.clone()),
                sources: vec![(format!("sidecar {DC_CREATOR}"), v)],
                conflicting: false,
            },
            None => read(raw),
        },
    }
}

pub fn plan_target(t: &Target, edit: &CreatorEdit) -> CreatorPlan {
    match t {
        Target::Embedded(s) => plan(s, edit),
        Target::Sidecar { raw, sidecar } => plan_sidecar(raw, *sidecar, edit),
    }
}

/// Sidecar mode (METADATA_MODEL §8 W-S): only `XMP-dc:Creator` in the sidecar; the RAW is never
/// written (SAFETY_MODEL I-10), so a creator the RAW itself holds cannot be cleared.
fn plan_sidecar(raw: &Snapshot, sidecar: Option<&Snapshot>, edit: &CreatorEdit) -> CreatorPlan {
    let before = read_target(&Target::Sidecar { raw, sidecar }).effective;
    let in_sidecar = sidecar.and_then(|s| s.list(DC_CREATOR));
    let unchanged = FieldPlan {
        status: EntryStatus::NoChange,
        change: None,
        ops: vec![],
        expect: vec![],
        notes: vec![],
    };
    let (ops, expect, after) = match edit {
        CreatorEdit::Set(items) => {
            if let Err(e) = validate(items) {
                return FieldPlan::blocked(e);
            }
            if in_sidecar.as_ref() == Some(items) {
                return unchanged;
            }
            (
                vec![TagOp::Set {
                    tag: DC_CREATOR.into(),
                    values: items.clone(),
                }],
                vec![Expect::Equals {
                    tag: DC_CREATOR.into(),
                    values: items.clone(),
                }],
                Some(items.clone()),
            )
        }
        CreatorEdit::Clear => {
            if let Some(v) = read(raw).effective {
                return FieldPlan::blocked(format!(
                    "the RAW file itself holds creator {v:?}; a sidecar can override it but not remove it"
                ));
            }
            if in_sidecar.is_none() {
                return unchanged;
            }
            (
                vec![TagOp::Delete {
                    tag: DC_CREATOR.into(),
                }],
                vec![Expect::Absent {
                    tag: DC_CREATOR.into(),
                }],
                None,
            )
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
            before,
            after,
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

    #[test]
    fn set_on_file_without_iptc_writes_exif_and_xmp() {
        let p = plan(
            &snap(json!({"IFD0:Make": "NIKON"})),
            &CreatorEdit::Set(vec!["森 Morii".into()]),
        );
        assert_eq!(p.status, EntryStatus::Ready);
        assert_eq!(p.ops.len(), 2);
        assert!(!p.ops.iter().any(|o| o.tag().starts_with("IPTC")));
        assert_eq!(p.change.unwrap().kind, ChangeKind::Add);
    }

    #[test]
    fn latin_iptc_blocks_non_latin_but_accepts_cp1252() {
        let s = snap(json!({"IPTC:By-line": "Café", "Photoshop:IPTCDigest": "abc"}));
        assert!(matches!(
            plan(&s, &CreatorEdit::Set(vec!["森".into()])).status,
            EntryStatus::Blocked(_)
        ));
        let ok = plan(&s, &CreatorEdit::Set(vec!["© Zoë".into()]));
        assert_eq!(ok.status, EntryStatus::Ready);
        assert!(ok.ops.contains(&TagOp::UpdateIptcDigest));
        assert!(ok.expect.contains(&Expect::IptcDigestCurrent));
    }

    #[test]
    fn utf8_iptc_checks_bytes_not_chars() {
        let s = snap(json!({"IPTC:By-line": "x", "IPTC:CodedCharacterSet": "UTF8"}));
        assert_eq!(
            plan(&s, &CreatorEdit::Set(vec!["森".repeat(10)])).status,
            EntryStatus::Ready
        );
        assert!(matches!(
            plan(&s, &CreatorEdit::Set(vec!["森".repeat(11)])).status,
            EntryStatus::Blocked(_)
        ));
    }

    #[test]
    fn unchanged_value_is_no_change() {
        let s = snap(json!({"IFD0:Artist": "A; B", "XMP-dc:Creator": ["A", "B"]}));
        assert_eq!(
            plan(&s, &CreatorEdit::Set(vec!["A".into(), "B".into()])).status,
            EntryStatus::NoChange
        );
        assert_eq!(
            plan(&snap(json!({})), &CreatorEdit::Clear).status,
            EntryStatus::NoChange
        );
    }

    #[test]
    fn existing_nonstandard_location_is_updated_and_conflicts_noted() {
        let s = snap(json!({"IFD0:Artist": "Old", "XMP-tiff:Artist": "Other"}));
        let p = plan(&s, &CreatorEdit::Set(vec!["New".into()]));
        assert!(p.ops.iter().any(|o| o.tag() == TIFF_ARTIST));
        assert!(p.notes.iter().any(|n| n.contains("disagreed")));
    }

    #[test]
    fn clear_deletes_only_present_locations() {
        let s = snap(json!({"IFD0:Artist": "A", "IPTC:By-line": "A", "Photoshop:IPTCDigest": "x"}));
        let p = plan(&s, &CreatorEdit::Clear);
        let tags: Vec<&str> = p.ops.iter().map(|o| o.tag()).collect();
        assert_eq!(tags, vec![ARTIST, IPTC_BYLINE, IPTC_DIGEST]);
        assert_eq!(p.change.unwrap().kind, ChangeKind::Remove);
    }

    #[test]
    fn sidecar_mode_writes_only_xmp_and_cannot_clear_the_raw() {
        let raw = snap(json!({"IFD0:Artist": "Camera Owner"}));
        let t = Target::Sidecar {
            raw: &raw,
            sidecar: None,
        };
        assert_eq!(read_target(&t).effective, Some(vec!["Camera Owner".into()]));
        let p = plan_target(&t, &CreatorEdit::Set(vec!["Morii".into()]));
        assert_eq!(p.status, EntryStatus::Ready);
        assert_eq!(
            p.ops.iter().map(|o| o.tag()).collect::<Vec<_>>(),
            vec![DC_CREATOR]
        );
        assert!(p.notes.iter().any(|n| n.contains("keeps")));
        assert!(matches!(
            plan_target(&t, &CreatorEdit::Clear).status,
            EntryStatus::Blocked(_)
        ));
        // the sidecar wins when reading; the same value again is no change
        let side = snap(json!({"XMP-dc:Creator": ["Morii"]}));
        let t2 = Target::Sidecar {
            raw: &raw,
            sidecar: Some(&side),
        };
        assert_eq!(read_target(&t2).effective, Some(vec!["Morii".into()]));
        assert_eq!(
            plan_target(&t2, &CreatorEdit::Set(vec!["Morii".into()])).status,
            EntryStatus::NoChange
        );
    }

    #[test]
    fn invalid_input_and_multiple_iptc_records_are_blocked() {
        for bad in [
            vec![],
            vec!["a;b".to_string()],
            vec![" a".to_string()],
            vec!["a\nb".to_string()],
        ] {
            assert!(matches!(
                plan(&snap(json!({})), &CreatorEdit::Set(bad)).status,
                EntryStatus::Blocked(_)
            ));
        }
        let s = snap(json!({"IPTC:By-line": "a", "IPTC2:By-line": "b"}));
        assert!(matches!(
            plan(&s, &CreatorEdit::Set(vec!["x".into()])).status,
            EntryStatus::Blocked(_)
        ));
    }
}
