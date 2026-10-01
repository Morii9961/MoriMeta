// SPDX-License-Identifier: GPL-3.0-or-later
//! The commands the frontend may call (BACKEND_INTERFACE §1–5). Each forwards to `mm-core`;
//! long work runs off the IPC thread and reports through the event channel.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use mm_core::executor::{self, ExecOptions, ExecProgress, ProgressSink};
use mm_core::planner::{self, PlanCtl, PlanProgress, PlanStage, TimeTool};
use mm_core::service::{AssetId, EntryFilter, PAGE_SIZE, ServiceError};
use mm_core::{history, inspect, recovery, retention, settings, undo};
use mm_domain::rules::{Action, PRESET_SCHEMA_VERSION, Preset, Rule};
use mm_domain::time::{self, SequenceOrder};
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::core::{Core, ExifToolState, StartupInfo, lock};
use crate::dto::*;

type Res<T> = Result<T, String>;
type CoreState<'a> = State<'a, Arc<Core>>;

fn e(x: impl std::fmt::Display) -> String {
    x.to_string()
}

/// At most about four progress events a second (ARCHITECTURE §5.3), and always the last one.
struct Throttle(Mutex<Instant>);

impl Throttle {
    fn new() -> Throttle {
        Throttle(Mutex::new(Instant::now() - Duration::from_secs(1)))
    }
    fn ready(&self, last: bool) -> bool {
        let mut t = lock(&self.0);
        if last || t.elapsed() >= Duration::from_millis(250) {
            *t = Instant::now();
            true
        } else {
            false
        }
    }
}

// ---------------------------------------------------------------------------------------------
// app state

#[tauri::command]
pub fn subscribe(core: CoreState, on_event: Channel<AppEvent>) {
    let first = lock(&core.events).replace(on_event).is_none();
    // development builds only: `MM_DEV_IMPORT` names folders (separated by ';') to import at
    // launch, so the UI can be exercised by scripts without the file dialog. The paths come from
    // the developer's environment, never from the frontend (ARCHITECTURE §5.1).
    if cfg!(debug_assertions)
        && first
        && let Some(v) = std::env::var_os("MM_DEV_IMPORT")
    {
        let dirs: Vec<PathBuf> = std::env::split_paths(&v).collect();
        let core = core.inner().clone();
        std::thread::spawn(move || import(&core, dirs, true));
    }
}

#[derive(Serialize)]
pub struct AppInfo {
    pub version: &'static str,
    pub data_dir: String,
    pub launched: bool,
    pub exiftool: ExifToolState,
    pub startup: StartupInfo,
    /// Why writes are refused in this run (elevated, ExifTool, integrity), if they are.
    pub writes_refused: Option<String>,
    pub backup: BackupDto,
    pub dev: bool,
}

#[tauri::command]
pub fn app_info(core: CoreState) -> AppInfo {
    let exiftool = lock(&core.exiftool).clone();
    let writes_refused = match core.gate.write() {
        Ok(_) | Err(ServiceError::Busy(_)) => None,
        Err(x) => Some(x.to_string()),
    };
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: core.data.display().to_string(),
        launched: !exiftool.starting,
        exiftool,
        startup: lock(&core.startup).clone(),
        writes_refused,
        backup: backup_dto(&core),
        dev: cfg!(debug_assertions),
    }
}

fn backup_dto(core: &Core) -> BackupDto {
    let store = lock(&core.store);
    let root = store.backup_root().to_path_buf();
    let problem = executor::check_backup_location(&store)
        .err()
        .map(|x| x.to_string());
    let usage = retention::Policy::from_settings(&store)
        .ok()
        .and_then(|p| retention::usage(&store, &p).ok());
    BackupDto {
        root: root.display().to_string(),
        bytes: usage.as_ref().map_or(0, |u| u.total_bytes),
        operations: usage.as_ref().map_or(0, |u| u.ops.len()),
        free: mm_fs::volume_space(&root).ok().map(|s| s.free),
        sync_warning: usage.and_then(|u| u.sync_warning),
        problem,
    }
}

// ---------------------------------------------------------------------------------------------
// import (ARCHITECTURE §5.1: paths come only from the backend's own dialogs and drop events)

const PHOTO_EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "nef", "nrw", "xmp", "tif", "tiff", "png", "heic", "heif", "hif", "avif",
    "webp", "dng", "cr2", "cr3", "arw", "raf", "orf", "rw2", "pef", "srw",
];

