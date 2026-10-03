// SPDX-License-Identifier: GPL-3.0-or-later
//! Startup journal repair is previewed in isolation. Original DB/WAL/SHM are preserved first.
//! A flushed marker blocks ordinary startup until replacement completes or is resumed.
use crate::errors::ue;
use mm_core::service::InstanceLock;
use mm_store::Store;
use serde::{Deserialize, Serialize};
use std::io::Write;
use std::path::{Path, PathBuf};

const DATABASE: &str = "morimeta.sqlite";
const FILES: [&str; 3] = [DATABASE, "morimeta.sqlite-wal", "morimeta.sqlite-shm"];
const MARKER: &str = "journal-repair.pending.json";

#[derive(Clone, Serialize, Deserialize)]
struct Commit {
    stage: String,
    original: [Option<String>; 3],
    candidate: String,
}
struct Prepared {
    commit: Commit,
    imported: Vec<String>,
    skipped: Vec<(String, String)>,
}

fn safe_path(path: &Path) -> Result<(), String> {
    if !path.is_absolute() {
        return Err("journal repair needs an absolute local path".into());
    }
    let mut part = PathBuf::new();
    for component in path.components() {
        part.push(component);
        if !part.exists() {
            continue;
        }
        let p = mm_fs::probe(&part).map_err(ue)?;
        if p.reparse_point || p.cloud_placeholder {
            return Err(format!(
                "journal repair does not read links or cloud files: {}",
                part.display()
            ));
        }
    }
    Ok(())
}

fn hash(path: &Path) -> Result<Option<String>, String> {
    safe_path(path)?;
    match std::fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ue(e)),
        Ok(meta) => {
            if !meta.is_file() {
                return Err("a journal repair input is not a regular file".into());
            }
            if mm_fs::probe(path).map_err(ue)?.links > 1 {
                return Err("journal repair does not replace multiply linked files".into());
            }
            Ok(Some(mm_fs::hex(&mm_fs::hash_path(path).map_err(ue)?)))
        }
    }
}

fn roots(data: &Path) -> Result<Vec<PathBuf>, String> {
    let mut roots = vec![data.join("backups")];
    let registry = data.join(mm_store::BACKUP_LOCATIONS);
    safe_path(&registry)?;
    if registry.exists() {
        if std::fs::metadata(&registry).map_err(ue)?.len() > 65_536 {
            return Err("the backup location registry is too large".into());
        }
        roots.extend(
            std::fs::read_to_string(registry)
                .map_err(ue)?
                .lines()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(PathBuf::from),
        );
    }
    roots.sort();
    roots.dedup();
    Ok(roots)
}

fn inspect_roots(roots: &[PathBuf]) -> Result<bool, String> {
    let mut any = false;
    for root in roots {
        safe_path(root)?;
        if !root.exists() {
            continue;
        }
        for entry in std::fs::read_dir(root).map_err(ue)? {
            let path = entry.map_err(ue)?.path();
            safe_path(&path)?;
            if !path.is_dir() {
                continue;
            }
            for name in [mm_store::MANIFEST_LOG, mm_store::PLAN_FILE] {
                safe_path(&path.join(name))?;
            }
            any |= path.join(mm_store::MANIFEST_LOG).is_file();
        }
    }
    Ok(any)
}

