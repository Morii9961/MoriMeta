// SPDX-License-Identifier: GPL-3.0-or-later
//! `capture_time` field for Embedded JPEG/TIFF targets (METADATA_MODEL §5, §8): reading the
//! effective time and turning the result of a time tool (`time::apply`) into tag writes (§5.3).
//!
//! Every location receives the new wall-clock time in its own existing shape: a date-only value
//! stays date-only, a value with a time keeps its own UTC offset (the MVP tools never change
//! offsets; that is D-18), and its fraction of a second is kept by Shift / Preserve Relative
//! Timing and dropped by Absolute / Sequence (§5.2). Locations other than EXIF DateTimeOriginal
//! and CreateDate are only updated when they already exist.
//!
//! Checked with ExifTool 13.59 (2026-09-27): all these formats read back exactly as written;
//! but `IPTC:TimeCreated` written without an offset gets the computer's own time zone, so an
//! IPTC time is only ever written with the offset it already has.

use chrono::{NaiveDate, NaiveDateTime};

use crate::iptc;
use crate::plan::{ChangeKind, EntryStatus, Expect, FieldChange, FieldPlan, TagOp, Target};
use crate::snapshot::Snapshot;
use crate::time::{CaptureTime, SubSec, TimeOpError, parse_offset};

pub const FIELD: &str = "capture_time";

pub const DTO: &str = "ExifIFD:DateTimeOriginal";
pub const SUBSEC_DTO: &str = "ExifIFD:SubSecTimeOriginal";
pub const OFFSET_DTO: &str = "ExifIFD:OffsetTimeOriginal";
pub const CREATE: &str = "ExifIFD:CreateDate";
pub const SUBSEC_CREATE: &str = "ExifIFD:SubSecTimeDigitized";
pub const IFD0_DTO: &str = "IFD0:DateTimeOriginal";
pub const XMP_DTO: &str = "XMP-exif:DateTimeOriginal";
pub const XMP_DATE_CREATED: &str = "XMP-photoshop:DateCreated";
pub const XMP_CREATE: &str = "XMP-xmp:CreateDate";
pub const IPTC_DATE: &str = "IPTC:DateCreated";
pub const IPTC_TIME: &str = "IPTC:TimeCreated";

/// A date/time as ExifTool prints it: `YYYY:MM:DD[ HH:MM:SS[.frac]][±HH:MM|Z]`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Stamp {
    date: NaiveDate,
    /// `HH:MM:SS`, fraction digits, offset text (`+09:00`, `Z`).
    time: Option<(String, Option<String>, Option<String>)>,
}

fn parse_stamp(s: &str) -> Option<Stamp> {
    let s = s.trim_end_matches('\0').trim();
    let date = NaiveDate::parse_from_str(s.get(..10)?, "%Y:%m:%d").ok()?;
    let rest = &s[10..];
    if rest.is_empty() {
        return Some(Stamp { date, time: None });
    }
    let rest = rest.strip_prefix(' ')?;
    let hms = rest.get(..8)?;
    chrono::NaiveTime::parse_from_str(hms, "%H:%M:%S").ok()?;
    let mut tail = &rest[8..];
    let mut frac = None;
    if let Some(t) = tail.strip_prefix('.') {
        let n = t.bytes().take_while(u8::is_ascii_digit).count();
        if n == 0 {
            return None;
        }
        frac = Some(t[..n].to_owned());
        tail = &t[n..];
    }
    let offset = match tail {
        "" => None,
        "Z" => Some("Z".to_owned()),
        o => {
            parse_offset(o)?;
            Some(o.to_owned())
        }
    };
    Some(Stamp {
        date,
        time: Some((hms.to_owned(), frac, offset)),
    })
}

/// Parse an IPTC `TimeCreated` (`HH:MM:SS±HH:MM`) into its time and offset.
fn parse_iptc_time(s: &str) -> Option<(String, String)> {
    let s = s.trim();
    let hms = s.get(..8)?;
    chrono::NaiveTime::parse_from_str(hms, "%H:%M:%S").ok()?;
    let off = &s[8..];
    parse_offset(off).map(|_| (hms.to_owned(), off.to_owned()))
}

/// `YYYY:MM:DD HH:MM:SS[.frac][±HH:MM]`: how the Preview shows a capture time.
pub fn display(t: &CaptureTime) -> String {
    let mut s = t.exif_datetime();
    if let Some(ss) = &t.subsec {
        s.push('.');
        s.push_str(ss.as_str());
    }
    if let Some(o) = t.exif_offset() {
        s.push_str(&o);
    }
    s
}

