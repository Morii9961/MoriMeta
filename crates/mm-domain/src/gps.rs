// SPDX-License-Identifier: GPL-3.0-or-later
//! `gps` field for Embedded JPEG/TIFF targets (METADATA_MODEL §7): read the position, set
//! coordinates, or remove GPS entirely.
//!
//! GPS values are read numerically (`-GPS:all#` and the XMP GPS tags with `#`, requested before
//! `-all`), so they are compared as numbers (SAFETY_MODEL §5 V2): coordinates within 1e-7°
//! (about 1 cm), altitude within 1 mm. Checked with ExifTool 13.59 (2026-09-27): a latitude
//! written as 35.6812345 reads back as 35.6812345000306; XMP GPS takes and returns signed decimal
//! degrees; `-GPS:all=` removes the whole GPS directory; ExifTool adds `GPSVersionID` itself when
//! it creates the GPS directory.
//!
//! Setting coordinates never changes the GPS time stamp; removal deletes every GPS tag, including
//! the time stamp, in the GPS directory and in XMP (not place names, which are Location fields).

use crate::plan::{ChangeKind, EntryStatus, Expect, FieldChange, FieldPlan, TagOp, Target};
use crate::snapshot::Snapshot;

pub const FIELD: &str = "gps";

pub const LAT: &str = "GPS:GPSLatitude";
pub const LAT_REF: &str = "GPS:GPSLatitudeRef";
pub const LON: &str = "GPS:GPSLongitude";
pub const LON_REF: &str = "GPS:GPSLongitudeRef";
pub const ALT: &str = "GPS:GPSAltitude";
pub const ALT_REF: &str = "GPS:GPSAltitudeRef";
pub const XMP_LAT: &str = "XMP-exif:GPSLatitude";
pub const XMP_LON: &str = "XMP-exif:GPSLongitude";
pub const XMP_ALT: &str = "XMP-exif:GPSAltitude";
pub const XMP_ALT_REF: &str = "XMP-exif:GPSAltitudeRef";
/// Tags read numerically for planning and verification (before `-all`).
pub const NUMERIC_READ: &[&str] = &[
    "-GPS:all#",
    "-XMP-exif:GPSLatitude#",
    "-XMP-exif:GPSLongitude#",
    "-XMP-exif:GPSAltitude#",
    "-XMP-exif:GPSAltitudeRef#",
];

const DEG_TOLERANCE: &str = "0.0000001";
const ALT_TOLERANCE: &str = "0.001";

/// WGS84, decimal degrees; altitude in metres (negative = below sea level).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GeoPoint {
    pub lat: f64,
    pub lon: f64,
    pub alt: Option<f64>,
}

impl GeoPoint {
    /// `lat,lon[,alt]` in decimal degrees and metres, or a position in degrees, minutes and
    /// seconds with hemisphere letters as maps and cameras print it (`35°41′22.2″N 139°41′30.1″E`,
    /// `N 35 41.37, E 139 41.5`), range-checked.
    pub fn parse(s: &str) -> Result<GeoPoint, String> {
        if s.chars().any(|c| "NSEWnsew°º'\"′″’”".contains(c)) {
            let p = parse_dms(s)?;
            p.validate()?;
            return Ok(p);
        }
        let parts: Vec<f64> = s
            .split(',')
            .map(|p| p.trim().parse::<f64>())
            .collect::<Result<_, _>>()
            .map_err(|_| format!("{s:?}: use lat,lon[,alt] in decimal degrees and metres"))?;
        let p = match parts.as_slice() {
            [lat, lon] => GeoPoint {
                lat: *lat,
                lon: *lon,
                alt: None,
            },
            [lat, lon, alt] => GeoPoint {
                lat: *lat,
                lon: *lon,
                alt: Some(*alt),
            },
            _ => return Err(format!("{s:?}: use lat,lon[,alt]")),
        };
        p.validate()?;
        Ok(p)
    }