#[tauri::command]
pub async fn import_dialog(app: AppHandle, kind: String) -> Res<()> {
    let window = app.get_webview_window("main");
    let picked: Option<(Vec<PathBuf>, bool)> = tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new();
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        if kind == "folder" {
            d.set_title("Add folder").pick_folders().map(|v| (v, true))
        } else {
            d.set_title("Add files")
                .add_filter("Photos and sidecars", PHOTO_EXTENSIONS)
                .add_filter("All files", &["*"])
                .pick_files()
                .map(|v| (v, false))
        }
    })
    .await
    .map_err(e)?;
    if let Some((paths, folders)) = picked {
        let core = app.state::<Arc<Core>>().inner().clone();
        std::thread::spawn(move || import(&core, paths, folders));
    }
    Ok(())
}

/// Files and folders dropped on the window.
pub fn dropped(core: Arc<Core>, paths: Vec<PathBuf>) {
    std::thread::spawn(move || {
        let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) =
            paths.into_iter().partition(|p| p.is_dir());
        if !files.is_empty() {
            import(&core, files, false);
        }
        if !dirs.is_empty() {
            import(&core, dirs, true);
        }
    });
}

fn import(core: &Arc<Core>, paths: Vec<PathBuf>, folders: bool) {
    let (assets, summary, added) = {
        let mut session = lock(&core.session);
        let mut summary = ImportSummary::default();
        let mut added: Vec<AssetId> = Vec::new();
        let mut merge = |r: mm_core::service::ImportReport| {
            let s = ImportSummary::from(&r);
            summary.added += s.added;
            summary.read_only += s.read_only;
            summary.duplicates += s.duplicates;
            summary.failed.extend(s.failed);
            summary.other_files += s.other_files;
            summary.not_followed += s.not_followed;
            summary.placeholders += s.placeholders;
            summary.skipped_folders += s.skipped_folders;
            summary.orphan_sidecars += s.orphan_sidecars;
            summary.backup_files += s.backup_files;
            added.extend(r.added);
        };
        if folders {
            for d in &paths {
                merge(session.import_folder(d));
            }
        } else {
            merge(session.import(&paths));
        }
        let assets: Vec<AssetDto> = added
            .iter()
            .filter_map(|id| AssetDto::of(&session, *id))
            .collect();
        (assets, summary, added)
    };
    core.emit(AppEvent::Imported { assets, summary });
    if !added.is_empty() {
        scan(core.clone(), added);
    }
}

/// Library rows of `ids`, read in batches and pushed as they come (SCREEN_SPEC 1#large).
fn scan(core: Arc<Core>, ids: Vec<AssetId>) {
    std::thread::spawn(move || {
        core.wait_launched();
        let paths = match lock(&core.session).paths(&ids) {
            Ok(p) => p,
            Err(x) => {
                core.emit(AppEvent::ScanDone {
                    cancelled: false,
                    error: Some(x.to_string()),
                });
                return;
            }
        };
        let cancel = Arc::new(AtomicBool::new(false));
        *lock(&core.scan_cancel) = Some(cancel.clone());
        let throttle = Arc::new(Throttle::new());
        let (c2, t2) = (core.clone(), throttle.clone());
        let ctl = PlanCtl {
            progress: Some(Arc::new(move |p: &PlanProgress| {
                if t2.ready(p.done == p.total) {
                    c2.emit(AppEvent::ScanProgress {
                        done: p.done,
                        total: p.total,
                    });
                }
            })),
            cancel: Some(cancel.clone()),
            readers: 0,
        };
        let mut reader = lock(&core.reader);
        let result = match reader.as_mut() {
            None => Err("ExifTool is not available".to_owned()),
            Some(engine) => inspect::scan_rows(engine, &paths, &ctl, &mut |rows| {
                let rows = rows.iter().map(|r| RowDto::of(ids[r.index].0, r)).collect();
                core.emit(AppEvent::Rows { rows });
            })
            .map_err(e),
        };
        drop(reader);
        core.emit(AppEvent::ScanProgress {
            done: paths.len(),
            total: paths.len(),
        });
        let cancelled = cancel.load(Ordering::SeqCst);
        core.emit(AppEvent::ScanDone {
            cancelled,
            error: result.err().filter(|_| !cancelled),
        });
    });
}

