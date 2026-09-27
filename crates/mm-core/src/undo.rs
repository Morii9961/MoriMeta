//! Undo planning (SAFETY_MODEL §7). Undo restores verified backups byte-for-byte; it is itself an
//! Operation (with its own backups), so it can be undone again.

use std::path::Path;

use mm_domain::plan::{EntryAction, EntryStatus, Plan, PlanEntry, PlanKind};
use mm_store::{FileState, Store};

use crate::{CoreError, fingerprint, hash_opt, new_id};

pub fn plan_undo(store: &Store, op_id: &str, exiftool_version: &str) -> Result<Plan, CoreError> {
    let op = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if op.status == "running" || op.status == "interrupted" {
        return Err(CoreError::RecoveryPending(vec![op_id.to_owned()]));
    }
    let mut entries = Vec::new();
    for f in store.files(op_id)? {
        if f.state != FileState::Done {
            continue; // never changed by this operation
        }
        let (Some(h0), Some(h1)) = (f.h0.clone(), f.h1.clone()) else {
            continue;
        };
        let path = Path::new(&f.path);
        let seq = entries.len() as u32;
        let base = |status, fp, action, notes: Vec<String>| PlanEntry {
            seq,
            path: f.path.clone(),
            fingerprint: fp,
            status,
            changes: vec![],
            action,
            notes,
        };
        if !path.exists() {
            entries.push(base(
                EntryStatus::Blocked("file was deleted or moved after the operation".into()),
                mm_domain::plan::Fingerprint {
                    size: 0,
                    file_id: String::new(),
                    mtime: 0,
                },
                None,
                vec![],
            ));
            continue;
        }
        let fp = fingerprint(path)?;
        let cur = hash_opt(path);
        let entry = if cur.as_deref() == Some(h1.as_str()) {
            if hash_opt(Path::new(&f.backup_path)).as_deref() != Some(h0.as_str()) {
                base(
                    EntryStatus::Blocked("backup missing or damaged".into()),
                    fp,
                    None,
                    vec![],
                )
            } else {
                base(
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
            base(
                EntryStatus::NoChange,
                fp,
                None,
                vec!["already in its original state".into()],
            )
        } else {
            base(
                EntryStatus::Blocked("changed after the operation (conflict); not restored".into()),
                fp,
                None,
                vec![],
            )
        };
        entries.push(entry);
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