    pub fn validate(&self) -> Result<(), String> {
        if !(-90.0..=90.0).contains(&self.lat) || !self.lat.is_finite() {
            return Err(format!("latitude {} outside -90..90", self.lat));
        }
        if !(-180.0..=180.0).contains(&self.lon) || !self.lon.is_finite() {
            return Err(format!("longitude {} outside -180..180", self.lon));
        }
        if let Some(a) = self.alt
            && (!a.is_finite() || a.abs() > 100_000.0)
        {
            return Err(format!("altitude {a} m is not plausible"));
        }
        Ok(())
    }

    /// `lat, lon[, alt m]` with 7 decimals (about 1 cm).
    pub fn display(&self) -> String {
        match self.alt {
            Some(a) => format!("{:.7}, {:.7}, {a:.2} m", self.lat, self.lon),
            None => format!("{:.7}, {:.7}", self.lat, self.lon),
        }
    }
}

/// Degrees[, minutes[, seconds]] per coordinate, each with its hemisphere letter before or
/// after it; latitude and longitude in either order. Minutes and seconds below 60; a decimal
/// part only on the last number given.
fn parse_dms(s: &str) -> Result<GeoPoint, String> {
    let bad = || {
        format!(
            "{s:?}: use lat,lon[,alt] in decimal degrees, or degrees, minutes and seconds with N/S and E/W"
        )
    };
    enum Tok {
        Num(f64, bool),
        Hemi(char),
    }
    let mut toks = Vec::new();
    let mut num = String::new();
    let flush = |num: &mut String, toks: &mut Vec<Tok>| -> Result<(), String> {
        if !num.is_empty() {
            let v: f64 = num.parse().map_err(|_| bad())?;
            toks.push(Tok::Num(v, num.contains('.')));
            num.clear();
        }
        Ok(())
    };
    for c in s.chars() {
        match c {
            '0'..='9' | '.' => num.push(c),
            'N' | 'S' | 'E' | 'W' | 'n' | 's' | 'e' | 'w' => {
                flush(&mut num, &mut toks)?;
                toks.push(Tok::Hemi(c.to_ascii_uppercase()));
            }
            // any white space: text copied from a map or a web page often has no-break spaces
            c if c.is_whitespace() => flush(&mut num, &mut toks)?,
            ',' | ';' | '°' | 'º' | '\'' | '"' | '′' | '″' | '’' | '”' => {
                flush(&mut num, &mut toks)?
            }
            _ => return Err(bad()),
        }
    }
    flush(&mut num, &mut toks)?;
    // two coordinates, each a hemisphere letter and 1-3 numbers, the letter first or last
    let prefix = matches!(toks.first(), Some(Tok::Hemi(_)));
    let mut coords: Vec<(char, Vec<(f64, bool)>)> = Vec::new();
    let mut nums: Vec<(f64, bool)> = Vec::new();
    for t in toks {
        match t {
            Tok::Num(v, frac) => nums.push((v, frac)),
            Tok::Hemi(h) if prefix => {
                if let Some(last) = coords.last_mut() {
                    last.1 = std::mem::take(&mut nums);
                }
                coords.push((h, Vec::new()));
            }
            Tok::Hemi(h) => coords.push((h, std::mem::take(&mut nums))),
        }
    }
    if prefix && let Some(last) = coords.last_mut() {
        last.1 = std::mem::take(&mut nums);
    }
    if coords.len() != 2 || !nums.is_empty() {
        return Err(bad());
    }
    let value = |(h, parts): &(char, Vec<(f64, bool)>)| -> Result<f64, String> {
        if parts.is_empty() || parts.len() > 3 {
            return Err(bad());
        }
        // only the last number may have a decimal part; minutes and seconds below 60
        if parts[..parts.len() - 1].iter().any(|(_, frac)| *frac)
            || parts[1..].iter().any(|(v, _)| *v >= 60.0)
        {
            return Err(bad());
        }
        let v = parts
            .iter()
            .zip([1.0, 60.0, 3600.0])
            .map(|((v, _), d)| v / d)
            .sum::<f64>();
        Ok(if *h == 'S' || *h == 'W' { -v } else { v })
    };
    let lat_first = matches!(coords[0].0, 'N' | 'S');
    let (la, lo) = if lat_first {
        (&coords[0], &coords[1])
    } else {
        (&coords[1], &coords[0])
    };
    if !matches!(la.0, 'N' | 'S') || !matches!(lo.0, 'E' | 'W') {
        return Err(bad());
    }
    Ok(GeoPoint {
        lat: value(la)?,
        lon: value(lo)?,
        alt: None,
    })
}