fn from_stamp(st: &Stamp) -> Option<CaptureTime> {
    let (hms, frac, off) = st.time.as_ref()?;
    let local = NaiveDateTime::parse_from_str(
        &format!("{} {hms}", st.date.format("%Y:%m:%d")),
        "%Y:%m:%d %H:%M:%S",
    )
    .ok()?;
    let offset = match off.as_deref() {
        Some("Z") => parse_offset("+00:00"),
        Some(o) => parse_offset(o),
        None => None,
    };
    Some(CaptureTime {
        local,
        subsec: frac.as_deref().and_then(SubSec::parse),
        offset,
    })
}

/// The effective capture time (METADATA_MODEL §8 read priority): EXIF DateTimeOriginal (+ its
/// sub-seconds and offset) > XMP-exif:DateTimeOriginal > XMP-photoshop:DateCreated (with a
/// time) > IPTC DateCreated + TimeCreated > IFD0:DateTimeOriginal. `Err` when the first
/// location that holds a value does not hold a valid one (only Absolute can then be applied).
pub fn read(snap: &Snapshot) -> Result<Option<CaptureTime>, String> {
    if let Some(v) = snap.text(DTO) {
        return CaptureTime::from_exif(
            &v,
            snap.text(SUBSEC_DTO).as_deref(),
            snap.text(OFFSET_DTO).as_deref(),
        )
        .map(Some)
        .map_err(|e| format!("{DTO}: {e:?}"));
    }
    for tag in [XMP_DTO, XMP_DATE_CREATED] {
        if let Some(v) = snap.text(tag) {
            match parse_stamp(&v) {
                Some(st) if st.time.is_some() => return Ok(from_stamp(&st)),
                Some(_) => {} // date only: no time to read
                None => return Err(format!("{tag}: unrecognised value {v:?}")),
            }
        }
    }
    if let (Some(d), Some(t)) = (snap.text(IPTC_DATE), snap.text(IPTC_TIME)) {
        let (Some(date), Some((hms, off))) = (
            NaiveDate::parse_from_str(&d, "%Y:%m:%d").ok(),
            parse_iptc_time(&t),
        ) else {
            return Err(format!("IPTC date/time: unrecognised value {d:?} {t:?}"));
        };
        return Ok(from_stamp(&Stamp {
            date,
            time: Some((hms, None, Some(off))),
        }));
    }
    if let Some(v) = snap.text(IFD0_DTO) {
        return CaptureTime::from_exif(&v, None, None)
            .map(Some)
            .map_err(|e| format!("{IFD0_DTO}: {e:?}"));
    }
    Ok(None)
}

