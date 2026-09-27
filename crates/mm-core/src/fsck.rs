// SPDX-License-Identifier: GPL-3.0-or-later
//! Offline consistency check of one Operation (SAFETY_MODEL §10 "mm-cli fsck"). Read-only.

use std::path::Path;

use mm_store::{FileState, Store};

use crate::{CoreError, ROLE_CREATE, ROLE_RECREATE, ROLE_REMOVE, hash_opt};

#[derive(Debug, Clone)]
pub struct FsckReport {
    pub op_id: String,
    pub files: usize,
    pub problems: Vec<String>,
}

pub fn fsck(store: &Store, op_id: &str) -> Result<FsckReport, CoreError> {
    let files = store.files(op_id)?;
    // backups removed by the retention policy are not expected (SAFETY_MODEL §6.3)
    let pruned = store
        .operation(op_id)?
        .is_some_and(|o| o.pruned_ms.is_some());
    let mut problems = Vec::new();
    for f in &files {
        let id = format!("#{} {}", f.seq, f.path);
        let cur = hash_opt(Path::new(&f.path));
        if Path::new(&f.temp_path).exists() {
            problems.push(format!("{id}: temporary file left behind"));
        }
        if Path::new(&f.bak_path).exists() {
            problems.push(format!("{id}: bak file left behind"));
        }
        match f.state {
            FileState::Done if f.role == ROLE_REMOVE => {
                // moved into the backup store: the path is expected to be empty
                if cur.is_some() {
                    problems.push(format!("{id}: a file exists at this path again"));
                }
                if !pruned && hash_opt(Path::new(&f.backup_path)) != f.h0 {
                    problems.push(format!("{id}: backup missing or damaged"));
                }
            }
            FileState::Done => {
                if cur.is_none() {
                    problems.push(format!("{id}: missing (done)"));
                } else if cur != f.h1 {
                    problems.push(format!(
                        "{id}: content differs from the committed result (changed later?)"
                    ));
                }
                if !pruned && hash_opt(Path::new(&f.backup_path)) != f.h0 {
                    problems.push(format!("{id}: backup missing or damaged"));
                }
            }
            FileState::NotStarted
            | FileState::Failed
            | FileState::Skipped
            | FileState::Conflict
            | FileState::Cancelled => {
                // a file an Undo did not get to recreate is expected to be absent
                if cur.is_none() && f.role != ROLE_RECREATE && f.role != ROLE_CREATE {
                    problems.push(format!("{id}: missing"));
                } else if f.h0.is_some() && f.state != FileState::Conflict && cur != f.h0 {
                    problems.push(format!(
                        "{id}: not written by this operation but differs from its pre-image"
                    ));
                }
            }
            FileState::Attention => problems.push(format!(
                "{id}: needs attention: {}",
                f.error.clone().unwrap_or_default()
            )),
            s => problems.push(format!("{id}: unresolved state {}", s.as_str())),
        }
    }
    Ok(FsckReport {
        op_id: op_id.to_owned(),
        files: files.len(),
        problems,
    })
}