#[tauri::command]
pub fn scan_cancel(core: CoreState) {
    if let Some(c) = lock(&core.scan_cancel).as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

/// Read the rows of these files again (after an Operation, or "rescan").
#[tauri::command]
pub fn rescan(core: CoreState, ids: Vec<u64>) {
    scan(core.inner().clone(), ids.into_iter().map(AssetId).collect());
}

/// Take files out of the Session (they stay on disk untouched).
#[tauri::command]
pub fn session_clear(core: CoreState) {
    *lock(&core.session) = Default::default();
}

// ---------------------------------------------------------------------------------------------
// Inspector, selection, attention

fn paths_of(core: &Core, ids: &[u64]) -> Res<Vec<PathBuf>> {
    let ids: Vec<AssetId> = ids.iter().copied().map(AssetId).collect();
    lock(&core.session).paths(&ids).map_err(e)
}

fn with_reader<T>(core: &Core, f: impl FnOnce(&mut mm_core::engine::Engine) -> Res<T>) -> Res<T> {
    core.wait_launched();
    let mut r = lock(&core.reader);
    match r.as_mut() {
        Some(engine) => f(engine),
        None => Err("ExifTool is not available".into()),
    }
}

#[tauri::command]
pub async fn asset_detail(app: AppHandle, id: u64) -> Res<inspect::AssetDetail> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = paths_of(&core, &[id])?.remove(0);
        with_reader(&core, |engine| {
            inspect::asset_detail(engine, &path).map_err(e)
        })
    })
    .await
    .map_err(e)?
}

#[tauri::command]
pub async fn selection_aggregate(
    app: AppHandle,
    ids: Vec<u64>,
) -> Res<Vec<inspect::FieldAggregate>> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = paths_of(&core, &ids)?;
        with_reader(&core, |engine| {
            inspect::selection_aggregate(engine, &paths, &PlanCtl::default()).map_err(e)
        })
    })
    .await
    .map_err(e)?
}

#[tauri::command]
pub async fn attention(app: AppHandle, ids: Vec<u64>) -> Res<AttentionDto> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = paths_of(&core, &ids)?;
        let a = with_reader(&core, |engine| {
            inspect::attention(engine, &paths, &PlanCtl::default()).map_err(e)
        })?;
        let to_ids = |v: Vec<usize>| v.into_iter().filter_map(|i| ids.get(i).copied()).collect();
        let changed: Vec<u64> = lock(&core.session)
            .changed_since_import()
            .into_iter()
            .map(|a| a.0)
            .filter(|i| ids.contains(i))
            .collect();
        Ok(AttentionDto {
            read_only: to_ids(a.read_only),
            conflicts: to_ids(a.conflicts),
            cloud_placeholders: to_ids(a.cloud_placeholders),
            cloud_files: to_ids(a.cloud_files),
            darktable_sidecars: to_ids(a.darktable_sidecars),
            c2pa: to_ids(a.c2pa),
            links: to_ids(a.links),
            unreadable: to_ids(a.unreadable),
            long_paths: to_ids(a.long_paths),
            removable: to_ids(a.removable),
            network: to_ids(a.network),
            other_file_system: to_ids(a.other_file_system),
            changed_since_import: changed,
        })
    })
    .await
    .map_err(e)?
}

/// "Clear read-only attribute…" (INTERACTION_SPEC §7): only as the user's explicit action.
#[tauri::command]
pub fn clear_read_only(core: CoreState, id: u64) -> Res<bool> {
    let path = paths_of(&core, &[id])?.remove(0);
    mm_core::service::clear_read_only(&core.gate, &path).map_err(e)
}

// ---------------------------------------------------------------------------------------------
// Plan and Preview

fn rule(action: Action) -> Rule {
    Rule {
        name: String::new(),
        enabled: true,
        when: vec![],
        then: vec![action],
    }
}

fn time_tool(core: &Core, t: &TimeEditDto) -> Res<TimeTool> {
    let local = |v: &str| time::parse_local(v).ok_or_else(|| format!("{v:?} is not a time"));
    let shift = |v: &str| time::parse_shift(v).ok_or_else(|| format!("{v:?} is not a shift"));
    Ok(match t {
        TimeEditDto::Absolute { to } => TimeTool::Absolute(local(to)?),
        TimeEditDto::Shift { by } => TimeTool::Shift(shift(by)?),
        TimeEditDto::Sequence { start, step, order } => TimeTool::Sequence {
            start: local(start)?,
            step: shift(step)?,
            order: if order == "name" {
                SequenceOrder::NaturalFileName
            } else {
                SequenceOrder::CaptureTimeThenName
            },
        },
        TimeEditDto::Preserve { anchor, to } => TimeTool::PreserveRelative {
            anchor: paths_of(core, &[*anchor])?.remove(0),
            new_local: local(to)?,
        },
    })
}

