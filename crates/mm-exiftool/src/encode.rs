// SPDX-License-Identifier: GPL-3.0-or-later
//! Argfile encoding for `exiftool -stay_open True -@ -` (ARCHITECTURE ADR-10).
//!
//! Every argument travels as one *plain* argfile line. Tag values are XML-escaped and the
//! command carries `-ex`, so ExifTool unescapes them on write. `#[CSTR]` lines are never used:
//! exiftool.pl adds a backslash before `$` and `@` in them (docs/SPIKE_REPORT.md §2.1).
//!
//! A [`Line`] can only be built through the constructors below, so no unvalidated string can
//! reach the ExifTool process.

use std::fmt;
use std::path::Path;

/// Why a tag value cannot be written exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ValueError {
    /// NUL cannot be carried by an argfile line.
    Nul,
    /// C0 control character other than TAB/LF/CR (ExifTool stores these as `.` in XMP).
    ControlChar(char),
    /// U+FFFE / U+FFFF are not XML characters.
    NonCharacter(char),
    /// An empty value means "delete the tag" to ExifTool; deletion must be requested explicitly.
    Empty,
}

/// Why a tag name or path cannot be used as an argument.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ArgError {
    InvalidTagName(String),
    RelativePath(String),
    /// Paths containing CR, LF, NUL or `|` are refused (line splitting; CVE-2022-23935 pattern).
    ForbiddenPathChar(String),
    NonUnicodePath,
}

impl fmt::Display for ValueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl fmt::Display for ArgError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ValueError {}
impl std::error::Error for ArgError {}

/// Characters MoriMeta can write exactly through `-ex` (XML 1.0 Char minus U+FFFE/U+FFFF).
pub fn check_value(v: &str) -> Result<(), ValueError> {
    if v.is_empty() {
        return Err(ValueError::Empty);
    }
    for c in v.chars() {
        match c {
            '\0' => return Err(ValueError::Nul),
            '\t' | '\n' | '\r' => {}
            c if (c as u32) < 0x20 => return Err(ValueError::ControlChar(c)),
            '\u{FFFE}' | '\u{FFFF}' => return Err(ValueError::NonCharacter(c)),
            _ => {}
        }
    }
    Ok(())
}

/// XML-escape a value for a command that carries `-ex`.
pub fn xml_value(v: &str) -> Result<String, ValueError> {
    check_value(v)?;
    let mut out = String::with_capacity(v.len() + 8);
    for (i, c) in v.chars().enumerate() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            '\t' => out.push_str("&#9;"),
            '\n' => out.push_str("&#10;"),
            '\r' => out.push_str("&#13;"),
            // exiftool.pl removes one space directly after '=' on plain lines
            ' ' if i == 0 => out.push_str("&#32;"),
            _ => out.push(c),
        }
    }
    Ok(out)
}

/// A validated ExifTool tag reference such as `XMP-dc:Creator`, `IFD0:Artist`,
/// `XMP-dc:Rights-x-default` or `ExifIFD:DateTimeOriginal`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct TagName(String);

