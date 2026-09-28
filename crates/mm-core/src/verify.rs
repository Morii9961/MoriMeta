// SPDX-License-Identifier: GPL-3.0-or-later
//! Pre-commit verification of a temporary output (SAFETY_MODEL §5, V1–V5).

use std::collections::BTreeSet;

use mm_domain::plan::{Expect, TagOp};
use mm_domain::snapshot::{Snapshot, value_text};
use mm_exiftool::Output;

/// Groups that describe the file or are computed, not stored metadata.
const NON_STORED: &[&str] = &["System:", "File:", "Composite:", "ExifTool:"];

/// Tags whose value legitimately changes whenever ExifTool rewrites the file.
const DERIVED: &[&str] = &["XMP-x:XMPToolkit"];

/// Pointer tags (e.g. `IFD1:ThumbnailOffset`, `Pentax:PreviewImageStart`, `MPF0:MPImageStart`):
/// their values are file offsets that move when a metadata block grows. What they point to is
/// covered by the unchanged length tags and, for the main image, by ImageDataHash (V4).
fn is_pointer(tag: &str) -> bool {
    let name = tag.rsplit(':').next().unwrap_or(tag);
    name.ends_with("Offset") || name.ends_with("Offsets") || name.ends_with("Start")
}

/// Structural tags ExifTool adds when it has to create the EXIF block.
const MANDATORY_EXIF: &[&str] = &[
    "IFD0:XResolution",
    "IFD0:YResolution",
    "IFD0:ResolutionUnit",
    "IFD0:YCbCrPositioning",
    "ExifIFD:ExifVersion",
    "ExifIFD:ComponentsConfiguration",
    "ExifIFD:FlashpixVersion",
    "ExifIFD:ColorSpace",
];

/// Warnings that do not indicate a problem with the written output (kept deliberately short).
const BENIGN_WARNINGS: &[&str] = &[];

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    /// V1: ExifTool reported an error or an unexpected warning.
    Engine(String),
    /// V2: a target value is not what was planned.
    Value(String),
    /// V3: something else changed.
    Collateral(Vec<String>),
    /// V4: image data hash differs or is unavailable.
    ImageData(String),
    /// V5: output cannot be read back cleanly.
    Unreadable(String),
    /// The source no longer has the value shown in the Preview (file changed since planning).
    ChangedSincePreview(String),
}

impl std::fmt::Display for VerifyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VerifyError::Collateral(v) => write!(f, "unexpected changes: {}", v.join(", ")),
            other => write!(f, "{other:?}"),
        }
    }
}

fn stored(s: &Snapshot) -> impl Iterator<Item = (&String, &serde_json::Value)> {
    s.iter()
        .filter(|(k, _)| !NON_STORED.iter().any(|p| k.starts_with(p)))
}

fn warnings(s: &Snapshot) -> Vec<String> {
    s.iter()
        .filter(|(k, _)| k.as_str() == "ExifTool:Warning" || k.starts_with("ExifTool:Warning"))
        .map(|(_, v)| value_text(v))
        .collect()
}

/// An ExifTool message without the ` - <file>` it ends with: the file is the backup or a
/// temporary name, which means nothing to the user and would put a path into the History.
pub fn without_file(line: &str) -> &str {
    match line.rsplit_once(" - ") {
        Some((msg, file))
            if file.starts_with('/')
                || file.starts_with('\\')
                || file
                    .as_bytes()
                    .get(1..3)
                    .is_some_and(|b| b == b":/" || b == b":\\") =>
        {
            msg
        }
        _ => line,
    }
}

/// V1: exit status and stderr of the write command.
pub fn check_write_output(out: &Output) -> Result<(), VerifyError> {
    let err = out.stderr_text();
    if out.status != 0 {
        let lines: Vec<&str> = err.lines().map(|l| without_file(l.trim())).collect();
        return Err(VerifyError::Engine(format!(
            "status {}: {}",
            out.status,
            lines.join("; ").trim_matches(|c| c == ';' || c == ' ')
        )));
    }
    for line in err.lines().map(str::trim).filter(|l| !l.is_empty()) {
        if line.starts_with("Error") || !BENIGN_WARNINGS.iter().any(|w| line.contains(w)) {
            return Err(VerifyError::Engine(without_file(line).to_owned()));
        }
    }
    Ok(())
}

