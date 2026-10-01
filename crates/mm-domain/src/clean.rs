// SPDX-License-Identifier: GPL-3.0-or-later
//! Clean Export (D-15 (c), METADATA_MODEL §10–10.1, validated by S7): a copy of a JPEG that keeps
//! only whitelisted tags and segments. The Preview predicts every tag and segment removed; the
//! output is checked against the segment whitelist, the tag whitelist, the source's image data
//! hash and the prediction, and is not exported unless every check passes.
//!
//! Tags are keyed `Group0:Group1:Tag` as `-a -G0:1 -u -U` reads them; a key repeated in one file
//! gets `#2`, `#3`… so that every occurrence is counted.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::jpeg::Jpeg;

/// Which categories the user keeps (METADATA_MODEL §10.1 defaults: camera, lens, exposure,
/// capture time, author and copyright on; title, description and keywords off). Orientation,
/// colour and the EXIF structure are always kept; maker notes never are.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KeepSpec {
    pub camera: bool,
    pub lens: bool,
    pub exposure: bool,
    pub capture_time: bool,
    pub author: bool,
    pub descriptive: bool,
}

impl Default for KeepSpec {
    fn default() -> Self {
        KeepSpec {
            camera: true,
            lens: true,
            exposure: true,
            capture_time: true,
            author: true,
            descriptive: false,
        }
    }
}

const CAMERA: &[&str] = &["EXIF:IFD0:Make", "EXIF:IFD0:Model"];
const LENS: &[&str] = &[
    "EXIF:ExifIFD:LensMake",
    "EXIF:ExifIFD:LensModel",
    "EXIF:ExifIFD:LensInfo",
];
const EXPOSURE: &[&str] = &[
    "EXIF:ExifIFD:ExposureTime",
    "EXIF:ExifIFD:FNumber",
    "EXIF:ExifIFD:ISO",
    "EXIF:ExifIFD:ExposureProgram",
    "EXIF:ExifIFD:ExposureCompensation",
    "EXIF:ExifIFD:MeteringMode",
    "EXIF:ExifIFD:Flash",
    "EXIF:ExifIFD:FocalLength",
    "EXIF:ExifIFD:FocalLengthIn35mmFormat",
    "EXIF:ExifIFD:WhiteBalance",
];
const CAPTURE_TIME: &[&str] = &[
    "EXIF:ExifIFD:DateTimeOriginal",
    "EXIF:ExifIFD:SubSecTimeOriginal",
    "EXIF:ExifIFD:OffsetTimeOriginal",
];
const AUTHOR: &[&str] = &[
    "EXIF:IFD0:Artist",
    "EXIF:IFD0:Copyright",
    "XMP:XMP-dc:Creator",
    "XMP:XMP-dc:Rights",
    "XMP:XMP-xmpRights:Marked",
    "XMP:XMP-xmpRights:UsageTerms",
    "XMP:XMP-xmpRights:WebStatement",
];
const DESCRIPTIVE: &[&str] = &[
    "XMP:XMP-dc:Title",
    "XMP:XMP-dc:Description",
    "XMP:XMP-dc:Subject",
];
/// Always kept: how the image is shown.
const ORIENTATION_COLOUR: &[&str] = &[
    "EXIF:IFD0:Orientation",
    "EXIF:ExifIFD:ColorSpace",
    "EXIF:ExifIFD:Gamma",
    "EXIF:InteropIFD:InteropIndex",
];
/// Always copied from the source, so that ExifTool does not substitute its defaults; ExifTool
/// may also add them when it creates EXIF. No personal data.
const STRUCTURE: &[&str] = &[
    "EXIF:IFD0:XResolution",
    "EXIF:IFD0:YResolution",
    "EXIF:IFD0:ResolutionUnit",
    "EXIF:IFD0:YCbCrPositioning",
    "EXIF:ExifIFD:ExifVersion",
    "EXIF:ExifIFD:ComponentsConfiguration",
    "EXIF:InteropIFD:InteropVersion",
];
/// Groups that may stay in the output: the colour profile and the Adobe colour-transform segment
/// (structural), and what ExifTool reports about the file rather than reading from it.
const STRUCTURAL_PREFIXES: &[&str] = &[
    "ICC_Profile:",
    "APP14:Adobe:",
    "File:",
    "Composite:",
    "ExifTool:",
];
/// Not stored in the file: left out of every comparison.
const NOT_STORED: &[&str] = &["File:", "Composite:", "ExifTool:", "SourceFile"];

