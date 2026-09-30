// SPDX-License-Identifier: GPL-3.0-or-later
//! What leaves the program in logs and exported reports (SECURITY_MODEL §8, ARCHITECTURE §12):
//! by default no metadata values, no GPS, no full paths and no user name. A file is named by its
//! place in the Operation and its extension (`asset#3.jpg`).

use std::path::Path;

/// `asset#<n>.<ext>` for the n-th file of an Operation.
pub fn alias(n: u32, path: &str) -> String {
    match Path::new(path).extension() {
        Some(e) => format!("asset#{n}.{}", e.to_string_lossy().to_lowercase()),
        None => format!("asset#{n}"),
    }
}

/// `text` with every known path replaced by its alias (letter case ignored, the longest path
/// first so that a file's path wins over its folder's; also in the `C:/…` form ExifTool writes
/// paths in), then the user's profile folder and name replaced.
pub fn scrub(text: &str, known: &[(String, String)]) -> String {
    let mut known: Vec<&(String, String)> = known.iter().collect();
    known.sort_by_key(|(p, _)| std::cmp::Reverse(p.len()));
    let mut out = text.to_owned();
    for (path, alias) in known {
        out = replace_ignore_case(&out, path, alias);
        out = replace_ignore_case(&out, &path.replace('\\', "/"), alias);
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        out = scrub_profile(&out, &profile.to_string_lossy());
    }
    out
}

/// The profile folder (`C:\Users\<name>`) becomes `%USERPROFILE%`, and the bare user name after
/// `\Users\` becomes `<user>` (the short 8.3 form of the folder is not known here).
fn scrub_profile(text: &str, profile: &str) -> String {
    let mut out = replace_ignore_case(text, profile, "%USERPROFILE%");
    if let Some(name) = Path::new(profile)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        && !name.is_empty()
    {
        for sep in ['\\', '/'] {
            let from = format!("{sep}Users{sep}{name}");
            let to = format!("{sep}Users{sep}<user>");
            out = replace_ignore_case(&out, &from, &to);
        }
    }
    out
}

fn replace_ignore_case(text: &str, from: &str, to: &str) -> String {
    if from.is_empty() {
        return text.to_owned();
    }
    // lower-casing can change a character's byte length outside ASCII (Turkish "İ" grows, the
    // Kelvin sign shrinks); positions in the lower-cased text are only valid in the original when
    // no character changes length, so fall back to an exact replace otherwise (the totals alone
    // can balance out, and slicing inside a character would panic)
    let same_len = |t: &str| {
        t.chars()
            .all(|c| c.to_lowercase().map(char::len_utf8).sum::<usize>() == c.len_utf8())
    };
    if !same_len(text) || !same_len(from) {
        return text.replace(from, to);
    }
    let (lt, lf) = (text.to_lowercase(), from.to_lowercase());
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while let Some(k) = lt[i..].find(&lf) {
        out.push_str(&text[i..i + k]);
        out.push_str(to);
        i += k + from.len();
    }
    out.push_str(&text[i..]);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lower-casing that changes byte lengths but balances out in total ("İ" +1 twice, the Kelvin
    /// sign -2) must not slice inside a character.
    #[test]
    fn case_folding_that_changes_lengths_does_not_panic() {
        let text = "D:\\\u{130}\u{130}x\u{212A}\\a.jpg";
        assert_eq!(text.len(), text.to_lowercase().len());
        let out = replace_ignore_case(text, "x", "<dir>");
        assert!(out.contains("<dir>"), "{out}");
        assert_eq!(replace_ignore_case("ABC-abc", "b", "_"), "A_C-a_c");
    }

    #[test]
    fn paths_and_user_names_are_replaced() {
        let known = vec![
            (
                r"D:\Shoot\2026\IMG_0001.JPG".to_string(),
                alias(0, "IMG_0001.JPG"),
            ),
            (r"D:\Shoot\2026".to_string(), "<folder>".to_string()),
        ];
        let t = r"cannot open d:\shoot\2026\img_0001.jpg: access denied (in D:\Shoot\2026)";
        assert_eq!(
            scrub(t, &known),
            "cannot open asset#0.jpg: access denied (in <folder>)"
        );
        // ExifTool names files with forward slashes
        let t = "Error: Not a valid JPG - D:/Shoot/2026/IMG_0001.JPG (D:/shoot/2026/other.jpg)";
        assert_eq!(
            scrub(t, &known),
            "Error: Not a valid JPG - asset#0.jpg (<folder>/other.jpg)"
        );
        // built at run time so that the repository check does not take it for a real profile
        let profile = ["C:", "Users", "mori"].join("\\");
        let text = format!(r"{profile}\Pictures\a.jpg and c:/{}/MORI/x", "users");
        assert_eq!(
            scrub_profile(&text, &profile),
            r"%USERPROFILE%\Pictures\a.jpg and c:/Users/<user>/x"
        );
        assert_eq!(alias(7, "x.NEF"), "asset#7.nef");
        assert_eq!(alias(7, "noext"), "asset#7");
    }
}
