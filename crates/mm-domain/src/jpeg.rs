// SPDX-License-Identifier: GPL-3.0-or-later
//! JPEG marker segments (METADATA_MODEL §10.1, from the S7 prototype): the complete marker
//! structure, entropy-coded data after each SOS (with byte stuffing, RSTn and fill bytes) and
//! the bytes after EOI, so that what a file contains is not limited to what a metadata reader
//! decodes. Clean Export checks its output against [`check_clean`].

use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Kind {
    Soi,
    App { n: u8, id: String },
    Com,
    Dqt,
    Dht,
    Dri,
    Sof { n: u8 },
    Sos,
    Eoi,
    Other { marker: u8 },
}

#[derive(Debug, Clone, Serialize)]
pub struct Segment {
    pub offset: usize,
    pub marker: u8,
    pub kind: Kind,
    /// Payload length without marker and length field (for SOS: the header only).
    pub payload_len: usize,
    /// For SOS: the entropy-coded data that follows the header.
    pub scan_len: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct Jpeg {
    pub segments: Vec<Segment>,
    /// Bytes after EOI (MPF secondary images, appended data, …).
    pub trailer_len: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    NotJpeg,
    Truncated(usize),
    BadMarker(usize),
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NotJpeg => write!(f, "not a JPEG file"),
            ParseError::Truncated(at) => write!(f, "the JPEG structure is truncated at byte {at}"),
            ParseError::BadMarker(at) => write!(f, "invalid JPEG marker at byte {at}"),
        }
    }
}

/// Identifier strings that classify APPn segments.
const APP_IDS: &[(&[u8], &str)] = &[
    (b"JFIF\0", "JFIF"),
    (b"JFXX\0", "JFXX"),
    (b"Exif\0\0", "Exif"),
    (b"Exif\0\xff", "Exif"),
    (b"http://ns.adobe.com/xap/1.0/\0", "XMP"),
    (b"http://ns.adobe.com/xmp/extension/\0", "ExtendedXMP"),
    (b"ICC_PROFILE\0", "ICC_PROFILE"),
    (b"MPF\0", "MPF"),
    (b"FPXR\0", "FPXR"),
    (b"Photoshop 3.0\0", "Photoshop"),
    (b"Adobe", "Adobe"),
    (b"Ducky", "Ducky"),
    (b"JP", "JUMBF"),
    (b"FLIR\0", "FLIR"),
];

fn app_id(n: u8, payload: &[u8]) -> String {
    for (prefix, name) in APP_IDS {
        if payload.starts_with(prefix) {
            // JUMBF is only meaningful in APP11
            if *name == "JUMBF" && n != 11 {
                continue;
            }
            return (*name).to_owned();
        }
    }
    let printable: String = payload
        .iter()
        .take(16)
        .take_while(|b| **b != 0)
        .map(|b| {
            if b.is_ascii_graphic() {
                *b as char
            } else {
                '.'
            }
        })
        .collect();
    format!("unknown:{printable}")
}

pub fn parse(b: &[u8]) -> Result<Jpeg, ParseError> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return Err(ParseError::NotJpeg);
    }
    let mut segments = vec![Segment {
        offset: 0,
        marker: 0xD8,
        kind: Kind::Soi,
        payload_len: 0,
        scan_len: 0,
    }];
    let mut i = 2usize;
    loop {
        // markers may be preceded by any number of 0xFF fill bytes
        if i >= b.len() {
            return Err(ParseError::Truncated(i));
        }
        if b[i] != 0xFF {
            return Err(ParseError::BadMarker(i));
        }
        while i < b.len() && b[i] == 0xFF {
            i += 1;
        }
        if i >= b.len() {
            return Err(ParseError::Truncated(i));
        }
        let m = b[i];
        let off = i - 1;
        i += 1;
        match m {
            0xD9 => {
                segments.push(Segment {
                    offset: off,
                    marker: m,
                    kind: Kind::Eoi,
                    payload_len: 0,
                    scan_len: 0,
                });
                return Ok(Jpeg {
                    segments,
                    trailer_len: b.len() - i,
                });
            }
            // standalone markers are not valid between segments
            0x01 | 0xD0..=0xD7 => return Err(ParseError::BadMarker(off)),
            _ => {}
        }
        if i + 2 > b.len() {
            return Err(ParseError::Truncated(i));
        }
        let len = u16::from_be_bytes([b[i], b[i + 1]]) as usize;
        if len < 2 || i + len > b.len() {
            return Err(ParseError::Truncated(i));
        }
        let payload = &b[i + 2..i + len];
        let kind = match m {
            0xE0..=0xEF => Kind::App {
                n: m - 0xE0,
                id: app_id(m - 0xE0, payload),
            },
            0xFE => Kind::Com,
            0xDB => Kind::Dqt,
            0xC4 => Kind::Dht,
            0xDD => Kind::Dri,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => Kind::Sof { n: m - 0xC0 },
            0xDA => Kind::Sos,
            other => Kind::Other { marker: other },
        };
        i += len;
        let mut scan_len = 0;
        if kind == Kind::Sos {
            // entropy-coded data: up to a marker that is not 0xFF00 stuffing, RSTn or fill
            let start = i;
            loop {
                if i + 1 >= b.len() {
                    return Err(ParseError::Truncated(i));
                }
                if b[i] == 0xFF {
                    let n = b[i + 1];
                    if n == 0x00 || (0xD0..=0xD7).contains(&n) {
                        i += 2;
                        continue;
                    }
                    if n == 0xFF {
                        i += 1;
                        continue;
                    }
                    break;
                }
                i += 1;
            }
            scan_len = i - start;
        }
        segments.push(Segment {
            offset: off,
            marker: m,
            kind,
            payload_len: len - 2,
            scan_len,
        });
    }
}

