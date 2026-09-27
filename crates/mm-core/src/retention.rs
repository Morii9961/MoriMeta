// SPDX-License-Identifier: GPL-3.0-or-later
//! Backup retention (SAFETY_MODEL §6.3). The defaults are the recommendation of D-7, which is not
//! decided yet, so they are parameters. Pruning only removes MoriMeta's own backup folders
//! (invariant I-9) and keeps the History record, marked as no longer undoable.

use std::path::Path;

use mm_store::{FileState, Store};

use crate::CoreError;

#[derive(Debug, Clone, PartialEq)]
pub struct Policy {
    /// Backups older than this are pruned.
    pub max_age_days: u32,
    /// Oldest backups are pruned while all backups together exceed this share of the backup
    /// volume's capacity.
    pub max_share_of_volume: f64,
    /// The most recent Operations are never pruned automatically.
    pub keep_latest: usize,
}

impl Default for Policy {
    /// D-7 recommendation: 30 days, 10 % of the volume; always the latest 10 Operations.
    fn default() -> Self {
        Policy {
            max_age_days: 30,
            max_share_of_volume: 0.10,
            keep_latest: 10,
        }
    }
}

/// Settings keys of the policy (PRODUCT_SPEC §6.16 Backup).
pub const SETTING_MAX_AGE_DAYS: &str = "backup.max_age_days";
pub const SETTING_MAX_SHARE: &str = "backup.max_share_of_volume";
pub const SETTING_KEEP_LATEST: &str = "backup.keep_latest";

impl Policy {
    /// The user's settings over the defaults; an unreadable value is an error, not a silent
    /// default, since it decides what is deleted.
    pub fn from_settings(store: &Store) -> Result<Policy, CoreError> {
        let mut p = Policy::default();
        let bad = |k: &str, v: &str| CoreError::Input(format!("setting {k} = {v:?} is not valid"));
        if let Some(v) = store.setting(SETTING_MAX_AGE_DAYS)? {
            p.max_age_days = v.parse().map_err(|_| bad(SETTING_MAX_AGE_DAYS, &v))?;
        }
        if let Some(v) = store.setting(SETTING_MAX_SHARE)? {
            p.max_share_of_volume = v
                .parse::<f64>()
                .ok()
                .filter(|x| (0.0..=1.0).contains(x))
                .ok_or_else(|| bad(SETTING_MAX_SHARE, &v))?;
        }
        if let Some(v) = store.setting(SETTING_KEEP_LATEST)? {
            p.keep_latest = v.parse().map_err(|_| bad(SETTING_KEEP_LATEST, &v))?;
        }
        Ok(p)
    }
}

/// Why an Operation's backups are not pruned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protection {
    /// Running, interrupted, a file that needs attention, or files that can still be resumed:
    /// never pruned, not even on request.
    Unfinished,
    /// Marked "keep" by the user: not pruned automatically.
    Kept,
    /// Among the most recent Operations: not pruned automatically.
    Recent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Reason {
    Age,
    Size,
    Requested,
}

#[derive(Debug, Clone)]
pub struct OpBackup {
    pub op_id: String,
    pub title: String,
    pub created_ms: i64,
    pub bytes: u64,
    pub pruned: bool,
    pub protection: Option<Protection>,
}

#[derive(Debug, Clone)]
pub struct Usage {
    /// Oldest first.
    pub ops: Vec<OpBackup>,
    pub total_bytes: u64,
    /// Capacity of the volume that holds the backups.
    pub volume_bytes: u64,
}

fn dir_bytes(dir: &Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|rd| {
            rd.filter_map(|e| e.ok()?.metadata().ok())
                .filter(|m| m.is_file())
                .map(|m| m.len())
                .sum()
        })
        .unwrap_or(0)
}

/// Every Operation with the size of its backup folder and what protects it.
pub fn usage(store: &Store, policy: &Policy) -> Result<Usage, CoreError> {
    let ops = store.operations()?; // oldest first
    let recent_from = ops.len().saturating_sub(policy.keep_latest);
    let mut out = Vec::new();
    for (i, o) in ops.iter().enumerate() {
        let files = store.files(&o.id)?;
        let open = matches!(o.status.as_str(), "running" | "interrupted")
            || files.iter().any(|f| {
                matches!(
                    f.state,
                    FileState::Attention | FileState::NotStarted | FileState::Cancelled
                ) || !f.state.is_terminal()
            });
        let protection = if open {
            Some(Protection::Unfinished)
        } else if o.keep {
            Some(Protection::Kept)
        } else if i >= recent_from {
            Some(Protection::Recent)
        } else {
            None
        };
        let pruned = o.pruned_ms.is_some();
        out.push(OpBackup {
            op_id: o.id.clone(),
            title: o.title.clone(),
            created_ms: o.created_ms,
            bytes: if pruned {
                0
            } else {
                dir_bytes(Path::new(&o.backup_dir))
            },
            pruned,
            protection,
        });
    }
    let volume_bytes = mm_fs::volume_space(&store.data_dir().join("backups"))?.total;
    Ok(Usage {
        total_bytes: out.iter().map(|o| o.bytes).sum(),
        ops: out,
        volume_bytes,
    })
}