fn plan_ctl(core: &Arc<Core>) -> PlanCtl {
    let cancel = Arc::new(AtomicBool::new(false));
    *lock(&core.plan_cancel) = Some(cancel.clone());
    let throttle = Throttle::new();
    let c2 = core.clone();
    PlanCtl {
        progress: Some(Arc::new(move |p: &PlanProgress| {
            if throttle.ready(p.done == p.total) {
                c2.emit(AppEvent::PlanProgress {
                    stage: match p.stage {
                        PlanStage::Files => "files",
                        PlanStage::Metadata => "metadata",
                    },
                    done: p.done,
                    total: p.total,
                });
            }
        })),
        cancel: Some(cancel),
        readers: settings::workers(&lock(&core.store)),
    }
}

fn with_planner<T>(core: &Core, f: impl FnOnce(&mut mm_core::engine::Engine) -> Res<T>) -> Res<T> {
    core.wait_launched();
    let mut engines = lock(&core.engines);
    match engines.first_mut() {
        Some(engine) => f(engine),
        None => Err("ExifTool is not available".into()),
    }
}

#[tauri::command]
pub async fn plan_batch(app: AppHandle, ids: Vec<u64>, edit: BatchEditDto) -> Res<PlanView> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = paths_of(&core, &ids)?;
        let mut rules = Vec::new();
        match &edit.creator {
            Some(ListEdit::Set { values }) => rules.push(rule(Action::SetCreator {
                names: values.clone(),
            })),
            Some(ListEdit::Clear) => rules.push(rule(Action::ClearCreator)),
            None => {}
        }
        match &edit.copyright {
            Some(TextEdit::Set { value }) => rules.push(rule(Action::SetCopyright {
                value: value.clone(),
            })),
            Some(TextEdit::Clear) => rules.push(rule(Action::ClearCopyright)),
            None => {}
        }
        match &edit.gps {
            Some(GpsEditDto::Set { position }) => rules.push(rule(Action::SetGps {
                position: position.clone(),
            })),
            Some(GpsEditDto::Remove) => rules.push(rule(Action::RemoveGps)),
            None => {}
        }
        let title = if edit.title.is_empty() {
            "Batch edit".to_owned()
        } else {
            edit.title.clone()
        };
        let preset = Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            name: title.clone(),
            rules,
        };
        let tool = edit
            .time
            .as_ref()
            .map(|t| time_tool(&core, t))
            .transpose()?;
        let ctl = plan_ctl(&core);
        let plan = with_planner(&core, |engine| {
            planner::plan_batch(
                engine,
                &paths,
                &preset,
                tool.as_ref().map(|t| (t, edit.digitized)),
                &title,
                &ctl,
            )
            .map_err(e)
        })?;
        let view = PlanView::of(&plan);
        lock(&core.book).insert(plan);
        Ok(view)
    })
    .await
    .map_err(e)?
}