impl TagName {
    pub fn new(name: &str) -> Result<TagName, ArgError> {
        let ok = !name.is_empty()
            && name.len() <= 128
            && !name.starts_with(['-', ':'])
            && !name.ends_with(':')
            && !name.contains("::")
            && name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | ':'));
        if ok {
            Ok(TagName(name.to_owned()))
        } else {
            Err(ArgError::InvalidTagName(name.to_owned()))
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One argfile line. Invariant: non-empty, no CR/LF/NUL, does not start with whitespace or `#`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line(String);

impl Line {
    /// A fixed option or option argument known at compile time (e.g. `-json`, `StructFormat=JSONQ`).
    pub fn option(s: &'static str) -> Line {
        assert!(Self::plain_ok(s), "invalid static option line: {s:?}");
        Line(s.to_owned())
    }

    /// `-<tag>=<value>` (requires the command to carry `-ex`, see [`Command::write`]).
    pub fn assign(tag: &TagName, value: &str) -> Result<Line, ValueError> {
        Ok(Line(format!("-{}={}", tag.as_str(), xml_value(value)?)))
    }

    /// `-<tag>=` : delete the tag.
    pub fn delete(tag: &TagName) -> Line {
        Line(format!("-{}=", tag.as_str()))
    }

    /// `-<tag>` : request a tag when reading.
    pub fn request(tag: &TagName) -> Line {
        Line(format!("-{}", tag.as_str()))
    }

    /// `--<tag>` : exclude a tag (e.g. from `-tagsFromFile` copies).
    pub fn exclude(tag: &TagName) -> Line {
        Line(format!("--{}", tag.as_str()))
    }

    /// An absolute path, forward slashes, verbatim prefix removed.
    pub fn path(p: &Path) -> Result<Line, ArgError> {
        let s = p
            .to_str()
            .ok_or(ArgError::NonUnicodePath)?
            .replace('\\', "/");
        let s = s
            .strip_prefix("//?/UNC/")
            .map(|r| format!("//{r}"))
            .or_else(|| s.strip_prefix("//?/").map(str::to_owned))
            .unwrap_or(s);
        if s.contains(['\r', '\n', '\0', '|']) {
            return Err(ArgError::ForbiddenPathChar(s));
        }
        let bytes = s.as_bytes();
        let drive_abs = bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'/';
        let unc = s.starts_with("//") && s.len() > 2;
        if !(drive_abs || unc) {
            return Err(ArgError::RelativePath(s));
        }
        Ok(Line(s))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    fn plain_ok(s: &str) -> bool {
        !s.is_empty()
            && !s.contains(['\r', '\n', '\0'])
            && !s.starts_with(|c: char| c.is_whitespace() || c == '#')
    }
}

/// A command ready to be sent: argfile lines in order.
#[derive(Debug, Clone, Default)]
pub struct Command {
    lines: Vec<Line>,
}

impl Command {
    /// A read command: `-json -api StructFormat=JSONQ` (all values quoted, SPIKE_REPORT §2.2).
    pub fn read_json() -> Command {
        let mut c = Command::default();
        c.push(Line::option("-json"));
        c.push(Line::option("-api"));
        c.push(Line::option("StructFormat=JSONQ"));
        c
    }

    /// A write command: starts with `-ex` so that [`Line::assign`] values are unescaped.
    pub fn write() -> Command {
        let mut c = Command::default();
        c.push(Line::option("-ex"));
        c
    }

    pub fn empty() -> Command {
        Command::default()
    }

    pub fn push(&mut self, l: Line) -> &mut Command {
        self.lines.push(l);
        self
    }

    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    /// Render lines followed by the terminator for `id`. Re-asserts the line invariant
    /// (defence in depth: nothing that could split a line may reach stdin).
    pub(crate) fn render(&self, id: u64) -> Vec<u8> {
        let mut out = String::new();
        for l in &self.lines {
            assert!(Line::plain_ok(&l.0), "line invariant violated");
            out.push_str(&l.0);
            out.push('\n');
        }
        out.push_str(&format!(
            "-echo4\n{{mm-end:{id}:${{status}}}}\n-execute{id}\n"
        ));
        out.into_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_escape_structural_characters() {
        assert_eq!(xml_value(" a&b\n").unwrap(), "&#32;a&amp;b&#10;");
        assert_eq!(xml_value("$x@y ${status}").unwrap(), "$x@y ${status}");
        assert_eq!(xml_value("a b").unwrap(), "a b");
        assert_eq!(xml_value("\u{7f}\u{85}森").unwrap(), "\u{7f}\u{85}森");
    }

    #[test]
    fn values_outside_the_domain_are_refused() {
        assert_eq!(xml_value(""), Err(ValueError::Empty));
        assert_eq!(xml_value("a\0"), Err(ValueError::Nul));
        assert_eq!(xml_value("a\u{1}"), Err(ValueError::ControlChar('\u{1}')));
        assert_eq!(
            xml_value("\u{FFFF}"),
            Err(ValueError::NonCharacter('\u{FFFF}'))
        );
    }

    #[test]
    fn assigned_lines_never_split() {
        let t = TagName::new("XMP-dc:Title").unwrap();
        for v in [
            "\n-o\nC:/evil.jpg",
            "\r\n-execute\r\n",
            "x\n#[CSTR]y",
            "\n{ready1}\n",
        ] {
            let l = Line::assign(&t, v).unwrap();
            assert!(!l.as_str().contains(['\n', '\r']));
        }
    }

    #[test]
    fn tag_names_are_restricted() {
        assert!(TagName::new("XMP-dc:Rights-x-default").is_ok());
        assert!(TagName::new("IFD0:Artist").is_ok());
        for bad in [
            "",
            "-o",
            "a b",
            "a=b",
            "a\nb",
            "XMP:dc<Title",
            "a::b",
            ":a",
            "a:",
        ] {
            assert!(TagName::new(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn paths_must_be_absolute_and_clean() {
        assert_eq!(
            Line::path(Path::new(r"C:\a\-o.jpg")).unwrap().as_str(),
            "C:/a/-o.jpg"
        );
        assert_eq!(
            Line::path(Path::new(r"\\?\C:\a\#b.jpg")).unwrap().as_str(),
            "C:/a/#b.jpg"
        );
        assert_eq!(
            Line::path(Path::new(r"\\?\UNC\nas\share\x.jpg"))
                .unwrap()
                .as_str(),
            "//nas/share/x.jpg"
        );
        assert!(matches!(
            Line::path(Path::new("-o.jpg")),
            Err(ArgError::RelativePath(_))
        ));
        assert!(matches!(
            Line::path(Path::new("C:/a|")),
            Err(ArgError::ForbiddenPathChar(_))
        ));
    }

    #[test]
    fn render_appends_terminator() {
        let mut c = Command::read_json();
        c.push(Line::option("-ver"));
        let r = String::from_utf8(c.render(42)).unwrap();
        assert!(r.ends_with("-echo4\n{mm-end:42:${status}}\n-execute42\n"));
    }
}