/// What the policy would prune at `now_ms`, oldest first; shown to the user before anything is
/// removed (SAFETY_MODEL §6.3: they are told which Operations lose their undo).
pub fn prune_plan(usage: &Usage, policy: &Policy, now_ms: i64) -> Vec<(String, Reason)> {
    let max_age_ms = i64::from(policy.max_age_days) * 86_400_000;
    let cap = (usage.volume_bytes as f64 * policy.max_share_of_volume) as u64;
    let mut total = usage.total_bytes;
    let mut out = Vec::new();
    for o in &usage.ops {
        if o.pruned || o.protection.is_some() {
            continue;
        }
        let why = if now_ms - o.created_ms > max_age_ms {
            Reason::Age
        } else if total > cap {
            Reason::Size
        } else {
            continue;
        };
        total -= o.bytes;
        out.push((o.op_id.clone(), why));
    }
    out
}

/// Remove the backups of `op_ids`. `requested` is the user's own choice in Settings, which may
/// include kept and recent Operations; unfinished ones are refused either way. The History record
/// stays, marked pruned.
pub fn prune(
    store: &mut Store,
    policy: &Policy,
    op_ids: &[String],
    requested: bool,
) -> Result<Vec<String>, CoreError> {
    let u = usage(store, policy)?;
    let mut done = Vec::new();
    for id in op_ids {
        let o = u
            .ops
            .iter()
            .find(|o| &o.op_id == id)
            .ok_or_else(|| CoreError::Input(format!("no operation {id}")))?;
        match o.protection {
            Some(Protection::Unfinished) => {
                return Err(CoreError::Input(format!(
                    "{id} is not finished; its backups are needed"
                )));
            }
            Some(_) if !requested => {
                return Err(CoreError::Input(format!("{id} is protected from pruning")));
            }
            _ => {}
        }
        let dir = store.backup_dir(id);
        // the database first: an interrupted prune is finished next time, never taken for an
        // undoable Operation
        store.mark_pruned(id)?;
        remove_backup_dir(&dir)?;
        done.push(id.clone());
    }
    Ok(done)
}

/// Delete a backup folder: the append-only record first, so that a partly deleted folder is never
/// imported again by `rebuild-journal`; then the rest. Only plain files directly inside it are
/// expected; anything else is left and reported.
fn remove_backup_dir(dir: &Path) -> Result<(), CoreError> {
    if !dir.exists() {
        return Ok(());
    }
    let log = dir.join(mm_store::MANIFEST_LOG);
    if log.exists() {
        std::fs::remove_file(&log)?;
    }
    for e in std::fs::read_dir(dir)? {
        let e = e?;
        if e.file_type()?.is_file() {
            std::fs::remove_file(e.path())?;
        }
    }
    std::fs::remove_dir(dir).map_err(|e| {
        CoreError::Io(std::io::Error::new(
            e.kind(),
            format!("{}: {e} (unexpected entries left)", dir.display()),
        ))
    })
}