impl Segment {
    pub fn label(&self) -> String {
        match &self.kind {
            Kind::App { n, id } => format!("APP{n}:{id}"),
            Kind::Sof { n } => format!("SOF{n}"),
            Kind::Other { marker } => format!("marker:{marker:02X}"),
            Kind::Soi => "SOI".into(),
            Kind::Com => "COM".into(),
            Kind::Dqt => "DQT".into(),
            Kind::Dht => "DHT".into(),
            Kind::Dri => "DRI".into(),
            Kind::Sos => "SOS".into(),
            Kind::Eoi => "EOI".into(),
        }
    }

    /// Whether Clean Export output may hold this kind of segment (METADATA_MODEL §10.1 segment
    /// whitelist; the counts of Exif and XMP are checked by [`check_clean`]).
    pub fn allowed_in_clean(&self) -> bool {
        match &self.kind {
            Kind::Soi
            | Kind::Eoi
            | Kind::Dqt
            | Kind::Dht
            | Kind::Dri
            | Kind::Sof { .. }
            | Kind::Sos => true,
            Kind::App { n, id } => matches!(
                (n, id.as_str()),
                (0, "JFIF") | (1, "Exif") | (1, "XMP") | (2, "ICC_PROFILE") | (14, "Adobe")
            ),
            Kind::Com | Kind::Other { .. } => false,
        }
    }

    /// Bytes on disk (marker, length field, payload, scan data).
    pub fn bytes(&self) -> usize {
        match self.kind {
            Kind::Soi | Kind::Eoi => 2,
            _ => 4 + self.payload_len + self.scan_len,
        }
    }
}

/// The segment-level check of Clean Export output: reasons to refuse it, none when it passes.
pub fn check_clean(j: &Jpeg) -> Vec<String> {
    let mut why = vec![];
    let count = |id: &str| {
        j.segments
            .iter()
            .filter(|s| matches!(&s.kind, Kind::App { n: 1, id: x } if x == id))
            .count()
    };
    for s in &j.segments {
        if !s.allowed_in_clean() {
            why.push(format!(
                "segment {} at {} is not allowed",
                s.label(),
                s.offset
            ));
        }
    }
    if count("Exif") > 1 {
        why.push(format!(
            "{} Exif segments (multi-segment EXIF)",
            count("Exif")
        ));
    }
    if count("XMP") > 1 {
        why.push(format!("{} standard XMP segments", count("XMP")));
    }
    if j.trailer_len > 0 {
        why.push(format!(
            "{} bytes after the end of the image",
            j.trailer_len
        ));
    }
    why
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(m: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![0xFF, m];
        v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn minimal(extra_after_eoi: &[u8]) -> Vec<u8> {
        let mut b = vec![0xFF, 0xD8];
        b.extend(seg(0xE0, b"JFIF\0\x01\x01\0\0\x01\0\x01\0\0"));
        b.extend(seg(0xDB, &[0u8; 65]));
        b.extend(seg(0xC0, &[8, 0, 1, 0, 1, 1, 1, 0x11, 0]));
        b.extend(seg(0xDA, &[1, 1, 0, 0, 0x3F, 0]));
        b.extend_from_slice(&[0x12, 0xFF, 0x00, 0x34, 0xFF, 0xD0, 0x56]); // data, stuffing, RST0
        b.extend_from_slice(&[0xFF, 0xD9]);
        b.extend_from_slice(extra_after_eoi);
        b
    }

    #[test]
    fn scan_data_with_stuffing_and_restart_markers() {
        let j = parse(&minimal(b"")).unwrap();
        let sos = j.segments.iter().find(|s| s.kind == Kind::Sos).unwrap();
        assert_eq!(sos.scan_len, 7);
        assert!(check_clean(&j).is_empty());
        let total: usize = j.segments.iter().map(Segment::bytes).sum();
        assert_eq!(total, minimal(b"").len());
    }

    #[test]
    fn trailer_unknown_app_comment_and_second_exif_are_refused() {
        let mut b = minimal(b"SECRET");
        b.splice(2..2, seg(0xE5, b"MMTEST\0xyz"));
        b.splice(2..2, seg(0xFE, b"a comment"));
        b.splice(2..2, seg(0xE1, b"Exif\0\0a"));
        b.splice(2..2, seg(0xE1, b"Exif\0\0b"));
        b.splice(2..2, seg(0xEB, b"JP\0\x01jumb"));
        let j = parse(&b).unwrap();
        assert_eq!(j.trailer_len, 6);
        let why = check_clean(&j).join("\n");
        for part in [
            "APP5:unknown:MMTEST",
            "COM",
            "2 Exif",
            "APP11:JUMBF",
            "6 bytes after",
        ] {
            assert!(why.contains(part), "{part} in {why}");
        }
    }

    #[test]
    fn truncated_or_not_jpeg_is_an_error() {
        let b = minimal(b"");
        assert!(parse(&b[..b.len() - 3]).is_err());
        assert_eq!(parse(b"GIF89a....").unwrap_err(), ParseError::NotJpeg);
    }
}
