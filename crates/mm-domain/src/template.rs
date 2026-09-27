// SPDX-License-Identifier: GPL-3.0-or-later
//! Template variables in edit values (PRODUCT_SPEC §6.9): `© {creator} {year}`. Evaluated per
//! file from its original snapshot (before any change of the same Plan). A missing value is never
//! silently replaced by an empty string: the file's change is blocked unless the template gives a
//! default (`{camera|Unknown}`). `{{` and `}}` are literal braces.

use std::path::Path;

use crate::capture;
use crate::creator;
use crate::plan::Target;
use crate::snapshot::Snapshot;

/// The MVP variables. `{index}` needs a sort key and is v1.
pub const VARIABLES: &[&str] = &[
    "year", "month", "day", "camera", "lens", "filename", "folder", "creator",
];

#[derive(Debug, Clone, PartialEq, Eq)]
enum Part {
    Text(String),
    Var {
        name: String,
        default: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Template(Vec<Part>);

impl Template {
    pub fn parse(s: &str) -> Result<Template, String> {
        let mut parts = Vec::new();
        let mut text = String::new();
        let mut chars = s.chars().peekable();
        while let Some(c) = chars.next() {
            match c {
                '{' if chars.peek() == Some(&'{') => {
                    chars.next();
                    text.push('{');
                }
                '}' if chars.peek() == Some(&'}') => {
                    chars.next();
                    text.push('}');
                }
                '{' => {
                    let mut inner = String::new();
                    loop {
                        match chars.next() {
                            Some('}') => break,
                            Some('{') | None => {
                                return Err(format!(
                                    "unclosed {{ in {s:?} (write {{{{ for a literal brace)"
                                ));
                            }
                            Some(ch) => inner.push(ch),
                        }
                    }
                    let (name, default) = match inner.split_once('|') {
                        Some((n, d)) => (n.trim(), Some(d.to_owned())),
                        None => (inner.trim(), None),
                    };
                    if !VARIABLES.contains(&name) {
                        return Err(format!(
                            "unknown variable {{{name}}}; available: {}",
                            VARIABLES
                                .iter()
                                .map(|v| format!("{{{v}}}"))
                                .collect::<Vec<_>>()
                                .join(" ")
                        ));
                    }
                    if !text.is_empty() {
                        parts.push(Part::Text(std::mem::take(&mut text)));
                    }
                    parts.push(Part::Var {
                        name: name.to_owned(),
                        default,
                    });
                }
                '}' => {
                    return Err(format!(
                        "unmatched }} in {s:?} (write }}}} for a literal brace)"
                    ));
                }
                c => text.push(c),
            }
        }
        if !text.is_empty() {
            parts.push(Part::Text(text));
        }
        Ok(Template(parts))
    }

    /// No variables: the same value for every file.
    pub fn is_literal(&self) -> bool {
        self.0.iter().all(|p| matches!(p, Part::Text(_)))
    }

    pub fn variables(&self) -> Vec<&str> {
        self.0
            .iter()
            .filter_map(|p| match p {
                Part::Var { name, .. } => Some(name.as_str()),
                Part::Text(_) => None,
            })
            .collect()
    }

    pub fn render(&self, ctx: &TemplateCtx) -> Result<String, String> {
        let mut out = String::new();
        for p in &self.0 {
            match p {
                Part::Text(t) => out.push_str(t),
                Part::Var { name, default } => match (ctx.get(name), default) {
                    (Some(v), _) => out.push_str(&v),
                    (None, Some(d)) => out.push_str(d),
                    (None, None) => {
                        return Err(format!(
                            "{{{name}}} has no value for this file (give a default: {{{name}|…}})"
                        ));
                    }
                },
            }
        }
        Ok(out)
    }
}

/// The values of the variables for one file.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TemplateCtx {
    pub capture: Option<(i32, u32, u32)>,
    pub camera: Option<String>,
    pub lens: Option<String>,
    /// File name without extension (for a RAW, the RAW's).
    pub filename: Option<String>,
    pub folder: Option<String>,
    pub creator: Option<String>,
}

fn nonempty(s: Option<String>) -> Option<String> {
    s.map(|v| v.trim().to_owned()).filter(|v| !v.is_empty())
}

impl TemplateCtx {
    /// From a target's original snapshot(s) and the path the user sees (the RAW for a sidecar).
    pub fn from_target(t: &Target, shown_path: &str) -> TemplateCtx {
        use chrono::Datelike;
        let image: &Snapshot = match t {
            Target::Embedded(s) => s,
            Target::Sidecar { raw, .. } => raw,
        };
        let p = Path::new(shown_path);
        TemplateCtx {
            capture: capture::read_target(t)
                .ok()
                .flatten()
                .map(|c| (c.local.year(), c.local.month(), c.local.day())),
            camera: nonempty(image.text("IFD0:Model")),
            lens: nonempty(image.text("ExifIFD:LensModel"))
                .or_else(|| nonempty(image.text("XMP-aux:Lens"))),
            filename: nonempty(p.file_stem().map(|s| s.to_string_lossy().into_owned())),
            folder: nonempty(
                p.parent()
                    .and_then(Path::file_name)
                    .map(|s| s.to_string_lossy().into_owned()),
            ),
            creator: creator::read_target(t)
                .effective
                .map(|v| v.join("; "))
                .filter(|v| !v.is_empty()),
        }
    }