#[tauri::command]
pub fn plan_cancel(core: CoreState) {
    if let Some(c) = lock(&core.plan_cancel).as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

#[tauri::command]
pub fn plan_view(core: CoreState, id: String, version: u32) -> Res<PlanView> {
    let book = lock(&core.book);
    Ok(PlanView::of(book.version(&id, version).map_err(e)?))
}

fn filter_of(f: &str) -> EntryFilter {
    match f {
        "ready" => EntryFilter::Ready,
        "no_change" => EntryFilter::NoChange,
        "blocked" => EntryFilter::Blocked,
        "unsupported" => EntryFilter::Unsupported,
        "excluded" => EntryFilter::Excluded,
        _ => EntryFilter::All,
    }
}

#[tauri::command]
pub fn plan_page(
    core: CoreState,
    id: String,
    version: u32,
    filter: String,
    page: usize,
) -> Res<Value> {
    let book = lock(&core.book);
    let p = book
        .page(&id, version, filter_of(&filter), page, PAGE_SIZE)
        .map_err(e)?;
    serde_json::to_value(PageDto {
        version: p.version,
        matching: p.matching,
        entries: p.entries.iter().map(|e| EntryDto::of(e)).collect(),
    })
    .map_err(e)
}

#[tauri::command]
pub fn plan_exclude(
    core: CoreState,
    id: String,
    version: u32,
    seqs: Vec<u32>,
    excluded: bool,
) -> Res<PlanView> {
    let mut book = lock(&core.book);
    let v = book.exclude(&id, version, &seqs, excluded).map_err(e)?;
    Ok(PlanView::of(book.version(&id, v).map_err(e)?))
}

#[tauri::command]
pub fn plan_exclude_field(
    core: CoreState,
    id: String,
    version: u32,
    seqs: Option<Vec<u32>>,
    field: String,
    excluded: bool,
) -> Res<PlanView> {
    let mut book = lock(&core.book);
    let v = book
        .exclude_field(&id, version, seqs.as_deref(), &field, excluded)
        .map_err(e)?;
    Ok(PlanView::of(book.version(&id, v).map_err(e)?))
}

#[tauri::command]
pub fn plan_preflight(core: CoreState, id: String, version: u32) -> Res<Value> {
    let book = lock(&core.book);
    let store = lock(&core.store);
    let mut pf =
        serde_json::to_value(book.preflight(&store, &id, version).map_err(e)?).map_err(e)?;
    // ExifTool must be running and the package intact (INTERACTION_SPEC §4)
    let et = lock(&core.exiftool).clone();
    if let Some(problem) = et.error.or(et.integrity) {
        pf["exiftool"] = Value::String(problem);
        pf["ok"] = Value::Bool(false);
    }
    Ok(pf)
}

/// The user confirmed this version in the Preview, with the acknowledgements the dialog
/// collected: a single-use token for exactly it.
#[tauri::command]
pub fn plan_confirm(core: CoreState, id: String, version: u32, acks: Vec<String>) -> Res<String> {
    lock(&core.book)
        .confirm_with(&id, version, &acks)
        .map_err(e)
}

#[tauri::command]
pub async fn op_execute(
    app: AppHandle,
    id: String,
    version: u32,
    token: String,
) -> Res<OpReportDto> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let cancel = Arc::new(AtomicBool::new(false));
        *lock(&core.exec_cancel) = Some(cancel.clone());
        let throttle = Throttle::new();
        let c2 = core.clone();
        let opts = ExecOptions {
            progress: Some(ProgressSink(Arc::new(move |p: &ExecProgress| {
                if throttle.ready(p.done == p.total) {
                    c2.emit(AppEvent::ExecProgress(ExecProgressDto {
                        total: p.total,
                        done: p.done,
                        ok: p.ok,
                        failed: p.failed,
                        skipped: p.skipped,
                        last_seq: p.last.map(|l| l.0),
                        last_state: p.last.map(|l| l.1.as_str().to_owned()),
                    }));
                }
            }))),
            cancel: Some(cancel),
            ..Default::default()
        };
        let mut engines = lock(&core.engines);
        let mut store = lock(&core.store);
        let mut book = lock(&core.book);
        let rep = book
            .execute(
                &core.gate,
                &mut store,
                &mut engines,
                &id,
                version,
                &token,
                &opts,
            )
            .map_err(e)?;
        *lock(&core.exec_cancel) = None;
        Ok(OpReportDto::from(&rep))
    })
    .await
    .map_err(e)?
}

/// Cancel… (INTERACTION_SPEC §10): no new files; a file not yet committed is abandoned.
#[tauri::command]
pub fn op_cancel(core: CoreState) {
    if let Some(c) = lock(&core.exec_cancel).as_ref() {
        c.store(true, Ordering::SeqCst);
    }
}

// ---------------------------------------------------------------------------------------------
// History, undo, recovery

#[tauri::command]
pub fn history_list(core: CoreState, page: usize) -> Res<Vec<history::OpSummary>> {
    history::list(&lock(&core.store), page, 100).map_err(e)
}

#[tauri::command]
pub fn op_detail(core: CoreState, op_id: String) -> Res<history::OpDetail> {
    history::detail(&lock(&core.store), &op_id).map_err(e)
}

fn exiftool_version(core: &Core) -> Res<String> {
    core.wait_launched();
    lock(&core.exiftool)
        .version
        .clone()
        .ok_or_else(|| "ExifTool is not available".into())
}