/// Plan the tag writes that give this file the capture time `after` (the time tool's result).
/// `keep_subsec` is true for Shift and Preserve Relative Timing; `digitized` also sets EXIF
/// CreateDate ("also change the digitized time", on by default).
pub fn plan(
    snap: &Snapshot,
    after: &Result<CaptureTime, TimeOpError>,
    keep_subsec: bool,
    digitized: bool,
) -> FieldPlan {
    let before = read(snap).ok().flatten();
    let after = match after {
        Ok(a) => a,
        Err(TimeOpError::NoValidSourceTime) => {
            return FieldPlan::blocked(
                "no valid capture time to start from; only Absolute can be applied".into(),
            );
        }
        Err(TimeOpError::OutOfRange) => {
            return FieldPlan::blocked("the result is outside years 0001–9999".into());
        }
        Err(e) => return FieldPlan::blocked(format!("{e}")),
    };
    let iptc_here = iptc::present(snap);
    if iptc_here && iptc::has_extra_records(snap) {
        return FieldPlan::blocked("file contains more than one IPTC record; not written".into());
    }
    let exif = after.exif_datetime();
    let mut ops = Vec::new();
    let mut expect = Vec::new();
    let mut set = |tag: &str, v: String| {
        ops.push(TagOp::Set {
            tag: tag.into(),
            values: vec![v.clone()],
        });
        expect.push(Expect::Equals {
            tag: tag.into(),
            values: vec![v],
        });
    };
    set(DTO, exif.clone());
    if digitized {
        set(CREATE, exif.clone());
    }
    if snap.contains(IFD0_DTO) {
        set(IFD0_DTO, exif.clone());
    }
    for tag in [XMP_DTO, XMP_DATE_CREATED, XMP_CREATE] {
        let Some(v) = snap.text(tag) else { continue };
        let Some(st) = parse_stamp(&v) else {
            return FieldPlan::blocked(format!("{tag}: unrecognised value {v:?}; not written"));
        };
        let mut s = after.local.format("%Y:%m:%d").to_string();
        if let Some((_, frac, off)) = &st.time {
            s.push(' ');
            s.push_str(&after.local.format("%H:%M:%S").to_string());
            if keep_subsec && let Some(f) = frac {
                s.push('.');
                s.push_str(f);
            }
            if let Some(o) = off {
                s.push_str(o);
            }
        }
        set(tag, s);
    }
    if iptc_here {
        if snap.contains(IPTC_DATE) {
            set(IPTC_DATE, after.local.format("%Y:%m:%d").to_string());
        }
        if let Some(t) = snap.text(IPTC_TIME) {
            let Some((_, off)) = parse_iptc_time(&t) else {
                // ExifTool would fill in the computer's time zone: never guess an offset
                return FieldPlan::blocked(format!(
                    "{IPTC_TIME}: unrecognised value {t:?}; not written"
                ));
            };
            set(
                IPTC_TIME,
                format!("{}{off}", after.local.format("%H:%M:%S")),
            );
        }
    }
    let mut notes = Vec::new();
    if !keep_subsec {
        for tag in [SUBSEC_DTO, SUBSEC_CREATE] {
            if (tag == SUBSEC_DTO || digitized) && snap.contains(tag) {
                ops.push(TagOp::Delete { tag: tag.into() });
                expect.push(Expect::Absent { tag: tag.into() });
            }
        }
    }
    // only what actually changes
    let changes = |o: &TagOp| match o {
        TagOp::Set { tag, values } => {
            snap.text(tag).as_deref() != values.first().map(String::as_str)
        }
        TagOp::Delete { tag } => snap.contains(tag),
        TagOp::UpdateIptcDigest => true,
    };
    let keep: Vec<bool> = ops.iter().map(changes).collect();
    let mut k = keep.iter();
    ops.retain(|_| *k.next().unwrap_or(&true));
    let mut k = keep.iter();
    expect.retain(|_| *k.next().unwrap_or(&true));
    if ops.is_empty() {
        return FieldPlan {
            status: EntryStatus::NoChange,
            change: None,
            ops,
            expect,
            notes,
        };
    }
    if ops.iter().any(|o| o.tag().starts_with("IPTC:")) && snap.contains(iptc::DIGEST) {
        ops.push(TagOp::UpdateIptcDigest);
        expect.push(Expect::IptcDigestCurrent);
    }
    if let Some(b) = &before {
        // a location that disagreed with the capture time is overwritten too: say so
        for tag in [
            XMP_DTO,
            XMP_DATE_CREATED,
            XMP_CREATE,
            IPTC_DATE,
            IFD0_DTO,
            CREATE,
        ] {
            let Some(v) = snap.text(tag) else { continue };
            let agrees = parse_stamp(&v).is_some_and(|st| {
                st.date == b.local.date()
                    && st
                        .time
                        .as_ref()
                        .is_none_or(|(hms, _, _)| *hms == b.local.format("%H:%M:%S").to_string())
            });
            let written = ops.iter().any(|o| o.tag() == tag);
            if !agrees && written {
                notes.push(format!(
                    "{tag} held {v:?}, which differed from the capture time; it is set to the new time too"
                ));
            }
        }
    }
    let located: Vec<String> = [XMP_DTO, XMP_DATE_CREATED, XMP_CREATE, IPTC_DATE, IFD0_DTO]
        .into_iter()
        .filter(|t| snap.contains(t))
        .map(str::to_owned)
        .collect();
    if !located.is_empty() {
        notes.push(format!(
            "also updated where present: {}",
            located.join(", ")
        ));
    }
    let offsets: Vec<String> = offsets_in(snap);
    notes.extend(offset_note(&offsets));
    FieldPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.as_ref().map(|b| vec![display(b)]),
            after: Some(vec![display(after)]),
            kind: if before.is_some() {
                ChangeKind::Modify
            } else {
                ChangeKind::Add
            },
        }),
        ops,
        expect,
        notes,
    }
}