impl KeepSpec {
    /// Every tag this spec keeps, `Group0:Group1:Tag`.
    pub fn keep_tags(&self) -> BTreeSet<&'static str> {
        let mut out: BTreeSet<&'static str> = ORIENTATION_COLOUR
            .iter()
            .chain(STRUCTURE)
            .copied()
            .collect();
        for (on, tags) in [
            (self.camera, CAMERA),
            (self.lens, LENS),
            (self.exposure, EXPOSURE),
            (self.capture_time, CAPTURE_TIME),
            (self.author, AUTHOR),
            (self.descriptive, DESCRIPTIVE),
        ] {
            if on {
                out.extend(tags.iter().copied());
            }
        }
        out
    }

    /// The tags copied back from the source, as ExifTool names them (`Group1:Tag`).
    pub fn copy_tags(&self) -> Vec<String> {
        self.keep_tags()
            .iter()
            .filter_map(|k| k.split_once(':').map(|(_, rest)| rest.to_owned()))
            .collect()
    }
}

/// A tag key without its occurrence number (`EXIF:IFD0:Make#2` → `EXIF:IFD0:Make`).
fn base(k: &str) -> &str {
    k.split('#').next().unwrap_or(k)
}

fn structural(k: &str) -> bool {
    STRUCTURAL_PREFIXES.iter().any(|p| k.starts_with(p))
}

fn stored(k: &str) -> bool {
    !NOT_STORED.iter().any(|p| k.starts_with(p))
}

/// The privacy category a removed tag is shown under (METADATA_MODEL §10; grouping only: the
/// export removes everything not kept, categorised or not).
pub fn category(k: &str) -> &'static str {
    let last = k.rsplit(':').next().unwrap_or(k);
    let has = |xs: &[&str]| xs.iter().any(|x| k.contains(x));
    if k.contains(":GPS:") || last.contains("GPS") {
        "gps"
    } else if k.contains("SerialNumber") {
        "serial_numbers"
    } else if k.contains("OwnerName") {
        "owner"
    } else if has(&[
        "xmpMM",
        "DocumentID",
        "InstanceID",
        "ImageUniqueID",
        "DocumentAncestors",
    ]) {
        "history_ids"
    } else if has(&[
        "ThumbnailImage",
        "PreviewImage",
        "PhotoshopThumbnail",
        "MPImage",
        "JpgFromRaw",
        "ThumbnailOffset",
        "ThumbnailLength",
    ]) {
        "embedded_previews"
    } else if has(&["RegionInfo", "PersonInImage", "mwg-rs"]) {
        "people"
    } else if k.ends_with(":Comment") || has(&["UserComment", ":XP"]) {
        "comments"
    } else if k.starts_with("JUMBF") {
        "content_credentials"
    } else if k.starts_with("MakerNotes:") {
        "maker_notes"
    } else if k.starts_with("JFIF:")
        || has(&["FlashpixVersion", "ExifImageWidth", "ExifImageHeight"])
    {
        "structural"
    } else if k.ends_with(":Software") || has(&["CreatorTool", "XMPToolkit"]) {
        "software"
    } else if has(&["City", "Country", "State", "Sublocation", "Location"]) {
        "location_names"
    } else if has(&[
        "Title",
        "Description",
        "Subject",
        "Keywords",
        "Caption",
        "Headline",
    ]) {
        "descriptive"
    } else {
        "other"
    }
}

