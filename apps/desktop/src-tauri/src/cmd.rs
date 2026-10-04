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
use mm_core::{history, inspect, presets, recovery, retention, settings, undo};
use mm_domain::rules::{Action, PRESET_SCHEMA_VERSION, Preset, Rule};
use mm_domain::time::{self, SequenceOrder};
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, State};

use crate::core::{Core, ExifToolState, StartupInfo, lock};
use crate::dto::*;
use crate::errors::ue;

type Res<T> = Result<T, String>;
type CoreState<'a> = State<'a, Arc<Core>>;

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

/// About MoriMeta (RELEASE_PLAN §6): the versions a bug report needs.
#[derive(Serialize)]
pub struct AboutDto {
    pub version: &'static str,
    pub exiftool: Option<String>,
    pub registry_version: u32,
    pub webview2: Option<String>,
    pub os: String,
    pub dev: bool,
}

#[tauri::command]
pub fn about(core: CoreState) -> AboutDto {
    AboutDto {
        version: env!("CARGO_PKG_VERSION"),
        exiftool: lock(&core.exiftool).version.clone(),
        registry_version: mm_domain::creator::REGISTRY_VERSION,
        webview2: tauri::webview_version().ok(),
        os: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        dev: cfg!(debug_assertions),
    }
}

const NOTICES: &str = "THIRD_PARTY_NOTICES.md";

/// About › Third-party notices (RELEASE_PLAN §7.2): the file `tools/third_party_notices.py`
/// generates when the installer is staged, shipped next to the program.
#[tauri::command]
pub fn third_party_notices(app: AppHandle) -> Res<String> {
    let mut places = Vec::new();
    if let Ok(r) = app.path().resource_dir() {
        places.push(r.join(NOTICES));
    }
    if cfg!(debug_assertions) {
        places.push(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("staging")
                .join(NOTICES),
        );
    }
    places
        .iter()
        .find_map(|p| std::fs::read_to_string(p).ok())
        .ok_or_else(|| {
            "the third-party notices are added when the installer is built \
             (python apps/desktop/scripts/stage-exiftool.py)"
                .to_string()
        })
}

#[tauri::command]
pub fn app_info(core: CoreState) -> AppInfo {
    let exiftool = lock(&core.exiftool).clone();
    let writes_refused = match core.gate.write() {
        Ok(_) | Err(ServiceError::Busy(_)) => None,
        Err(x) => Some(ue(x)),
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
    let problem = executor::check_backup_location(&store).err().map(ue);
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
    .map_err(ue)?;
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
                    error: Some(ue(x)),
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
            .map_err(ue),
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
    lock(&core.session).paths(&ids).map_err(ue)
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
            inspect::asset_detail(engine, &path).map_err(ue)
        })
    })
    .await
    .map_err(ue)?
}

/// The Inspector's embedded preview (SCREEN_SPEC §2): the file's own JPEG thumbnail as a
/// `data:` URL (the CSP allows images only from the app and `data:`), or None.
#[tauri::command]
pub async fn asset_preview(app: AppHandle, id: u64) -> Res<Option<String>> {
    use base64::Engine as _;
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let path = paths_of(&core, &[id])?.remove(0);
        let jpeg = with_reader(&core, |engine| {
            inspect::embedded_preview(engine, &path).map_err(ue)
        })?;
        Ok(jpeg.map(|b| {
            format!(
                "data:image/jpeg;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(b)
            )
        }))
    })
    .await
    .map_err(ue)?
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
            inspect::selection_aggregate(engine, &paths, &PlanCtl::default()).map_err(ue)
        })
    })
    .await
    .map_err(ue)?
}