fn prepare(data: &Path, sources: &[PathBuf]) -> Result<Prepared, String> {
    safe_path(data)?;
    inspect_roots(sources)?;
    let token = mm_fs::random_token().map_err(ue)?;
    let stage = format!("journal-repair-{token}");
    let dir = data.join(&stage);
    std::fs::create_dir(&dir).map_err(ue)?;
    std::fs::create_dir(dir.join("original")).map_err(ue)?;
    std::fs::create_dir(dir.join("detached")).map_err(ue)?;
    let mut original = [None, None, None];
    for (index, name) in FILES.iter().enumerate() {
        let source = data.join("db").join(name);
        original[index] = hash(&source)?;
        if let Some(expected) = &original[index] {
            let mut input = mm_fs::open_lock(&source).map_err(ue)?;
            let copied = mm_fs::copy_new_hashing(&mut input, &dir.join("original").join(name))
                .map_err(ue)?;
            if &mm_fs::hex(&copied) != expected {
                return Err("the journal changed while its repair copy was made".into());
            }
        }
    }
    let mut candidate = Store::open(&dir.join("candidate")).map_err(ue)?;
    let report = candidate.import_from_backups_in(sources).map_err(ue)?;
    // Keep flags/settings/presets are not in the manifests. Reconstructed backups default to
    // protected, so losing the former keep flags cannot silently enable automatic cleanup.
    for id in &report.imported {
        candidate.set_keep(id, true).map_err(ue)?;
    }
    candidate.checkpoint().map_err(ue)?;
    drop(candidate);
    let candidate = hash(&dir.join("candidate/db").join(DATABASE))?
        .ok_or("the reconstructed journal is missing")?;
    let prepared = Prepared {
        commit: Commit {
            stage,
            original,
            candidate,
        },
        imported: report.imported,
        skipped: report.skipped,
    };
    let report = serde_json::json!({ "imported": prepared.imported, "skipped": prepared.skipped, "settings_and_presets": "not reconstructed", "backups": "protected by default", "photos": "not read or modified by repair" });
    write_new(
        &dir.join("preview.json"),
        &serde_json::to_vec_pretty(&report).map_err(ue)?,
    )?;
    Ok(prepared)
}

fn write_new(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .map_err(ue)?;
    file.write_all(bytes).map_err(ue)?;
    file.sync_all().map_err(ue)
}

fn commit(data: &Path, record: &Commit, begin: bool) -> Result<(), String> {
    commit_steps(data, record, begin, |_| Ok(()))
}

fn commit_steps(
    data: &Path,
    record: &Commit,
    begin: bool,
    mut checkpoint: impl FnMut(u8) -> Result<(), String>,
) -> Result<(), String> {
    if !record.stage.starts_with("journal-repair-")
        || !record
            .stage
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("the interrupted journal repair record is invalid".into());
    }
    let dir = data.join(&record.stage);
    safe_path(&dir)?;
    let db = data.join("db").join(DATABASE);
    safe_path(&db)?;
    let installed = hash(&db)?;
    let candidate = dir.join("candidate/db").join(DATABASE);
    if installed.as_ref() == Some(&record.candidate) && !candidate.exists() && !begin {
        if FILES
            .iter()
            .skip(1)
            .any(|name| data.join("db").join(name).exists())
        {
            return Err(
                "journal side files appeared after replacement; repair remains pending".into(),
            );
        }
        // Replacement finished before the marker was removed; don't run it twice.
        std::fs::remove_file(data.join(MARKER)).map_err(ue)?;
        return Ok(());
    }
    if installed != record.original[0] {
        return Err("the journal changed; prepare a new repair preview".into());
    }
    if hash(&candidate)?.as_ref() != Some(&record.candidate) {
        return Err("the reconstructed journal changed; repair was not applied".into());
    }
    for (index, name) in FILES.iter().enumerate() {
        if let Some(expected) = &record.original[index]
            && hash(&dir.join("original").join(name))?.as_ref() != Some(expected)
        {
            return Err("the preserved journal copy changed; repair was not applied".into());
        }
    }
    if begin {
        for (index, name) in FILES.iter().enumerate() {
            if hash(&data.join("db").join(name))? != record.original[index] {
                return Err("the journal changed; prepare a new repair preview".into());
            }
        }
        write_new(&data.join(MARKER), &serde_json::to_vec(record).map_err(ue)?)?;
    }
    checkpoint(1)?;
    for (index, name) in FILES.iter().enumerate().skip(1) {
        let current = data.join("db").join(name);
        if current.exists() {
            if hash(&current)? != record.original[index] {
                return Err("the journal side files changed; repair was not applied".into());
            }
            mm_fs::move_no_replace(&current, &dir.join("detached").join(name))
                .map_err(|e| e.to_string())?;
        }
    }
    checkpoint(2)?;
    std::fs::create_dir_all(data.join("db")).map_err(ue)?;
    if installed.is_some() {
        mm_fs::replace_file(&db, &candidate, &dir.join("replaced.sqlite"))
            .map_err(|e| e.to_string())?;
    } else {
        mm_fs::move_no_replace(&candidate, &db).map_err(|e| e.to_string())?;
    }
    if hash(&db)?.as_ref() != Some(&record.candidate) {
        return Err("the installed journal did not match its repair preview".into());
    }
    checkpoint(3)?;
    std::fs::remove_file(data.join(MARKER)).map_err(ue)?;
    Ok(())
}

