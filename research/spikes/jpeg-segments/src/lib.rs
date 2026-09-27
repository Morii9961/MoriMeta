//! S7 spike: JPEG marker-segment parser and segment-level whitelist for Clean Export output.
//!
//! Parses the complete marker structure (including entropy-coded data after each SOS, RSTn and
//! fill bytes) and reports every byte range, so that "what the file contains" is not limited to
//! what a metadata reader chooses to decode. Bytes after EOI are reported as a trailer.

use serde_json::{Value, json};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Kind {
    Soi,
    App { n: u8, id: String },
    Com,
    Dqt,
    Dht,
    Dri,
    Sof(u8),
    Sos,
    Eoi,
    Other(u8),
}

#[derive(Debug, Clone)]
pub struct Segment {
    pub offset: usize,
    pub marker: u8,
    pub kind: Kind,
    /// Payload length excluding marker and length field (for SOS: header only).
    pub payload_len: usize,
    /// For SOS: length of entropy-coded data that follows the header.
    pub scan_len: usize,
}

#[derive(Debug)]
pub struct Jpeg {
    pub segments: Vec<Segment>,
    /// Bytes after EOI (MPF secondary images, appended data, …).
    pub trailer_len: usize,
    pub trailer_offset: usize,
}

#[derive(Debug, PartialEq, Eq)]
pub enum ParseError {
    NotJpeg,
    Truncated(usize),
    BadMarker(usize),
}

/// Identifier strings used to classify APPn segments.
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
    for (pfx, name) in APP_IDS {
        if payload.starts_with(pfx) {
            // JUMBF is only meaningful in APP11
            if *name == "JUMBF" && n != 11 {
                continue;
            }
            return (*name).to_string();
        }
    }
    let printable: String = payload.iter().take(16).take_while(|b| **b != 0).map(|b| if b.is_ascii_graphic() { *b as char } else { '.' }).collect();
    format!("unknown:{printable}")
}

pub fn parse(b: &[u8]) -> Result<Jpeg, ParseError> {
    if b.len() < 4 || b[0] != 0xFF || b[1] != 0xD8 {
        return Err(ParseError::NotJpeg);
    }
    let mut segs = vec![Segment { offset: 0, marker: 0xD8, kind: Kind::Soi, payload_len: 0, scan_len: 0 }];
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
                segs.push(Segment { offset: off, marker: m, kind: Kind::Eoi, payload_len: 0, scan_len: 0 });
                return Ok(Jpeg { segments: segs, trailer_len: b.len() - i, trailer_offset: i });
            }
            0x01 | 0xD0..=0xD7 => return Err(ParseError::BadMarker(off)), // standalone markers are not valid here
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
            0xE0..=0xEF => Kind::App { n: m - 0xE0, id: app_id(m - 0xE0, payload) },
            0xFE => Kind::Com,
            0xDB => Kind::Dqt,
            0xC4 => Kind::Dht,
            0xDD => Kind::Dri,
            0xC0..=0xC3 | 0xC5..=0xC7 | 0xC9..=0xCB | 0xCD..=0xCF => Kind::Sof(m - 0xC0),
            0xDA => Kind::Sos,
            other => Kind::Other(other),
        };
        i += len;
        let mut scan_len = 0;
        if kind == Kind::Sos {
            // entropy-coded data: runs until a marker that is not 0xFF00 stuffing, RSTn or fill
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
        segs.push(Segment { offset: off, marker: m, kind, payload_len: len - 2, scan_len });
    }
}

impl Segment {
    pub fn label(&self) -> String {
        match &self.kind {
            Kind::App { n, id } => format!("APP{n}:{id}"),
            Kind::Sof(n) => format!("SOF{n}"),
            Kind::Other(m) => format!("marker:{m:02X}"),
            k => format!("{k:?}").to_uppercase(),
        }
    }
}

/// Segment-level whitelist for Clean Export output (candidate, SAFETY/METADATA v0.3 §S7).
/// Returns reasons for rejection; empty = pass.
pub fn check_clean(j: &Jpeg) -> Vec<String> {
    let mut why = vec![];
    let mut exif = 0;
    let mut xmp = 0;
    for s in &j.segments {
        let ok = match &s.kind {
            Kind::Soi | Kind::Eoi | Kind::Dqt | Kind::Dht | Kind::Dri | Kind::Sof(_) | Kind::Sos => true,
            Kind::App { n: 0, id } if id == "JFIF" => true,
            Kind::App { n: 1, id } if id == "Exif" => {
                exif += 1;
                true
            }
            Kind::App { n: 1, id } if id == "XMP" => {
                xmp += 1;
                true
            }
            Kind::App { n: 2, id } if id == "ICC_PROFILE" => true,
            Kind::App { n: 14, id } if id == "Adobe" => true,
            _ => false,
        };
        if !ok {
            why.push(format!("segment {} at {} not allowed", s.label(), s.offset));
        }
    }
    if exif > 1 {
        why.push(format!("{exif} Exif APP1 segments (multi-segment EXIF)"));
    }
    if xmp > 1 {
        why.push(format!("{xmp} standard XMP APP1 segments"));
    }
    if j.trailer_len > 0 {
        why.push(format!("{} bytes after EOI", j.trailer_len));
    }
    why
}

pub fn to_json(j: &Jpeg) -> Value {
    json!({
        "segments": j.segments.iter().map(|s| json!({"offset": s.offset, "label": s.label(), "payload_len": s.payload_len, "scan_len": s.scan_len})).collect::<Vec<_>>(),
        "trailer_len": j.trailer_len,
        "trailer_offset": j.trailer_offset,
    })
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
    fn parses_scan_with_stuffing_and_restart() {
        let j = parse(&minimal(b"")).unwrap();
        let sos = j.segments.iter().find(|s| s.kind == Kind::Sos).unwrap();
        assert_eq!(sos.scan_len, 7);
        assert!(check_clean(&j).is_empty());
    }

    #[test]
    fn reports_trailer_and_unknown_app() {
        let mut b = minimal(b"SECRET");
        let app5 = seg(0xE5, b"MMTEST\0xyz");
        b.splice(2..2, app5);
        let j = parse(&b).unwrap();
        assert_eq!(j.trailer_len, 6);
        let why = check_clean(&j);
        assert!(why.iter().any(|w| w.contains("APP5:unknown:MMTEST")));
        assert!(why.iter().any(|w| w.contains("after EOI")));
    }

    #[test]
    fn truncated_is_error() {
        let b = minimal(b"");
        assert!(parse(&b[..b.len() - 3]).is_err());
    }
}
