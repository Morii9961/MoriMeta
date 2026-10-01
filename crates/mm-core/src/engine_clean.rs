// SPDX-License-Identifier: GPL-3.0-or-later
//! The two ExifTool commands of Clean Export (METADATA_MODEL §10.1): the full inventory of a file
//! (`-a -G0:1 -u -U`, every occurrence of a tag kept, plus the SHA-256 image data hash) and the
//! whitelist copy (`-all= -tagsFromFile @ -ICC_Profile <kept tags> -XMP-x:XMPToolkit= -o`).
//! The copy writes only a new file; the source is read, never written.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use mm_exiftool::{Command, Line, Output, TagName};
use serde::de::{Deserializer, MapAccess, SeqAccess, Visitor};

use crate::CoreError;
use crate::engine::{Engine, read_timeout, source_key, write_timeout};

/// One file's tags in the order ExifTool reports them, duplicates included.
struct Pairs(Vec<(String, serde_json::Value)>);

impl<'de> serde::Deserialize<'de> for Pairs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Pairs;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut m: A) -> Result<Pairs, A::Error> {
                let mut out = Vec::new();
                while let Some((k, v)) = m.next_entry::<String, serde_json::Value>()? {
                    out.push((k, v));
                }
                Ok(Pairs(out))
            }
        }
        d.deserialize_map(V)
    }
}

struct Files(Vec<Pairs>);

impl<'de> serde::Deserialize<'de> for Files {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Files;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("an array of objects")
            }
            fn visit_seq<A: SeqAccess<'de>>(self, mut s: A) -> Result<Files, A::Error> {
                let mut out = Vec::new();
                while let Some(p) = s.next_element::<Pairs>()? {
                    out.push(p);
                }
                Ok(Files(out))
            }
        }
        d.deserialize_seq(V)
    }
}

fn text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// A file's full inventory for Clean Export.
#[derive(Debug, Clone, Default)]
pub struct Inventory {
    /// `Group0:Group1:Tag` → value; a repeated key gets `#2`, `#3`…
    pub tags: BTreeMap<String, String>,
    /// SHA-256 of the image data.
    pub image_hash: Option<String>,
}

impl Engine {
    pub fn read_inventory(&mut self, path: &Path) -> Result<Inventory, CoreError> {
        let mut c = Command::read_json();
        for o in [
            "-a",
            "-G0:1",
            "-u",
            "-U",
            "-api",
            "RequestTags=ImageDataHash",
            "-api",
            "ImageHashType=SHA256",
        ] {
            c.push(Line::option(o));
        }
        c.push(Line::path(path).map_err(|e| CoreError::Input(format!("{}: {e}", path.display())))?);
        let bytes = std::fs::metadata(path).map(|m| m.len()).unwrap_or(0);
        let out = self.exec(&c, read_timeout(bytes))?;
        parse_inventory(&out, path)
    }

    /// `-all= -tagsFromFile @ -ICC_Profile <copy> -XMP-x:XMPToolkit= -o <out> <source>`: a new file
    /// with only the whitelisted tags. `out` must not exist (`-o` never overwrites).
    pub fn clean_copy(
        &mut self,
        source: &Path,
        out: &Path,
        copy: &[String],
    ) -> Result<Output, CoreError> {
        let mut c = Command::empty();
        for o in ["-all=", "-tagsFromFile", "@", "-ICC_Profile"] {
            c.push(Line::option(o));
        }
        for t in copy {
            let t = TagName::new(t).map_err(|e| CoreError::Internal(format!("tag {t}: {e}")))?;
            c.push(Line::request(&t));
        }
        c.push(Line::option("-XMP-x:XMPToolkit="));
        c.push(Line::option("-o"));
        let path =
            |p: &Path| Line::path(p).map_err(|e| CoreError::Input(format!("{}: {e}", p.display())));
        c.push(path(out)?);
        c.push(path(source)?);
        let bytes = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
        self.exec(&c, write_timeout(bytes))
    }
}

fn parse_inventory(out: &Output, path: &Path) -> Result<Inventory, CoreError> {
    let files: Files = serde_json::from_slice(&out.stdout)
        .map_err(|e| CoreError::Engine(format!("unparsable ExifTool output: {e}")))?;
    let key = source_key(path);
    let pairs = files
        .0
        .into_iter()
        .find(|p| {
            p.0.iter()
                .any(|(k, v)| k == "SourceFile" && Some(text(v)) == key)
        })
        .ok_or_else(|| {
            CoreError::Engine(format!("no metadata result ({})", out.stderr_text().trim()))
        })?;
    let mut inv = Inventory::default();
    for (k, v) in pairs.0 {
        if k == "SourceFile" {
            continue;
        }
        if k.ends_with(":ImageDataHash") {
            inv.image_hash = Some(text(&v));
            continue;
        }
        let mut name = k.clone();
        let mut n = 2;
        while inv.tags.contains_key(&name) {
            name = format!("{k}#{n}");
            n += 1;
        }
        inv.tags.insert(name, text(&v));
    }
    Ok(inv)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_tags_are_all_kept() {
        let out = Output {
            stdout: br#"[{"SourceFile":"C:/a.jpg","EXIF:IFD0:Make":"A","EXIF:IFD0:Make":"B","Composite:ImageDataHash":"abc","XMP:XMP-dc:Subject":["x","y"]}]"#.to_vec(),
            stderr: vec![],
            status: 0,
            elapsed: std::time::Duration::ZERO,
        };
        let inv = parse_inventory(&out, Path::new("C:\\a.jpg")).unwrap();
        assert_eq!(inv.tags["EXIF:IFD0:Make"], "A");
        assert_eq!(inv.tags["EXIF:IFD0:Make#2"], "B");
        assert_eq!(inv.tags["XMP:XMP-dc:Subject"], r#"["x","y"]"#);
        assert_eq!(inv.image_hash.as_deref(), Some("abc"));
    }
}
