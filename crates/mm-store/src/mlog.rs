// SPDX-License-Identifier: GPL-3.0-or-later
//! `manifest.jsonl`: the append-only record next to each Operation's backups (SAFETY_MODEL §6.1),
//! and rebuilding the Journal from it when the database is lost (PHASE1_REPORT G-7).
//!
//! The first line describes the Operation, one line per file registers it (path, role, temporary
//! name, bak name, backup file) — before any file is touched. Every later journal change appends
//! one line; `BackedUp` (H0) and `Ready` (H1) lines are flushed to disk before the transaction
//! goes on, so that a commit is never preceded only by records that exist in the database.
//! `plan.json` next to it holds the executable Plan, so a rebuilt Operation can still be resumed.

use std::collections::BTreeMap;
use std::io::{BufRead, BufReader};
use std::path::Path;

use rusqlite::params;
use serde_json::Value;

use crate::{Result, Store, now_ms};

pub const MANIFEST_LOG: &str = "manifest.jsonl";
pub const PLAN_FILE: &str = "plan.json";

/// Outcome of [`Store::import_from_backups`].
#[derive(Debug, Clone, Default)]
pub struct ImportReport {
    /// Operations added to the Journal.
    pub imported: Vec<String>,
    /// Backup folders not imported, with the reason.
    pub skipped: Vec<(String, String)>,
}

#[derive(Debug, Default)]
struct FileRec {
    path: String,
    role: String,
    temp: String,
    bak: String,
    backup: String,
    state: String,
    h0: Option<String>,
    h1: Option<String>,
    new_file_id: Option<String>,
    error: Option<String>,
}

#[derive(Debug)]
struct OpRec {
    header: Value,
    status: String,
    finished_ms: Option<i64>,
    files: BTreeMap<u32, FileRec>,
}

fn text(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_owned)
}

/// Replay `manifest.jsonl`. A torn last line (the process ended while appending) is ignored;
/// a damaged line before the end makes the whole record unusable.
fn replay(path: &Path) -> std::result::Result<OpRec, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("cannot read {MANIFEST_LOG}: {e}"))?;
    let lines: Vec<String> = BufReader::new(file)
        .lines()
        .collect::<std::result::Result<_, _>>()
        .map_err(|e| format!("cannot read {MANIFEST_LOG}: {e}"))?;
    let mut op: Option<OpRec> = None;
    for (i, line) in lines.iter().enumerate() {
        let v: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) if i + 1 == lines.len() => break, // torn tail
            Err(e) => return Err(format!("line {}: {e}", i + 1)),
        };
        let kind = text(&v, "t").unwrap_or_default();
        if kind == "op" {
            op = Some(OpRec {
                header: v,
                status: "running".into(),
                finished_ms: None,
                files: BTreeMap::new(),
            });
            continue;
        }
        let o = op
            .as_mut()
            .ok_or_else(|| "first line does not describe the operation".to_string())?;
        let seq = v.get("seq").and_then(Value::as_u64).map(|s| s as u32);
        match (kind.as_str(), seq) {
            ("file", Some(seq)) => {
                o.files.insert(
                    seq,
                    FileRec {
                        path: text(&v, "path").unwrap_or_default(),
                        role: text(&v, "role").unwrap_or_default(),
                        temp: text(&v, "temp").unwrap_or_default(),
                        bak: text(&v, "bak").unwrap_or_default(),
                        backup: text(&v, "backup").unwrap_or_default(),
                        state: "planned".into(),
                        ..Default::default()
                    },
                );
            }
            ("state", Some(seq)) => {
                let f = o
                    .files
                    .get_mut(&seq)
                    .ok_or_else(|| format!("line {}: unknown file {seq}", i + 1))?;
                f.state = text(&v, "state").unwrap_or_default();
                // same semantics as the database update: absent values leave the old ones
                f.h0 = text(&v, "h0").or(f.h0.take());
                f.h1 = text(&v, "h1").or(f.h1.take());
                f.new_file_id = text(&v, "new_file_id").or(f.new_file_id.take());
                f.error = text(&v, "error").or(f.error.take());
            }
            ("paths", Some(seq)) => {
                let f = o
                    .files
                    .get_mut(&seq)
                    .ok_or_else(|| format!("line {}: unknown file {seq}", i + 1))?;
                f.temp = text(&v, "temp").unwrap_or_default();
                f.bak = text(&v, "bak").unwrap_or_default();
                f.backup = text(&v, "backup").unwrap_or_default();
                f.state = "planned".into();
                (f.h0, f.h1, f.error) = (None, None, None);
            }
            ("status", _) => {
                o.status = text(&v, "status").unwrap_or_default();
                if let Some(ms) = v.get("finished_ms").and_then(Value::as_i64) {
                    o.finished_ms = Some(ms);
                }
            }
            _ => return Err(format!("line {}: unknown record", i + 1)),
        }
    }
    op.ok_or_else(|| "empty record".to_string())
}

