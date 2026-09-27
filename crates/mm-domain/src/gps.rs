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

use crate::plan::{ChangeKind, EntryStatus, Expect, FieldChange, FieldPlan, TagOp};
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
    /// `lat,lon[,alt]` in decimal degrees and metres, range-checked.
    pub fn parse(s: &str) -> Result<GeoPoint, String> {
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
        if let Some(a) = self.alt {
            if !a.is_finite() || a.abs() > 100_000.0 {
                return Err(format!("altitude {a} m is not plausible"));
            }
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
                if let Some(a) = p.alt {
                    if snap.contains(XMP_ALT) {
                        near(
                            &mut ops,
                            &mut expect,
                            XMP_ALT,
                            format!("{:.3}", a.abs()),
                            ALT_TOLERANCE,
                        );
                        altitude_ref(&mut ops, &mut expect, XMP_ALT_REF, a < 0.0);
                    }
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