#[tauri::command]
pub async fn attention(app: AppHandle, ids: Vec<u64>) -> Res<AttentionDto> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = paths_of(&core, &ids)?;
        let a = with_reader(&core, |engine| {
            inspect::attention(engine, &paths, &PlanCtl::default()).map_err(ue)
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
    .map_err(ue)?
}

/// "Clear read-only attribute…" (INTERACTION_SPEC §7): only as the user's explicit action.
#[tauri::command]
pub fn clear_read_only(core: CoreState, id: u64) -> Res<bool> {
    let path = paths_of(&core, &[id])?.remove(0);
    mm_core::service::clear_read_only(&core.gate, &path).map_err(ue)
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
            .map_err(ue)
        })?;
        let view = PlanView::of(&plan);
        lock(&core.book).insert(plan);
        Ok(view)
    })
    .await
    .map_err(ue)?
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
    Ok(PlanView::of(book.version(&id, version).map_err(ue)?))
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
        .map_err(ue)?;
    serde_json::to_value(PageDto {
        version: p.version,
        matching: p.matching,
        entries: p.entries.iter().map(|e| EntryDto::of(e)).collect(),
    })
    .map_err(ue)
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
    let v = book.exclude(&id, version, &seqs, excluded).map_err(ue)?;
    Ok(PlanView::of(book.version(&id, v).map_err(ue)?))
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
        .map_err(ue)?;
    Ok(PlanView::of(book.version(&id, v).map_err(ue)?))
}

#[tauri::command]
pub fn plan_preflight(core: CoreState, id: String, version: u32) -> Res<Value> {
    let book = lock(&core.book);
    let store = lock(&core.store);
    let mut pf =
        serde_json::to_value(book.preflight(&store, &id, version).map_err(ue)?).map_err(ue)?;
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
        .map_err(ue)
}

fn execution_options(core: &Arc<Core>, cancel: Arc<std::sync::atomic::AtomicBool>) -> ExecOptions {
    let throttle = Throttle::new();
    let c2 = core.clone();
    ExecOptions {
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
    }
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
        let active = core.begin_write()?;
        let opts = execution_options(&core, active.cancel.clone());
        let mut engines = lock(&core.engines);
        let mut store = lock(&core.store);
        core.sync_workers(&active.permit, &mut engines, &store)?;
        let mut book = lock(&core.book);
        let rep = book
            .execute_permitted(
                &active.permit,
                &mut store,
                &mut engines,
                &id,
                version,
                &token,
                &opts,
            )
            .map_err(ue)?;
        Ok(OpReportDto::from(&rep))
    })
    .await
    .map_err(ue)?
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
    history::list(&lock(&core.store), page, 100).map_err(ue)
}

#[tauri::command]
pub fn op_detail(core: CoreState, op_id: String) -> Res<history::OpDetail> {
    history::detail(&lock(&core.store), &op_id).map_err(ue)
}

/// History "Now vs. after operation" for the files on screen (hashing reads each file).
#[tauri::command]
pub async fn now_vs_after(
    app: AppHandle,
    op_id: String,
    seqs: Vec<u32>,
) -> Res<Vec<(u32, history::NowState)>> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        history::now_vs_after(&lock(&core.store), &op_id, &seqs).map_err(ue)
    })
    .await
    .map_err(ue)?
}

fn exiftool_version(core: &Core) -> Res<String> {
    core.wait_launched();
    lock(&core.exiftool)
        .version
        .clone()
        .ok_or_else(|| "ExifTool is not available".into())
}

#[tauri::command]
pub fn undo_plan(core: CoreState, op_id: String, force_seqs: Option<Vec<u32>>) -> Res<PlanView> {
    let v = exiftool_version(&core)?;
    let mut plan = undo::plan_undo(&lock(&core.store), &op_id, &v).map_err(ue)?;
    if let Some(seqs) = force_seqs {
        for seq in seqs {
            let e = plan
                .entries
                .iter_mut()
                .find(|e| e.seq == seq)
                .ok_or_else(|| format!("plan has no entry {seq}"))?;
            if !undo::is_forced(e) {
                return Err("only conflicting undo entries can be forced".into());
            }
            e.excluded = false;
        }
    }
    let view = PlanView::of(&plan);
    lock(&core.book).insert(plan);
    Ok(view)
}

