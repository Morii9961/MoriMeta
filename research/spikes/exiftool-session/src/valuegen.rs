//! Deterministic value generator for round-trip testing (seeded, reproducible).

pub struct SplitMix64(pub u64);

impl SplitMix64 {
    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
    pub fn below(&mut self, n: u64) -> u64 {
        self.next() % n
    }
    pub fn pick<'a, T>(&mut self, xs: &'a [T]) -> &'a T {
        &xs[self.below(xs.len() as u64) as usize]
    }
}

/// Character classes. `accepted` = the domain MoriMeta's input validation lets through.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Class {
    Ascii,
    Whitespace,
    XmlSpecial,
    ArgfileSpecial,
    Marker,
    Cjk,
    Emoji,
    Combining,
    Rtl,
    Invisible,
    C1,
    Del,
    // rejected domain
    C0,
    Nul,
    NonChar,
}

pub const ACCEPTED: &[Class] = &[
    Class::Ascii,
    Class::Whitespace,
    Class::XmlSpecial,
    Class::ArgfileSpecial,
    Class::Marker,
    Class::Cjk,
    Class::Emoji,
    Class::Combining,
    Class::Rtl,
    Class::Invisible,
    Class::C1,
    Class::Del,
];

pub const REJECTED: &[Class] = &[Class::C0, Class::Nul, Class::NonChar];

fn piece(rng: &mut SplitMix64, c: Class) -> String {
    match c {
        Class::Ascii => char::from(0x20 + rng.below(0x5F) as u8).to_string(),
        Class::Whitespace => rng.pick(&[" ", "  ", "\t", "\n", "\r", "\r\n", "\u{00A0}", "\u{3000}"]).to_string(),
        Class::XmlSpecial => rng.pick(&["&", "<", ">", "\"", "'", "&amp;", "&#10;", "]]>", "<![CDATA["]).to_string(),
        Class::ArgfileSpecial => rng
            .pick(&["#", "-", "=", "$", "@", "{", "}", "\\", "%", "#[CSTR]", "\\n", "\\\\", "$status", "${status}", "-=", "+=", "<", "^"])
            .to_string(),
        Class::Marker => rng
            .pick(&["{ready}", "{ready1}", "{ready42}\n", "{mm-end:1:0}", "\n{mm-end:7:0}\n", "-execute", "\n-execute\n", "-o", "\n-o\nC:/x.jpg", "-@", "\n-stay_open\nFalse\n", "-config"])
            .to_string(),
        Class::Cjk => char::from_u32(0x4E00 + rng.below(0x51A6) as u32).unwrap().to_string(),
        Class::Emoji => char::from_u32(0x1F300 + rng.below(0x350) as u32).unwrap_or('🌲').to_string(),
        Class::Combining => char::from_u32(0x0300 + rng.below(0x70) as u32).unwrap().to_string(),
        Class::Rtl => char::from_u32(0x05D0 + rng.below(0x1B) as u32).unwrap().to_string(),
        Class::Invisible => rng.pick(&["\u{200B}", "\u{FEFF}", "\u{2028}", "\u{2029}", "\u{200E}", "\u{202E}"]).to_string(),
        Class::C1 => char::from_u32(0x80 + rng.below(0x20) as u32).unwrap().to_string(),
        Class::Del => "\u{7F}".to_string(),
        Class::C0 => {
            let mut v;
            loop {
                v = rng.below(0x20) as u32;
                if !matches!(v, 0 | 9 | 10 | 13) {
                    break;
                }
            }
            char::from_u32(v).unwrap().to_string()
        }
        Class::Nul => "\0".to_string(),
        Class::NonChar => rng.pick(&["\u{FFFE}", "\u{FFFF}"]).to_string(),
    }
}

/// A random non-empty value composed of 1–12 pieces from `classes` (ASCII letters sprinkled in).
pub fn value(rng: &mut SplitMix64, classes: &[Class]) -> (String, Vec<Class>) {
    let n = 1 + rng.below(12) as usize;
    let mut s = String::new();
    let mut used = Vec::new();
    for _ in 0..n {
        let c = if rng.below(4) == 0 { Class::Ascii } else { *rng.pick(classes) };
        s.push_str(&piece(rng, c));
        if !used.contains(&c) {
            used.push(c);
        }
    }
    (s, used)
}
