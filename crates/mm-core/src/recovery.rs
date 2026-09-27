//! Crash recovery (SAFETY_MODEL §10). Runs before any new write. Decides every unfinished file
//! from the journal and the observed disk state; only files registered in the journal are touched,
//! and a registered file is removed only after its hash has been checked (I-9).

use std::path::Path;

use mm_store::{FileRow, FileState, FileUpdate, OpStatus, Store};

use crate::{CoreError, ROLE_RECREATE, ROLE_REMOVE, hash_opt};

#[derive(Debug, Clone)]
pub struct RecoveredFile {
    pub seq: u32,
    pub path: String,
    pub from: FileState,
    pub to: FileState,
    pub action: String,
}

#[derive(Debug, Clone)]
pub struct RecoveryReport {
    pub op_id: String,
    pub files: Vec<RecoveredFile>,
}

pub fn recover(store: &mut Store) -> Result<Vec<RecoveryReport>, CoreError> {
    let mut reports = Vec::new();
    for op_id in store.unfinished()? {
        store.set_status(&op_id, OpStatus::Interrupted)?;
        let mut rep = RecoveryReport {
            op_id: op_id.clone(),
            files: vec![],
        };
        for f in store.files(&op_id)? {
            if f.state.is_terminal() {
                continue;
            }
            let (to, action) = decide(&f);
            let err = (to == FileState::Attention).then(|| action.clone());
            store.set_state(
                &op_id,
                f.seq,
                to,
                &FileUpdate {
                    error: err,
                    ..Default::default()
                },
            )?;
            rep.files.push(RecoveredFile {
                seq: f.seq,
                path: f.path.clone(),
                from: f.state,
                to,
                action,
            });
        }
        let files = store.files(&op_id)?;
        let status = if files.iter().any(|f| f.state == FileState::Attention) {
            OpStatus::Interrupted
        } else {
            OpStatus::Recovered
        };
        // Attention keeps the operation "interrupted": a person has to look at it.
        if status == OpStatus::Recovered {
            store.finish_operation(&op_id, status)?;
        } else {
            store.write_manifest(&op_id)?;
        }
        reports.push(rep);
    }
    Ok(reports)
}

fn remove_if_hash(p: &Path, want: Option<&str>) -> bool {
    match (want, hash_opt(p)) {
        (Some(w), Some(h)) if h == w => std::fs::remove_file(p).is_ok(),
        _ => false,
    }
}

/// The decision table. Never deletes the original path; never overwrites anything.
pub(crate) fn decide(f: &FileRow) -> (FileState, String) {
    if f.role == ROLE_RECREATE {
        return decide_recreate(f);
    }
    if f.role == ROLE_REMOVE {
        return decide_remove(f);
    }
    let path = Path::new(&f.path);
    let temp = Path::new(&f.temp_path);
    let bak = Path::new(&f.bak_path);
    let backup = Path::new(&f.backup_path);
    let cur = hash_opt(path);
    let h0 = f.h0.as_deref();
    let h1 = f.h1.as_deref();
    match f.state {
        FileState::Planned => {
            // nothing irreversible happened; a partial backup copy and a temp file may exist
            if cur.is_none() {
                return (
                    FileState::Attention,
                    "original missing; backup copy kept".into(),
                );
            }
            let _ = std::fs::remove_file(temp);
            let _ = std::fs::remove_file(backup);
            (FileState::NotStarted, "not started".into())
        }
        FileState::BackedUp => {
            let _ = std::fs::remove_file(temp);
            if cur.as_deref() == h0 {
                remove_if_hash(backup, h0);
                (
                    FileState::NotStarted,
                    "not started (backup discarded)".into(),
                )
            } else {
                (
                    FileState::Attention,
                    "file differs from its backup; nothing changed on disk".into(),
                )
            }
        }
        FileState::Ready | FileState::Committed => {
            if cur.is_some() && cur.as_deref() == h1 {
                remove_if_hash(bak, h0);
                let _ = std::fs::remove_file(temp); // no longer exists after a commit
                (FileState::Done, "commit had completed".into())
            } else if cur.is_some() && cur.as_deref() == h0 {
                let _ = std::fs::remove_file(temp); // registered random name; holds no user data
                remove_if_hash(backup, h0);
                (FileState::NotStarted, "not committed".into())
            } else if cur.is_none() && h0.is_some() && hash_opt(bak).as_deref() == h0 {
                match mm_fs::move_no_replace(bak, path) {
                    Ok(()) => {
                        let _ = std::fs::remove_file(temp);
                        remove_if_hash(backup, h0);
                        (
                            FileState::NotStarted,
                            "interrupted inside ReplaceFileW; original put back".into(),
                        )
                    }
                    Err(e) => (
                        FileState::Attention,
                        format!(
                            "original is at {} but could not be moved back ({e})",
                            bak.display()
                        ),
                    ),
                }
            } else {
                (
                    FileState::Attention,
                    "unexpected content; nothing deleted (backup kept)".into(),
                )
            }
        }
        other => (other, "terminal".into()),
    }
}

