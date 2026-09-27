// SPDX-License-Identifier: GPL-3.0-or-later
//! Input validation for text values (METADATA_MODEL §3 "可接受的值").
//!
//! These are the characters that can be carried to ExifTool exactly (docs/SPIKE_REPORT.md §2):
//! no NUL, no C0 control characters except TAB/LF/CR in multi-line fields, no U+FFFE/U+FFFF.
//! An empty string is not a value: clearing a field is a separate, explicit action.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextKind {
    SingleLine,
    MultiLine,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TextError {
    Empty,
    /// Control character not allowed in this kind of field (reported with its position).
    ControlChar {
        ch: char,
        at: usize,
    },
    NonCharacter {
        ch: char,
        at: usize,
    },
}

impl std::fmt::Display for TextError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for TextError {}

pub fn validate_text(v: &str, kind: TextKind) -> Result<(), TextError> {
    if v.is_empty() {
        return Err(TextError::Empty);
    }
    for (at, ch) in v.chars().enumerate() {
        match ch {
            '\t' | '\n' | '\r' if kind == TextKind::MultiLine => {}
            c if (c as u32) < 0x20 => return Err(TextError::ControlChar { ch: c, at }),
            '\u{FFFE}' | '\u{FFFF}' => return Err(TextError::NonCharacter { ch, at }),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_line_rejects_breaks_and_controls() {
        assert!(validate_text("森 Morii © 2026", TextKind::SingleLine).is_ok());
        assert!(validate_text("$x @y ${status} &amp; <b>", TextKind::SingleLine).is_ok());
        assert_eq!(
            validate_text("a\nb", TextKind::SingleLine),
            Err(TextError::ControlChar { ch: '\n', at: 1 })
        );
        assert_eq!(
            validate_text("a\tb", TextKind::SingleLine),
            Err(TextError::ControlChar { ch: '\t', at: 1 })
        );
        assert_eq!(
            validate_text("", TextKind::SingleLine),
            Err(TextError::Empty)
        );
    }

    #[test]
    fn multi_line_allows_tab_lf_cr_only() {
        assert!(validate_text("line1\r\nline2\tx", TextKind::MultiLine).is_ok());
        assert!(matches!(
            validate_text("a\u{7}", TextKind::MultiLine),
            Err(TextError::ControlChar { .. })
        ));
        assert!(matches!(
            validate_text("a\0", TextKind::MultiLine),
            Err(TextError::ControlChar { .. })
        ));
        assert!(matches!(
            validate_text("\u{FFFE}", TextKind::MultiLine),
            Err(TextError::NonCharacter { .. })
        ));
    }
}
