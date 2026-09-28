// SPDX-License-Identifier: GPL-3.0-or-later
//! Crash recovery (SAFETY_MODEL §10). Runs before any new write. Decides every unfinished file
//! from the journal and the observed disk state; only files registered in the journal are touched,
//! and a registered file is removed only after its hash has been checked (I-9).

use std::path::Path;

use mm_store::{FileRow, FileState, FileUpdate, OpStatus, Store};

use crate::{CoreError, ROLE_CREATE, ROLE_RECREATE, ROLE_REMOVE, hash_opt};

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
            crate::log::event(
                if to == FileState::Attention {
                    "warn"
                } else {
                    "info"
                },
                "recovered",
                &[
                    ("op", &op_id),
                    ("asset", &crate::privacy::alias(f.seq, &f.path)),
                    ("from", &f.state.as_str()),
                    ("to", &to.as_str()),
                    ("action", &crate::log::scrub_for(&action, f.seq, &f.path)),
                ],
            );
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

/// Error text of a file the user chose to keep as found after an interruption.
pub const KEPT_PREFIX: &str = "kept as found after an interruption: ";

/// The user's choice for files recovery could not settle (SAFETY_MODEL §10 step 3): keep what is
/// on disk. Each file becomes `Conflict` (terminal; this Operation never touches it again). Our
/// own registered leftovers go only after their hash check, and a bak holding the pre-image only
/// when the backup store has a verified copy of it (I-9). The backup stays, so the pre-image can
/// still be restored through the undo Plan (a forced restore). Once no file needs attention, the
/// Operation ends `recovered` and writes are allowed again.
pub fn resolve_keep(store: &mut Store, op_id: &str, seqs: &[u32]) -> Result<(), CoreError> {
    let files = store.files(op_id)?;
    for &seq in seqs {
        let f = files
            .iter()
            .find(|f| f.seq == seq)
            .ok_or_else(|| CoreError::Input(format!("{op_id} has no file {seq}")))?;
        if f.state != FileState::Attention {
            return Err(CoreError::Input(format!(
                "file {seq} of {op_id} does not need attention ({})",
                f.state.as_str()
            )));
        }
    }
    for &seq in seqs {
        let f = files.iter().find(|f| f.seq == seq).expect("checked above");
        remove_if_hash(Path::new(&f.temp_path), f.h1.as_deref());
        if f.h0.is_some() && hash_opt(Path::new(&f.backup_path)) == f.h0 {
            remove_if_hash(Path::new(&f.bak_path), f.h0.as_deref());
        }
        store.set_state(
            op_id,
            seq,
            FileState::Conflict,
            &FileUpdate {
                error: Some(format!(
                    "{KEPT_PREFIX}{}",
                    f.error.as_deref().unwrap_or("needed attention")
                )),
                ..Default::default()
            },
        )?;
    }
    if !store
        .files(op_id)?
        .iter()
        .any(|f| f.state == FileState::Attention || !f.state.is_terminal())
    {
        store.finish_operation(op_id, OpStatus::Recovered)?;
    }
    Ok(())
}

fn remove_if_hash(p: &Path, want: Option<&str>) -> bool {
    match (want, hash_opt(p)) {
        (Some(w), Some(h)) if h == w => std::fs::remove_file(p).is_ok(),
        _ => false,
    }
}

/// The decision table. Never deletes the original path; never overwrites anything.
pub(crate) fn decide(f: &FileRow) -> (FileState, String) {
    if f.role == ROLE_RECREATE || f.role == ROLE_CREATE {
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

/// One Operation for the recovery screen (PRODUCT_SPEC §6.15): done N, not processed M, needs
/// attention K.
#[derive(Debug, Clone, serde::Serialize)]
pub struct RecoverySummary {
    pub op_id: String,
    pub title: String,
    pub status: String,
    pub done: usize,
    /// Not started or cancelled: what "continue" would process.
    pub remaining: usize,
    pub attention: usize,
    pub other: usize,
}

/// Operations that were interrupted and still ask for a decision: continue the remaining files,
/// undo the finished part, or keep things as they are (`dismiss`).
pub fn summary(store: &Store) -> Result<Vec<RecoverySummary>, CoreError> {
    let mut out = Vec::new();
    for o in store.operations()? {
        if !matches!(o.status.as_str(), "interrupted" | "recovered" | "running") {
            continue;
        }
        let files = store.files(&o.id)?;
        let count = |f: &dyn Fn(FileState) -> bool| files.iter().filter(|x| f(x.state)).count();
        let done = count(&|s| s == FileState::Done);
        let remaining = count(&|s| matches!(s, FileState::NotStarted | FileState::Cancelled));
        let attention = count(&|s| s == FileState::Attention);
        if o.status == "recovered" && remaining == 0 && attention == 0 {
            continue; // nothing left to decide
        }
        out.push(RecoverySummary {
            op_id: o.id.clone(),
            title: o.title.clone(),
            status: o.status.clone(),
            done,
            remaining,
            attention,
            other: files.len() - done - remaining - attention,
        });
    }
    Ok(out)
}

/// "Keep as it is and close": the remaining files stay unprocessed and the Operation leaves the
/// recovery screen as `cancelled` (still resumable from History). Files that need attention must
/// be resolved first.
pub fn dismiss(store: &mut Store, op_id: &str) -> Result<(), CoreError> {
    let o = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if o.status != "recovered" {
        return Err(CoreError::Input(format!(
            "{op_id} is {}; run recovery and resolve files that need attention first",
            o.status
        )));
    }
    store.finish_operation(op_id, OpStatus::Cancelled)?;
    Ok(())
}