#[tauri::command]
pub fn undo_plan(core: CoreState, op_id: String) -> Res<PlanView> {
    let v = exiftool_version(&core)?;
    let plan = undo::plan_undo(&lock(&core.store), &op_id, &v).map_err(e)?;
    let view = PlanView::of(&plan);
    lock(&core.book).insert(plan);
    Ok(view)
}

/// Retry failed (INTERACTION_SPEC §12, DECISIONS H-2): a new Plan that goes through Preview.
#[tauri::command]
pub fn retry_plan(core: CoreState, op_id: String) -> Res<PlanView> {
    let v = exiftool_version(&core)?;
    let plan = history::retry_plan(&lock(&core.store), &op_id, &v).map_err(e)?;
    let view = PlanView::of(&plan);
    lock(&core.book).insert(plan);
    Ok(view)
}

/// Export log… (ARCHITECTURE §12): the Operation as JSON in a new file the user names; paths
/// become `asset#n.ext` and values are left out unless included. Returns the file written, or
/// None when the dialog was cancelled.
#[tauri::command]
pub async fn export_log(
    app: AppHandle,
    op_id: String,
    include_paths: bool,
    include_values: bool,
) -> Res<Option<String>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new()
            .set_title("Export log")
            .set_file_name(format!("MoriMeta-{op_id}.json"))
            .add_filter("JSON", &["json"]);
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(out) = d.save_file() else {
            return Ok(None);
        };
        let opts = history::ExportOptions {
            include_paths,
            include_values,
        };
        history::export_log(&lock(&core.store), &op_id, &out, opts).map_err(e)?;
        Ok(Some(out.display().to_string()))
    })
    .await
    .map_err(e)?
}

#[derive(Serialize)]
pub struct RestoreReport {
    pub folder: String,
    pub restored: usize,
    pub without_backup: usize,
    pub notes: Vec<String>,
}

/// Restore backup to folder… (SCREEN_SPEC History): copies of the files as they were before the
/// Operation, as new files in a folder the user picks; the originals are not touched.
#[tauri::command]
pub async fn restore_to(app: AppHandle, op_id: String) -> Res<Option<RestoreReport>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new().set_title("Restore backup to folder");
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(dir) = d.pick_folder() else {
            return Ok(None);
        };
        let _permit = core.gate.write().map_err(e)?;
        let done = history::restore_backups_to(&lock(&core.store), &op_id, &dir).map_err(e)?;
        Ok(Some(RestoreReport {
            folder: dir.display().to_string(),
            restored: done.iter().filter(|r| r.to.is_some()).count(),
            without_backup: done.iter().filter(|r| r.to.is_none()).count(),
            notes: done.into_iter().filter_map(|r| r.note).collect(),
        }))
    })
    .await
    .map_err(e)?
}

#[tauri::command]
pub fn recovery_status(core: CoreState) -> Res<Vec<recovery::RecoverySummary>> {
    recovery::summary(&lock(&core.store)).map_err(e)
}

/// "Keep as is and close".
#[tauri::command]
pub fn recovery_dismiss(core: CoreState, op_id: String) -> Res<()> {
    recovery::dismiss(&mut lock(&core.store), &op_id).map_err(e)
}

/// "Continue remaining".
#[tauri::command]
pub async fn recovery_resume(app: AppHandle, op_id: String) -> Res<OpReportDto> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let _permit = core.gate.write().map_err(e)?;
        let mut engines = lock(&core.engines);
        let mut store = lock(&core.store);
        let rep = executor::resume(&mut store, &mut engines, &op_id, &ExecOptions::default())
            .map_err(e)?;
        Ok(OpReportDto::from(&rep))
    })
    .await
    .map_err(e)?
}

// ---------------------------------------------------------------------------------------------
// settings

#[tauri::command]
pub fn settings_list(core: CoreState) -> Res<Vec<SettingDto>> {
    let store = lock(&core.store);
    settings::KEYS
        .iter()
        .map(|k| {
            Ok(SettingDto {
                name: k.name,
                value: settings::get(&store, k.name).map_err(e)?,
                default: k.default,
                about: k.about,
            })
        })
        .collect()
}

#[tauri::command]
pub fn setting_set(core: CoreState, name: String, value: String) -> Res<()> {
    let mut store = lock(&core.store);
    settings::set(&mut store, &name, &value).map_err(e)?;
    if name == "backup.root" && !value.is_empty() {
        store.set_backup_root(PathBuf::from(value));
    }
    Ok(())
}