/// Retry failed (INTERACTION_SPEC §12, DECISIONS H-2): a new Plan that goes through Preview.
#[tauri::command]
pub fn retry_plan(core: CoreState, op_id: String) -> Res<PlanView> {
    let v = exiftool_version(&core)?;
    let plan = history::retry_plan(&lock(&core.store), &op_id, &v).map_err(ue)?;
    let view = PlanView::of(&plan);
    lock(&core.book).insert(plan);
    Ok(view)
}

/// Failed/skipped/conflicting files are read again, rather than replaying their stale Preview.
#[tauri::command]
pub async fn plan_again(app: AppHandle, op_id: String) -> Res<PlanView> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let ctl = plan_ctl(&core);
        let plan = with_planner(&core, |engine| {
            history::replan(engine, &lock(&core.store), &op_id, &ctl).map_err(ue)
        })?;
        let view = PlanView::of(&plan);
        lock(&core.book).insert(plan);
        Ok(view)
    })
    .await
    .map_err(ue)?
}

/// Resolve selected attention files by keeping their current content; no photo is written.
#[tauri::command]
pub async fn recovery_keep(app: AppHandle, op_id: String, seqs: Vec<u32>) -> Res<()> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let _active = core.begin_write()?;
        recovery::resolve_keep(&mut lock(&core.store), &op_id, &seqs).map_err(ue)
    })
    .await
    .map_err(ue)?
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
        history::export_log(&lock(&core.store), &op_id, &out, opts).map_err(ue)?;
        Ok(Some(out.display().to_string()))
    })
    .await
    .map_err(ue)?
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
        core.wait_launched();
        let _active = core.begin_write()?;
        let done = history::restore_backups_to(&lock(&core.store), &op_id, &dir).map_err(ue)?;
        Ok(Some(RestoreReport {
            folder: dir.display().to_string(),
            restored: done.iter().filter(|r| r.to.is_some()).count(),
            without_backup: done.iter().filter(|r| r.to.is_none()).count(),
            notes: done.into_iter().filter_map(|r| r.note).collect(),
        }))
    })
    .await
    .map_err(ue)?
}

/// Find history in a backup folder… (Settings › Advanced): after the whole data folder was lost,
/// bring back the Operations kept in a backup location the user picks (BACKEND_INTERFACE §6).
/// Under the write gate, since crash recovery runs for interrupted ones; the launch state is
/// updated so their recovery decisions are asked for. None when the dialog was cancelled.
#[tauri::command]
pub async fn history_import(app: AppHandle) -> Res<Option<history::HistoryImport>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new().set_title("Find history in a backup folder");
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(dir) = d.pick_folder() else {
            return Ok(None);
        };
        core.wait_launched();
        let _active = core.begin_write()?;
        let found = {
            let mut store = lock(&core.store);
            let found = history::import_from_folder(&mut store, &dir).map_err(ue)?;
            let mut info = lock(&core.startup);
            info.recovered_files += found.recovered_files;
            info.recovery_waiting = recovery::waiting_for_backups(&store).map_err(ue)?;
            info.needs_decision = recovery::summary(&store).map_err(ue)?;
            found
        };
        core.emit(AppEvent::Status);
        Ok(Some(found))
    })
    .await
    .map_err(ue)?
}

#[tauri::command]
pub fn recovery_status(core: CoreState) -> Res<Vec<recovery::RecoverySummary>> {
    recovery::summary(&lock(&core.store)).map_err(ue)
}

/// "Keep as is and close".
#[tauri::command]
pub fn recovery_dismiss(core: CoreState, op_id: String) -> Res<()> {
    recovery::dismiss(&mut lock(&core.store), &op_id).map_err(ue)
}

/// "Continue remaining".
#[tauri::command]
pub async fn recovery_resume(app: AppHandle, op_id: String) -> Res<OpReportDto> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let active = core.begin_write()?;
        let mut engines = lock(&core.engines);
        let mut store = lock(&core.store);
        core.sync_workers(&active.permit, &mut engines, &store)?;
        let opts = execution_options(&core, active.cancel.clone());
        let rep = executor::resume(&mut store, &mut engines, &op_id, &opts).map_err(ue)?;
        Ok(OpReportDto::from(&rep))
    })
    .await
    .map_err(ue)?
}

