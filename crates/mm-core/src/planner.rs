//! Planning (ARCHITECTURE §6.2): environment pre-checks, fingerprints, metadata snapshot, and the
//! pure field planner. Produces an immutable `Plan`; nothing is written.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use mm_domain::capture;
use mm_domain::copyright::{self, CopyrightEdit};
use mm_domain::creator::{self, CreatorEdit};
use mm_domain::gps::{self, GpsEdit};
use mm_domain::plan::{
    EntryAction, EntryStatus, FieldPlan, Fingerprint, Plan, PlanEntry, PlanKind,
};
use mm_domain::snapshot::Snapshot;
use mm_domain::time::{
    self, NaiveDateTime, SequenceOrder, TimeDelta, TimeItem, TimeOp, TimeOpError,
};

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

pub fn plan_gps(
    engine: &mut Engine,
    inputs: &[PathBuf],
    edit: &GpsEdit,
    title: &str,
) -> Result<Plan, CoreError> {
    plan_field(engine, inputs, title, |s| gps::plan(s, edit))
}

/// A time tool as the user specified it (METADATA_MODEL §5.2); the anchor of Preserve Relative
/// Timing is one of the input files.
#[derive(Debug, Clone)]
pub enum TimeTool {
    Absolute(NaiveDateTime),
    Shift(TimeDelta),
    Sequence {
        start: NaiveDateTime,
        step: TimeDelta,
        order: SequenceOrder,
    },
    PreserveRelative {
        anchor: PathBuf,
        new_local: NaiveDateTime,
    },
}

/// Capture time for the whole selection at once: Sequence orders the files and Preserve Relative
/// Timing measures from its anchor, so no file can be planned on its own. `digitized` also sets
/// EXIF CreateDate (on by default, METADATA_MODEL §5.3).
pub fn plan_capture_time(
    engine: &mut Engine,
    inputs: &[PathBuf],
    tool: &TimeTool,
    digitized: bool,
    title: &str,
) -> Result<Plan, CoreError> {
    let keep_subsec = matches!(tool, TimeTool::Shift(_) | TimeTool::PreserveRelative { .. });
    plan_with(engine, inputs, title, |readable, entries| {
        let items: Vec<TimeItem> = readable
            .iter()
            .map(|(idx, s)| TimeItem {
                id: *idx as u64,
                file_name: Path::new(&entries[*idx].path)
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                time: capture::read(s).ok().flatten(),
            })
            .collect();
        let name = |id: u64| entries[id as usize].path.clone();
        let op = match tool {
            TimeTool::Absolute(l) => TimeOp::Absolute(*l),
            TimeTool::Shift(d) => TimeOp::Shift(*d),
            TimeTool::Sequence { start, step, order } => TimeOp::Sequence {
                start: *start,
                step: *step,
                order: *order,
            },
            TimeTool::PreserveRelative { anchor, new_local } => {
                let a = normalize(anchor)?.to_string_lossy().into_owned();
                let id = items
                    .iter()
                    .find(|it| entries[it.id as usize].path == a)
                    .ok_or_else(|| {
                        CoreError::Input(format!(
                            "anchor {a} is not a writable file of this selection"
                        ))
                    })?
                    .id;
                TimeOp::PreserveRelative {
                    anchor: id,
                    new_local: *new_local,
                }
            }
        };
        let results = time::apply(&op, &items).map_err(|e| match e {
            TimeOpError::OrderNeedsValidTimes(ids) => CoreError::Input(format!(
                "ordering by capture time needs a valid time on every file; missing: {}",
                ids.into_iter().map(name).collect::<Vec<_>>().join(", ")
            )),
            other => CoreError::Input(other.to_string()),
        })?;
        let n = results.len();
        Ok(readable
            .iter()
            .map(|(idx, s)| {
                let r = results.iter().find(|r| r.id == *idx as u64);
                let after = r
                    .map(|r| r.after.clone())
                    .unwrap_or(Err(TimeOpError::EmptySelection));
                let mut fp = capture::plan(s, &after, keep_subsec, digitized);
                if let (TimeTool::Sequence { .. }, Some(r)) = (tool, r) {
                    fp.notes.insert(
                        0,
                        format!("position {} of {n} in the sequence", r.index + 1),
                    );
                }
                fp
            })
            .collect())
    })
}

/// Pre-checks, fingerprints and the snapshot of every input, then the pure field planner.
fn plan_field(
    engine: &mut Engine,
    inputs: &[PathBuf],
    title: &str,
    field: impl Fn(&Snapshot) -> FieldPlan,
) -> Result<Plan, CoreError> {
    plan_with(engine, inputs, title, |readable, _| {
        Ok(readable.iter().map(|(_, s)| field(s)).collect())
    })
}

/// The shared planning pipeline; `field_all` receives every readable file's (entry index,
/// snapshot) together and returns one field plan per readable file, in the same order.
fn plan_with(
    engine: &mut Engine,
    inputs: &[PathBuf],
    title: &str,
    field_all: impl FnOnce(&[(usize, Snapshot)], &[PlanEntry]) -> Result<Vec<FieldPlan>, CoreError>,
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
    let mut readable = Vec::new();
    for ((idx, _), snap) in to_read.into_iter().zip(snaps) {
        match snap {
            Err(why) => {
                entries[idx].status = EntryStatus::Blocked(format!("metadata unreadable: {why}"))
            }
            Ok(s) => readable.push((idx, s)),
        }
    }
    let plans = field_all(&readable, &entries)?;
    for ((idx, _), cp) in readable.iter().zip(plans) {
        let e = &mut entries[*idx];
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