/// A file an Undo moves into the backup store: the commit renames the path to the registered bak
/// name, so the pre-image H0 is at the path (not committed), at the bak name, or — once the bak
/// has been removed after its hash check — only in the backup store.
fn decide_remove(f: &FileRow) -> (FileState, String) {
    let path = Path::new(&f.path);
    let bak = Path::new(&f.bak_path);
    let backup = Path::new(&f.backup_path);
    let cur = hash_opt(path);
    let h0 = f.h0.as_deref();
    match f.state {
        FileState::Planned if cur.is_none() => (
            FileState::Attention,
            "file missing before it was moved; nothing deleted".into(),
        ),
        FileState::Planned => {
            let _ = std::fs::remove_file(backup); // a partial copy at most
            (FileState::NotStarted, "not started".into())
        }
        FileState::BackedUp | FileState::Ready | FileState::Committed
            if cur.is_some() && cur.as_deref() == h0 =>
        {
            remove_if_hash(backup, h0);
            (FileState::NotStarted, "not moved".into())
        }
        FileState::Ready | FileState::Committed
            if cur.is_none() && h0.is_some() && hash_opt(bak).as_deref() == h0 =>
        {
            remove_if_hash(bak, h0);
            (FileState::Done, "move had completed".into())
        }
        FileState::Ready | FileState::Committed
            if cur.is_none() && h0.is_some() && hash_opt(backup).as_deref() == h0 =>
        {
            (
                FileState::Done,
                "move had completed (content in the backup store)".into(),
            )
        }
        FileState::BackedUp | FileState::Ready | FileState::Committed => (
            FileState::Attention,
            "unexpected content; nothing deleted (backup kept)".into(),
        ),
        other => (other, "terminal".into()),
    }
}

/// A file recreated by an Undo: its pre-image is "absent" and the commit is a rename that never
/// replaces anything, so the path holds the recreated content (H1) only if the rename happened.
/// Whatever else is at the path is not ours and is never touched.
fn decide_recreate(f: &FileRow) -> (FileState, String) {
    let temp = Path::new(&f.temp_path);
    let cur = hash_opt(Path::new(&f.path));
    let h1 = f.h1.as_deref();
    match f.state {
        FileState::Planned | FileState::BackedUp => {
            let _ = std::fs::remove_file(temp); // registered random name; holds only backup data
            (FileState::NotStarted, "not recreated".into())
        }
        FileState::Ready if cur.is_some() && cur.as_deref() == h1 => {
            (FileState::Done, "recreate had completed".into())
        }
        FileState::Ready => {
            remove_if_hash(temp, h1);
            (FileState::NotStarted, "not recreated".into())
        }
        FileState::Committed if cur.is_some() && cur.as_deref() == h1 => {
            (FileState::Done, "recreate had completed".into())
        }
        FileState::Committed => (
            FileState::Attention,
            "recreated file is missing or changed; nothing deleted (backup kept)".into(),
        ),
        other => (other, "terminal".into()),
    }
}