/// Close the window once nothing is running (after "Stop, then close").
#[tauri::command]
pub fn app_close(app: AppHandle, core: CoreState) -> Res<()> {
    if core.running() {
        return Err("an Operation is still running".into());
    }
    if let Some(w) = app.get_webview_window("main") {
        w.close().map_err(ue)?;
    }
    Ok(())
}

// ---------------------------------------------------------------------------------------------
// Presets and rules (PRODUCT_SPEC §6.10–6.11, SCREEN_SPEC §5–6)

#[derive(Serialize)]
pub struct PresetDto {
    pub id: String,
    pub name: String,
    pub builtin: bool,
    pub fields: Vec<&'static str>,
    pub last_used_ms: Option<i64>,
    /// Imported and not used yet: its first Plan notes every change (SECURITY_MODEL §9).
    pub untrusted: bool,
    pub preset: Preset,
    /// A template variable reads a field another enabled rule changes (INTERACTION_SPEC §1).
    pub lint: Vec<String>,
}

#[tauri::command]
pub fn presets_list(core: CoreState) -> Res<Vec<PresetDto>> {
    let store = lock(&core.store);
    Ok(presets::list(&store)
        .map_err(ue)?
        .into_iter()
        .map(|p| PresetDto {
            lint: p.preset.lint(),
            id: p.id,
            name: p.name,
            builtin: p.builtin,
            fields: p.fields.iter().map(|f| f.name()).collect(),
            last_used_ms: p.last_used_ms,
            untrusted: p.untrusted,
            preset: p.preset,
        })
        .collect())
}

/// Save a Preset (new when `id` is None); it is validated first. Returns its id.
#[tauri::command]
pub fn preset_save(core: CoreState, id: Option<String>, preset: Preset) -> Res<String> {
    presets::save(&mut lock(&core.store), id.as_deref(), &preset).map_err(ue)
}

#[tauri::command]
pub fn preset_duplicate(core: CoreState, id: String) -> Res<String> {
    presets::duplicate(&mut lock(&core.store), &id).map_err(ue)
}

#[tauri::command]
pub fn preset_delete(core: CoreState, id: String) -> Res<()> {
    presets::delete(&mut lock(&core.store), &id).map_err(ue)
}

/// Import… : a Preset file the user picks, checked (size, schema, limits) and kept as untrusted
/// until first used. Returns its id, or None when the dialog was cancelled.
#[tauri::command]
pub async fn preset_import(app: AppHandle) -> Res<Option<String>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new()
            .set_title("Import preset")
            .add_filter("MoriMeta preset", &["json"]);
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(path) = d.pick_file() else {
            return Ok(None);
        };
        let size = std::fs::metadata(&path).map_err(ue)?.len();
        if size > mm_domain::rules::MAX_PRESET_BYTES as u64 {
            return Err(format!(
                "a preset file is at most {} KB (this one is {} KB)",
                mm_domain::rules::MAX_PRESET_BYTES >> 10,
                size >> 10
            ));
        }
        let json = std::fs::read_to_string(&path).map_err(ue)?;
        presets::import(&mut lock(&core.store), &json)
            .map(Some)
            .map_err(ue)
    })
    .await
    .map_err(ue)?
}

/// Export… : the Preset as JSON to a file the user names.
#[tauri::command]
pub async fn preset_export(app: AppHandle, id: String) -> Res<Option<String>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let p = presets::get(&lock(&core.store), &id).map_err(ue)?;
        let mut d = rfd::FileDialog::new()
            .set_title("Export preset")
            .set_file_name(format!(
                "{}.json",
                p.name
                    .replace(['\\', '/', ':', '*', '?', '"', '<', '>', '|'], "_")
            ))
            .add_filter("MoriMeta preset", &["json"]);
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(out) = d.save_file() else {
            return Ok(None);
        };
        std::fs::write(&out, p.preset.to_json()).map_err(ue)?;
        Ok(Some(out.display().to_string()))
    })
    .await
    .map_err(ue)?
}

