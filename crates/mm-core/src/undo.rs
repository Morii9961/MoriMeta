//! Undo planning (SAFETY_MODEL §7). Undo restores verified backups byte-for-byte; it is itself an
//! Operation (with its own backups), so it can be undone again.

use std::path::Path;

use mm_domain::plan::{EntryAction, EntryStatus, Fingerprint, Plan, PlanEntry, PlanKind};
use mm_store::{FileRow, FileState, Store};

use crate::{CoreError, ROLE_CREATE, ROLE_RECREATE, ROLE_REMOVE, fingerprint, hash_opt, new_id};

pub fn plan_undo(store: &Store, op_id: &str, exiftool_version: &str) -> Result<Plan, CoreError> {
    let op = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if op.status == "running" || op.status == "interrupted" {
        return Err(CoreError::RecoveryPending(vec![op_id.to_owned()]));
    }
    if op.pruned_ms.is_some() {
        return Err(CoreError::Input(format!(
            "the backups of {op_id} were removed by the retention policy; it cannot be undone"
        )));
    }
    let mut entries = Vec::new();
    for f in store.files(op_id)? {
        if f.state != FileState::Done {
            continue; // never changed by this operation
        }
        let seq = entries.len() as u32;
        if let Some((status, fingerprint, action, notes)) = undo_one(&f)? {
            entries.push(PlanEntry {
                seq,
                path: f.path.clone(),
                raw: None,
                fingerprint,
                status,
                changes: vec![],
                action,
                notes,
                excluded: false,
            });
        }
    }
    Ok(Plan {
        id: new_id("plan")?,
        version: 1,
        kind: PlanKind::Undo {
            of: op_id.to_owned(),
        },
        title: format!("Undo: {}", op.title),
        registry_version: op.registry_version,
        exiftool_version: exiftool_version.to_owned(),
        entries,
    })
}

type Decision = (EntryStatus, Fingerprint, Option<EntryAction>, Vec<String>);

fn absent() -> Fingerprint {
    Fingerprint {
        size: 0,
        file_id: String::new(),
        mtime: 0,
    }
}

fn blocked(why: &str, fp: Fingerprint) -> Decision {
    (EntryStatus::Blocked(why.into()), fp, None, vec![])
}

/// How to undo one file that the Operation changed (SAFETY_MODEL §7.2), by its journal role:
/// written in place → restore the backup; recreated → move it into the backup store; moved into
/// the backup store → recreate it. A file that is gone is recreated from its backup.
fn undo_one(f: &FileRow) -> Result<Option<Decision>, CoreError> {
    let path = Path::new(&f.path);
    let missing = mm_fs::ensure_absent(path).is_ok();
    if f.role == ROLE_RECREATE || f.role == ROLE_CREATE {
        let Some(h1) = f.h1.clone() else {
            return Ok(None);
        };
        if missing {
            return Ok(Some((
                EntryStatus::NoChange,
                absent(),
                None,
                vec!["already absent".into()],
            )));
        }
        let fp = fingerprint(path)?;
        return Ok(Some(if hash_opt(path).as_deref() == Some(h1.as_str()) {
            (
                EntryStatus::Ready,
                fp,
                Some(EntryAction::MoveToBackupStore { h: h1 }),
                vec![
                    "the operation created this file; it will be moved into the backup store"
                        .into(),
                ],
            )
        } else {
            blocked("changed after the undo (conflict); not removed", fp)
        }));
    }
    let Some(h0) = f.h0.clone() else {
        return Ok(None);
    };
    if missing {
        return recreate(f, path, h0).map(Some);
    }
    let fp = fingerprint(path)?;
    let cur = hash_opt(path);
    if f.role == ROLE_REMOVE {
        // the file was moved into the backup store; something is at its path again
        return Ok(Some(if cur.as_deref() == Some(h0.as_str()) {
            (
                EntryStatus::NoChange,
                fp,
                None,
                vec!["already in its original state".into()],
            )
        } else {
            blocked("a different file now exists at this path; not restored", fp)
        }));
    }
    let Some(h1) = f.h1.clone() else {
        return Ok(None);
    };
    Ok(Some(if cur.as_deref() == Some(h1.as_str()) {
        if hash_opt(Path::new(&f.backup_path)).as_deref() != Some(h0.as_str()) {
            blocked("backup missing or damaged", fp)
        } else {
            (
                EntryStatus::Ready,
                fp,
                Some(EntryAction::Restore {
                    backup: f.backup_path.clone(),
                    h0,
                    h1,
                }),
                vec!["restore the backup made before the operation".into()],
            )
        }
    } else if cur.as_deref() == Some(h0.as_str()) {
        (
            EntryStatus::NoChange,
            fp,
            None,
            vec!["already in its original state".into()],
        )
    } else {
        blocked("changed after the operation (conflict); not restored", fp)
    }))
}

/// The file is gone: recreate its pre-image `h0` at its path from the backup, if possible.
fn recreate(f: &FileRow, path: &Path, h0: String) -> Result<Decision, CoreError> {
    let backup = Path::new(&f.backup_path);
    if !path.parent().is_some_and(Path::is_dir) {
        return Ok(blocked(
            "its folder no longer exists; not recreated",
            absent(),
        ));
    }
    if hash_opt(backup).as_deref() != Some(h0.as_str()) {
        return Ok(blocked(
            "file is missing and its backup is missing or damaged",
            absent(),
        ));
    }
    let size = std::fs::metadata(backup)?.len();
    Ok((
        EntryStatus::Ready,
        absent(),
        Some(EntryAction::Recreate {
            backup: f.backup_path.clone(),
            h0,
            size,
        }),
        vec![
            "file was deleted or moved after the operation; the original will be recreated at \
             this path (a moved copy elsewhere is not touched)"
                .into(),
        ],
    ))
}
