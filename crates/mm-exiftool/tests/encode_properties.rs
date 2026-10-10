// SPDX-License-Identifier: GPL-3.0-or-later
//! Property tests for the argfile encoding (ARCHITECTURE ADR-10, DEVELOPMENT_PLAN §5.1): whatever
//! text a value, a tag name or a path holds, it becomes at most one plain argfile line, and a value
//! that is accepted comes back unchanged after ExifTool's `-ex` unescaping. The round trip through
//! a real ExifTool is `engine.rs::values_round_trip_exactly_in_both_modes`.

use mm_exiftool::encode::{Line, TagName, ValueError, check_value, xml_value};
use proptest::prelude::*;
use std::path::Path;

/// What `-ex` does to a value: the five XML entities and decimal or hex character references.
fn unescape(s: &str) -> Option<String> {
    let mut out = String::new();
    let mut rest = s;
    while let Some(i) = rest.find('&') {
        out.push_str(&rest[..i]);
        let end = rest[i..].find(';')? + i;
        let ent = &rest[i + 1..end];
        let c = match ent {
            "amp" => '&',
            "lt" => '<',
            "gt" => '>',
            "quot" => '"',
            "apos" => '\'',
            _ => {
                let n = match ent.strip_prefix("#x") {
                    Some(h) => u32::from_str_radix(h, 16).ok()?,
                    None => ent.strip_prefix('#')?.parse().ok()?,
                };
                char::from_u32(n)?
            }
        };
        out.push(c);
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    Some(out)
}

fn plain(l: &Line) -> bool {
    let s = l.as_str();
    !s.is_empty()
        && !s.contains(['\r', '\n', '\0'])
        && !s.starts_with(|c: char| c.is_whitespace() || c == '#')
}

/// Text that is mostly what ExifTool and argfiles treat specially.
fn tricky() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        Just("\n".to_owned()),
        Just("\r\n".to_owned()),
        Just("\t".to_owned()),
        Just(" ".to_owned()),
        Just("-execute".to_owned()),
        Just("-o".to_owned()),
        Just("#[CSTR]".to_owned()),
        Just("{ready}".to_owned()),
        Just("{mm-end:1:0}".to_owned()),
        Just("${status}".to_owned()),
        Just("&amp;".to_owned()),
        Just("&#10;".to_owned()),
        Just("<>\"'&=|@$".to_owned()),
        Just("\u{0}\u{1}\u{1f}\u{7f}\u{85}".to_owned()),
        Just("\u{FFFE}\u{FFFF}\u{FEFF}".to_owned()),
        Just("森 Ünïcødé 🙂".to_owned()),
        "[a-zA-Z0-9]{1,4}",
    ];
    proptest::collection::vec(piece, 0..8).prop_map(|v| v.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn a_value_is_one_line_and_reads_back_exactly(v in prop_oneof![tricky(), any::<String>()]) {
        let tag = TagName::new("XMP-dc:Title").unwrap();
        match Line::assign(&tag, &v) {
            Err(e) => prop_assert_eq!(Err(e), check_value(&v)),
            Ok(line) => {
                prop_assert!(check_value(&v).is_ok());
                prop_assert!(plain(&line), "{:?}", line);
                let value = line.as_str().strip_prefix("-XMP-dc:Title=").unwrap();
                prop_assert_eq!(Some(value.to_owned()), xml_value(&v).ok());
                // exiftool.pl drops one space right after '=' on a plain line
                prop_assert!(!value.starts_with(' '));
                prop_assert_eq!(unescape(value), Some(v.clone()));
            }
        }
    }

    #[test]
    fn refused_values_are_exactly_the_unwritable_ones(v in prop_oneof![tricky(), any::<String>()]) {
        let expected = if v.is_empty() {
            Err(ValueError::Empty)
        } else if let Some(c) = v.chars().find(|&c| c == '\0' || ((c as u32) < 0x20 && !"\t\n\r".contains(c)) || c == '\u{FFFE}' || c == '\u{FFFF}') {
            Err(match c {
                '\0' => ValueError::Nul,
                '\u{FFFE}' | '\u{FFFF}' => ValueError::NonCharacter(c),
                c => ValueError::ControlChar(c),
            })
        } else {
            Ok(())
        };
        prop_assert_eq!(check_value(&v), expected);
    }

    #[test]
    fn tag_names_cannot_carry_an_option_or_a_value(s in prop_oneof![tricky(), "[-:a-zA-Z0-9_=. ]{0,12}", any::<String>()]) {
        if let Ok(t) = TagName::new(&s) {
            let n = t.as_str();
            prop_assert!(!n.starts_with(['-', ':']) && !n.ends_with(':') && !n.contains("::"));
            prop_assert!(n.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':')));
            for l in [Line::request(&t), Line::exclude(&t), Line::delete(&t)] {
                prop_assert!(plain(&l));
            }
        }
    }

    #[test]
    fn paths_are_absolute_single_lines(
        prefix in prop_oneof![Just(""), Just("C:\\"), Just("c:/"), Just("\\\\?\\C:\\"), Just("\\\\?\\UNC\\"), Just("\\\\"), Just("-")],
        rest in prop_oneof![tricky(), any::<String>()],
    ) {
        let s = format!("{prefix}{rest}");
        if let Ok(l) = Line::path(Path::new(&s)) {
            prop_assert!(plain(&l), "{:?}", l);
            let p = l.as_str();
            prop_assert!(!p.contains(['|', '\\']));
            let b = p.as_bytes();
            let drive = b.len() >= 3 && b[0].is_ascii_alphabetic() && b[1] == b':' && b[2] == b'/';
            prop_assert!(drive || (p.starts_with("//") && p.len() > 2), "{}", p);
        }
    }
}