/// Effective capture time for a target: for a RAW, the sidecar's XMP time wins over the RAW's own
/// EXIF time (which a sidecar cannot change).
pub fn read_target(t: &Target) -> Result<Option<CaptureTime>, String> {
    match t {
        Target::Embedded(s) => read(s),
        Target::Sidecar { raw, sidecar } => {
            if let Some(sc) = sidecar {
                for tag in [XMP_DTO, XMP_DATE_CREATED] {
                    if let Some(v) = sc.text(tag) {
                        match parse_stamp(&v) {
                            Some(st) if st.time.is_some() => return Ok(from_stamp(&st)),
                            Some(_) => {}
                            None => return Err(format!("sidecar {tag}: unrecognised value {v:?}")),
                        }
                    }
                }
            }
            read(raw)
        }
    }
}

pub fn plan_target(
    t: &Target,
    after: &Result<CaptureTime, TimeOpError>,
    keep_subsec: bool,
    digitized: bool,
) -> FieldPlan {
    match t {
        Target::Embedded(s) => plan(s, after, keep_subsec, digitized),
        Target::Sidecar { raw, sidecar } => plan_sidecar(raw, *sidecar, after, digitized),
    }
}

/// Sidecar mode (METADATA_MODEL §5.3 Sidecar row): `XMP-exif:DateTimeOriginal`,
/// `XMP-photoshop:DateCreated` and, with "digitized", `XMP-xmp:CreateDate`, each with the new
/// time in full (fraction and offset as the tool's result has them). The exact tag set is
/// provisional until the third-party checks of V-07.
fn plan_sidecar(
    raw: &Snapshot,
    sidecar: Option<&Snapshot>,
    after: &Result<CaptureTime, TimeOpError>,
    digitized: bool,
) -> FieldPlan {
    let before = read_target(&Target::Sidecar { raw, sidecar })
        .ok()
        .flatten();
    let after = match after {
        Ok(a) => a,
        Err(TimeOpError::NoValidSourceTime) => {
            return FieldPlan::blocked(
                "no valid capture time to start from; only Absolute can be applied".into(),
            );
        }
        Err(TimeOpError::OutOfRange) => {
            return FieldPlan::blocked("the result is outside years 0001–9999".into());
        }
        Err(e) => return FieldPlan::blocked(format!("{e}")),
    };
    let value = display(after);
    let mut ops = Vec::new();
    let mut expect = Vec::new();
    let mut tags = vec![XMP_DTO, XMP_DATE_CREATED];
    if digitized {
        tags.push(XMP_CREATE);
    }
    for tag in tags {
        if sidecar.and_then(|s| s.text(tag)).as_deref() == Some(value.as_str()) {
            continue;
        }
        ops.push(TagOp::Set {
            tag: tag.into(),
            values: vec![value.clone()],
        });
        expect.push(Expect::Equals {
            tag: tag.into(),
            values: vec![value.clone()],
        });
    }
    if ops.is_empty() {
        return FieldPlan {
            status: EntryStatus::NoChange,
            change: None,
            ops,
            expect,
            notes: vec![],
        };
    }
    FieldPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.as_ref().map(|b| vec![display(b)]),
            after: Some(vec![value]),
            kind: if before.is_some() {
                ChangeKind::Modify
            } else {
                ChangeKind::Add
            },
        }),
        ops,
        expect,
        notes: std::iter::once(
            "written to the XMP sidecar; the RAW file keeps its own EXIF time".to_owned(),
        )
        .chain(offset_note(
            &after.exif_offset().into_iter().collect::<Vec<_>>(),
        ))
        .collect(),
    }
}

/// Every distinct UTC offset the file's capture-time locations carry, in reading order.
fn offsets_in(snap: &Snapshot) -> Vec<String> {
    let stamps = [XMP_DTO, XMP_DATE_CREATED, XMP_CREATE]
        .into_iter()
        .filter_map(|t| snap.text(t))
        .filter_map(|v| parse_stamp(&v)?.time?.2);
    let iptc_offset = snap
        .text(IPTC_TIME)
        .and_then(|t| parse_iptc_time(&t))
        .map(|(_, o)| o);
    let mut offsets: Vec<String> = Vec::new();
    for o in snap
        .text(OFFSET_DTO)
        .into_iter()
        .chain(stamps)
        .chain(iptc_offset)
    {
        if !offsets.contains(&o) {
            offsets.push(o);
        }
    }
    offsets
}