#[derive(Debug, Clone, PartialEq)]
pub enum GpsEdit {
    Set(GeoPoint),
    Remove,
}

fn number(snap: &Snapshot, tag: &str) -> Option<f64> {
    snap.text(tag)?.trim().parse().ok()
}

/// Position in the GPS directory (signed by its references), else in XMP.
pub fn read(snap: &Snapshot) -> Option<GeoPoint> {
    let sign = |r: Option<String>, neg: &str| if r.as_deref() == Some(neg) { -1.0 } else { 1.0 };
    if let (Some(lat), Some(lon)) = (number(snap, LAT), number(snap, LON)) {
        let alt = number(snap, ALT).map(|a| {
            // 1 = below sea level
            if snap.text(ALT_REF).as_deref() == Some("1") {
                -a
            } else {
                a
            }
        });
        return Some(GeoPoint {
            lat: lat * sign(snap.text(LAT_REF), "S"),
            lon: lon * sign(snap.text(LON_REF), "W"),
            alt,
        });
    }
    if let (Some(lat), Some(lon)) = (number(snap, XMP_LAT), number(snap, XMP_LON)) {
        let alt = number(snap, XMP_ALT).map(|a| {
            if snap.text(XMP_ALT_REF).as_deref() == Some("1") {
                -a
            } else {
                a
            }
        });
        return Some(GeoPoint { lat, lon, alt });
    }
    None
}

fn near(ops: &mut Vec<TagOp>, expect: &mut Vec<Expect>, tag: &str, value: String, within: &str) {
    ops.push(TagOp::Set {
        tag: tag.into(),
        values: vec![value.clone()],
    });
    expect.push(Expect::Near {
        tag: tag.into(),
        value,
        within: within.into(),
    });
}

fn equals(ops: &mut Vec<TagOp>, expect: &mut Vec<Expect>, tag: &str, value: &str) {
    ops.push(TagOp::Set {
        tag: tag.into(),
        values: vec![value.into()],
    });
    expect.push(Expect::Equals {
        tag: tag.into(),
        values: vec![value.into()],
    });
}

/// The altitude reference is written by name and read back as a number: ExifTool 13.59 turns a
/// written `1` into 0 (above sea level), and a negative altitude written alone loses its sign.
fn altitude_ref(ops: &mut Vec<TagOp>, expect: &mut Vec<Expect>, tag: &str, below: bool) {
    ops.push(TagOp::Set {
        tag: tag.into(),
        values: vec![
            if below {
                "Below Sea Level"
            } else {
                "Above Sea Level"
            }
            .into(),
        ],
    });
    expect.push(Expect::Equals {
        tag: tag.into(),
        values: vec![if below { "1" } else { "0" }.into()],
    });
}

fn same(a: Option<f64>, b: Option<f64>, tol: f64) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => (x - y).abs() <= tol,
        (None, None) => true,
        _ => false,
    }
}

