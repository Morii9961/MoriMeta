//! Planning (ARCHITECTURE §6.2): environment pre-checks, fingerprints, metadata snapshot, and the
//! pure field planner. Produces an immutable `Plan`; nothing is written.

use std::collections::HashSet;
use std::path::PathBuf;

use mm_domain::copyright::{self, CopyrightEdit};
use mm_domain::creator::{self, CreatorEdit};
use mm_domain::plan::{
    EntryAction, EntryStatus, FieldPlan, Fingerprint, Plan, PlanEntry, PlanKind,
};
use mm_domain::snapshot::Snapshot;

use crate::engine::Engine;
use crate::{CoreError, fingerprint, new_id, normalize};

/// Formats writable in this build (Embedded target). TIFF follows after its own S2/S3 checks.
fn writable_ext(p: &std::path::Path) -> bool {
    p.extension()
        .map(|e| matches!(e.to_ascii_lowercase().to_str(), Some("jpg" | "jpeg")))
        .unwrap_or(false)
}

/// Environment checks at planning time (SAFETY_MODEL §8). Repeated at execution.
fn precheck(p: &std::path::Path) -> Result<(), String> {
    let pr = mm_fs::probe(p).map_err(|e| format!("cannot inspect file: {e}"))?;
    if pr.reparse_point {
        return Err("symbolic link or reparse point (not written)".into());
    }
    if pr.links > 1 {
        return Err("file has more than one hard link (not written)".into());
    }
    if pr.read_only {
        return Err("read-only attribute is set (treated as locked by the user)".into());
    }
    if pr.cloud_placeholder {
        return Err("cloud placeholder that is not downloaded".into());
    }
    Ok(())
}

pub fn plan_creator(
    engine: &mut Engine,
    inputs: &[PathBuf],
    edit: &CreatorEdit,
    title: &str,
) -> Result<Plan, CoreError> {
    plan_field(engine, inputs, title, |s| creator::plan(s, edit))
}

pub fn plan_copyright(
    engine: &mut Engine,
    inputs: &[PathBuf],
    edit: &CopyrightEdit,
    title: &str,
) -> Result<Plan, CoreError> {
    plan_field(engine, inputs, title, |s| copyright::plan(s, edit))
}

/// Pre-checks, fingerprints and the snapshot of every input, then the pure field planner.
fn plan_field(
    engine: &mut Engine,
    inputs: &[PathBuf],
    title: &str,
    field: impl Fn(&Snapshot) -> FieldPlan,
) -> Result<Plan, CoreError> {
    let mut entries = Vec::new();
    let mut seen = HashSet::new();
    let mut to_read: Vec<(usize, PathBuf)> = Vec::new();
    for input in inputs {
        let path = normalize(input)?;
        let fp = fingerprint(&path)?;
        if !seen.insert(fp.file_id.clone()) {
            continue; // same file through another path
        }
        let seq = entries.len() as u32;
        let status = if !writable_ext(&path) {
            EntryStatus::Unsupported("format is read-only in this build".into())
        } else {
            match precheck(&path) {
                Ok(()) => EntryStatus::Ready,
                Err(why) => EntryStatus::Blocked(why),
            }
        };
        if status == EntryStatus::Ready {
            to_read.push((entries.len(), path.clone()));
        }
        entries.push(PlanEntry {
            seq,
            path: path.to_string_lossy().into_owned(),
            fingerprint: fp,
            status,
            changes: vec![],
            action: None,
            notes: vec![],
        });
    }
    let paths: Vec<PathBuf> = to_read.iter().map(|(_, p)| p.clone()).collect();
    let snaps = engine.read_snapshots(&paths)?;
    for ((idx, _), snap) in to_read.into_iter().zip(snaps) {
        let e = &mut entries[idx];
        match snap {
            Err(why) => e.status = EntryStatus::Blocked(format!("metadata unreadable: {why}")),
            Ok(s) => {
                let cp = field(&s);
                e.status = cp.status;
                e.notes = cp.notes;
                if let Some(ch) = cp.change {
                    e.changes.push(ch);
                }
                if e.status == EntryStatus::Ready {
                    e.action = Some(EntryAction::Write {
                        ops: cp.ops,
                        expect: cp.expect,
                    });
                }
            }
        }
    }
    Ok(Plan {
        id: new_id("plan")?,
        version: 1,
        kind: PlanKind::Apply,
        title: title.to_owned(),
        registry_version: creator::REGISTRY_VERSION,
        exiftool_version: engine.version().to_owned(),
        entries,
    })
}

/// True when the file still matches the planning-time fingerprint.
pub fn same_file(now: &Fingerprint, planned: &Fingerprint) -> bool {
    now == planned
}