/// D-18: no MVP time tool changes an offset. A file that has one keeps it (a camera set to the
/// wrong time zone needs the v1.3 correction), and the Preview says so.
fn offset_note(offsets: &[String]) -> Option<String> {
    (!offsets.is_empty()).then(|| {
        format!(
            "UTC offset not changed ({}): only the clock time is set; correcting a time zone is not part of this version",
            offsets.join(", ")
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::time::ymd_hms;
    use serde_json::json;

    fn snap(v: serde_json::Value) -> Snapshot {
        Snapshot::from_json(&v)
    }

    fn value<'a>(p: &'a FieldPlan, tag: &str) -> Option<&'a str> {
        p.ops.iter().find_map(|o| match o {
            TagOp::Set { tag: t, values } if t == tag => values.first().map(String::as_str),
            _ => None,
        })
    }

    fn at(local: NaiveDateTime, subsec: Option<&str>, off: Option<&str>) -> CaptureTime {
        CaptureTime {
            local,
            subsec: subsec.and_then(SubSec::parse),
            offset: off.and_then(parse_offset),
        }
    }

    #[test]
    fn read_follows_the_priority_and_rejects_invalid_values() {
        let s = snap(json!({"ExifIFD:DateTimeOriginal": "2020:01:02 03:04:05",
            "ExifIFD:SubSecTimeOriginal": "07", "ExifIFD:OffsetTimeOriginal": "+09:00",
            "XMP-exif:DateTimeOriginal": "1999:01:01 00:00:00"}));
        let t = read(&s).unwrap().unwrap();
        assert_eq!(display(&t), "2020:01:02 03:04:05.07+09:00");
        // XMP date-only values carry no time; IPTC date + time do
        let s = snap(json!({"XMP-photoshop:DateCreated": "2004:02:26",
            "IPTC:DateCreated": "2002:06:20", "IPTC:TimeCreated": "10:11:12+02:00"}));
        assert_eq!(
            display(&read(&s).unwrap().unwrap()),
            "2002:06:20 10:11:12+02:00"
        );
        assert!(
            read(&snap(
                json!({"ExifIFD:DateTimeOriginal": "0000:00:00 00:00:00"})
            ))
            .is_err()
        );
        assert_eq!(read(&snap(json!({}))), Ok(None));
    }

    #[test]
    fn every_location_keeps_its_shape_and_offset() {
        let s = snap(json!({
            "ExifIFD:DateTimeOriginal": "2020:01:02 03:04:05", "ExifIFD:SubSecTimeOriginal": "07",
            "ExifIFD:CreateDate": "2020:01:02 03:04:05",
            "XMP-photoshop:DateCreated": "2004:02:26",
            "XMP-exif:DateTimeOriginal": "2020:01:02 03:04:05.07+09:00",
            "IPTC:DateCreated": "2020:01:02", "IPTC:TimeCreated": "03:04:05+08:00",
            "Photoshop:IPTCDigest": "x"}));
        let after = at(ymd_hms(2020, 1, 3, 4, 4, 5), Some("07"), None);
        let p = plan(&s, &Ok(after), true, true);
        assert_eq!(p.status, EntryStatus::Ready);
        assert_eq!(value(&p, DTO), Some("2020:01:03 04:04:05"));
        assert_eq!(value(&p, CREATE), Some("2020:01:03 04:04:05"));
        assert_eq!(value(&p, XMP_DATE_CREATED), Some("2020:01:03"));
        assert_eq!(value(&p, XMP_DTO), Some("2020:01:03 04:04:05.07+09:00"));
        assert_eq!(value(&p, IPTC_DATE), Some("2020:01:03"));
        assert_eq!(value(&p, IPTC_TIME), Some("04:04:05+08:00"));
        assert!(p.ops.contains(&TagOp::UpdateIptcDigest));
        // the date-only XMP value (2004) disagreed with the capture time: the Preview says so
        assert!(
            p.notes
                .iter()
                .any(|n| n.contains(XMP_DATE_CREATED) && n.contains("differed")),
            "{:?}",
            p.notes
        );
        assert!(
            !p.notes
                .iter()
                .any(|n| n.contains(XMP_DTO) && n.contains("differed"))
        );
        assert!(!p.ops.iter().any(|o| o.tag() == SUBSEC_DTO)); // kept by Shift
        assert!(!p.ops.iter().any(|o| o.tag() == OFFSET_DTO)); // offsets never change
        // D-18: the Preview says the offsets stay (each distinct offset once)
        assert!(
            p.notes
                .iter()
                .any(|n| n.starts_with("UTC offset not changed (+09:00, +08:00)")),
            "{:?}",
            p.notes
        );
    }

    #[test]
    fn absolute_drops_fractions_and_nothing_is_created_that_did_not_exist() {
        let s = snap(json!({"ExifIFD:DateTimeOriginal": "2020:01:02 03:04:05",
            "ExifIFD:SubSecTimeOriginal": "07", "ExifIFD:SubSecTimeDigitized": "07",
            "XMP-exif:DateTimeOriginal": "2020:01:02 03:04:05.07"}));
        let p = plan(
            &s,
            &Ok(at(ymd_hms(2021, 5, 6, 7, 8, 9), None, None)),
            false,
            true,
        );
        assert!(p.ops.contains(&TagOp::Delete {
            tag: SUBSEC_DTO.into()
        }));
        assert!(p.ops.contains(&TagOp::Delete {
            tag: SUBSEC_CREATE.into()
        }));
        assert_eq!(value(&p, XMP_DTO), Some("2021:05:06 07:08:09"));
        assert!(!p.ops.iter().any(|o| o.tag().starts_with("IPTC")));
        assert!(!p.ops.iter().any(|o| o.tag() == XMP_CREATE));
        // without "digitized", CreateDate and its fraction are left alone
        let q = plan(
            &s,
            &Ok(at(ymd_hms(2021, 5, 6, 7, 8, 9), None, None)),
            false,
            false,
        );
        assert!(
            !q.ops
                .iter()
                .any(|o| o.tag() == CREATE || o.tag() == SUBSEC_CREATE)
        );
    }

    #[test]
    fn sidecar_mode_reads_the_sidecar_first_and_writes_xmp_only() {
        let raw = snap(json!({"ExifIFD:DateTimeOriginal": "2004:06:09 16:02:35",
            "ExifIFD:OffsetTimeOriginal": "+09:00"}));
        let t = Target::Sidecar {
            raw: &raw,
            sidecar: None,
        };
        assert_eq!(
            display(&read_target(&t).unwrap().unwrap()),
            "2004:06:09 16:02:35+09:00"
        );
        let after = at(ymd_hms(2004, 6, 9, 17, 2, 35), None, Some("+09:00"));
        let p = plan_target(&t, &Ok(after), true, true);
        assert_eq!(
            p.ops.iter().map(|o| o.tag()).collect::<Vec<_>>(),
            vec![XMP_DTO, XMP_DATE_CREATED, XMP_CREATE]
        );
        assert_eq!(value(&p, XMP_DTO), Some("2004:06:09 17:02:35+09:00"));
        // once written, the sidecar is what is read, and the same time is no change
        let side = snap(
            json!({"XMP-exif:DateTimeOriginal": "2004:06:09 17:02:35+09:00",
            "XMP-photoshop:DateCreated": "2004:06:09 17:02:35+09:00",
            "XMP-xmp:CreateDate": "2004:06:09 17:02:35+09:00"}),
        );
        let t2 = Target::Sidecar {
            raw: &raw,
            sidecar: Some(&side),
        };
        let now = read_target(&t2).unwrap().unwrap();
        assert_eq!(display(&now), "2004:06:09 17:02:35+09:00");
        assert_eq!(
            plan_target(&t2, &Ok(now), true, true).status,
            EntryStatus::NoChange
        );
    }

    #[test]
    fn unchanged_blocked_and_unrecognised_values() {
        let s = snap(json!({"ExifIFD:DateTimeOriginal": "2020:01:02 03:04:05",
            "ExifIFD:CreateDate": "2020:01:02 03:04:05"}));
        let same = at(ymd_hms(2020, 1, 2, 3, 4, 5), None, None);
        assert_eq!(
            plan(&s, &Ok(same.clone()), true, true).status,
            EntryStatus::NoChange
        );
        assert!(matches!(
            plan(&s, &Err(TimeOpError::NoValidSourceTime), true, true).status,
            EntryStatus::Blocked(_)
        ));
        let bad = snap(json!({"ExifIFD:DateTimeOriginal": "2020:01:02 03:04:05",
            "IPTC:DateCreated": "2020:01:02", "IPTC:TimeCreated": "03:04:05"}));
        assert!(matches!(
            plan(
                &bad,
                &Ok(at(ymd_hms(2020, 1, 3, 3, 4, 5), None, None)),
                true,
                true
            )
            .status,
            EntryStatus::Blocked(_)
        ));
    }
}
