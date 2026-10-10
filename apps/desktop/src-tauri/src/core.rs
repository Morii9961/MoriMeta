// SPDX-License-Identifier: GPL-3.0-or-later
//! The app's backend state (BACKEND_INTERFACE §0 launch sequence): data folder and its instance
//! lock, the Journal, the write gate, ExifTool sessions, the Session of imported files and the
//! Plans awaiting Preview. Everything that decides what is written lives in `mm-core`.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

use mm_core::engine::Engine;
use mm_core::integrity::Scope;
use mm_core::service::{
    self, InstanceLock, OperationGate, PlanBook, ServiceError, Session, WritePermit,
};
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
    pub prunes: Mutex<mm_core::backups::PruneBook>,
    pub updates: crate::updater::Updates,
    pub startup: Mutex<StartupInfo>,
    /// The frontend's event channel (`subscribe`).
    pub events: Mutex<Option<Channel<AppEvent>>>,
    pub scan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub plan_cancel: Mutex<Option<Arc<AtomicBool>>>,
    pub exec_cancel: Mutex<Option<Arc<AtomicBool>>>,
    /// The Clean Export Preview the user is looking at.
    pub clean: Mutex<Option<mm_core::clean_export::CleanPlan>>,
    launched: (Mutex<bool>, Condvar),
    /// A write Operation (apply, undo, resume) is running: closing the window asks first.
    running: AtomicUsize,
}

/// Marks a write Operation as running for as long as it lives.
pub struct Running<'a>(&'a AtomicUsize);

/// Owns a write slot and its cancellation handle. A rejected request never changes the current
/// operation's handle, and every exit path (including errors) clears its own handle.
pub struct ActiveWrite<'a> {
    pub permit: WritePermit<'a>,
    pub cancel: Arc<AtomicBool>,
    _running: Running<'a>,
    core: &'a Core,
}

