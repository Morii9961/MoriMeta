// SPDX-License-Identifier: GPL-3.0-or-later
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
    /// The backup folder is not where the Operation recorded it (a drive that is not connected):
    /// Undo and "Restore to folder" wait for it.
    pub backups_unavailable: bool,
    /// Whether an undo Plan can be made now (not running, not interrupted, backups present).
    pub undoable: bool,
    /// Files stopped while they were being written (their temporary output discarded, the
    /// original unchanged), as the completion summary of a stopped Operation counts them apart
    /// from the files never started (INTERACTION_SPEC §10).
    pub rolled_back: usize,
    /// Written files whose change carried a warning (the completion summary's "2 warnings").
    pub warnings: usize,
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
    /// What the user acknowledged before it ran (INTERACTION_SPEC §5).
    pub acks: Vec<String>,
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
    let unavailable = o.pruned_ms.is_none() && !std::path::Path::new(&o.backup_dir).is_dir();
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
        backups_unavailable: unavailable,
        undoable: !pending
            && !unavailable
            && o.pruned_ms.is_none()
            && files.iter().any(|f| f.state == FileState::Done),
        warnings: files
            .iter()
            .filter(|f| f.state == FileState::Done)
            .filter(|f| {
                plan.as_ref()
                    .and_then(|p| p.entries.iter().find(|e| e.seq == f.seq))
                    .is_some_and(|e| e.warnings().next().is_some())
            })
            .count(),
        rolled_back: files
            .iter()
            .filter(|f| {
                f.state == FileState::Cancelled
                    && f.error.as_deref().is_some_and(|e| {
                        e.starts_with("cancelled before the commit")
                            || e.starts_with("cancelled while ExifTool was working")
                    })
            })
            .count(),
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
        acks: store.acks(op_id)?,
        files_detail,
    })
}

/// What an exported log may contain (SECURITY_MODEL §8: redacted unless the user chooses).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExportOptions {
    /// Full paths instead of `asset#n.ext`.
    pub include_paths: bool,
    /// Field values before and after (creator, copyright, time, GPS…).
    pub include_values: bool,
}

/// The detail as it is exported: paths become `asset#n.ext` and field values are left out unless
/// `opts` includes them; error texts are scrubbed of the Operation's paths and the user's name.
pub fn export_view(store: &Store, op_id: &str, opts: ExportOptions) -> Result<OpDetail, CoreError> {
    let mut d = detail(store, op_id)?;
    let known: Vec<(String, String)> = d
        .files_detail
        .iter()
        .flat_map(|f| {
            let a = crate::privacy::alias(f.seq, &f.path);
            let folder = std::path::Path::new(&f.path)
                .parent()
                .map(|p| (p.to_string_lossy().into_owned(), "<folder>".to_string()));
            [Some((f.path.clone(), a)), folder].into_iter().flatten()
        })
        .collect();
    let scrub = |t: &str| -> String {
        if opts.include_paths {
            t.to_owned()
        } else {
            crate::privacy::scrub(t, &known)
        }
    };
    let title = scrub(&d.summary.title);
    d.summary.title = title;
    for f in &mut d.files_detail {
        f.error = f.error.as_deref().map(scrub);
        if !opts.include_paths {
            f.path = crate::privacy::alias(f.seq, &f.path);
        }
        if !opts.include_values {
            for c in &mut f.changes {
                c.before = None;
                c.after = None;
            }
        }
    }
    Ok(d)
}

/// "Export Log": the redacted detail (see [`export_view`]) as JSON, written to a new file (never
/// replacing one). The caller shows the user what it will contain first (ARCHITECTURE §12).
pub fn export_log(
    store: &Store,
    op_id: &str,
    out: &std::path::Path,
    opts: ExportOptions,
) -> Result<(), CoreError> {
    use std::io::Write;
    let d = export_view(store, op_id, opts)?;
    let mut v = serde_json::to_value(&d).map_err(|e| CoreError::Internal(e.to_string()))?;
    v["redacted"] = serde_json::json!({
        "paths": !opts.include_paths,
        "values": !opts.include_values,
    });
    let text = serde_json::to_string_pretty(&v).map_err(|e| CoreError::Internal(e.to_string()))?;
    let taken = || CoreError::Input(format!("{}: a file with this name exists", out.display()));
    if mm_fs::ensure_absent(out).is_err() {
        return Err(taken());
    }
    // written under a name of its own and renamed once complete: an interrupted export never
    // leaves a truncated report under the name the user chose
    let mut partial = out.as_os_str().to_owned();
    partial.push(".partial");
    let partial = std::path::PathBuf::from(partial);
    let written = (|| -> std::io::Result<()> {
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial)?;
        f.write_all(text.as_bytes())?;
        f.sync_all()
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(CoreError::Input(format!("{}: {e}", out.display())));
    }
    match mm_fs::move_no_replace(&partial, out) {
        Ok(()) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&partial);
            match e {
                mm_fs::Win32Error(80 | 183) => Err(taken()),
                e => Err(CoreError::Input(format!("{}: {e}", out.display()))),
            }
        }
    }
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
    // INTERACTION_SPEC §12: a file skipped as read-only comes back only once the attribute is
    // cleared; until then it stays in the Plan, excluded, with the reason
    for e in &mut plan.entries {
        if mm_fs::probe(std::path::Path::new(&e.path)).is_ok_and(|p| p.read_only) {
            e.excluded = true;
            e.notes
                .push("still read-only: clear the attribute to include it".into());
        }
    }
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