/// Apply a Preset to files: a Plan that opens in Preview (SCREEN_SPEC 3#p-apply).
#[tauri::command]
pub async fn plan_preset(app: AppHandle, ids: Vec<u64>, preset_id: String) -> Res<PlanView> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = paths_of(&core, &ids)?;
        let info = presets::get(&lock(&core.store), &preset_id).map_err(ue)?;
        let ctl = plan_ctl(&core);
        let mut plan = with_planner(&core, |engine| {
            planner::plan_preset(engine, &paths, &info.preset, &ctl).map_err(ue)
        })?;
        if info.untrusted {
            presets::mark_untrusted(&mut plan);
        }
        presets::used(&mut lock(&core.store), &preset_id).map_err(ue)?;
        let view = PlanView::of(&plan);
        lock(&core.book).insert(plan);
        Ok(view)
    })
    .await
    .map_err(ue)?
}

// ---------------------------------------------------------------------------------------------
// Clean Export (D-15 (c), PRODUCT_SPEC §6.8.3)

#[derive(Serialize)]
pub struct CleanEntryDto {
    pub seq: u32,
    pub name: String,
    pub source: String,
    pub status: mm_core::clean_export::CleanStatus,
    /// Removed tags per privacy category.
    pub categories: Vec<(String, usize)>,
    pub removed: usize,
    pub kept: usize,
    pub segments: Vec<mm_domain::clean::RemovedSegment>,
    pub lens_lost: bool,
}

#[derive(Serialize)]
pub struct CleanPlanDto {
    pub id: String,
    pub entries: Vec<CleanEntryDto>,
}

/// Build the Clean Export Preview for these files: what each copy loses. Nothing is written.
#[tauri::command]
pub async fn clean_plan(
    app: AppHandle,
    ids: Vec<u64>,
    spec: mm_domain::clean::KeepSpec,
) -> Res<CleanPlanDto> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let paths = paths_of(&core, &ids)?;
        let ctl = plan_ctl(&core);
        let plan = with_planner(&core, |engine| {
            mm_core::clean_export::plan(engine, &paths, spec, &ctl).map_err(ue)
        })?;
        let dto = CleanPlanDto {
            id: plan.id.clone(),
            entries: plan
                .entries
                .iter()
                .map(|e| {
                    let mut cats: Vec<(String, usize)> = Vec::new();
                    for r in &e.prediction.remove {
                        match cats.iter_mut().find(|(c, _)| *c == r.category) {
                            Some((_, n)) => *n += 1,
                            None => cats.push((r.category.clone(), 1)),
                        }
                    }
                    cats.sort_by_key(|c| std::cmp::Reverse(c.1));
                    CleanEntryDto {
                        seq: e.seq,
                        name: e.name.clone(),
                        source: e.source.clone(),
                        status: e.status.clone(),
                        categories: cats,
                        removed: e.prediction.remove.len(),
                        kept: e.prediction.keep.len(),
                        segments: e.prediction.remove_segments.clone(),
                        lens_lost: e.prediction.lens_lost,
                    }
                })
                .collect(),
        };
        *lock(&core.clean) = Some(plan);
        Ok(dto)
    })
    .await
    .map_err(ue)?
}

/// Every tag one copy loses and keeps (the File detail of the Clean Export Preview).
#[tauri::command]
pub fn clean_entry(core: CoreState, seq: u32) -> Res<mm_domain::clean::Prediction> {
    let plan = lock(&core.clean);
    let plan = plan.as_ref().ok_or("no Clean Export preview")?;
    plan.entries
        .iter()
        .find(|e| e.seq == seq)
        .map(|e| e.prediction.clone())
        .ok_or_else(|| format!("plan has no entry {seq}"))
}