/// Called before Core opens the database. The instance lock spans preview and adoption.
pub fn startup(data: &Path) -> Result<(), String> {
    startup_with(data, mm_fs::is_elevated().map_err(ue)?, confirm, || {
        rfd::FileDialog::new()
            .set_title("Choose MoriMeta backup root / 选择 MoriMeta 备份根目录")
            .pick_folder()
    })
}

fn startup_with(
    data: &Path,
    elevated: bool,
    mut consent: impl FnMut(&str, &str) -> bool,
    mut choose: impl FnMut() -> Option<PathBuf>,
) -> Result<(), String> {
    safe_path(data)?;
    let _instance = InstanceLock::acquire(data).map_err(ue)?;
    let marker = data.join(MARKER);
    safe_path(&marker)?;
    if marker.exists() {
        if elevated {
            return Err(
                "restart MoriMeta without administrator rights to repair its journal".into(),
            );
        }
        let record: Commit =
            serde_json::from_slice(&std::fs::read(&marker).map_err(ue)?).map_err(ue)?;
        if !consent(
            "Resume interrupted journal repair? / 继续中断的数据库修复？",
            "The original journal copies remain preserved. This completes only the previously confirmed database replacement; no photos are modified.\n原数据库副本仍保留。仅完成之前已确认的数据库替换，不修改照片。",
        ) {
            return Err("journal repair cancelled; the original copies are preserved".into());
        }
        return commit(data, &record, false);
    }
    let db = data.join("db").join(DATABASE);
    safe_path(&db)?;
    let corrupt = if !db.exists() {
        false
    } else {
        match Store::open(data) {
            Ok(store) => match store.operations() {
                Ok(operations) if !operations.is_empty() => return Ok(()),
                Ok(_) => false,
                Err(error) if error.is_corrupt() => true,
                Err(error) => return Err(ue(error)),
            },
            Err(error) if error.is_corrupt() => true,
            Err(error) => return Err(ue(error)),
        }
    };
    let sources = roots(data)?;
    let has_backups = inspect_roots(&sources)?;
    if !corrupt && !has_backups {
        return Ok(());
    }
    if elevated {
        return Err("restart MoriMeta without administrator rights to repair its journal".into());
    }
    let mut sources = sources;
    if !has_backups {
        let Some(chosen) = choose() else {
            return Err("journal repair cancelled; the original copies are preserved".into());
        };
        sources.push(chosen);
    }
    let preview = prepare(data, &sources)?;
    if preview.imported.is_empty() {
        return Err(format!(
            "No valid operations could be reconstructed. Original journal preserved. / 无法重建有效操作，原数据库已保留。\n{}",
            data.join(&preview.commit.stage).display()
        ));
    }
    let summary = format!(
        "{} operations reconstructed; {} backup records skipped.\nOriginal DB/WAL/SHM copies and preview report: {}\nSettings and custom presets cannot be reconstructed. Imported backups will be protected from automatic cleanup.\nNo photos are modified by repair. Normal recovery runs after launch.\n\n已重建 {} 个操作，跳过 {} 个备份记录。\n原数据库与附属文件和预览报告保存在上述目录。设置和自定义预设无法重建；重建的备份会默认保留。修复不修改照片，启动后照常进行中断恢复。",
        preview.imported.len(),
        preview.skipped.len(),
        data.join(&preview.commit.stage).display(),
        preview.imported.len(),
        preview.skipped.len()
    );
    if !consent(
        "Use the reconstructed journal? / 使用重建的数据库？",
        &summary,
    ) {
        return Err("journal repair cancelled; the original copies are preserved".into());
    }
    commit(data, &preview.commit, true)
}