pub fn plan(snap: &Snapshot, edit: &GpsEdit) -> FieldPlan {
    let before = read(snap);
    let gps_keys: Vec<&String> = snap.keys().filter(|k| k.starts_with("GPS:")).collect();
    let xmp_keys: Vec<&String> = snap
        .keys()
        .filter(|k| k.starts_with("XMP-exif:GPS"))
        .collect();
    let mut ops = Vec::new();
    let mut expect = Vec::new();
    let mut notes = Vec::new();
    let after = match edit {
        GpsEdit::Set(p) => {
            if let Err(e) = p.validate() {
                return FieldPlan::blocked(e);
            }
            let unchanged_ifd = same(number(snap, LAT), Some(p.lat.abs()), 1e-7)
                && same(number(snap, LON), Some(p.lon.abs()), 1e-7)
                && snap.text(LAT_REF).as_deref() == Some(if p.lat < 0.0 { "S" } else { "N" })
                && snap.text(LON_REF).as_deref() == Some(if p.lon < 0.0 { "W" } else { "E" })
                && same(number(snap, ALT), p.alt.map(f64::abs), 1e-3);
            let xmp_present = snap.contains(XMP_LAT) || snap.contains(XMP_LON);
            let unchanged_xmp = !xmp_present
                || (same(number(snap, XMP_LAT), Some(p.lat), 1e-7)
                    && same(number(snap, XMP_LON), Some(p.lon), 1e-7));
            if unchanged_ifd && unchanged_xmp {
                return FieldPlan {
                    status: EntryStatus::NoChange,
                    change: None,
                    ops,
                    expect,
                    notes,
                };
            }
            near(
                &mut ops,
                &mut expect,
                LAT,
                format!("{:.7}", p.lat.abs()),
                DEG_TOLERANCE,
            );
            equals(
                &mut ops,
                &mut expect,
                LAT_REF,
                if p.lat < 0.0 { "S" } else { "N" },
            );
            near(
                &mut ops,
                &mut expect,
                LON,
                format!("{:.7}", p.lon.abs()),
                DEG_TOLERANCE,
            );
            equals(
                &mut ops,
                &mut expect,
                LON_REF,
                if p.lon < 0.0 { "W" } else { "E" },
            );
            match p.alt {
                Some(a) => {
                    near(
                        &mut ops,
                        &mut expect,
                        ALT,
                        format!("{:.3}", a.abs()),
                        ALT_TOLERANCE,
                    );
                    altitude_ref(&mut ops, &mut expect, ALT_REF, a < 0.0);
                }
                None => {
                    // an old altitude would describe another place
                    for tag in [ALT, ALT_REF, XMP_ALT, XMP_ALT_REF] {
                        if snap.contains(tag) {
                            ops.push(TagOp::Delete { tag: tag.into() });
                            expect.push(Expect::Absent { tag: tag.into() });
                        }
                    }
                    if snap.contains(ALT) || snap.contains(XMP_ALT) {
                        notes.push("the old altitude is removed: no altitude was given".into());
                    }
                }
            }
            if xmp_present {
                near(
                    &mut ops,
                    &mut expect,
                    XMP_LAT,
                    format!("{:.7}", p.lat),
                    DEG_TOLERANCE,
                );
                near(
                    &mut ops,
                    &mut expect,
                    XMP_LON,
                    format!("{:.7}", p.lon),
                    DEG_TOLERANCE,
                );
                if let Some(a) = p.alt
                    && snap.contains(XMP_ALT)
                {
                    near(
                        &mut ops,
                        &mut expect,
                        XMP_ALT,
                        format!("{:.3}", a.abs()),
                        ALT_TOLERANCE,
                    );
                    altitude_ref(&mut ops, &mut expect, XMP_ALT_REF, a < 0.0);
                }
                notes.push("XMP GPS updated because the file already has it".into());
            }
            if snap.contains("GPS:GPSTimeStamp") {
                notes.push("GPS time stamp kept unchanged".into());
            }
            Some(*p)
        }
        GpsEdit::Remove => {
            if gps_keys.is_empty() && xmp_keys.is_empty() {
                return FieldPlan {
                    status: EntryStatus::NoChange,
                    change: None,
                    ops,
                    expect,
                    notes,
                };
            }
            if !gps_keys.is_empty() {
                ops.push(TagOp::Delete {
                    tag: "GPS:all".into(),
                });
                for k in &gps_keys {
                    expect.push(Expect::Absent { tag: (*k).clone() });
                }
            }
            for k in &xmp_keys {
                ops.push(TagOp::Delete { tag: (*k).clone() });
                expect.push(Expect::Absent { tag: (*k).clone() });
            }
            None
        }
    };
    FieldPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.map(|b| vec![b.display()]),
            after: after.map(|a| vec![a.display()]),
            // removing is a Remove even when the old tags did not form a complete position
            kind: match (before.is_some(), after.is_some()) {
                (_, false) => ChangeKind::Remove,
                (false, true) => ChangeKind::Add,
                (true, true) => ChangeKind::Modify,
            },
        }),
        ops,
        expect,
        notes,
    }
}