/// V2–V5 given full reads of the source (the verified backup) and the temporary output.
pub fn check_output(
    source: &Snapshot,
    temp: &Snapshot,
    ops: &[TagOp],
    expect: &[Expect],
) -> Result<(), VerifyError> {
    // V5: readable, no error
    if let Some(e) = temp.get("ExifTool:Error") {
        return Err(VerifyError::Unreadable(value_text(e)));
    }
    let new_warnings: Vec<String> = warnings(temp)
        .into_iter()
        .filter(|w| !warnings(source).contains(w))
        .collect();
    if !new_warnings.is_empty() {
        return Err(VerifyError::Unreadable(format!(
            "new warnings: {}",
            new_warnings.join("; ")
        )));
    }
    // V2: planned values
    for e in expect {
        match e {
            Expect::Equals { tag, values } => {
                let got = temp.list(tag);
                if got.as_ref() != Some(values) {
                    return Err(VerifyError::Value(format!(
                        "{tag}: expected {values:?}, found {got:?}"
                    )));
                }
            }
            Expect::Absent { tag } => {
                if temp.contains(tag) {
                    return Err(VerifyError::Value(format!("{tag}: expected absent")));
                }
            }
            Expect::Near { tag, value, within } => {
                let got = temp.text(tag).and_then(|t| t.trim().parse::<f64>().ok());
                let (want, tol) = (value.parse::<f64>(), within.parse::<f64>());
                let ok = matches!((got, want, tol), (Some(g), Ok(w), Ok(t)) if (g - w).abs() <= t);
                if !ok {
                    return Err(VerifyError::Value(format!(
                        "{tag}: expected {value} ± {within}, found {got:?}"
                    )));
                }
            }
            Expect::IptcDigestCurrent => {
                if !temp.contains("Photoshop:IPTCDigest")
                    || warnings(temp).iter().any(|w| w.contains("IPTCDigest"))
                {
                    return Err(VerifyError::Value(
                        "IPTCDigest missing or not current".into(),
                    ));
                }
            }
        }
    }
    // V3: no collateral changes
    // a language-alternative write (`…-x-default`) changes the key it reads back under
    let mut allowed: BTreeSet<&str> = ops
        .iter()
        .flat_map(|o| [o.tag(), mm_domain::plan::read_key(o.tag())])
        .collect();
    allowed.extend(DERIVED);
    let source_had_exif = source
        .keys()
        .any(|k| k.starts_with("IFD0:") || k.starts_with("ExifIFD:"));
    if !source_had_exif {
        allowed.extend(MANDATORY_EXIF);
    }
    // ExifTool adds GPSVersionID itself when it has to create the GPS directory
    if !source.keys().any(|k| k.starts_with("GPS:")) {
        allowed.insert("GPS:GPSVersionID");
    }
    // `Group:all` (deleting a whole group, e.g. GPS removal) covers every tag of that group
    let whole_groups: Vec<&str> = ops
        .iter()
        .filter_map(|o| o.tag().strip_suffix(":all"))
        .collect();
    let a: std::collections::BTreeMap<&String, &serde_json::Value> = stored(source).collect();
    let b: std::collections::BTreeMap<&String, &serde_json::Value> = stored(temp).collect();
    let mut bad = Vec::new();
    for k in a.keys().chain(b.keys()).collect::<BTreeSet<_>>() {
        let base = k.split(" (").next().unwrap_or(k);
        // …but only for tags that disappeared; anything appearing or changing there still counts
        let in_deleted_group = base
            .split_once(':')
            .is_some_and(|(g, _)| whole_groups.contains(&g))
            && a.contains_key(k)
            && !b.contains_key(k);
        if allowed.contains(base)
            || in_deleted_group
            || (is_pointer(base) && a.contains_key(k) && b.contains_key(k))
        {
            continue;
        }
        if a.get(k) != b.get(k) {
            bad.push(k.to_string());
        }
    }
    if !bad.is_empty() {
        return Err(VerifyError::Collateral(bad));
    }
    // V4: image data unchanged — an XMP file (sidecar) has none, and must not gain any
    let is_xmp = |s: &Snapshot| s.text("File:FileType").as_deref() == Some("XMP");
    if is_xmp(temp)
        && !temp.contains("File:ImageDataHash")
        && !source.contains("File:ImageDataHash")
    {
        return Ok(());
    }
    let (hs, ht) = (
        source.text("File:ImageDataHash"),
        temp.text("File:ImageDataHash"),
    );
    match (hs, ht) {
        (Some(x), Some(y)) if x == y => Ok(()),
        (x, y) => Err(VerifyError::ImageData(format!("{x:?} vs {y:?}"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exiftool_messages_lose_the_file_they_name() {
        for (line, want) in [
            (
                "Warning: [minor] Maker notes could not be parsed - C:/d/backups/op-1/0001-a.jpg",
                "Warning: [minor] Maker notes could not be parsed",
            ),
            (
                "Error: Not a valid JPG - \\\\host\\share\\a.jpg",
                "Error: Not a valid JPG",
            ),
            ("Warning: a - b", "Warning: a - b"),
            ("Warning: no file", "Warning: no file"),
        ] {
            assert_eq!(without_file(line), want);
        }
    }
    use serde_json::json;

    fn s(v: serde_json::Value) -> Snapshot {
        Snapshot::from_json(&v)
    }

    #[test]
    fn numbers_within_tolerance_and_whole_group_deletion() {
        let src = s(
            json!({"GPS:GPSLatitude": 1.0, "GPS:GPSTimeStamp": "10:00:00",
            "IFD0:Make": "X", "File:ImageDataHash": "h"}),
        );
        let ops = vec![TagOp::Delete {
            tag: "GPS:all".into(),
        }];
        let gone = s(json!({"IFD0:Make": "X", "File:ImageDataHash": "h"}));
        let expect = vec![
            Expect::Absent {
                tag: "GPS:GPSLatitude".into(),
            },
            Expect::Absent {
                tag: "GPS:GPSTimeStamp".into(),
            },
        ];
        assert_eq!(check_output(&src, &gone, &ops, &expect), Ok(()));
        // a GPS tag that appears while the group is deleted is still a collateral change
        let odd =
            s(json!({"IFD0:Make": "X", "GPS:GPSMapDatum": "WGS84", "File:ImageDataHash": "h"}));
        assert!(matches!(
            check_output(&src, &odd, &ops, &expect),
            Err(VerifyError::Collateral(_))
        ));
        // numeric comparison within tolerance
        let set = vec![TagOp::Set {
            tag: "GPS:GPSLatitude".into(),
            values: vec!["35.6812345".into()],
        }];
        let near = |v: &str| {
            vec![Expect::Near {
                tag: "GPS:GPSLatitude".into(),
                value: v.into(),
                within: "0.0000001".into(),
            }]
        };
        let written = s(
            json!({"GPS:GPSLatitude": 35.6812345000306, "GPS:GPSTimeStamp": "10:00:00",
            "IFD0:Make": "X", "File:ImageDataHash": "h"}),
        );
        assert_eq!(
            check_output(&src, &written, &set, &near("35.6812345")),
            Ok(())
        );
        assert!(matches!(
            check_output(&src, &written, &set, &near("35.6812347")),
            Err(VerifyError::Value(_))
        ));
    }

    #[test]
    fn default_language_write_allows_its_read_key_but_not_other_languages() {
        let src = s(
            json!({"XMP-dc:Rights": "Old", "XMP-dc:Rights-de": "Alt", "File:ImageDataHash": "h"}),
        );
        let ops = vec![TagOp::Set {
            tag: "XMP-dc:Rights-x-default".into(),
            values: vec!["New".into()],
        }];
        let expect = vec![Expect::Equals {
            tag: "XMP-dc:Rights".into(),
            values: vec!["New".into()],
        }];
        let ok = s(
            json!({"XMP-dc:Rights": "New", "XMP-dc:Rights-de": "Alt", "File:ImageDataHash": "h"}),
        );
        assert_eq!(check_output(&src, &ok, &ops, &expect), Ok(()));
        // ExifTool without a language code drops the other languages: V3 must refuse that
        let dropped = s(json!({"XMP-dc:Rights": "New", "File:ImageDataHash": "h"}));
        assert!(matches!(
            check_output(&src, &dropped, &ops, &expect),
            Err(VerifyError::Collateral(v)) if v == vec!["XMP-dc:Rights-de".to_string()]
        ));
    }

    #[test]
    fn accepts_planned_change_and_derived_tags() {
        let src = s(
            json!({"IFD0:Artist": "Old", "IFD1:ThumbnailOffset": "100", "File:ImageDataHash": "h"}),
        );
        let tmp = s(
            json!({"IFD0:Artist": "New", "XMP-dc:Creator": "New", "XMP-x:XMPToolkit": "Image::ExifTool 13.59",
                           "IFD1:ThumbnailOffset": "180", "File:ImageDataHash": "h", "File:FileSize": "1"}),
        );
        let ops = vec![
            TagOp::Set {
                tag: "IFD0:Artist".into(),
                values: vec!["New".into()],
            },
            TagOp::Set {
                tag: "XMP-dc:Creator".into(),
                values: vec!["New".into()],
            },
        ];
        let ex = vec![
            Expect::Equals {
                tag: "IFD0:Artist".into(),
                values: vec!["New".into()],
            },
            Expect::Equals {
                tag: "XMP-dc:Creator".into(),
                values: vec!["New".into()],
            },
        ];
        assert_eq!(check_output(&src, &tmp, &ops, &ex), Ok(()));
    }

    #[test]
    fn rejects_collateral_value_and_pixel_changes() {
        let src = s(json!({"IFD0:Artist": "Old", "IFD0:Software": "X", "File:ImageDataHash": "h"}));
        let ops = vec![TagOp::Set {
            tag: "IFD0:Artist".into(),
            values: vec!["New".into()],
        }];
        let ex = vec![Expect::Equals {
            tag: "IFD0:Artist".into(),
            values: vec!["New".into()],
        }];
        let collateral = s(json!({"IFD0:Artist": "New", "File:ImageDataHash": "h"}));
        assert!(matches!(
            check_output(&src, &collateral, &ops, &ex),
            Err(VerifyError::Collateral(_))
        ));
        let wrong =
            s(json!({"IFD0:Artist": "Ne", "IFD0:Software": "X", "File:ImageDataHash": "h"}));
        assert!(matches!(
            check_output(&src, &wrong, &ops, &ex),
            Err(VerifyError::Value(_))
        ));
        let pixels =
            s(json!({"IFD0:Artist": "New", "IFD0:Software": "X", "File:ImageDataHash": "z"}));
        assert!(matches!(
            check_output(&src, &pixels, &ops, &ex),
            Err(VerifyError::ImageData(_))
        ));
        let warned = s(
            json!({"IFD0:Artist": "New", "IFD0:Software": "X", "File:ImageDataHash": "h", "ExifTool:Warning": "x"}),
        );
        assert!(matches!(
            check_output(&src, &warned, &ops, &ex),
            Err(VerifyError::Unreadable(_))
        ));
    }

    #[test]
    fn moved_pointers_are_derived_but_new_or_removed_pointers_are_not() {
        let ops = vec![TagOp::Set {
            tag: "IFD0:Artist".into(),
            values: vec!["A".into()],
        }];
        let ex = vec![Expect::Equals {
            tag: "IFD0:Artist".into(),
            values: vec!["A".into()],
        }];
        let src = s(
            json!({"IFD0:Make": "P", "Pentax:PreviewImageStart": "100", "Pentax:PreviewImageLength": "9", "File:ImageDataHash": "h"}),
        );
        let moved = s(
            json!({"IFD0:Make": "P", "IFD0:Artist": "A", "Pentax:PreviewImageStart": "180", "Pentax:PreviewImageLength": "9", "File:ImageDataHash": "h"}),
        );
        assert_eq!(check_output(&src, &moved, &ops, &ex), Ok(()));
        let shrunk = s(
            json!({"IFD0:Make": "P", "IFD0:Artist": "A", "Pentax:PreviewImageStart": "180", "Pentax:PreviewImageLength": "8", "File:ImageDataHash": "h"}),
        );
        assert!(matches!(
            check_output(&src, &shrunk, &ops, &ex),
            Err(VerifyError::Collateral(_))
        ));
        let dropped = s(
            json!({"IFD0:Make": "P", "IFD0:Artist": "A", "Pentax:PreviewImageLength": "9", "File:ImageDataHash": "h"}),
        );
        assert!(matches!(
            check_output(&src, &dropped, &ops, &ex),
            Err(VerifyError::Collateral(_))
        ));
    }

    #[test]
    fn mandatory_exif_allowed_only_when_exif_was_created() {
        let ops = vec![TagOp::Set {
            tag: "IFD0:Artist".into(),
            values: vec!["A".into()],
        }];
        let ex = vec![Expect::Equals {
            tag: "IFD0:Artist".into(),
            values: vec!["A".into()],
        }];
        let tmp = s(
            json!({"IFD0:Artist": "A", "IFD0:YCbCrPositioning": "Centered", "File:ImageDataHash": "h"}),
        );
        assert_eq!(
            check_output(&s(json!({"File:ImageDataHash": "h"})), &tmp, &ops, &ex),
            Ok(())
        );
        let with_exif = s(json!({"IFD0:Make": "N", "File:ImageDataHash": "h"}));
        let tmp2 = s(
            json!({"IFD0:Make": "N", "IFD0:Artist": "A", "IFD0:YCbCrPositioning": "Centered", "File:ImageDataHash": "h"}),
        );
        assert!(matches!(
            check_output(&with_exif, &tmp2, &ops, &ex),
            Err(VerifyError::Collateral(_))
        ));
    }
}