/// Finish prunes that were interrupted between the database mark and the deletion.
pub fn finish_interrupted(store: &Store) -> Result<Vec<String>, CoreError> {
    let mut out = Vec::new();
    for o in store.operations()? {
        let dir = store.backup_dir(&o.id);
        if o.pruned_ms.is_some() && dir.exists() {
            remove_backup_dir(&dir)?;
            out.push(o.id);
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_store::{NewFile, NewOperation, OpStatus};
    use std::path::PathBuf;

    /// An Operation whose one file ended in `state`, with a 1,000-byte backup.
    fn op(store: &mut Store, id: &str, state: FileState, status: OpStatus) {
        let dir = store.backup_dir(id);
        let backup = dir.join("00000000.jpg");
        store
            .begin_operation(
                &NewOperation {
                    id: id.into(),
                    kind: "apply".into(),
                    title: id.into(),
                    plan_json: "{}".into(),
                    app_version: "0".into(),
                    exiftool_version: "0".into(),
                    registry_version: 0,
                    undo_of: None,
                },
                &[NewFile {
                    seq: 0,
                    path: format!("C:/p/{id}.jpg"),
                    role: "embedded".into(),
                    temp_path: String::new(),
                    bak_path: String::new(),
                    backup_path: backup.to_string_lossy().into_owned(),
                }],
            )
            .unwrap();
        std::fs::write(&backup, vec![0u8; 1000]).unwrap();
        store.set_state(id, 0, state, &Default::default()).unwrap();
        if status != OpStatus::Running {
            store.finish_operation(id, status).unwrap();
        }
    }

    fn lab(name: &str) -> (PathBuf, Store) {
        let d = std::env::temp_dir().join(format!("mm-retention-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let s = Store::open(&d).unwrap();
        (d, s)
    }

    #[test]
    fn protection_age_and_size() {
        let (d, mut s) = lab("plan");
        op(&mut s, "op-a", FileState::Done, OpStatus::Completed);
        op(&mut s, "op-b", FileState::Cancelled, OpStatus::Cancelled); // resumable
        op(&mut s, "op-c", FileState::Done, OpStatus::Completed);
        op(&mut s, "op-d", FileState::Done, OpStatus::Completed);
        s.set_keep("op-c", true).unwrap();
        let policy = Policy {
            keep_latest: 1,
            ..Policy::default()
        };
        let u = usage(&s, &policy).unwrap();
        let prot: Vec<_> = u.ops.iter().map(|o| o.protection).collect();
        assert_eq!(
            prot,
            [
                None,
                Some(Protection::Unfinished),
                Some(Protection::Kept),
                Some(Protection::Recent)
            ]
        );
        assert!(u.ops.iter().all(|o| o.bytes > 1000)); // backup + manifest + plan
        let now = mm_store::now_ms();
        assert!(prune_plan(&u, &policy, now).is_empty());
        let later = now + 31 * 86_400_000;
        assert_eq!(
            prune_plan(&u, &policy, later),
            [("op-a".to_string(), Reason::Age)]
        );
        // over the size cap: only unprotected ones, oldest first, until under the cap
        let tight = Usage {
            volume_bytes: u.total_bytes * 5, // cap = half of the total at 10 %
            ..u.clone()
        };
        assert_eq!(
            prune_plan(&tight, &policy, now),
            [("op-a".to_string(), Reason::Size)]
        );
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn policy_from_settings() {
        let (d, mut s) = lab("settings");
        assert_eq!(Policy::from_settings(&s).unwrap(), Policy::default());
        s.set_setting(SETTING_MAX_AGE_DAYS, "7").unwrap();
        s.set_setting(SETTING_MAX_SHARE, "0.25").unwrap();
        let p = Policy::from_settings(&s).unwrap();
        assert_eq!(
            (p.max_age_days, p.max_share_of_volume, p.keep_latest),
            (7, 0.25, 10)
        );
        s.set_setting(SETTING_MAX_SHARE, "2").unwrap();
        assert!(Policy::from_settings(&s).is_err());
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn pruning_keeps_history_and_refuses_unfinished() {
        let (d, mut s) = lab("prune");
        op(&mut s, "op-a", FileState::Done, OpStatus::Completed);
        op(
            &mut s,
            "op-b",
            FileState::Attention,
            OpStatus::CompletedWithErrors,
        );
        op(&mut s, "op-c", FileState::Done, OpStatus::Completed);
        let p = Policy {
            keep_latest: 1,
            ..Policy::default()
        };
        assert!(prune(&mut s, &p, &["op-b".into()], true).is_err());
        assert!(prune(&mut s, &p, &["op-c".into()], false).is_err()); // recent
        assert_eq!(
            prune(&mut s, &p, &["op-a".into()], false).unwrap(),
            ["op-a"]
        );
        assert!(!s.backup_dir("op-a").exists());
        let a = s.operation("op-a").unwrap().unwrap();
        assert!(a.pruned_ms.is_some());
        // the user may prune a recent one on request
        prune(&mut s, &p, &["op-c".into()], true).unwrap();
        // nothing to import again from the backups
        assert!(s.import_from_backups().unwrap().imported.is_empty());

        // an interrupted prune (marked, folder still there) is finished
        op(&mut s, "op-e", FileState::Done, OpStatus::Completed);
        s.mark_pruned("op-e").unwrap();
        assert_eq!(finish_interrupted(&s).unwrap(), ["op-e"]);
        assert!(!s.backup_dir("op-e").exists());
        let _ = std::fs::remove_dir_all(&d);
    }
}