fn confirm(title: &str, description: &str) -> bool {
    rfd::MessageDialog::new()
        .set_level(rfd::MessageLevel::Warning)
        .set_title(title)
        .set_description(description)
        .set_buttons(rfd::MessageButtons::OkCancel)
        .show()
        == rfd::MessageDialogResult::Ok
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(label: &str) -> PathBuf {
        let data = std::env::temp_dir().join(format!(
            "mm-journal-repair-{label}-{}-{}",
            std::process::id(),
            mm_store::now_ms()
        ));
        let mut store = Store::open(&data).unwrap();
        store
            .begin_operation(
                &mm_store::NewOperation {
                    id: "op-synthetic".into(),
                    kind: "apply".into(),
                    title: "Synthetic record only".into(),
                    plan_json: "{}".into(),
                    app_version: "0.1".into(),
                    exiftool_version: "13.59".into(),
                    registry_version: 0,
                    undo_of: None,
                },
                &[],
            )
            .unwrap();
        store
            .finish_operation("op-synthetic", mm_store::OpStatus::Completed)
            .unwrap();
        store.checkpoint().unwrap();
        drop(store);
        data
    }
    #[test]
    fn corrupt_journal_is_preserved_and_reconstruction_is_protected() {
        let data = fixture("corrupt");
        let db = data.join("db").join(DATABASE);
        std::fs::write(&db, b"synthetic corruption").unwrap();
        assert!(Store::open(&data).err().unwrap().is_corrupt());
        let plan = prepare(&data, &[data.join("backups")]).unwrap();
        assert_eq!(plan.imported, ["op-synthetic"]);
        assert_eq!(std::fs::read(&db).unwrap(), b"synthetic corruption");
        commit(&data, &plan.commit, true).unwrap();
        assert_eq!(
            std::fs::read(
                data.join(&plan.commit.stage)
                    .join("original")
                    .join(DATABASE)
            )
            .unwrap(),
            b"synthetic corruption"
        );
        let store = Store::open(&data).unwrap();
        assert!(store.operation("op-synthetic").unwrap().unwrap().keep);
        assert!(!data.join(MARKER).exists());
    }
    #[test]
    fn changed_journal_refuses_adoption_and_keeps_all_inputs() {
        let data = fixture("changed");
        let plan = prepare(&data, &[data.join("backups")]).unwrap();
        std::fs::write(data.join("db").join(DATABASE), b"changed after preview").unwrap();
        assert!(commit(&data, &plan.commit, true).is_err());
        assert!(!data.join(MARKER).exists());
        assert_eq!(
            std::fs::read(data.join("db").join(DATABASE)).unwrap(),
            b"changed after preview"
        );
    }
    #[test]
    fn missing_journal_and_interrupted_install_are_resumable() {
        let data = fixture("missing");
        std::fs::remove_file(data.join("db").join(DATABASE)).unwrap();
        let plan = prepare(&data, &[data.join("backups")]).unwrap();
        write_new(
            &data.join(MARKER),
            &serde_json::to_vec(&plan.commit).unwrap(),
        )
        .unwrap();
        commit(&data, &plan.commit, false).unwrap();
        assert_eq!(Store::open(&data).unwrap().operations().unwrap().len(), 1);
        write_new(
            &data.join(MARKER),
            &serde_json::to_vec(&plan.commit).unwrap(),
        )
        .unwrap();
        commit(&data, &plan.commit, false).unwrap();
        assert!(!data.join(MARKER).exists());
    }
    #[test]
    fn malformed_resume_record_cannot_escape_the_data_folder() {
        let data = fixture("escape");
        let mut plan = prepare(&data, &[data.join("backups")]).unwrap();
        plan.commit.stage = "../foreign".into();
        assert!(commit(&data, &plan.commit, false).is_err());
    }

    #[test]
    fn every_install_boundary_retains_original_side_files_and_resumes() {
        for stop in [1, 2, 3] {
            let data = fixture(&format!("boundary-{stop}"));
            std::fs::write(data.join("db").join(DATABASE), b"synthetic corrupt DB").unwrap();
            // Opaque synthetic side-file bytes test preservation, not SQLite WAL replay.
            std::fs::write(data.join("db").join(FILES[1]), b"synthetic WAL bytes").unwrap();
            std::fs::write(data.join("db").join(FILES[2]), b"synthetic SHM bytes").unwrap();
            let plan = prepare(&data, &[data.join("backups")]).unwrap();
            assert!(
                commit_steps(&data, &plan.commit, true, |step| if step == stop {
                    Err("simulated interruption".into())
                } else {
                    Ok(())
                })
                .is_err()
            );
            assert!(data.join(MARKER).is_file());
            commit(&data, &plan.commit, false).unwrap();
            assert!(!data.join(MARKER).exists());
            assert_eq!(
                std::fs::read(
                    data.join(&plan.commit.stage)
                        .join("original")
                        .join(FILES[1])
                )
                .unwrap(),
                b"synthetic WAL bytes"
            );
            assert_eq!(
                std::fs::read(
                    data.join(&plan.commit.stage)
                        .join("original")
                        .join(FILES[2])
                )
                .unwrap(),
                b"synthetic SHM bytes"
            );
            assert_eq!(Store::open(&data).unwrap().operations().unwrap().len(), 1);
        }
    }

    #[test]
    fn startup_requires_consent_and_never_treats_a_newer_schema_as_corruption() {
        let data = fixture("startup");
        assert!(
            startup_with(
                &data,
                false,
                |_, _| panic!("healthy journal needs no repair prompt"),
                || panic!("no dialog")
            )
            .is_ok()
        );
        let db = data.join("db").join(DATABASE);
        std::fs::write(&db, b"synthetic corrupt journal").unwrap();
        let mut prompts = 0;
        assert!(
            startup_with(
                &data,
                false,
                |_, description| {
                    prompts += 1;
                    assert!(description.contains("1 operations reconstructed"));
                    false
                },
                || panic!("known backup root")
            )
            .is_err()
        );
        assert_eq!(prompts, 1);
        assert_eq!(std::fs::read(&db).unwrap(), b"synthetic corrupt journal");
        assert!(!data.join(MARKER).exists());
        assert!(
            startup_with(
                &data,
                true,
                |_, _| panic!("no elevated repair"),
                || panic!("no elevated picker")
            )
            .is_err()
        );
        startup_with(&data, false, |_, _| true, || None).unwrap();
        assert!(
            Store::open(&data)
                .unwrap()
                .operation("op-synthetic")
                .unwrap()
                .unwrap()
                .keep
        );
        assert!(!mm_store::StoreError::NewerSchema(99).is_corrupt());
        assert!(!mm_store::StoreError::Io(std::io::Error::from_raw_os_error(5)).is_corrupt());
    }

    #[test]
    fn instance_lock_and_candidate_changes_prevent_repair() {
        let data = fixture("locked");
        let instance = InstanceLock::acquire(&data).unwrap();
        assert!(
            startup_with(
                &data,
                false,
                |_, _| panic!("another instance owns the folder"),
                || None
            )
            .is_err()
        );
        drop(instance);
        let plan = prepare(&data, &[data.join("backups")]).unwrap();
        std::fs::write(
            data.join(&plan.commit.stage)
                .join("candidate/db")
                .join(DATABASE),
            b"altered candidate",
        )
        .unwrap();
        assert!(commit(&data, &plan.commit, true).is_err());
        assert!(!data.join(MARKER).exists());
        assert_eq!(Store::open(&data).unwrap().operations().unwrap().len(), 1);
    }
}