    fn get(&self, name: &str) -> Option<String> {
        match name {
            "year" => self.capture.map(|(y, _, _)| format!("{y:04}")),
            "month" => self.capture.map(|(_, m, _)| format!("{m:02}")),
            "day" => self.capture.map(|(_, _, d)| format!("{d:02}")),
            "camera" => self.camera.clone(),
            "lens" => self.lens.clone(),
            "filename" => self.filename.clone(),
            "folder" => self.folder.clone(),
            "creator" => self.creator.clone(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> TemplateCtx {
        let snap = Snapshot::from_json(&json!({
            "ExifIFD:DateTimeOriginal": "2024:03:07 10:20:30",
            "IFD0:Model": "NIKON Z 8",
            "IFD0:Artist": "Morii",
            "XMP-aux:Lens": "NIKKOR Z 24-120mm f/4 S",
        }));
        TemplateCtx::from_target(&Target::Embedded(&snap), r"C:\photos\Hokkaido\DSC_0001.JPG")
    }

    #[test]
    fn renders_every_variable() {
        let t = Template::parse(
            "{year}-{month}-{day} {camera} / {lens} / {filename} in {folder} by {creator}",
        )
        .unwrap();
        assert_eq!(
            t.render(&ctx()).unwrap(),
            "2024-03-07 NIKON Z 8 / NIKKOR Z 24-120mm f/4 S / DSC_0001 in Hokkaido by Morii"
        );
        assert_eq!(t.variables().len(), 8);
    }

    #[test]
    fn missing_values_are_errors_unless_defaulted() {
        let empty = TemplateCtx::default();
        let t = Template::parse("© {creator} {year}").unwrap();
        assert!(t.render(&empty).unwrap_err().contains("{creator}"));
        let t = Template::parse("© {creator|Unknown} {year|2026}").unwrap();
        assert_eq!(t.render(&empty).unwrap(), "© Unknown 2026");
        // an empty default is allowed when the user writes it
        assert_eq!(
            Template::parse("{lens|}").unwrap().render(&empty).unwrap(),
            ""
        );
    }

    #[test]
    fn literal_braces_and_errors() {
        let t = Template::parse("{{not a var}} ©").unwrap();
        assert!(t.is_literal());
        assert_eq!(t.render(&TemplateCtx::default()).unwrap(), "{not a var} ©");
        assert!(Template::parse("{index}").unwrap_err().contains("unknown"));
        assert!(Template::parse("© {year").is_err());
        assert!(Template::parse("a } b").is_err());
        assert!(Template::parse("{ year }").is_ok());
    }

    #[test]
    fn a_raw_reads_its_sidecar_for_creator_and_time() {
        let raw = Snapshot::from_json(&json!({
            "ExifIFD:DateTimeOriginal": "2020:01:01 00:00:00",
            "IFD0:Model": "NIKON D850",
        }));
        let sc = Snapshot::from_json(&json!({
            "XMP-exif:DateTimeOriginal": "2021:02:03 04:05:06",
            "XMP-dc:Creator": "Mori",
        }));
        let c = TemplateCtx::from_target(
            &Target::Sidecar {
                raw: &raw,
                sidecar: Some(&sc),
            },
            r"C:\p\D850\a.NEF",
        );
        assert_eq!(c.capture, Some((2021, 2, 3)));
        assert_eq!(c.creator.as_deref(), Some("Mori"));
        assert_eq!(c.camera.as_deref(), Some("NIKON D850"));
        assert_eq!(c.filename.as_deref(), Some("a"));
    }
}
