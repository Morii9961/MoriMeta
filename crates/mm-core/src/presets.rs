// SPDX-License-Identifier: GPL-3.0-or-later
//! The user's Presets (PRODUCT_SPEC §6.11): create, edit, duplicate, delete, export, import, apply.
//! Stored as their JSON (schema_version 1); every save and import is validated. The built-in
//! Presets are not stored and cannot be changed, only duplicated.

use mm_domain::rules::{self, Field, Preset};
use mm_store::Store;

use crate::{CoreError, new_id};

#[derive(Debug, Clone)]
pub struct PresetInfo {
    /// `builtin:<name>` for the built-in ones.
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub fields: Vec<Field>,
    pub last_used_ms: Option<i64>,
    pub preset: Preset,
}

const BUILTIN: &str = "builtin:";

pub fn list(store: &Store) -> Result<Vec<PresetInfo>, CoreError> {
    let mut out: Vec<PresetInfo> = rules::builtin()
        .into_iter()
        .map(|p| PresetInfo {
            id: format!("{BUILTIN}{}", p.name),
            name: p.name.clone(),
            builtin: true,
            fields: p.fields(),
            last_used_ms: None,
            preset: p,
        })
        .collect();
    for r in store.presets()? {
        // a stored Preset that no longer validates is listed by name, never applied
        let p = Preset::from_json(&r.json)
            .map_err(|e| CoreError::Input(format!("stored preset {:?}: {e}", r.name)))?;
        out.push(PresetInfo {
            id: r.id,
            name: r.name,
            builtin: false,
            fields: p.fields(),
            last_used_ms: r.last_used_ms,
            preset: p,
        });
    }
    Ok(out)
}

pub fn get(store: &Store, id: &str) -> Result<PresetInfo, CoreError> {
    list(store)?
        .into_iter()
        .find(|p| p.id == id)
        .ok_or_else(|| CoreError::Input(format!("no preset {id}")))
}

/// Save a new Preset, or replace the one with `id`; returns its id.
pub fn save(store: &mut Store, id: Option<&str>, preset: &Preset) -> Result<String, CoreError> {
    preset.validate().map_err(CoreError::Input)?;
    let id = match id {
        Some(i) if i.starts_with(BUILTIN) => {
            return Err(CoreError::Input(
                "built-in presets cannot be changed; duplicate it first".into(),
            ));
        }
        Some(i) => {
            if !store.presets()?.iter().any(|r| r.id == i) {
                return Err(CoreError::Input(format!("no preset {i}")));
            }
            i.to_owned()
        }
        None => new_id("preset")?,
    };
    store.save_preset(&id, &preset.name, &preset.to_json())?;
    Ok(id)
}

/// Import from a file's text (schema-checked, PRODUCT_SPEC §6.11): always a new Preset.
pub fn import(store: &mut Store, json: &str) -> Result<String, CoreError> {
    let p = Preset::from_json(json.trim_start_matches('\u{feff}')).map_err(CoreError::Input)?;
    save(store, None, &p)
}

pub fn duplicate(store: &mut Store, id: &str) -> Result<String, CoreError> {
    let mut p = get(store, id)?.preset;
    p.name = format!("{} (copy)", p.name);
    save(store, None, &p)
}

pub fn delete(store: &mut Store, id: &str) -> Result<(), CoreError> {
    if id.starts_with(BUILTIN) {
        return Err(CoreError::Input(
            "built-in presets cannot be deleted".into(),
        ));
    }
    if !store.delete_preset(id)? {
        return Err(CoreError::Input(format!("no preset {id}")));
    }
    Ok(())
}

/// Note that a Preset was applied (its "last used" time).
pub fn used(store: &mut Store, id: &str) -> Result<(), CoreError> {
    if !id.starts_with(BUILTIN) {
        store.touch_preset(id)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_duplicate_import_delete() {
        let d = std::env::temp_dir().join(format!("mm-presets-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut s = Store::open(&d).unwrap();
        assert_eq!(list(&s).unwrap().len(), rules::builtin().len());
        let copy = duplicate(&mut s, "builtin:Remove GPS").unwrap();
        let p = get(&s, &copy).unwrap();
        assert_eq!(p.name, "Remove GPS (copy)");
        assert!(!p.builtin && p.last_used_ms.is_none());

        let mut edited = p.preset.clone();
        edited.name = "No GPS".into();
        assert_eq!(save(&mut s, Some(&copy), &edited).unwrap(), copy);
        assert_eq!(get(&s, &copy).unwrap().name, "No GPS");
        used(&mut s, &copy).unwrap();
        assert!(get(&s, &copy).unwrap().last_used_ms.is_some());

        assert!(save(&mut s, Some("builtin:Remove GPS"), &edited).is_err());
        assert!(delete(&mut s, "builtin:Remove GPS").is_err());
        assert!(import(&mut s, r#"{"schema_version":1,"name":"x","rules":[]}"#).is_err());
        let imported = import(&mut s, &format!("\u{feff}{}", edited.to_json())).unwrap();
        assert_ne!(imported, copy);
        delete(&mut s, &copy).unwrap();
        assert!(delete(&mut s, &copy).is_err());
        assert_eq!(list(&s).unwrap().len(), rules::builtin().len() + 1);
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }
}