/// Export the current Clean Export plan into a folder the user picks (the backend's dialog,
/// ARCHITECTURE §5.1). Returns None when the dialog was cancelled.
#[tauri::command]
pub async fn clean_export(
    app: AppHandle,
    plan_id: String,
    number_taken: bool,
) -> Res<Option<Vec<mm_core::clean_export::Exported>>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut d =
            rfd::FileDialog::new().set_title("Clean export: choose the folder for the copies");
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(dir) = d.pick_folder() else {
            return Ok(None);
        };
        export_clean(&core, &plan_id, &dir, number_taken).map(Some)
    })
    .await
    .map_err(ue)?
}

/// The dialog and this helper use the same write gate as Apply and Restore to folder.
fn export_clean(
    core: &Arc<Core>,
    plan_id: &str,
    dir: &std::path::Path,
    number_taken: bool,
) -> Res<Vec<mm_core::clean_export::Exported>> {
    // Refuse a busy/disabled request immediately; reserve the actual slot after launch has
    // completed its integrity and recovery checks.
    drop(core.gate.write().map_err(ue)?);
    core.wait_launched();
    let active = core.begin_write()?;
    executor::require_no_pending_recovery(&lock(&core.store)).map_err(ue)?;
    let plan = clean_preview(core, plan_id)?;
    let mut engines = lock(&core.engines);
    let engine = engines.first_mut().ok_or("ExifTool is not available")?;
    let throttle = Throttle::new();
    let c2 = core.clone();
    let progress = move |done: usize, total: usize| {
        if throttle.ready(done == total) {
            c2.emit(AppEvent::ExecProgress(ExecProgressDto {
                total,
                done,
                ..Default::default()
            }));
        }
    };
    let on = if number_taken {
        mm_core::clean_export::OnConflict::Number
    } else {
        mm_core::clean_export::OnConflict::Skip
    };
    mm_core::clean_export::export(
        engine,
        &plan,
        dir,
        on,
        &progress,
        Some(active.cancel.clone()),
    )
    .map_err(ue)
}

fn clean_preview(core: &Core, id: &str) -> Res<mm_core::clean_export::CleanPlan> {
    let plan = lock(&core.clean).clone().ok_or("no Clean Export preview")?;
    if plan.id != id {
        return Err("the Clean Export preview changed; preview it again".into());
    }
    Ok(plan)
}

/// Backup location… (First Launch step 2, Settings › Backup): a folder the user picks in the
/// backend's dialog. It must be local and writable (INTERACTION_SPEC §15); it applies to later
/// Operations. Returns the backup state, or None when the dialog was cancelled.
#[tauri::command]
pub async fn choose_backup_folder(app: AppHandle) -> Res<Option<BackupDto>> {
    let window = app.get_webview_window("main");
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        let mut d = rfd::FileDialog::new().set_title("Backup location");
        if let Some(w) = &window {
            d = d.set_parent(w);
        }
        let Some(dir) = d.pick_folder() else {
            return Ok(None);
        };
        {
            let mut store = lock(&core.store);
            let previous = store.backup_root().to_path_buf();
            store.set_backup_root(dir.clone());
            // refuse a location writes could not use (network, removable, not writable)
            if let Err(why) = executor::check_backup_location(&store) {
                store.set_backup_root(previous);
                return Err(ue(why));
            }
            // Validation used the proposed root; restore it until persistence succeeds.
            store.set_backup_root(previous);
            settings::set(&mut store, "backup.root", &dir.display().to_string()).map_err(ue)?;
        }
        Ok(Some(backup_dto(&core)))
    })
    .await
    .map_err(ue)?
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
                value: settings::get(&store, k.name).map_err(ue)?,
                default: k.default,
                about: k.about,
            })
        })
        .collect()
}

#[tauri::command]
pub fn setting_set(core: CoreState, name: String, value: String) -> Res<()> {
    let mut store = lock(&core.store);
    settings::set(&mut store, &name, &value).map_err(ue)?;
    Ok(())
}

/// Backup management uses opaque Operation ids; no deletion path comes from the frontend.
#[tauri::command]
pub async fn backup_usage(app: AppHandle) -> Res<retention::Usage> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let store = lock(&core.store);
        let policy = retention::Policy::from_settings(&store).map_err(ue)?;
        retention::usage(&store, &policy).map_err(ue)
    })
    .await
    .map_err(ue)?
}

