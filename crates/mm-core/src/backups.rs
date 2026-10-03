// SPDX-License-Identifier: GPL-3.0-or-later
//! Backup cleanup for the desktop adapter: inspect usage, preview the exact selection, then
//! execute with a single-use token. Changed protection or policy requires a new preview.

use mm_store::Store;
use serde::Serialize;

use crate::retention::{self, OpBackup, Policy};
use crate::service::{OperationGate, ServiceError, WritePermit};
use crate::{CoreError, new_id};

#[derive(Debug, Clone, Serialize)]
pub struct PrunePreview {
    pub token: String,
    pub operations: Vec<OpBackup>,
    pub bytes: u64,
    /// The user explicitly chose these backups, including kept or recent ones.
    pub requested: bool,
}

struct Pending {
    preview: PrunePreview,
    policy: Policy,
}

#[derive(Default)]
pub struct PruneBook {
    pending: Option<Pending>,
}

impl PruneBook {
    /// `None`: preview the retention policy's candidates. `Some`: the user's selection.
    /// Preparing a new preview invalidates the previous token, even when preparation fails.
    pub fn preview(
        &mut self,
        store: &Store,
        selected: Option<&[String]>,
    ) -> Result<PrunePreview, ServiceError> {
        self.pending = None;
        let policy = Policy::from_settings(store)?;
        let usage = retention::usage(store, &policy)?;
        let requested = selected.is_some();
        let mut ids = selected.map(<[String]>::to_vec).unwrap_or_else(|| {
            retention::prune_plan(&usage, &policy, mm_store::now_ms())
                .into_iter()
                .map(|(id, _)| id)
                .collect()
        });
        ids.sort();
        ids.dedup();
        if ids.is_empty() {
            return Err(CoreError::Input("nothing to prune".into()).into());
        }
        let operations = retention::selection(&usage, &ids, requested)?;
        let preview = PrunePreview {
            token: new_id("prune")?,
            bytes: operations.iter().map(|o| o.bytes).sum(),
            operations,
            requested,
        };
        self.pending = Some(Pending {
            preview: preview.clone(),
            policy,
        });
        Ok(preview)
    }

    pub fn execute(
        &mut self,
        gate: &OperationGate,
        store: &mut Store,
        token: &str,
    ) -> Result<Vec<String>, ServiceError> {
        let permit = gate.write()?;
        self.execute_permitted(&permit, store, token)
    }

    /// A desktop command reserves its write slot before publishing running/cancel state.
    pub fn execute_permitted(
        &mut self,
        _permit: &WritePermit<'_>,
        store: &mut Store,
        token: &str,
    ) -> Result<Vec<String>, ServiceError> {
        if !self
            .pending
            .as_ref()
            .is_some_and(|p| p.preview.token == token)
        {
            return Err(ServiceError::NotConfirmed);
        }
        let pending = self.pending.take().ok_or(ServiceError::NotConfirmed)?;
        let ids: Vec<String> = pending
            .preview
            .operations
            .iter()
            .map(|o| o.op_id.clone())
            .collect();
        let policy = Policy::from_settings(store)?;
        let usage = retention::usage(store, &policy)?;
        let current = retention::selection(&usage, &ids, pending.preview.requested)?;
        if policy != pending.policy || current != pending.preview.operations {
            return Err(
                CoreError::Input("the backup selection changed; preview it again".into()).into(),
            );
        }
        Ok(retention::prune(
            store,
            &policy,
            &ids,
            pending.preview.requested,
        )?)
    }
}

/// Change an Operation's automatic-retention protection without accepting an arbitrary path.
pub fn keep(
    gate: &OperationGate,
    store: &mut Store,
    op_id: &str,
    value: bool,
) -> Result<(), ServiceError> {
    let _permit = gate.write()?;
    store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    store.set_keep(op_id, value)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_store::{FileState, NewFile, NewOperation, OpStatus};

    #[test]
    fn cleanup_requires_a_current_preview_and_never_prunes_unfinished_backups() {
        let dir = std::env::temp_dir().join(format!("mm-prune-book-{}", std::process::id()));
        let mut store = Store::open(&dir).unwrap();
        for id in ["op-done", "op-open"] {
            let backup = store.backup_dir(id).join("00000000-0123abcd.jpg");
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
                        path: String::new(),
                        role: "embedded".into(),
                        temp_path: String::new(),
                        bak_path: String::new(),
                        backup_path: backup.display().to_string(),
                    }],
                )
                .unwrap();
            std::fs::write(backup, b"original").unwrap();
        }
        store
            .set_state("op-done", 0, FileState::Done, &Default::default())
            .unwrap();
        store
            .finish_operation("op-done", OpStatus::Completed)
            .unwrap();
        let gate = OperationGate::default();
        let mut book = PruneBook::default();
        let ids = vec!["op-done".to_owned()];
        assert!(book.preview(&store, None).is_err()); // recent backups protected by default
        assert!(
            book.preview(&store, Some(&[ids[0].clone(), "op-open".into()]))
                .is_err()
        );
        assert!(
            retention::prune(
                &mut store,
                &Policy::default(),
                &[ids[0].clone(), "op-open".into()],
                true
            )
            .is_err()
        );
        assert!(store.backup_dir("op-done").exists());
        assert!(
            store
                .operation("op-done")
                .unwrap()
                .unwrap()
                .pruned_ms
                .is_none()
        );
        let old = book.preview(&store, Some(&ids)).unwrap();
        let p = book
            .preview(&store, Some(&[ids[0].clone(), ids[0].clone()]))
            .unwrap();
        assert_eq!(p.operations.len(), 1);
        assert!(p.bytes >= 8);
        assert!(book.execute(&gate, &mut store, &old.token).is_err());
        {
            let _permit = gate.write().unwrap();
            assert!(matches!(
                book.execute(&gate, &mut store, &p.token),
                Err(ServiceError::Busy(_))
            ));
        }
        keep(&gate, &mut store, "op-done", true).unwrap();
        assert!(book.execute(&gate, &mut store, &p.token).is_err()); // protection changed
        assert!(store.backup_dir("op-done").exists());
        let p = book.preview(&store, Some(&ids)).unwrap();
        crate::settings::set(&mut store, "backup.keep_latest", "1").unwrap();
        assert!(book.execute(&gate, &mut store, &p.token).is_err()); // policy changed
        let p = book.preview(&store, Some(&ids)).unwrap();
        assert_eq!(book.execute(&gate, &mut store, &p.token).unwrap(), ids);
        assert!(book.execute(&gate, &mut store, &p.token).is_err()); // single use
        assert!(!store.backup_dir("op-done").exists());
        assert!(
            store
                .operation("op-done")
                .unwrap()
                .unwrap()
                .pruned_ms
                .is_some()
        );
        assert!(store.backup_dir("op-open").exists());
        assert!(keep(&gate, &mut store, "missing", true).is_err());
        gate.refuse_writes("test integrity refusal");
        assert!(matches!(
            keep(&gate, &mut store, "op-open", true),
            Err(ServiceError::WritesDisabled(_))
        ));
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
