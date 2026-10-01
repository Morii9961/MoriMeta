// SPDX-License-Identifier: GPL-3.0-or-later
//! The app's backend state (BACKEND_INTERFACE §0 launch sequence): data folder and its instance
//! lock, the Journal, the write gate, ExifTool sessions, the Session of imported files and the
//! Plans awaiting Preview. Everything that decides what is written lives in `mm-core`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use mm_core::engine::Engine;
use mm_core::integrity::Scope;
use mm_core::service::{self, InstanceLock, OperationGate, PlanBook, Session};
use mm_exiftool::EngineConfig;
use mm_store::Store;
use serde::Serialize;
use tauri::ipc::Channel;

use crate::dto::AppEvent;

/// The pinned ExifTool version (research/exiftool.lock.json), for the development package.
const LOCK: &str = include_str!("../../../../research/exiftool.lock.json");

pub struct Core {
    pub data: PathBuf,
    _lock: InstanceLock,
    pub store: Mutex<Store>,
    pub gate: OperationGate,
    /// Sessions for planning and execution (one per worker).
    pub engines: Mutex<Vec<Engine>>,
    /// One session for what the UI reads (Library rows, Inspector, selection aggregate), so a
    /// scan does not wait for a Plan and the other way round.
    pub reader: Mutex<Option<Engine>>,
    pub exiftool: Mutex<ExifToolState>,
    pub session: Mutex<Session>,
    pub book: Mutex<PlanBook>,
    pub startup: Mutex<StartupInfo>,
    /// The frontend's event channel (`subscribe`).
    pub events: Mutex<Option<Channel<AppEvent>>>,
    pub scan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub plan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub exec_cancel: Mutex<Option<Arc<AtomicBool>>>,
    launched: (Mutex<bool>, Condvar),
    /// A write Operation (apply, undo, resume) is running: closing the window asks first.
    running: AtomicBool,
}

/// Marks a write Operation as running for as long as it lives.
pub struct Running<'a>(&'a AtomicBool);

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExifToolState {
    /// Still starting.
    pub starting: bool,
    pub version: Option<String>,
    /// ExifTool could not be started: nothing can be read or written (SCREEN_SPEC 6#e-exif).
    pub error: Option<String>,
    /// The package did not match its manifest (SECURITY_MODEL §5): writes are refused.
    pub integrity: Option<String>,
    pub package: Option<String>,
}

/// What the launch sequence found (SAFETY_MODEL §10, INTERACTION_SPEC §13).
#[derive(Debug, Clone, Default, Serialize)]
pub struct StartupInfo {
    /// Files settled by crash recovery at this launch.
    pub recovered_files: usize,
    pub recovery_waiting: Vec<(String, String)>,
    pub needs_decision: Vec<mm_core::recovery::RecoverySummary>,
    pub prunes_left: Vec<(String, String)>,
    pub elevated: bool,
    pub backup_problem: Option<String>,
    pub error: Option<String>,
}

pub fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    // a panic while holding a lock must not take the whole app down with poisoned locks
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// `%LOCALAPPDATA%\MoriMeta` (development builds: `MoriMeta-dev`, like `mm-cli`); `MM_DATA`
/// overrides both.
pub fn data_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("MM_DATA") {
        return PathBuf::from(d);
    }
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    base.join(if cfg!(debug_assertions) {
        "MoriMeta-dev"
    } else {
        "MoriMeta"
    })
}

fn pinned_version() -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(LOCK).ok()?;
    v["version"].as_str().map(str::to_owned)
}

/// The ExifTool package: `MM_EXIFTOOL_PKG`, the bundled resource, or (development builds) the
/// pinned package fetched into the repository by `research/scripts/fetch_exiftool.py`.
pub fn package_dir(resources: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MM_EXIFTOOL_PKG") {
        return Some(PathBuf::from(p));
    }
    if let Some(r) = resources {
        let p = r.join("exiftool");
        if p.join("exiftool_files").is_dir() {
            return Some(p);
        }
    }
    if cfg!(debug_assertions) {
        let v = pinned_version()?;
        let p = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../research/.work/exiftool")
            .join(&v)
            .join("win64")
            .join(format!("exiftool-{v}_64"));
        if p.join("exiftool_files").is_dir() {
            return Some(p);
        }
    }
    None
}

/// The package's manifest: next to it (release), or in development builds the one made from the
/// official zip (`research/exiftool-<version>.manifest.json`).
fn manifest_for(pkg: &Path) -> PathBuf {
    let own = pkg.join(mm_core::integrity::MANIFEST_NAME);
    if own.exists() || !cfg!(debug_assertions) {
        return own;
    }
    let v = pinned_version().unwrap_or_default();
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(format!("../../../research/exiftool-{v}.manifest.json"))
}

/// D-17 (DECISIONS §2): `perl.exe exiftool.pl`, without the launcher.
fn engine_config(data: &Path, pkg: &Path) -> Result<EngineConfig, String> {
    let run = data.join("run");
    let cwd = run.join("exiftool-cwd");
    let temp = run.join("tmp");
    std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    Ok(EngineConfig {
        program: pkg.join("exiftool_files").join("perl.exe"),
        script: Some(pkg.join("exiftool_files").join("exiftool.pl")),
        cwd,
        temp,
        max_output: mm_exiftool::MAX_OUTPUT,
    })
}