impl Store {
    /// Rebuild the Journal entries of every Operation that has a backup folder with a
    /// `manifest.jsonl` but is missing from the database (the database was lost or replaced).
    /// Operations already in the database are left alone, so this can be run repeatedly.
    /// Afterwards, run crash recovery as usual: it decides every unfinished file from the disk.
    pub fn import_from_backups(&mut self) -> Result<ImportReport> {
        let mut report = ImportReport::default();
        let mut dirs: Vec<_> = Vec::new();
        for root in self.backup_roots() {
            if let Ok(rd) = std::fs::read_dir(&root) {
                dirs.extend(rd.filter_map(|e| e.ok()).filter(|e| e.path().is_dir()));
            }
        }
        dirs.sort_by_key(|e| e.file_name());
        for d in dirs {
            let id = d.file_name().to_string_lossy().into_owned();
            if self.operation(&id)?.is_some() {
                continue;
            }
            let dir = d.path();
            let log = dir.join(MANIFEST_LOG);
            if !log.exists() {
                report
                    .skipped
                    .push((id, format!("no {MANIFEST_LOG} in this backup folder")));
                continue;
            }
            let op = match replay(&log) {
                Ok(op) => op,
                Err(why) => {
                    report.skipped.push((id, why));
                    continue;
                }
            };
            if text(&op.header, "id").as_deref() != Some(id.as_str()) {
                report
                    .skipped
                    .push((id, "record belongs to another operation".into()));
                continue;
            }
            let plan_json = std::fs::read_to_string(dir.join(PLAN_FILE)).unwrap_or_default();
            let h = &op.header;
            let tx = self.conn.transaction()?;
            tx.execute(
                "INSERT INTO operations(id, kind, title, status, created_ms, finished_ms, plan_json, app_version, exiftool_version, registry_version, undo_of, backup_dir)
                 VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    id,
                    text(h, "kind").unwrap_or_default(),
                    text(h, "title").unwrap_or_default(),
                    op.status,
                    h.get("created_ms").and_then(Value::as_i64).unwrap_or(0),
                    op.finished_ms,
                    plan_json,
                    text(h, "app_version").unwrap_or_default(),
                    text(h, "exiftool_version").unwrap_or_default(),
                    h.get("registry_version").and_then(Value::as_i64).unwrap_or(0),
                    text(h, "undo_of"),
                    dir.to_string_lossy(),
                ],
            )?;
            for (seq, f) in &op.files {
                tx.execute(
                    "INSERT INTO op_files(op_id, seq, path, role, temp_path, bak_path, backup_path, state, h0, h1, new_file_id, error, updated_ms)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
                    params![id, seq, f.path, f.role, f.temp, f.bak, f.backup, f.state, f.h0, f.h1, f.new_file_id, f.error, now_ms()],
                )?;
            }
            tx.commit()?;
            report.imported.push(id);
        }
        Ok(report)
    }
}
