// SPDX-License-Identifier: GPL-3.0-or-later
//! IPTC IIM rules shared by every field that updates an existing IPTC copy (METADATA_MODEL §6):
//! character set, encoded byte limits, duplicate records, and the Photoshop IPTC digest.

use crate::snapshot::Snapshot;

pub const CHARSET: &str = "IPTC:CodedCharacterSet";
pub const DIGEST: &str = "Photoshop:IPTCDigest";

pub fn present(snap: &Snapshot) -> bool {
    snap.keys().any(|k| k.starts_with("IPTC:"))
}

/// ExifTool reports additional/duplicate IPTC blocks as IPTC2, IPTC3, …
pub fn has_extra_records(snap: &Snapshot) -> bool {
    snap.keys()
        .any(|k| k.len() > 5 && k.starts_with("IPTC") && k.as_bytes()[4].is_ascii_digit())
}

pub fn is_utf8(snap: &Snapshot) -> bool {
    snap.text(CHARSET)
        .map(|v| v.eq_ignore_ascii_case("UTF8") || v == "\u{1b}%G")
        .unwrap_or(false)
}

/// Encoded byte length of `s` in the file's IPTC character set, or None if not representable
/// (Latin = cp1252 when CodedCharacterSet is not UTF-8).
pub fn encoded_len(s: &str, utf8: bool) -> Option<usize> {
    if utf8 {
        return Some(s.len());
    }
    s.chars()
        .all(crate::cp1252::encodable)
        .then(|| s.chars().count())
}

/// Why `value` cannot be stored in the IPTC `dataset` of this file (limit `max` bytes), if so.
/// ExifTool would silently truncate an over-long value, cutting UTF-8 characters (S3).
pub fn refuse(snap: &Snapshot, dataset: &str, value: &str, max: usize) -> Option<String> {
    match encoded_len(value, is_utf8(snap)) {
        None => Some(format!(
            "IPTC {dataset} uses the Latin character set and cannot store {value:?}; \
             convert IPTC to UTF-8, remove the IPTC copy, or exclude this file"
        )),
        Some(n) if n > max => Some(format!(
            "IPTC {dataset} allows {max} bytes; {value:?} needs {n} (ExifTool would truncate it)"
        )),
        _ => None,
    }
}