/// Categories the Preview marks as high risk.
pub fn high_risk(category: &str) -> bool {
    matches!(
        category,
        "gps"
            | "serial_numbers"
            | "owner"
            | "people"
            | "embedded_previews"
            | "comments"
            | "location_names"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedTag {
    pub key: String,
    pub category: String,
    /// The value, cut to 60 characters.
    pub value: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemovedSegment {
    pub label: String,
    pub bytes: usize,
    /// A segment MoriMeta does not recognise (shown as "unidentified → removed").
    pub unidentified: bool,
}

/// What the export of one file keeps and removes, before anything is written.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Prediction {
    pub keep: Vec<String>,
    pub remove: Vec<RemovedTag>,
    pub remove_segments: Vec<RemovedSegment>,
    /// The lens name exists only in the maker notes, which are never kept (METADATA_MODEL §10.1).
    pub lens_lost: bool,
}

fn short(v: &str) -> String {
    if v.chars().count() <= 60 {
        v.to_owned()
    } else {
        format!("{}…", v.chars().take(57).collect::<String>())
    }
}

pub fn predict(spec: &KeepSpec, tags: &BTreeMap<String, String>, segments: &Jpeg) -> Prediction {
    let keep_set = spec.keep_tags();
    let mut p = Prediction::default();
    for (k, v) in tags
        .iter()
        .filter(|(k, _)| stored(k) && k.as_str() != "ExifTool:Warning")
    {
        let b = base(k);
        if keep_set.contains(b) || structural(b) {
            p.keep.push(k.clone());
        } else {
            p.remove.push(RemovedTag {
                key: k.clone(),
                category: category(b).to_owned(),
                value: short(v),
            });
        }
    }
    for s in &segments.segments {
        if !s.allowed_in_clean() {
            let label = s.label();
            p.remove_segments.push(RemovedSegment {
                unidentified: label.contains("unknown:") || label.starts_with("marker:"),
                bytes: s.bytes(),
                label,
            });
        }
    }
    if segments.trailer_len > 0 {
        p.remove_segments.push(RemovedSegment {
            label: "after the end of the image".into(),
            bytes: segments.trailer_len,
            unidentified: true,
        });
    }
    p.lens_lost = spec.lens
        && !tags.keys().any(|k| base(k) == "EXIF:ExifIFD:LensModel")
        && tags.keys().any(|k| {
            k.starts_with("MakerNotes:")
                && (k.ends_with(":Lens") || k.ends_with(":LensID") || k.ends_with(":LensType"))
        });
    p
}

/// The output checks (METADATA_MODEL §10.1 ①–④): reasons to refuse the output, none when it may
/// be exported. `out_segment_problems` comes from [`crate::jpeg::check_clean`] on the output.
pub fn check(
    spec: &KeepSpec,
    source: &BTreeMap<String, String>,
    predicted: &Prediction,
    output: &BTreeMap<String, String>,
    out_segment_problems: &[String],
    source_hash: Option<&str>,
    output_hash: Option<&str>,
) -> Vec<String> {
    let keep_set = spec.keep_tags();
    let structure: BTreeSet<&str> = STRUCTURE.iter().copied().collect();
    let mut why: Vec<String> = out_segment_problems
        .iter()
        .map(|s| format!("segment: {s}"))
        .collect();
    let src: BTreeSet<&str> = source
        .keys()
        .map(String::as_str)
        .filter(|k| stored(k) && *k != "ExifTool:Warning")
        .collect();
    let out: BTreeSet<&str> = output
        .keys()
        .map(String::as_str)
        .filter(|k| stored(k) && *k != "ExifTool:Warning")
        .collect();
    let list = |v: Vec<&str>| v.into_iter().take(10).collect::<Vec<_>>().join(", ");
    let residue: Vec<&str> = out
        .iter()
        .copied()
        .filter(|k| !(keep_set.contains(base(k)) || structural(base(k))))
        .collect();
    if !residue.is_empty() {
        why.push(format!(
            "tags outside the whitelist in the output: {}",
            list(residue)
        ));
    }
    let predicted_removed: BTreeSet<&str> =
        predicted.remove.iter().map(|r| r.key.as_str()).collect();
    let actually_removed: BTreeSet<&str> = src.difference(&out).copied().collect();
    let not_removed: Vec<&str> = predicted_removed
        .difference(&actually_removed)
        .copied()
        .collect();
    if !not_removed.is_empty() {
        why.push(format!(
            "a predicted removal was not performed: {}",
            list(not_removed)
        ));
    }
    let lost: Vec<&str> = actually_removed
        .difference(&predicted_removed)
        .copied()
        .collect();
    if !lost.is_empty() {
        why.push(format!(
            "the Preview said these are kept, but they were removed: {}",
            list(lost)
        ));
    }
    let added: Vec<&str> = out
        .difference(&src)
        .copied()
        .filter(|k| !structure.contains(base(k)) && !structural(base(k)))
        .collect();
    if !added.is_empty() {
        why.push(format!("unexpected tags added: {}", list(added)));
    }
    match (source_hash, output_hash) {
        (Some(a), Some(b)) if a == b => {}
        _ => why.push("the image data differs from the source (or its hash is unavailable)".into()),
    }
    why
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jpeg;

    fn tags(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    fn plain_jpeg() -> Jpeg {
        let mut b = vec![0xFF, 0xD8, 0xFF, 0xE5, 0x00, 0x06, b'A', b'B', b'C', 0];
        b.extend_from_slice(&[0xFF, 0xD9, b'X']);
        jpeg::parse(&b).unwrap()
    }

    #[test]
    fn prediction_keeps_the_whitelist_and_groups_the_rest() {
        let src = tags(&[
            ("EXIF:IFD0:Make", "NIKON"),
            ("EXIF:IFD0:Orientation", "Horizontal"),
            ("EXIF:GPS:GPSLatitude", "43 deg"),
            ("EXIF:ExifIFD:SerialNumber", "1234"),
            ("MakerNotes:Nikon:LensID", "AF-S 50mm"),
            ("File:FileSize", "1 MB"),
            ("ICC_Profile:ICC-header:ProfileVersion", "2.1"),
        ]);
        let p = predict(&KeepSpec::default(), &src, &plain_jpeg());
        assert_eq!(
            p.keep,
            [
                "EXIF:IFD0:Make",
                "EXIF:IFD0:Orientation",
                "ICC_Profile:ICC-header:ProfileVersion"
            ]
        );
        let cats: Vec<&str> = p.remove.iter().map(|r| r.category.as_str()).collect();
        assert_eq!(cats, ["serial_numbers", "gps", "maker_notes"]);
        assert!(p.lens_lost, "lens only in maker notes");
        assert_eq!(p.remove_segments.len(), 2); // APP5 unknown + trailer
        assert!(p.remove_segments.iter().all(|s| s.unidentified));
        // keeping nothing optional removes the camera too
        let none = KeepSpec {
            camera: false,
            lens: false,
            exposure: false,
            capture_time: false,
            author: false,
            descriptive: false,
        };
        assert!(
            predict(&none, &src, &plain_jpeg())
                .remove
                .iter()
                .any(|r| r.key == "EXIF:IFD0:Make")
        );
    }

    #[test]
    fn checks_refuse_residue_surprises_and_changed_pixels() {
        let spec = KeepSpec::default();
        let src = tags(&[("EXIF:IFD0:Make", "NIKON"), ("EXIF:GPS:GPSLatitude", "43")]);
        let p = predict(&spec, &src, &plain_jpeg());
        let good = tags(&[("EXIF:IFD0:Make", "NIKON"), ("EXIF:IFD0:XResolution", "72")]);
        assert!(check(&spec, &src, &p, &good, &[], Some("h"), Some("h")).is_empty());
        let leaked = tags(&[("EXIF:IFD0:Make", "NIKON"), ("EXIF:GPS:GPSLatitude", "43")]);
        let why = check(&spec, &src, &p, &leaked, &[], Some("h"), Some("h")).join("\n");
        assert!(
            why.contains("outside the whitelist") && why.contains("not performed"),
            "{why}"
        );
        let lost = tags(&[]);
        assert!(
            check(&spec, &src, &p, &lost, &[], Some("h"), Some("h"))[0]
                .contains("said these are kept")
        );
        let added = tags(&[("EXIF:IFD0:Make", "NIKON"), ("EXIF:IFD0:Software", "x")]);
        assert!(
            check(&spec, &src, &p, &added, &[], Some("h"), Some("h"))
                .join("\n")
                .contains("unexpected tags added")
        );
        assert!(
            check(&spec, &src, &p, &good, &[], Some("h"), Some("other"))[0].contains("image data")
        );
        assert!(
            check(
                &spec,
                &src,
                &p,
                &good,
                &["APP5 x".into()],
                Some("h"),
                Some("h")
            )[0]
            .starts_with("segment:")
        );
    }
}
