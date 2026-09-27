//! History (PRODUCT_SPEC §6.14): the Operation Journal as a list (time, name, files, changes,
//! result, backup state) and as the detail of one Operation with field-level before/after, which
//! is also what "Export Log" writes.

use std::collections::BTreeMap;

use mm_domain::plan::{FieldChange, Plan};
use mm_store::{FileState, OperationRow, Store};
use serde::Serialize;

use crate::CoreError;

#[derive(Debug, Clone, Serialize)]
pub struct OpSummary {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
    pub undo_of: Option<String>,
    /// Undo Operations of this one, oldest first.
    pub undone_by: Vec<String>,
    pub files: usize,
    /// File count per journal state.
    pub states: BTreeMap<String, usize>,
    /// Field changes of the files that were written.
    pub changes: usize,
    pub keep: bool,
    pub backups_pruned: bool,
    /// Whether an undo Plan can be made now (not running, not interrupted, backups present).
    pub undoable: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileDetail {
    pub seq: u32,
    pub path: String,
    pub state: String,
    pub error: Option<String>,
    pub h0: Option<String>,
    pub h1: Option<String>,
    pub changes: Vec<FieldChange>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpDetail {
    #[serde(flatten)]
    pub summary: OpSummary,
    pub app_version: String,
    pub exiftool_version: String,
    pub files_detail: Vec<FileDetail>,
}

fn plan_of(o: &OperationRow) -> Option<Plan> {
    serde_json::from_str(&o.plan_json).ok()
}

fn summarize(
    store: &Store,
    o: &OperationRow,
    all: &[OperationRow],
) -> Result<OpSummary, CoreError> {
    let files = store.files(&o.id)?;
    let plan = plan_of(o);
    let mut states = BTreeMap::new();
    for f in &files {
        *states.entry(f.state.as_str().to_owned()).or_insert(0) += 1;
    }
    let changes = files
        .iter()
        .filter(|f| f.state == FileState::Done)
        .map(|f| {
            plan.as_ref()
                .and_then(|p| p.entries.iter().find(|e| e.seq == f.seq))
                .map_or(0, |e| e.changes.len())
        })
        .sum();
    let pending = matches!(o.status.as_str(), "running" | "interrupted");
    Ok(OpSummary {
        id: o.id.clone(),
        kind: o.kind.clone(),
        title: o.title.clone(),
        status: o.status.clone(),
        created_ms: o.created_ms,
        finished_ms: o.finished_ms,
        undo_of: o.undo_of.clone(),
        undone_by: all
            .iter()
            .filter(|u| u.undo_of.as_deref() == Some(o.id.as_str()))
            .map(|u| u.id.clone())
            .collect(),
        files: files.len(),
        states,
        changes,
        keep: o.keep,
        backups_pruned: o.pruned_ms.is_some(),
        undoable: !pending
            && o.pruned_ms.is_none()
            && files.iter().any(|f| f.state == FileState::Done),
    })
}

/// Newest first; `page` from 0.
pub fn list(store: &Store, page: usize, size: usize) -> Result<Vec<OpSummary>, CoreError> {
    let mut ops = store.operations()?;
    ops.reverse();
    let size = size.max(1);
    ops.iter()
        .skip(page * size)
        .take(size)
        .map(|o| summarize(store, o, &ops))
        .collect()
}

pub fn detail(store: &Store, op_id: &str) -> Result<OpDetail, CoreError> {
    let all = store.operations()?;
    let o = all
        .iter()
        .find(|o| o.id == op_id)
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    let plan = plan_of(o);
    let files_detail = store
        .files(op_id)?
        .into_iter()
        .map(|f| FileDetail {
            changes: plan
                .as_ref()
                .and_then(|p| p.entries.iter().find(|e| e.seq == f.seq))
                .map(|e| e.changes.clone())
                .unwrap_or_default(),
            seq: f.seq,
            path: f.path,
            state: f.state.as_str().to_owned(),
            error: f.error,
            h0: f.h0,
            h1: f.h1,
        })
        .collect();
    Ok(OpDetail {
        summary: summarize(store, o, &all)?,
        app_version: o.app_version.clone(),
        exiftool_version: o.exiftool_version.clone(),
        files_detail,
    })
}

/// "Export Log": the detail as JSON, written to a new file (never replacing one).
pub fn export_log(store: &Store, op_id: &str, out: &std::path::Path) -> Result<(), CoreError> {
    use std::io::Write;
    let d = detail(store, op_id)?;
    let text = serde_json::to_string_pretty(&d).map_err(|e| CoreError::Internal(e.to_string()))?;
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(out)
        .map_err(|e| CoreError::Input(format!("{}: {e}", out.display())))?;
    f.write_all(text.as_bytes())?;
    f.sync_all()?;
    Ok(())
}

/// "Retry Failed" (PRODUCT_SPEC §6.14): a new Plan of the persisted entries of the files that
/// failed or were skipped (in use, read-only…). The original content was not changed for them, so
/// the planned writes still apply as long as the file is unchanged, which execution checks
/// (fingerprint and before-values; a changed file becomes a Conflict). Files not started or
/// cancelled are continued with resume instead; conflicts need a new Plan.
pub fn retry_plan(store: &Store, op_id: &str, exiftool_version: &str) -> Result<Plan, CoreError> {
    let o = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if matches!(o.status.as_str(), "running" | "interrupted") {
        return Err(CoreError::RecoveryPending(vec![op_id.to_owned()]));
    }
    if o.app_version != crate::APP_VERSION || o.exiftool_version != exiftool_version {
        return Err(CoreError::VersionMismatch(format!(
            "operation made with app {} / ExifTool {}; plan the files again",
            o.app_version, o.exiftool_version
        )));
    }
    let mut plan =
        plan_of(&o).ok_or_else(|| CoreError::Internal("persisted plan unreadable".into()))?;
    if !matches!(plan.kind, mm_domain::plan::PlanKind::Apply) {
        return Err(CoreError::Input(
            "an undo is retried by planning the undo again".into(),
        ));
    }
    let retry: Vec<u32> = store
        .files(op_id)?
        .iter()
        .filter(|f| matches!(f.state, FileState::Failed | FileState::Skipped))
        .map(|f| f.seq)
        .collect();
    if retry.is_empty() {
        return Err(CoreError::Input(format!(
            "{op_id} has no failed or skipped files"
        )));
    }
    plan.entries.retain(|e| retry.contains(&e.seq));
    plan.id = crate::new_id("plan")?;
    plan.version = 1;
    plan.title = format!("Retry: {}", plan.title);
    Ok(plan)
}

/// Plan the same edit again (the Plan's recorded source) for the files of an Operation that
/// failed, were skipped or were in conflict: their metadata is read afresh, so a file changed
/// after the first Preview gets a correct Plan ("re-preview", PRODUCT_SPEC §6.15). Sequence and
/// Preserve Relative Timing depend on the whole selection and are not re-planned for a subset.
pub fn replan(
    engine: &mut crate::engine::Engine,
    store: &Store,
    op_id: &str,
    ctl: &crate::planner::PlanCtl,
) -> Result<Plan, CoreError> {
    use crate::planner::{self, TimeTool};
    use mm_domain::copyright::CopyrightEdit;
    use mm_domain::creator::CreatorEdit;
    use mm_domain::gps::{GeoPoint, GpsEdit};
    use mm_domain::plan::{PlanSource, TimeSpec};

    let o = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    let old = plan_of(&o).ok_or_else(|| CoreError::Internal("persisted plan unreadable".into()))?;
    let source = old.source.clone().ok_or_else(|| {
        CoreError::Input("this Operation does not record what it was planned from".into())
    })?;
    let seqs: Vec<u32> = store
        .files(op_id)?
        .iter()
        .filter(|f| {
            matches!(
                f.state,
                FileState::Failed | FileState::Skipped | FileState::Conflict
            )
        })
        .map(|f| f.seq)
        .collect();
    let paths: Vec<std::path::PathBuf> = old
        .entries
        .iter()
        .filter(|e| seqs.contains(&e.seq))
        .map(|e| std::path::PathBuf::from(e.raw.as_deref().unwrap_or(&e.path)))
        .collect();
    if paths.is_empty() {
        return Err(CoreError::Input(format!(
            "{op_id} has no failed, skipped or conflicting files"
        )));
    }
    let title = format!("Again: {}", old.title);
    let mut plan = match &source {
        PlanSource::Creator { set } => {
            let edit = set.clone().map_or(CreatorEdit::Clear, CreatorEdit::Set);
            planner::plan_creator(engine, &paths, &edit, &title, ctl)?
        }
        PlanSource::Copyright { set } => {
            let edit = set.clone().map_or(CopyrightEdit::Clear, CopyrightEdit::Set);
            planner::plan_copyright(engine, &paths, &edit, &title, ctl)?
        }
        PlanSource::Gps { set } => {
            let edit = match set {
                Some(s) => GpsEdit::Set(GeoPoint::parse(s).map_err(CoreError::Input)?),
                None => GpsEdit::Remove,
            };
            planner::plan_gps(engine, &paths, &edit, &title, ctl)?
        }
        PlanSource::CaptureTime { tool, digitized } => {
            if matches!(
                tool,
                TimeSpec::Sequence { .. } | TimeSpec::PreserveRelative { .. }
            ) {
                return Err(CoreError::Input(
                    "a Sequence or Preserve Relative Timing depends on the whole selection; \
                     select the files and plan it again"
                        .into(),
                ));
            }
            let tool = TimeTool::from_spec(tool)?;
            planner::plan_capture_time(engine, &paths, &tool, *digitized, &title, ctl)?
        }
        PlanSource::Preset { preset, .. } => {
            let mut p = planner::plan_preset(engine, &paths, preset, ctl)?;
            p.title = title;
            p
        }
        PlanSource::Undo { .. } => {
            return Err(CoreError::Input(
                "an undo is planned again with plan-undo".into(),
            ));
        }
    };
    plan.source = Some(source);
    Ok(plan)
}