impl Drop for ActiveWrite<'_> {
    fn drop(&mut self) {
        let mut current = lock(&self.core.exec_cancel);
        if current
            .as_ref()
            .is_some_and(|c| Arc::ptr_eq(c, &self.cancel))
        {
            *current = None;
        }
    }
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::SeqCst);
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
        Self::open_at(data_dir())
    }

    pub(crate) fn open_at(data: PathBuf) -> Result<Core, String> {
        std::fs::create_dir_all(&data).map_err(|e| format!("{}: {e}", data.display()))?;
        let (lock, store) = service::open_data(&data).map_err(crate::errors::ue)?;
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
            prunes: Mutex::new(mm_core::backups::PruneBook::default()),
            updates: crate::updater::Updates::default(),
            startup: Mutex::new(StartupInfo::default()),
            events: Mutex::new(None),
            scan_cancel: Mutex::new(None),
            plan_cancel: Mutex::new(None),
            exec_cancel: Mutex::new(None),
            clean: Mutex::new(None),
            launched: (Mutex::new(false), Condvar::new()),
            running: AtomicUsize::new(0),
        })
    }

    /// The rest of the launch sequence, in the background: recovery before any write, the key
    /// files of the ExifTool package before it first runs, the sessions, then the whole package.
    /// Commands that need ExifTool call [`Core::wait_launched`] first.
    pub fn launch(self: &Arc<Core>, resources: Option<PathBuf>) {
        let core = self.clone();
        std::thread::spawn(move || {
            core.launch_inner(resources.as_deref());
            core.log_exiftool("exiftool started", resources.as_deref());
            *lock(&core.launched.0) = true;
            core.launched.1.notify_all();
            core.emit(AppEvent::Status);
            core.check_whole_package();
            core.log_exiftool("exiftool package checked", resources.as_deref());
        });
    }

    /// The launch result in the program's log, for bug reports and the release check
    /// (`apps/desktop/scripts/release-check.mjs`). Only versions and categories: the log never
    /// holds a path (PRIVACY.md), and problem texts can.
    fn log_exiftool(&self, what: &str, resources: Option<&Path>) {
        let st = lock(&self.exiftool).clone();
        let source = match st.package.as_deref() {
            None => "none",
            Some(_) if std::env::var_os("MM_EXIFTOOL_PKG").is_some() => "MM_EXIFTOOL_PKG",
            Some(p) if resources.is_some_and(|r| Path::new(p) == r.join("exiftool")) => "bundled",
            Some(_) => "development",
        };
        let writes = match self.gate.write() {
            Ok(_) | Err(ServiceError::Busy(_)) => "allowed",
            Err(ServiceError::Elevated) => "refused: administrator rights",
            Err(_) => "refused",
        };
        let build = if cfg!(debug_assertions) {
            "development"
        } else {
            "release"
        };
        mm_core::log::event(
            "info",
            what,
            &[
                ("app", &env!("CARGO_PKG_VERSION")),
                ("build", &build),
                ("exiftool", &st.version.as_deref().unwrap_or("-")),
                ("source", &source),
                ("started", &(st.error.is_none() && st.version.is_some())),
                (
                    "integrity",
                    &if st.integrity.is_some() {
                        "failed"
                    } else {
                        "ok"
                    },
                ),
                ("writes", &writes),
            ],
        );
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
                    error: Some(crate::errors::ue(e)),
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
            let r = Engine::start(cfg.clone()).map_err(crate::errors::ue)?;
            let mut pool = Vec::with_capacity(workers);
            for _ in 0..workers {
                pool.push(Engine::start(cfg.clone()).map_err(crate::errors::ue)?);
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
        self.running.load(Ordering::SeqCst) > 0
    }

    pub fn mark_running(&self) -> Running<'_> {
        self.running.fetch_add(1, Ordering::SeqCst);
        Running(&self.running)
    }

    pub fn begin_write(&self) -> Result<ActiveWrite<'_>, String> {
        let permit = self.gate.write().map_err(crate::errors::ue)?;
        let cancel = Arc::new(AtomicBool::new(false));
        *lock(&self.exec_cancel) = Some(cancel.clone());
        Ok(ActiveWrite {
            permit,
            cancel,
            _running: self.mark_running(),
            core: self,
        })
    }

    /// Apply the configured worker count before the next operation. The permit prevents writes
    /// during resizing; existing sessions are preserved if starting any additional session fails.
    pub fn sync_workers(
        &self,
        _permit: &WritePermit<'_>,
        engines: &mut Vec<Engine>,
        store: &Store,
    ) -> Result<(), String> {
        let wanted = mm_core::settings::workers(store);
        let mut extra = Vec::new();
        if wanted > engines.len() {
            let peer = engines.first().ok_or("ExifTool is not available")?;
            for _ in engines.len()..wanted {
                extra.push(peer.spawn_peer().map_err(crate::errors::ue)?);
            }
        }
        engines.extend(extra);
        if wanted < engines.len() {
            for engine in engines.drain(wanted..) {
                engine.close();
            }
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_count_is_applied_before_the_next_operation_without_restarting_the_app() {
        if package_dir(None).is_none() {
            eprintln!("SKIP: pinned ExifTool not fetched");
            return;
        }
        let data = std::env::temp_dir().join(format!("mm-app-workers-{}", std::process::id()));
        let core = Arc::new(Core::open_at(data.clone()).unwrap());
        core.launch(None);
        core.wait_launched();
        // The background whole-package check also owns `core`; wait by performing that same
        // deterministic check before touching the pool. No installation or user data is changed.
        let Some(pkg) = package_dir(None) else {
            return;
        };
        mm_core::integrity::check_package(&pkg, &manifest_for(&pkg), Scope::All).unwrap();
        let active = match core.begin_write() {
            Ok(active) => active,
            Err(why) if mm_fs::is_elevated().unwrap_or(true) => {
                eprintln!("SKIP: elevated process: {why}");
                return;
            }
            Err(why) => panic!("{why}"),
        };
        {
            let mut engines = lock(&core.engines);
            let mut store = lock(&core.store);
            for wanted in [1, 3, 2] {
                mm_core::settings::set(&mut store, "exec.workers", &wanted.to_string()).unwrap();
                core.sync_workers(&active.permit, &mut engines, &store)
                    .unwrap();
                assert_eq!(engines.len(), wanted);
                assert!(
                    engines
                        .iter()
                        .all(|e| e.version() == mm_core::engine::EXIFTOOL_VERSION)
                );
            }
            assert!(mm_core::settings::set(&mut store, "exec.workers", "64").is_err());
            core.sync_workers(&active.permit, &mut engines, &store)
                .unwrap();
            assert_eq!(engines.len(), 2);
        }
        drop(active);
        core.shutdown();
        // launch's background Arc is allowed to finish naturally; artifacts stay in the
        // temporary test directory, outside the repository.
    }
}
