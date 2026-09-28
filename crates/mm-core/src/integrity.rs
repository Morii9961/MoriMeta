// SPDX-License-Identifier: GPL-3.0-or-later
//! Integrity of the bundled ExifTool (SECURITY_MODEL §5): a manifest of every file of the
//! package (relative path → BLAKE3), made when the package is built and checked when the app
//! starts — the key files before ExifTool first runs, all files afterwards. Any mismatch
//! disables writing (`service::OperationGate::refuse_writes`) and asks for a reinstall.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::CoreError;

/// The manifest's name inside a package folder.
pub const MANIFEST_NAME: &str = "exiftool.manifest";

/// Checked before ExifTool first runs: the interpreter, its runtime, the script and the core
/// module. Top-level programs (the launcher) listed in the manifest are key files too.
pub const KEY_FILES: &[&str] = &[
    "exiftool_files/perl.exe",
    "exiftool_files/perl532.dll",
    "exiftool_files/exiftool.pl",
    "exiftool_files/lib/Image/ExifTool.pm",
];

/// Perl loads code from here: a file that is not in the manifest is a problem too.
const CODE_DIR: &str = "exiftool_files/";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Manifest {
    pub version: String,
    /// Relative path with `/` → BLAKE3 hex.
    pub files: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Problem {
    Missing(String),
    Changed(String),
    /// Not in the manifest, in the folder Perl loads code from.
    Added(String),
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Problem::Missing(p) => write!(f, "missing: {p}"),
            Problem::Changed(p) => write!(f, "changed: {p}"),
            Problem::Added(p) => write!(f, "not part of the package: {p}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    /// [`KEY_FILES`] and the top-level programs: fast, before ExifTool first runs.
    Key,
    /// Every file, and nothing added to the code folder.
    All,
}

fn files_under(dir: &Path) -> Result<Vec<String>, CoreError> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)? {
            let e = e?;
            let t = e.file_type()?;
            if t.is_dir() {
                stack.push(e.path());
            } else {
                let path = e.path();
                let rel = path.strip_prefix(dir).map_err(|_| {
                    CoreError::Internal(format!("{} outside the package", path.display()))
                })?;
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    Ok(out)
}

fn hash(dir: &Path, rel: &str) -> Option<String> {
    mm_fs::hash_path(&dir.join(rel))
        .ok()
        .map(|h| mm_fs::hex(&h))
}

/// The manifest of the package in `dir` (made by the build, after the download's SHA-256 check).
pub fn generate(dir: &Path, version: &str) -> Result<Manifest, CoreError> {
    let mut files = BTreeMap::new();
    for rel in files_under(dir)? {
        if rel == MANIFEST_NAME {
            continue;
        }
        let h = hash(dir, &rel).ok_or_else(|| CoreError::Internal(format!("cannot read {rel}")))?;
        files.insert(rel, h);
    }
    Ok(Manifest {
        version: version.to_owned(),
        files,
    })
}

/// What differs between the package in `dir` and its manifest; empty when it is intact.
pub fn check(dir: &Path, m: &Manifest, scope: Scope) -> Result<Vec<Problem>, CoreError> {
    let key = |rel: &str| KEY_FILES.contains(&rel) || !rel.contains('/');
    let mut problems = Vec::new();
    if scope == Scope::Key {
        for k in KEY_FILES {
            if !m.files.contains_key(*k) {
                problems.push(Problem::Missing(format!("{k} (not in the manifest)")));
            }
        }
    }
    for (rel, want) in m
        .files
        .iter()
        .filter(|(r, _)| scope == Scope::All || key(r))
    {
        match hash(dir, rel) {
            None => problems.push(Problem::Missing(rel.clone())),
            Some(h) if &h != want => problems.push(Problem::Changed(rel.clone())),
            Some(_) => {}
        }
    }
    if scope == Scope::All {
        for rel in files_under(dir)? {
            if rel.starts_with(CODE_DIR) && !m.files.contains_key(&rel) {
                problems.push(Problem::Added(rel));
            }
        }
    }
    Ok(problems)
}

/// Read `<dir>/exiftool.manifest` (or `manifest`) and check `scope`; the problems as one text.
pub fn check_package(dir: &Path, manifest: &Path, scope: Scope) -> Result<(), String> {
    let text = std::fs::read_to_string(manifest)
        .map_err(|e| format!("ExifTool manifest {}: {e}", manifest.display()))?;
    let m: Manifest =
        serde_json::from_str(&text).map_err(|e| format!("ExifTool manifest unreadable: {e}"))?;
    let problems = check(dir, &m, scope).map_err(|e| e.to_string())?;
    if problems.is_empty() {
        return Ok(());
    }
    let shown: Vec<String> = problems.iter().take(5).map(Problem::to_string).collect();
    Err(format!(
        "the bundled ExifTool {} is not as shipped ({} problem(s): {}); writing is disabled, reinstall MoriMeta",
        m.version,
        problems.len(),
        shown.join("; ")
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn package(d: &Path) {
        for (rel, body) in [
            ("exiftool(-k).exe", "launcher"),
            ("README.txt", "readme"),
            ("exiftool_files/perl.exe", "perl"),
            ("exiftool_files/perl532.dll", "dll"),
            ("exiftool_files/exiftool.pl", "script"),
            ("exiftool_files/lib/Image/ExifTool.pm", "core"),
            ("exiftool_files/lib/Image/ExifTool/XMP.pm", "xmp"),
        ] {
            let p = d.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, body).unwrap();
        }
    }

    #[test]
    fn changes_are_found_by_scope() {
        let d = std::env::temp_dir().join(format!("mm-integrity-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        package(&d);
        let m = generate(&d, "13.59").unwrap();
        assert_eq!(m.files.len(), 7);
        assert!(
            m.files
                .contains_key("exiftool_files/lib/Image/ExifTool/XMP.pm")
        );
        assert_eq!(check(&d, &m, Scope::All).unwrap(), []);

        // a module that is not a key file: only the full check sees it
        std::fs::write(d.join("exiftool_files/lib/Image/ExifTool/XMP.pm"), "evil").unwrap();
        assert_eq!(check(&d, &m, Scope::Key).unwrap(), []);
        assert_eq!(
            check(&d, &m, Scope::All).unwrap(),
            [Problem::Changed(
                "exiftool_files/lib/Image/ExifTool/XMP.pm".into()
            )]
        );
        std::fs::write(d.join("exiftool_files/lib/Image/ExifTool/XMP.pm"), "xmp").unwrap();

        // key files and the launcher are checked before ExifTool runs
        std::fs::write(d.join("exiftool_files/perl532.dll"), "patched").unwrap();
        std::fs::remove_file(d.join("exiftool(-k).exe")).unwrap();
        assert_eq!(
            check(&d, &m, Scope::Key).unwrap(),
            [
                Problem::Missing("exiftool(-k).exe".into()),
                Problem::Changed("exiftool_files/perl532.dll".into())
            ]
        );
        package(&d);

        // code added where Perl looks for modules; an extra file at the top is not code
        std::fs::write(d.join("exiftool_files/lib/Image/ExifTool/Evil.pm"), "1;").unwrap();
        std::fs::write(d.join("exiftool.exe"), "a copy of the launcher").unwrap();
        assert_eq!(
            check(&d, &m, Scope::All).unwrap(),
            [Problem::Added(
                "exiftool_files/lib/Image/ExifTool/Evil.pm".into()
            )]
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