/// Effective position for a target: for a RAW, the sidecar's XMP GPS wins over the RAW's own.
pub fn read_target(t: &Target) -> Option<GeoPoint> {
    match t {
        Target::Embedded(s) => read(s),
        Target::Sidecar { raw, sidecar } => {
            let from_sidecar = sidecar.and_then(|s| {
                let (lat, lon) = (number(s, XMP_LAT)?, number(s, XMP_LON)?);
                let alt = number(s, XMP_ALT).map(|a| {
                    if s.text(XMP_ALT_REF).as_deref() == Some("1") {
                        -a
                    } else {
                        a
                    }
                });
                Some(GeoPoint { lat, lon, alt })
            });
            from_sidecar.or_else(|| read(raw))
        }
    }
}

pub fn plan_target(t: &Target, edit: &GpsEdit) -> FieldPlan {
    match t {
        Target::Embedded(s) => plan(s, edit),
        Target::Sidecar { raw, sidecar } => plan_sidecar(raw, *sidecar, edit),
    }
}

/// Sidecar mode (METADATA_MODEL §7): XMP GPS in the sidecar only. GPS inside a RAW cannot be
/// removed, so removal is Unsupported for such a file as a whole (no partial write).
fn plan_sidecar(raw: &Snapshot, sidecar: Option<&Snapshot>, edit: &GpsEdit) -> FieldPlan {
    let before = read_target(&Target::Sidecar { raw, sidecar });
    let empty = Snapshot::default();
    let side = sidecar.unwrap_or(&empty);
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
        GpsEdit::Set(p) => {
            if let Err(e) = p.validate() {
                return FieldPlan::blocked(e);
            }
            let alt_now = number(side, XMP_ALT).map(|a| {
                if side.text(XMP_ALT_REF).as_deref() == Some("1") {
                    -a
                } else {
                    a
                }
            });
            if same(number(side, XMP_LAT), Some(p.lat), 1e-7)
                && same(number(side, XMP_LON), Some(p.lon), 1e-7)
                && same(alt_now, p.alt, 1e-3)
            {
                return unchanged;
            }
            near(
                &mut ops,
                &mut expect,
                XMP_LAT,
                format!("{:.7}", p.lat),
                DEG_TOLERANCE,
            );
            near(
                &mut ops,
                &mut expect,
                XMP_LON,
                format!("{:.7}", p.lon),
                DEG_TOLERANCE,
            );
            match p.alt {
                Some(a) => {
                    near(
                        &mut ops,
                        &mut expect,
                        XMP_ALT,
                        format!("{:.3}", a.abs()),
                        ALT_TOLERANCE,
                    );
                    altitude_ref(&mut ops, &mut expect, XMP_ALT_REF, a < 0.0);
                }
                None => {
                    for tag in [XMP_ALT, XMP_ALT_REF] {
                        if side.contains(tag) {
                            ops.push(TagOp::Delete { tag: tag.into() });
                            expect.push(Expect::Absent { tag: tag.into() });
                        }
                    }
                }
            }
            Some(*p)
        }
        GpsEdit::Remove => {
            if read(raw).is_some() {
                return FieldPlan {
                    status: EntryStatus::Unsupported(
                        "GPS inside a RAW file cannot be removed; nothing is written".into(),
                    ),
                    change: None,
                    ops,
                    expect,
                    notes: vec![],
                };
            }
            let keys: Vec<String> = side
                .keys()
                .filter(|k| k.starts_with("XMP-exif:GPS"))
                .cloned()
                .collect();
            if keys.is_empty() {
                return unchanged;
            }
            for k in keys {
                ops.push(TagOp::Delete { tag: k.clone() });
                expect.push(Expect::Absent { tag: k });
            }
            None
        }
    };
    FieldPlan {
        status: EntryStatus::Ready,
        change: Some(FieldChange {
            field: FIELD.into(),
            before: before.map(|b| vec![b.display()]),
            after: after.map(|a| vec![a.display()]),
            kind: match (before.is_some(), after.is_some()) {
                (_, false) => ChangeKind::Remove,
                (false, true) => ChangeKind::Add,
                (true, true) => ChangeKind::Modify,
            },
        }),
        ops,
        expect,
        notes: vec!["written to the XMP sidecar; the RAW file is not modified".into()],
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
    fn parse_and_validate_input() {
        let p = GeoPoint::parse("35.6812345, 139.7671234, 40.5").unwrap();
        assert_eq!(p.display(), "35.6812345, 139.7671234, 40.50 m");
        assert!(GeoPoint::parse("-33.8688,151.2093").unwrap().alt.is_none());
        for bad in [
            "91,0",
            "0,181",
            "a,b",
            "1",
            "1,2,3,4",
            "0,0,1000000",
            "NaN,0",
        ] {
            assert!(GeoPoint::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn parse_degrees_minutes_seconds() {
        let close = |s: &str, lat: f64, lon: f64| {
            let p = GeoPoint::parse(s).unwrap_or_else(|e| panic!("{s}: {e}"));
            assert!(
                (p.lat - lat).abs() < 1e-6 && (p.lon - lon).abs() < 1e-6,
                "{s}: {p:?}"
            );
            assert!(p.alt.is_none());
        };
        close("35°41′22.2″N 139°41′30.1″E", 35.689_5, 139.691_694_4);
        close("35°41'22.2\"N, 139°41'30.1\"E", 35.689_5, 139.691_694_4);
        close("N 35 41.37, E 139 41.5", 35.6895, 139.691_666_7);
        close("33 51 24.4 S 70 38 53.7 W", -33.856_777_8, -70.648_25);
        close("139°41′30.1″E 35°41′22.2″N", 35.689_5, 139.691_694_4);
        close("51.5°N 0.12°w", 51.5, -0.12);
        for bad in [
            "35°61′N 139°E",
            "35.5°30′N 139°E",
            "35°N 139°N",
            "35°E 139°E",
            "35°41′N",
            "NaN,0",
            "35°41′N 139°41′E 40",
            "91°N 0°E",
            "35°41′22″X 139°E",
        ] {
            assert!(GeoPoint::parse(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn read_signs_by_reference_and_falls_back_to_xmp() {
        let s = snap(
            json!({"GPS:GPSLatitude": 54.9896666666667, "GPS:GPSLatitudeRef": "N",
            "GPS:GPSLongitude": 1.91416666666667, "GPS:GPSLongitudeRef": "W"}),
        );
        let p = read(&s).unwrap();
        assert!((p.lon + 1.9141667).abs() < 1e-6 && p.lat > 54.0);
        let x = snap(json!({"XMP-exif:GPSLatitude": -35.5, "XMP-exif:GPSLongitude": 139.7}));
        assert_eq!(read(&x).unwrap().lat, -35.5);
        assert_eq!(read(&snap(json!({}))), None);
    }

    #[test]
    fn set_writes_numbers_with_references_and_drops_a_stale_altitude() {
        let s = snap(json!({"GPS:GPSLatitude": 1.0, "GPS:GPSLatitudeRef": "N",
            "GPS:GPSLongitude": 2.0, "GPS:GPSLongitudeRef": "E", "GPS:GPSAltitude": 100.0,
            "GPS:GPSAltitudeRef": "0", "GPS:GPSTimeStamp": "14:58:24"}));
        let p = plan(
            &s,
            &GpsEdit::Set(GeoPoint::parse("-33.8688,151.2093").unwrap()),
        );
        assert_eq!(p.status, EntryStatus::Ready);
        assert_eq!(tags(&p), vec![LAT, LAT_REF, LON, LON_REF, ALT, ALT_REF]);
        assert!(p.expect.contains(&Expect::Equals {
            tag: LAT_REF.into(),
            values: vec!["S".into()]
        }));
        assert!(p.ops.contains(&TagOp::Delete { tag: ALT.into() }));
        assert!(!tags(&p).contains(&"GPS:GPSTimeStamp"));
        assert!(p.notes.iter().any(|n| n.contains("altitude")));
    }

    #[test]
    fn remove_deletes_every_gps_tag_and_xmp_gps_and_nothing_else() {
        let s = snap(
            json!({"GPS:GPSLatitude": 1.0, "GPS:GPSTimeStamp": "10:00:00",
            "XMP-exif:GPSLatitude": 1.0, "XMP-photoshop:City": "Tokyo"}),
        );
        let p = plan(&s, &GpsEdit::Remove);
        assert_eq!(tags(&p), vec!["GPS:all", XMP_LAT]);
        assert!(p.expect.contains(&Expect::Absent {
            tag: "GPS:GPSTimeStamp".into()
        }));
        assert_eq!(p.change.unwrap().kind, ChangeKind::Remove);
        assert_eq!(
            plan(&snap(json!({"IFD0:Make": "X"})), &GpsEdit::Remove).status,
            EntryStatus::NoChange
        );
    }

    #[test]
    fn sidecar_mode_sets_xmp_and_cannot_remove_raw_gps() {
        let raw = snap(json!({"GPS:GPSLatitude": 1.0, "GPS:GPSLatitudeRef": "N",
            "GPS:GPSLongitude": 2.0, "GPS:GPSLongitudeRef": "E"}));
        let t = Target::Sidecar {
            raw: &raw,
            sidecar: None,
        };
        let p = plan_target(
            &t,
            &GpsEdit::Set(GeoPoint::parse("-10.5,20.25,-3").unwrap()),
        );
        assert_eq!(tags(&p), vec![XMP_LAT, XMP_LON, XMP_ALT, XMP_ALT_REF]);
        assert!(matches!(
            plan_target(&t, &GpsEdit::Remove).status,
            EntryStatus::Unsupported(_)
        ));
        // a sidecar on its own (no RAW GPS) may lose its XMP GPS
        let empty = Snapshot::default();
        let side = snap(json!({"XMP-exif:GPSLatitude": 1.0, "XMP-exif:GPSLongitude": 2.0}));
        let t2 = Target::Sidecar {
            raw: &empty,
            sidecar: Some(&side),
        };
        assert_eq!(read_target(&t2).unwrap().lat, 1.0);
        assert_eq!(
            tags(&plan_target(&t2, &GpsEdit::Remove)),
            vec![XMP_LAT, XMP_LON]
        );
    }

    #[test]
    fn same_position_is_no_change() {
        let s = snap(
            json!({"GPS:GPSLatitude": 35.6812345000306, "GPS:GPSLatitudeRef": "N",
            "GPS:GPSLongitude": 139.767123400003, "GPS:GPSLongitudeRef": "E"}),
        );
        assert_eq!(
            plan(
                &s,
                &GpsEdit::Set(GeoPoint::parse("35.6812345,139.7671234").unwrap())
            )
            .status,
            EntryStatus::NoChange
        );
    }
}