impl Core {
    /// Instance lock, Journal and settings. Fails when another MoriMeta uses the data folder.
    pub fn open() -> Result<Core, String> {
        let data = data_dir();
        std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
        let (lock, store) = service::open_data(&data).map_err(|e| e.to_string())?;
        Ok(Core {
            data,
            _lock: lock,
            store: Mutex::new(store),
            gate: OperationGate::for_this_process(),
            engines: Mutex::new(Vec::new()),
            reader: Mutex::new(None),
            exiftool: Mutex::new(ExifToolState {
                starting: true,
                ..Default::default()
            }),
            session: Mutex::new(Session::default()),
            book: Mutex::new(PlanBook::default()),
            startup: Mutex::new(StartupInfo::default()),
            events: Mutex::new(None),
            scan_cancel: Mutex::new(None),
            plan_cancel: Mutex::new(None),
            exec_cancel: Mutex::new(None),
            launched: (Mutex::new(false), Condvar::new()),
            running: AtomicBool::new(false),
        })
    }

    /// The rest of the launch sequence, in the background: recovery before any write, the key
    /// files of the ExifTool package before it first runs, the sessions, then the whole package.
    /// Commands that need ExifTool call [`Core::wait_launched`] first.
    pub fn launch(self: &Arc<Core>, resources: Option<PathBuf>) {
        let core = self.clone();
        std::thread::spawn(move || {
            core.launch_inner(resources.as_deref());
            *lock(&core.launched.0) = true;
            core.launched.1.notify_all();
            core.emit(AppEvent::Status);
            core.check_whole_package();
        });
    }

    /// Wait until the launch sequence has finished (recovery done, sessions started or failed).
    pub fn wait_launched(&self) {
        let mut done = lock(&self.launched.0);
        while !*done {
            done = self
                .launched
                .1
                .wait(done)
                .unwrap_or_else(|e| e.into_inner());
        }
    }

    fn launch_inner(&self, resources: Option<&Path>) {
        let mut engines = lock(&self.engines);
        let mut reader = lock(&self.reader);
        {
            let mut store = lock(&self.store);
            let info = match service::startup(&mut store) {
                Ok(s) => StartupInfo {
                    recovered_files: s.recovered.iter().map(|r| r.files.len()).sum(),
                    recovery_waiting: s.recovery_waiting,
                    needs_decision: s.needs_decision,
                    prunes_left: s.prunes_left,
                    elevated: s.elevated,
                    backup_problem: s.backup_problem,
                    error: None,
                },
                Err(e) => StartupInfo {
                    error: Some(e.to_string()),
                    ..Default::default()
                },
            };
            *lock(&self.startup) = info;
        }
        let mut state = ExifToolState::default();
        let Some(pkg) = package_dir(resources) else {
            state.error = Some("the ExifTool package is missing; reinstall MoriMeta".into());
            self.gate.refuse_writes("ExifTool is not available");
            *lock(&self.exiftool) = state;
            return;
        };
        state.package = Some(pkg.display().to_string());
        let manifest = manifest_for(&pkg);
        if let Err(problem) = mm_core::integrity::check_package(&pkg, &manifest, Scope::Key) {
            self.gate.refuse_writes(problem.clone());
            state.integrity = Some(problem);
        }
        let started = engine_config(&self.data, &pkg).and_then(|cfg| {
            let workers = settings_workers(&self.store);
            let r = Engine::start(cfg.clone()).map_err(|e| e.to_string())?;
            let mut pool = Vec::with_capacity(workers);
            for _ in 0..workers {
                pool.push(Engine::start(cfg.clone()).map_err(|e| e.to_string())?);
            }
            Ok((r, pool))
        });
        match started {
            Ok((r, pool)) => {
                state.version = Some(r.version().to_owned());
                *reader = Some(r);
                *engines = pool;
            }
            Err(e) => {
                self.gate.refuse_writes("ExifTool is not available");
                state.error = Some(e);
            }
        }
        *lock(&self.exiftool) = state;
    }

    /// SECURITY_MODEL §5: every file of the package, after the first run.
    fn check_whole_package(&self) {
        let st = lock(&self.exiftool).clone();
        let Some(pkg) = st.package.as_deref().map(PathBuf::from) else {
            return;
        };
        if st.integrity.is_some() || st.error.is_some() {
            return;
        }
        if let Err(problem) =
            mm_core::integrity::check_package(&pkg, &manifest_for(&pkg), Scope::All)
        {
            self.gate.refuse_writes(problem.clone());
            lock(&self.exiftool).integrity = Some(problem);
            self.emit(AppEvent::Status);
        }
    }

    /// INTERACTION_SPEC §10: closing during an Operation always asks.
    pub fn running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn mark_running(&self) -> Running<'_> {
        self.running.store(true, Ordering::SeqCst);
        Running(&self.running)
    }

    pub fn emit(&self, e: AppEvent) {
        if let Some(ch) = lock(&self.events).as_ref() {
            let _ = ch.send(e);
        }
    }

    /// Stop the ExifTool sessions (window closed).
    pub fn shutdown(&self) {
        for e in lock(&self.engines).drain(..) {
            e.close();
        }
        if let Some(e) = lock(&self.reader).take() {
            e.close();
        }
    }
}

fn settings_workers(store: &Mutex<Store>) -> usize {
    mm_core::settings::workers(&lock(store)).max(1)
}