/// None selects policy candidates; Some selects the backups the user explicitly chose.
#[tauri::command]
pub async fn prune_preview(
    app: AppHandle,
    op_ids: Option<Vec<String>>,
) -> Res<mm_core::backups::PrunePreview> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let store = lock(&core.store);
        lock(&core.prunes)
            .preview(&store, op_ids.as_deref())
            .map_err(ue)
    })
    .await
    .map_err(ue)?
}

/// Executes exactly the previewed selection; changed policy/protection requires another preview.
#[tauri::command]
pub async fn prune_execute(app: AppHandle, token: String) -> Res<Vec<String>> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let active = core.begin_write()?;
        let mut store = lock(&core.store);
        lock(&core.prunes)
            .execute_permitted(&active.permit, &mut store, &token)
            .map_err(ue)
    })
    .await
    .map_err(ue)?
}

#[tauri::command]
pub fn backup_keep(core: CoreState, op_id: String, keep: bool) -> Res<()> {
    mm_core::backups::keep(&core.gate, &mut lock(&core.store), &op_id, keep).map_err(ue)
}

#[tauri::command]
pub fn settings_reset(core: CoreState) -> Res<()> {
    let _permit = core.gate.write().map_err(ue)?;
    settings::reset_all(&mut lock(&core.store)).map_err(ue)
}

#[tauri::command]
pub fn settings_migrations(core: CoreState) -> Vec<(i64, String)> {
    lock(&core.store).migrations()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_export_checks_the_gate_before_waiting_or_changing_cancel_state() {
        let dir = std::env::temp_dir().join(format!("mm-app-clean-gate-{}", std::process::id()));
        let core = Arc::new(Core::open_at(dir.clone()).unwrap());
        let active_cancel = Arc::new(AtomicBool::new(false));
        *lock(&core.exec_cancel) = Some(active_cancel.clone());
        // No launch, plan or engine: gate refusal must occur before any of those are accessed.
        match core.gate.write() {
            Ok(_permit) => {
                assert_eq!(
                    export_clean(&core, "unused", &dir, false).unwrap_err(),
                    "an Operation is already running"
                );
            }
            Err(ServiceError::Elevated) => {
                assert!(
                    export_clean(&core, "unused", &dir, false)
                        .unwrap_err()
                        .contains("administrator rights")
                );
            }
            Err(e) => panic!("unexpected gate error: {e:?}"),
        }
        assert!(Arc::ptr_eq(
            lock(&core.exec_cancel).as_ref().unwrap(),
            &active_cancel
        ));
        assert!(!core.running());
        let first = core.mark_running();
        let second = core.mark_running();
        drop(second);
        assert!(
            core.running(),
            "one command finishing must not hide the other"
        );
        drop(first);
        assert!(!core.running());
        if let Ok(active) = core.begin_write() {
            let cancel = active.cancel.clone();
            assert!(core.running());
            assert!(core.begin_write().is_err());
            assert!(Arc::ptr_eq(
                lock(&core.exec_cancel).as_ref().unwrap(),
                &cancel
            ));
            cancel.store(true, Ordering::SeqCst);
            assert!(
                lock(&core.exec_cancel)
                    .as_ref()
                    .unwrap()
                    .load(Ordering::SeqCst)
            );
            drop(active);
            assert!(lock(&core.exec_cancel).is_none());
            assert!(!core.running());
        }
        core.gate.refuse_writes("test integrity failure");
        let err = export_clean(&core, "unused", &dir, false).unwrap_err();
        assert!(
            err.contains("writing is disabled") || err.contains("administrator rights"),
            "{err}"
        );
        *lock(&core.clean) = Some(mm_core::clean_export::CleanPlan {
            id: "current".into(),
            spec: Default::default(),
            entries: vec![],
        });
        assert!(clean_preview(&core, "old").is_err());
        assert_eq!(clean_preview(&core, "current").unwrap().id, "current");
        drop(core);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
