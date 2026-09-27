//! Capture-time model and the four MVP time tools (METADATA_MODEL §5; v0.1 §33):
//! Absolute, Shift, Sequence, Preserve Relative Timing.
//!
//! Pure functions: no IO. Values are the camera's wall-clock time plus the original sub-second
//! digits and an optional UTC offset; the offset is never guessed.

use std::cmp::Ordering;
use std::fmt;

use chrono::{Datelike, FixedOffset, NaiveDate, NaiveDateTime, TimeDelta};

/// Sub-second digits exactly as stored (e.g. "07", "670").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubSec(String);

impl SubSec {
    /// Accepts 1–9 ASCII digits; surrounding spaces (camera padding) are ignored.
    pub fn parse(s: &str) -> Option<SubSec> {
        let t = s.trim_matches(' ');
        (!t.is_empty() && t.len() <= 9 && t.bytes().all(|b| b.is_ascii_digit()))
            .then(|| SubSec(t.to_owned()))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
    fn as_nanos(&self) -> u32 {
        let mut s = self.0.clone();
        while s.len() < 9 {
            s.push('0');
        }
        s.parse().unwrap_or(0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CaptureTime {
    pub local: NaiveDateTime,
    pub subsec: Option<SubSec>,
    pub offset: Option<FixedOffset>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Not `YYYY:MM:DD HH:MM:SS`, or an impossible date such as `0000:00:00 00:00:00`.
    InvalidDateTime(String),
    InvalidOffset(String),
}

/// EXIF can store years 0001–9999 only.
fn in_exif_range(t: &NaiveDateTime) -> bool {
    (1..=9999).contains(&t.year())
}

impl CaptureTime {
    /// Parse EXIF `DateTimeOriginal` (+ optional `SubSecTimeOriginal`, `OffsetTimeOriginal`).
    /// An unparsable sub-second value is dropped; an unparsable offset is an error (never guessed).
    pub fn from_exif(
        datetime: &str,
        subsec: Option<&str>,
        offset: Option<&str>,
    ) -> Result<CaptureTime, ParseError> {
        let dt = datetime.trim_end_matches('\0');
        let local = (dt.len() == 19)
            .then(|| NaiveDateTime::parse_from_str(dt, "%Y:%m:%d %H:%M:%S").ok())
            .flatten()
            .filter(in_exif_range)
            .ok_or_else(|| ParseError::InvalidDateTime(datetime.to_owned()))?;
        let offset = match offset.map(str::trim).filter(|s| !s.is_empty()) {
            None => None,
            Some(o) => {
                Some(parse_offset(o).ok_or_else(|| ParseError::InvalidOffset(o.to_owned()))?)
            }
        };
        Ok(CaptureTime {
            local,
            subsec: subsec.and_then(SubSec::parse),
            offset,
        })
    }

    /// `YYYY:MM:DD HH:MM:SS`
    pub fn exif_datetime(&self) -> String {
        self.local.format("%Y:%m:%d %H:%M:%S").to_string()
    }

    /// `±HH:MM`
    pub fn exif_offset(&self) -> Option<String> {
        self.offset.map(format_offset)
    }

    /// ISO 8601 for XMP: `YYYY-MM-DDTHH:MM:SS[.digits][±HH:MM]`
    pub fn xmp(&self) -> String {
        let mut s = self.local.format("%Y-%m-%dT%H:%M:%S").to_string();
        if let Some(ss) = &self.subsec {
            s.push('.');
            s.push_str(ss.as_str());
        }
        if let Some(o) = self.offset {
            s.push_str(&format_offset(o));
        }
        s
    }

    fn sort_key(&self) -> (NaiveDateTime, u32) {
        (
            self.local,
            self.subsec.as_ref().map(SubSec::as_nanos).unwrap_or(0),
        )
    }
}

/// EXIF offsets range from -12:00 to +14:00.
pub fn parse_offset(s: &str) -> Option<FixedOffset> {
    let b = s.as_bytes();
    if b.len() != 6 || !matches!(b[0], b'+' | b'-') || b[3] != b':' {
        return None;
    }
    let h: i32 = s[1..3].parse().ok()?;
    let m: i32 = s[4..6].parse().ok()?;
    if m >= 60 {
        return None;
    }
    let secs = (h * 3600 + m * 60) * if b[0] == b'-' { -1 } else { 1 };
    if !(-12 * 3600..=14 * 3600).contains(&secs) {
        return None;
    }
    FixedOffset::east_opt(secs)
}

fn format_offset(o: FixedOffset) -> String {
    let secs = o.local_minus_utc();
    let sign = if secs < 0 { '-' } else { '+' };
    let a = secs.abs();
    format!("{sign}{:02}:{:02}", a / 3600, (a % 3600) / 60)
}

/// One file as seen by the time tools. `time` is `None` when the stored value is absent or
/// invalid; such files only accept Absolute (METADATA_MODEL §5.1).
#[derive(Debug, Clone)]
pub struct TimeItem {
    pub id: u64,
    pub file_name: String,
    pub time: Option<CaptureTime>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SequenceOrder {
    /// Original capture time (incl. sub-seconds), ties broken by natural file-name order.
    CaptureTimeThenName,
    /// Natural file-name order ("IMG_2" before "IMG_10").
    NaturalFileName,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeOp {
    Absolute(NaiveDateTime),
    Shift(TimeDelta),
    Sequence {
        start: NaiveDateTime,
        step: TimeDelta,
        order: SequenceOrder,
    },
    PreserveRelative {
        anchor: u64,
        new_local: NaiveDateTime,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimeOpError {
    EmptySelection,
    /// The file has no valid capture time; only Absolute can be applied.
    NoValidSourceTime,
    /// The result would fall outside what EXIF can store (years 0001–9999).
    OutOfRange,
    AnchorNotInSelection,
    AnchorHasNoValidTime,
    /// Ordering by capture time needs a valid time on every file (ids listed).
    OrderNeedsValidTimes(Vec<u64>),
    NegativeStep,
}

impl fmt::Display for TimeOpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TimeOpError {}

/// Result for one file. `index` is the 0-based position in the Sequence order (Preview shows it);
/// for the other operations it is the position in the input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TimeResult {
    pub id: u64,
    pub index: usize,
    pub after: Result<CaptureTime, TimeOpError>,
}

fn checked_add(t: NaiveDateTime, d: TimeDelta) -> Result<NaiveDateTime, TimeOpError> {
    t.checked_add_signed(d)
        .filter(in_exif_range)
        .ok_or(TimeOpError::OutOfRange)
}

fn shift(t: &CaptureTime, d: TimeDelta) -> Result<CaptureTime, TimeOpError> {
    Ok(CaptureTime {
        local: checked_add(t.local, d)?,
        subsec: t.subsec.clone(),
        offset: t.offset,
    })
}

/// Apply a time operation to a selection. Whole-operation errors (empty selection, missing
/// anchor, unorderable sequence) are returned as `Err`; per-file problems are in each result.
pub fn apply(op: &TimeOp, items: &[TimeItem]) -> Result<Vec<TimeResult>, TimeOpError> {
    if items.is_empty() {
        return Err(TimeOpError::EmptySelection);
    }
    let per_item = |f: &dyn Fn(&TimeItem) -> Result<CaptureTime, TimeOpError>| -> Vec<TimeResult> {
        items
            .iter()
            .enumerate()
            .map(|(i, it)| TimeResult {
                id: it.id,
                index: i,
                after: f(it),
            })
            .collect()
    };
    match op {
        TimeOp::Absolute(l) => {
            if !in_exif_range(l) {
                return Err(TimeOpError::OutOfRange);
            }
            // sub-seconds are removed so no stale fraction survives; a known offset is kept
            Ok(per_item(&|it| {
                Ok(CaptureTime {
                    local: *l,
                    subsec: None,
                    offset: it.time.as_ref().and_then(|t| t.offset),
                })
            }))
        }
        TimeOp::Shift(d) => Ok(per_item(&|it| {
            it.time
                .as_ref()
                .ok_or(TimeOpError::NoValidSourceTime)
                .and_then(|t| shift(t, *d))
        })),
        TimeOp::PreserveRelative { anchor, new_local } => {
            let a = items
                .iter()
                .find(|it| it.id == *anchor)
                .ok_or(TimeOpError::AnchorNotInSelection)?;
            let at = a.time.as_ref().ok_or(TimeOpError::AnchorHasNoValidTime)?;
            let d = *new_local - at.local;
            Ok(per_item(&|it| {
                it.time
                    .as_ref()
                    .ok_or(TimeOpError::NoValidSourceTime)
                    .and_then(|t| shift(t, d))
            }))
        }
        TimeOp::Sequence { start, step, order } => {
            if *step < TimeDelta::zero() {
                return Err(TimeOpError::NegativeStep);
            }
            let mut sorted: Vec<&TimeItem> = items.iter().collect();
            match order {
                SequenceOrder::CaptureTimeThenName => {
                    let missing: Vec<u64> = items
                        .iter()
                        .filter(|it| it.time.is_none())
                        .map(|it| it.id)
                        .collect();
                    if !missing.is_empty() {
                        return Err(TimeOpError::OrderNeedsValidTimes(missing));
                    }
                    sorted.sort_by(|a, b| {
                        let (ta, tb) = (
                            a.time.as_ref().unwrap().sort_key(),
                            b.time.as_ref().unwrap().sort_key(),
                        );
                        ta.cmp(&tb)
                            .then_with(|| natural_cmp(&a.file_name, &b.file_name))
                            .then(a.id.cmp(&b.id))
                    });
                }
                SequenceOrder::NaturalFileName => sorted
                    .sort_by(|a, b| natural_cmp(&a.file_name, &b.file_name).then(a.id.cmp(&b.id))),
            }
            Ok(sorted
                .into_iter()
                .enumerate()
                .map(|(i, it)| {
                    let after = i32::try_from(i)
                        .ok()
                        .and_then(|n| step.checked_mul(n))
                        .ok_or(TimeOpError::OutOfRange)
                        .and_then(|d| checked_add(*start, d))
                        .map(|local| CaptureTime {
                            local,
                            subsec: None,
                            offset: it.time.as_ref().and_then(|t| t.offset),
                        });
                    TimeResult {
                        id: it.id,
                        index: i,
                        after,
                    }
                })
                .collect())
        }
    }
}

/// Natural order: digit runs compare by value ("2" < "10"), other runs case-insensitively.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    fn runs(s: &str) -> Vec<(bool, String)> {
        let mut out: Vec<(bool, String)> = Vec::new();
        for c in s.chars() {
            let d = c.is_ascii_digit();
            match out.last_mut() {
                Some((is_d, run)) if *is_d == d => run.push(c),
                _ => out.push((d, c.to_string())),
            }
        }
        out
    }
    let (ra, rb) = (runs(a), runs(b));
    // leading zeros ("01" vs "1") only break ties once everything else is equal
    let mut zeros = Ordering::Equal;
    for ((da, sa), (db, sb)) in ra.iter().zip(rb.iter()) {
        let o = match (da, db) {
            (true, true) => {
                let (ta, tb) = (sa.trim_start_matches('0'), sb.trim_start_matches('0'));
                if zeros == Ordering::Equal {
                    zeros = sa.len().cmp(&sb.len());
                }
                ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb))
            }
            _ => sa.to_lowercase().cmp(&sb.to_lowercase()),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    ra.len().cmp(&rb.len()).then(zeros).then_with(|| a.cmp(b))
}

/// Convenience for tests and callers: build a NaiveDateTime from components.
pub fn ymd_hms(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
    NaiveDate::from_ymd_opt(y, mo, d)
        .and_then(|x| x.and_hms_opt(h, mi, s))
        .expect("valid date")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: u64, name: &str, exif: Option<&str>) -> TimeItem {
        TimeItem {
            id,
            file_name: name.into(),
            time: exif.map(|e| CaptureTime::from_exif(e, None, Some("+02:00")).unwrap()),
        }
    }

    fn locals(r: &[TimeResult]) -> Vec<String> {
        r.iter()
            .map(|x| {
                x.after
                    .as_ref()
                    .map(|t| t.exif_datetime())
                    .unwrap_or_else(|e| format!("{e:?}"))
            })
            .collect()
    }

    #[test]
    fn parse_and_format_round_trip() {
        let t = CaptureTime::from_exif("2023:06:02 18:53:25", Some("67"), Some("+02:00")).unwrap();
        assert_eq!(t.exif_datetime(), "2023:06:02 18:53:25");
        assert_eq!(t.exif_offset().as_deref(), Some("+02:00"));
        assert_eq!(t.xmp(), "2023-06-02T18:53:25.67+02:00");
        let pad =
            CaptureTime::from_exif("2023:06:02 18:53:25", Some("09  "), Some("-05:30")).unwrap();
        assert_eq!(pad.xmp(), "2023-06-02T18:53:25.09-05:30");
    }

    #[test]
    fn invalid_values_are_rejected_not_guessed() {
        for bad in [
            "0000:00:00 00:00:00",
            "2023:02:30 10:00:00",
            "2023-06-02 18:53:25",
            "",
            "    :  :     :  :  ",
        ] {
            assert!(CaptureTime::from_exif(bad, None, None).is_err(), "{bad:?}");
        }
        assert!(matches!(
            CaptureTime::from_exif("2023:06:02 18:53:25", None, Some("+15:00")),
            Err(ParseError::InvalidOffset(_))
        ));
        assert!(
            CaptureTime::from_exif("2023:06:02 18:53:25", Some("x"), None)
                .unwrap()
                .subsec
                .is_none()
        );
    }

    #[test]
    fn absolute_drops_subsec_keeps_offset_and_accepts_invalid_sources() {
        let items = vec![
            TimeItem {
                id: 1,
                file_name: "a".into(),
                time: Some(
                    CaptureTime::from_exif("2023:06:02 18:53:25", Some("67"), Some("+02:00"))
                        .unwrap(),
                ),
            },
            TimeItem {
                id: 2,
                file_name: "b".into(),
                time: None,
            },
        ];
        let r = apply(&TimeOp::Absolute(ymd_hms(2026, 9, 4, 12, 27, 0)), &items).unwrap();
        let a = r[0].after.as_ref().unwrap();
        assert_eq!(
            (
                a.exif_datetime().as_str(),
                a.subsec.is_none(),
                a.exif_offset().as_deref()
            ),
            ("2026:09:04 12:27:00", true, Some("+02:00"))
        );
        assert_eq!(r[1].after.as_ref().unwrap().offset, None);
    }

    #[test]
    fn shift_crosses_day_year_and_leap_day() {
        let items = vec![
            item(1, "a", Some("2024:02:28 23:30:00")),
            item(2, "b", Some("2025:12:31 23:59:59")),
            item(3, "c", None),
        ];
        let r = apply(&TimeOp::Shift(TimeDelta::hours(1)), &items).unwrap();
        assert_eq!(
            locals(&r),
            vec![
                "2024:02:29 00:30:00",
                "2026:01:01 00:59:59",
                "NoValidSourceTime"
            ]
        );
        let back = apply(&TimeOp::Shift(TimeDelta::hours(-24)), &items[..1]).unwrap();
        assert_eq!(locals(&back), vec!["2024:02:27 23:30:00"]);
    }

    #[test]
    fn shift_keeps_subsec_and_offset() {
        let t = CaptureTime::from_exif("2023:06:02 18:53:25", Some("67"), Some("+02:00")).unwrap();
        let r = apply(
            &TimeOp::Shift(TimeDelta::seconds(37)),
            &[TimeItem {
                id: 1,
                file_name: "a".into(),
                time: Some(t),
            }],
        )
        .unwrap();
        assert_eq!(
            r[0].after.as_ref().unwrap().xmp(),
            "2023-06-02T18:54:02.67+02:00"
        );
    }

    #[test]
    fn shift_out_of_exif_range_is_an_error() {
        let r = apply(
            &TimeOp::Shift(TimeDelta::days(1)),
            &[item(1, "a", Some("9999:12:31 12:00:00"))],
        )
        .unwrap();
        assert_eq!(r[0].after, Err(TimeOpError::OutOfRange));
        let r = apply(
            &TimeOp::Shift(TimeDelta::days(-1)),
            &[item(1, "a", Some("0001:01:01 12:00:00"))],
        )
        .unwrap();
        assert_eq!(r[0].after, Err(TimeOpError::OutOfRange));
    }

    #[test]
    fn preserve_relative_matches_v01_example() {
        // v0.1 §12.6: 12:01, 12:04, 12:09 with the first set to 15:00 -> 15:00, 15:03, 15:08
        let items = vec![
            item(1, "a", Some("2026:09:04 12:01:00")),
            item(2, "b", Some("2026:09:04 12:04:00")),
            item(3, "c", Some("2026:09:04 12:09:00")),
        ];
        let r = apply(
            &TimeOp::PreserveRelative {
                anchor: 1,
                new_local: ymd_hms(2026, 9, 4, 15, 0, 0),
            },
            &items,
        )
        .unwrap();
        assert_eq!(
            locals(&r),
            vec![
                "2026:09:04 15:00:00",
                "2026:09:04 15:03:00",
                "2026:09:04 15:08:00"
            ]
        );
        assert_eq!(
            apply(
                &TimeOp::PreserveRelative {
                    anchor: 9,
                    new_local: ymd_hms(2026, 1, 1, 0, 0, 0)
                },
                &items
            ),
            Err(TimeOpError::AnchorNotInSelection)
        );
        let with_bad = vec![
            item(1, "a", None),
            item(2, "b", Some("2026:09:04 12:04:00")),
        ];
        assert_eq!(
            apply(
                &TimeOp::PreserveRelative {
                    anchor: 1,
                    new_local: ymd_hms(2026, 1, 1, 0, 0, 0)
                },
                &with_bad
            ),
            Err(TimeOpError::AnchorHasNoValidTime)
        );
    }

    #[test]
    fn sequence_matches_v01_example_and_orders() {
        // v0.1 §12.3: start 12:27, +2 min -> 12:27, 12:29, 12:31
        let items = vec![
            item(1, "IMG_10.jpg", Some("2026:01:01 10:00:02")),
            item(2, "IMG_2.jpg", Some("2026:01:01 10:00:01")),
            item(3, "img_1.jpg", Some("2026:01:01 10:00:03")),
        ];
        let step = TimeDelta::minutes(2);
        let start = ymd_hms(2026, 9, 4, 12, 27, 0);
        let by_time = apply(
            &TimeOp::Sequence {
                start,
                step,
                order: SequenceOrder::CaptureTimeThenName,
            },
            &items,
        )
        .unwrap();
        assert_eq!(
            by_time.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![2, 1, 3]
        );
        assert_eq!(
            locals(&by_time),
            vec![
                "2026:09:04 12:27:00",
                "2026:09:04 12:29:00",
                "2026:09:04 12:31:00"
            ]
        );
        let by_name = apply(
            &TimeOp::Sequence {
                start,
                step,
                order: SequenceOrder::NaturalFileName,
            },
            &items,
        )
        .unwrap();
        assert_eq!(
            by_name.iter().map(|r| r.id).collect::<Vec<_>>(),
            vec![3, 2, 1]
        );
        assert!(
            by_name
                .iter()
                .all(|r| r.after.as_ref().unwrap().subsec.is_none())
        );
    }

    #[test]
    fn sequence_by_time_refuses_files_without_valid_time() {
        let items = vec![
            item(1, "a", Some("2026:01:01 10:00:00")),
            item(2, "b", None),
        ];
        let op = TimeOp::Sequence {
            start: ymd_hms(2026, 1, 1, 0, 0, 0),
            step: TimeDelta::seconds(1),
            order: SequenceOrder::CaptureTimeThenName,
        };
        assert_eq!(
            apply(&op, &items),
            Err(TimeOpError::OrderNeedsValidTimes(vec![2]))
        );
        let op2 = TimeOp::Sequence {
            start: ymd_hms(2026, 1, 1, 0, 0, 0),
            step: TimeDelta::seconds(1),
            order: SequenceOrder::NaturalFileName,
        };
        assert!(apply(&op2, &items).unwrap().iter().all(|r| r.after.is_ok()));
    }

    #[test]
    fn sequence_ties_use_subsec_then_name() {
        let mk = |id, name: &str, ss| TimeItem {
            id,
            file_name: name.into(),
            time: Some(CaptureTime::from_exif("2026:01:01 10:00:00", ss, None).unwrap()),
        };
        let items = vec![
            mk(1, "b", Some("50")),
            mk(2, "a", Some("5")),
            mk(3, "c", Some("10")),
        ];
        let r = apply(
            &TimeOp::Sequence {
                start: ymd_hms(2026, 1, 1, 0, 0, 0),
                step: TimeDelta::seconds(1),
                order: SequenceOrder::CaptureTimeThenName,
            },
            &items,
        )
        .unwrap();
        // .10 < .5 == .50 -> tie between "a"(.5) and "b"(.50) broken by name
        assert_eq!(r.iter().map(|x| x.id).collect::<Vec<_>>(), vec![3, 2, 1]);
    }

    #[test]
    fn empty_selection_and_negative_step() {
        assert_eq!(
            apply(&TimeOp::Shift(TimeDelta::hours(1)), &[]),
            Err(TimeOpError::EmptySelection)
        );
        let op = TimeOp::Sequence {
            start: ymd_hms(2026, 1, 1, 0, 0, 0),
            step: TimeDelta::seconds(-1),
            order: SequenceOrder::NaturalFileName,
        };
        assert_eq!(
            apply(&op, &[item(1, "a", None)]),
            Err(TimeOpError::NegativeStep)
        );
    }

    #[test]
    fn natural_order() {
        let mut v = vec![
            "IMG_10.jpg",
            "img_2.JPG",
            "IMG_1.jpg",
            "IMG_01.jpg",
            "DSC.jpg",
            "IMG_1a.jpg",
        ];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(
            v,
            vec![
                "DSC.jpg",
                "IMG_1.jpg",
                "IMG_01.jpg",
                "IMG_1a.jpg",
                "img_2.JPG",
                "IMG_10.jpg"
            ]
        );
    }
}
