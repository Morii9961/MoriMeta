//! Windows-1252: the character set ExifTool assumes for IPTC without CodedCharacterSet [F-20].

/// The 27 characters that Windows-1252 maps into 0x80–0x9F (5 positions are undefined).
const HIGH: [char; 27] = [
    '€', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', 'Ž', '‘', '’', '“', '”', '•', '–',
    '—', '˜', '™', 'š', '›', 'œ', 'ž', 'Ÿ',
];

pub fn encodable(c: char) -> bool {
    let u = c as u32;
    u < 0x80 || (0xA0..=0xFF).contains(&u) || HIGH.contains(&c)
}

#[cfg(test)]
mod tests {
    use super::encodable;

    #[test]
    fn latin_and_specials() {
        for c in ['A', 'é', 'ß', '©', '€', '—', 'Ÿ'] {
            assert!(encodable(c), "{c}");
        }
        for c in ['森', 'Ł', 'α', '\u{81}', '🌲'] {
            assert!(!encodable(c), "{c}");
        }
    }
}
