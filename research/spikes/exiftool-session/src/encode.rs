//! Argfile line encoding for `exiftool -stay_open True -@ -`.
//!
//! Empirical basis (13.59, research/results/s1/argfile-probe.json, exiftool.pl FilterArgfileLine):
//! * a plain line loses leading whitespace, trailing CR/LF, and ONE space directly after `=`;
//!   blank lines and lines starting with `#` are ignored;
//! * a `#[CSTR]` line decodes \n \r \t \\ \" exactly, but `$` and `@` always gain a backslash,
//!   so values containing `$`/`@` cannot be carried by CSTR.
//!
//! Strategy XML (preferred): write commands carry `-ex`; every value is XML-escaped, so every
//! line is a plain line and `$`/`@` survive.  Strategy CSTR is kept for comparison.

use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EncodeError {
    /// NUL cannot be represented in an argfile line.
    Nul,
    /// A raw CR or LF in a line that must stay a single argument (paths, option names).
    LineBreak,
    /// Character not representable in XMP/XML 1.0 (C0 controls other than TAB/LF/CR, U+FFFE, U+FFFF).
    XmlIllegal(char),
    /// Line would be ignored or altered by the plain-line filter (empty, leading whitespace or `#`).
    PlainUnsafe,
    /// CSTR cannot carry `$` or `@` exactly.
    CstrUnrepresentable(char),
}

impl fmt::Display for EncodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for EncodeError {}

pub fn is_xml_char(c: char) -> bool {
    matches!(c, '\t' | '\n' | '\r') || (c >= ' ' && c != '\u{FFFE}' && c != '\u{FFFF}')
}

/// A structural argument (option, tag name, absolute path) that must travel as one plain line.
pub fn plain_line(arg: &str) -> Result<String, EncodeError> {
    if arg.contains('\0') {
        return Err(EncodeError::Nul);
    }
    if arg.contains(['\r', '\n']) {
        return Err(EncodeError::LineBreak);
    }
    match arg.chars().next() {
        None => return Err(EncodeError::PlainUnsafe),
        Some(c) if c.is_whitespace() || c == '#' => return Err(EncodeError::PlainUnsafe),
        _ => {}
    }
    Ok(arg.to_owned())
}

/// Escape a tag value for a command that carries `-ex` (ExifTool applies UnescapeXML on write).
pub fn xml_value(v: &str) -> Result<String, EncodeError> {
    let mut out = String::with_capacity(v.len() + 8);
    for (i, c) in v.chars().enumerate() {
        if c == '\0' {
            return Err(EncodeError::Nul);
        }
        if !is_xml_char(c) {
            return Err(EncodeError::XmlIllegal(c));
        }
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            // exiftool.pl strips one space directly after '='
            ' ' if i == 0 => out.push_str("&#32;"),
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// `-<tag>=<value>` for a command that carries `-ex`.
pub fn assign_xml(tag: &str, value: &str) -> Result<String, EncodeError> {
    plain_line(&format!("-{tag}={}", xml_value(value)?))
}

/// `#[CSTR]-<tag>=<value>` encoding (comparison strategy; rejects `$` and `@`).
pub fn assign_cstr(tag: &str, value: &str) -> Result<String, EncodeError> {
    let mut out = format!("#[CSTR]-{tag}=");
    for c in value.chars() {
        match c {
            '\0' => return Err(EncodeError::Nul),
            '$' | '@' => return Err(EncodeError::CstrUnrepresentable(c)),
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if !is_xml_char(c) => return Err(EncodeError::XmlIllegal(c)),
            c => out.push(c),
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_value_escapes_structural_characters() {
        assert_eq!(xml_value(" a&b\n").unwrap(), "&#32;a&amp;b&#10;");
        assert_eq!(xml_value("$x@y").unwrap(), "$x@y");
        assert_eq!(xml_value("a b").unwrap(), "a b");
        assert!(matches!(xml_value("a\u{1}b"), Err(EncodeError::XmlIllegal(_))));
        assert_eq!(xml_value("a\0"), Err(EncodeError::Nul));
    }

    #[test]
    fn encoded_values_never_contain_line_breaks() {
        for v in ["\n-o\nC:/evil.jpg", "\r\n-execute\r\n", "x\n#[CSTR]y"] {
            let line = assign_xml("XMP-dc:Title", v).unwrap();
            assert!(!line.contains(['\n', '\r']));
        }
    }

    #[test]
    fn plain_line_rejects_unsafe_starts() {
        assert_eq!(plain_line(""), Err(EncodeError::PlainUnsafe));
        assert_eq!(plain_line("#x"), Err(EncodeError::PlainUnsafe));
        assert_eq!(plain_line(" x"), Err(EncodeError::PlainUnsafe));
        assert_eq!(plain_line("a\nb"), Err(EncodeError::LineBreak));
    }
}