/// What a file holds now compared with the Operation (History "Now vs. after operation").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NowState {
    /// The content the Operation wrote (or, for a removal, still absent).
    AsWritten,
    /// The content from before the Operation (undone, or never written).
    Original,
    /// Something else: changed by another program or a later Operation.
    Changed,
    Missing,
    /// A cloud placeholder that is not on this computer: not read (SAFETY_MODEL §8.3).
    NotDownloaded,
}

/// Compare the current files of an Operation with what it wrote, for the given files (the rows
/// on screen: hashing reads each file, so the UI asks page by page).
pub fn now_vs_after(
    store: &Store,
    op_id: &str,
    seqs: &[u32],
) -> Result<Vec<(u32, NowState)>, CoreError> {
    let files = store.files(op_id)?;
    let mut out = Vec::new();
    for &seq in seqs {
        let f = files
            .iter()
            .find(|f| f.seq == seq)
            .ok_or_else(|| CoreError::Input(format!("{op_id} has no file {seq}")))?;
        if crate::is_placeholder(std::path::Path::new(&f.path)) {
            out.push((seq, NowState::NotDownloaded));
            continue;
        }
        let now = crate::hash_opt(std::path::Path::new(&f.path));
        let written = f.state == FileState::Done;
        let state = match (now, written) {
            (None, true) if f.role == crate::ROLE_REMOVE => NowState::AsWritten,
            (None, false) if f.h0.is_none() => NowState::Original, // created by it, not yet
            (None, _) => NowState::Missing,
            (Some(h), true) if Some(&h) == f.h1.as_ref() => NowState::AsWritten,
            (Some(h), _) if Some(&h) == f.h0.as_ref() => NowState::Original,
            (Some(_), false) => NowState::Original, // never written; no pre-image hash kept
            (Some(_), true) => NowState::Changed,
        };
        out.push((seq, state));
    }
    Ok(out)
}

/// One file of "Restore backup to folder…".
#[derive(Debug, Clone, Serialize)]
pub struct Restored {
    pub seq: u32,
    /// The new file, or None when this file has no backup (it did not exist before) or the
    /// backup is missing or damaged.
    pub to: Option<String>,
    pub note: Option<String>,
}

/// "Restore backup to folder…" (SCREEN_SPEC History): copy the content every file had before the
/// Operation into `dir` as new files named like the originals (`name (2).jpg` when taken). The
/// originals are not touched; each copy is checked against the recorded hash, and a copy that
/// does not match is removed again.
pub fn restore_backups_to(
    store: &Store,
    op_id: &str,
    dir: &std::path::Path,
) -> Result<Vec<Restored>, CoreError> {
    let o = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if o.pruned_ms.is_some() {
        return Err(CoreError::Input(format!(
            "the backups of {op_id} were removed by the retention policy"
        )));
    }
    crate::require_backups_present(store, op_id)?;
    std::fs::create_dir_all(dir)?;
    let mut out = Vec::new();
    for f in store.files(op_id)? {
        let backup = std::path::Path::new(&f.backup_path);
        let Some(h0) = f.h0.clone() else {
            out.push(Restored {
                seq: f.seq,
                to: None,
                note: Some("no backup: the file did not exist before or was not reached".into()),
            });
            continue;
        };
        if crate::hash_opt(backup).as_deref() != Some(h0.as_str()) {
            out.push(Restored {
                seq: f.seq,
                to: None,
                note: Some("backup missing or damaged".into()),
            });
            continue;
        }
        let name = std::path::Path::new(&f.path);
        let stem = name
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("file{}", f.seq));
        let ext = name
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        // the copy is made under a name that says it is incomplete and only takes the photo's name
        // once it is whole and verified: an interruption never leaves a partial file that looks
        // like a restored photo
        let mut src = std::fs::File::open(backup)?;
        let mut partial = None;
        for n in 0..10_000 {
            let candidate = dir.join(format!("{stem}{ext}.mmrestore-{n}"));
            match mm_fs::copy_new_hashing(&mut src, &candidate) {
                Ok(h) => {
                    partial = Some((candidate, mm_fs::hex(&h)));
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
        let (partial, h) = partial.ok_or_else(|| CoreError::Input("no free file name".into()))?;
        if h != h0 {
            let _ = std::fs::remove_file(&partial);
            out.push(Restored {
                seq: f.seq,
                to: None,
                note: Some("copy does not match the backup's hash; removed".into()),
            });
            continue;
        }
        let mut placed = None;
        for n in 1..10_000 {
            let candidate = if n == 1 {
                dir.join(format!("{stem}{ext}"))
            } else {
                dir.join(format!("{stem} ({n}){ext}"))
            };
            // never replaces: a taken name is skipped (ERROR_FILE_EXISTS, ERROR_ALREADY_EXISTS)
            match mm_fs::move_no_replace(&partial, &candidate) {
                Ok(()) => {
                    placed = Some(candidate);
                    break;
                }
                Err(mm_fs::Win32Error(80 | 183)) => continue,
                Err(e) => {
                    let _ = std::fs::remove_file(&partial);
                    return Err(CoreError::Io(std::io::Error::other(format!(
                        "cannot name the restored copy: {e}"
                    ))));
                }
            }
        }
        let Some(to) = placed else {
            let _ = std::fs::remove_file(&partial);
            return Err(CoreError::Input("no free file name".into()));
        };
        out.push(Restored {
            seq: f.seq,
            to: Some(to.to_string_lossy().into_owned()),
            note: None,
        });
    }
    Ok(out)
}
