//! Operation execution: the single-file transaction of SAFETY_MODEL §4.1 for every executable
//! plan entry, with journal records before every irreversible step (I-8).
//!
//! Fault points (for the fault-injection tests only) terminate the process immediately:
//!   1 before lock · 2 after lock/fingerprint · 3 after backup copy · 4 after BackedUp recorded ·
//!   5 after temp written · 6 after verification · 7 after Ready recorded · 8 after ReplaceFileW ·
//!   9 after Committed recorded · 10 after bak removed (before Done recorded)
//! (recreating a deleted file uses 1 and 5–10: there is no original to lock or back up;
//! moving a created file into the backup store uses 1–4 and 7–10: there is no temporary output)
//!
//! A full volume (SAFETY_MODEL §8.13) pauses the Operation: the file in progress is settled with
//! the recovery table, it and every later file become `Cancelled` (original unchanged, retried
//! by resume once space is freed), and the Operation ends `Cancelled`.

use std::collections::{BTreeMap, HashMap, VecDeque};
use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use mm_domain::plan::{EntryAction, Plan, PlanEntry, PlanKind};
use mm_domain::snapshot::Snapshot;
use mm_domain::{capture, copyright, creator, gps};
use mm_store::{FileState, FileUpdate, NewFile, NewOperation, OpStatus, Store};

use crate::engine::Engine;
use crate::verify::{self, VerifyError};
use crate::{
    APP_VERSION, CoreError, ROLE_CREATE, ROLE_EMBEDDED, ROLE_RECREATE, ROLE_REMOVE,
    fingerprint_of_handle, hash_opt, new_id,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FaultPoint {
    pub seq: u32,
    pub step: u8,
}

#[derive(Debug, Clone, Default)]
pub struct ExecOptions {
    /// Terminate the process at this point (crash test).
    pub fault: Option<FaultPoint>,
    /// Return an injected IO error at this point (error-path test).
    pub fail: Option<FaultPoint>,
    /// Return a simulated "disk full" IO error (Win32 112) at this point.
    pub disk_full: Option<FaultPoint>,
    /// Fill the (small, test) volume holding this directory at this point: a real disk full.
    pub fill: Option<(FaultPoint, PathBuf)>,
    /// Replace the 1 GiB backup-volume reserve of the space pre-check (tests only).
    pub space_reserve: Option<u64>,
    /// Called after each file settles, in completion order (ARCHITECTURE §5.3).
    pub progress: Option<ProgressSink>,
    /// Set by the user's Cancel (`op_cancel`): no new files start, files in progress finish and
    /// the Operation ends `cancelled`, resumable like a paused one.
    pub cancel: Option<Arc<AtomicBool>>,
    /// Set `cancel` when this point is reached, as if the user pressed Cancel there (tests).
    pub cancel_at: Option<FaultPoint>,
}

impl ExecOptions {
    fn cancelled(&self) -> bool {
        self.cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
    }
}

/// Counts of an Operation's files so far.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExecProgress {
    pub total: usize,
    /// Settled files, `ok + failed + skipped`.
    pub done: usize,
    pub ok: usize,
    pub failed: usize,
    /// Conflict, skipped or not started (cancelled).
    pub skipped: usize,
    /// The file that just settled.
    pub last: Option<(u32, FileState)>,
}

/// Receives [`ExecProgress`] from the workers; the adapter batches it for the UI.
#[derive(Clone)]
pub struct ProgressSink(pub Arc<dyn Fn(&ExecProgress) + Send + Sync>);

impl std::fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ProgressSink")
    }
}

/// Backup-volume reserve on top of the backups themselves (SAFETY_MODEL §6.2).
pub const SPACE_RESERVE: u64 = 1 << 30;

const DISK_FULL_STOP: &str = "paused: a volume is full; free space, then resume";
const USER_CANCEL: &str = "cancelled by the user; the files not started can be resumed";

#[derive(Debug, Clone)]
pub struct FileOutcome {
    pub seq: u32,
    pub path: String,
    pub state: FileState,
    pub reason: Option<String>,
}

#[derive(Debug, Clone)]
pub struct OpReport {
    pub op_id: String,
    pub status: OpStatus,
    pub files: Vec<FileOutcome>,
    pub note: Option<String>,
}

/// Fault-injection hook at a numbered step: terminate (crash test) or fail with an IO error.
fn fault(opts: &ExecOptions, seq: u32, step: u8) -> Result<(), CoreError> {
    let here = Some(FaultPoint { seq, step });
    if opts.fault == here {
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
        // SAFETY: terminating our own process; nothing after this runs (no destructors, like a crash).
        unsafe {
            TerminateProcess(GetCurrentProcess(), 77);
        }
        unreachable!();
    }
    if opts.cancel_at == here {
        if let Some(c) = &opts.cancel {
            c.store(true, Ordering::SeqCst);
        }
    }
    if opts.fail == here {
        return Err(CoreError::Io(std::io::Error::other(format!(
            "injected IO error at step {step}"
        ))));
    }
    if opts.disk_full == here {
        return Err(CoreError::Io(std::io::Error::from_raw_os_error(112)));
    }
    if let Some((_, dir)) = opts.fill.as_ref().filter(|(p, _)| Some(*p) == here) {
        mm_fs::fill_volume(dir).map_err(|e| CoreError::Internal(format!("fill: {e}")))?;
    }
    Ok(())
}

enum Outcome {
    Done,
    Failed(String),
    Skipped(String),
    Conflict(String),
    Attention(String),
    /// A volume is full; the original is unchanged. Pauses the Operation.
    DiskFull(String),
    /// The user cancelled before the commit; the original is unchanged (SAFETY_MODEL §11).
    Cancelled(String),
}

/// Space pre-check before any file is touched (SAFETY_MODEL §6.2): the backup volume needs
/// `Σ size × 1.05 + reserve`; each target directory's volume needs `largest file × 1.1` for the
/// temporary output (one worker).
fn check_space<'a>(
    store: &Store,
    entries: impl Iterator<Item = &'a PlanEntry>,
    reserve: u64,
) -> Result<(), CoreError> {
    let mut total = 0u64;
    let mut largest: BTreeMap<PathBuf, u64> = BTreeMap::new();
    for e in entries {
        let size = match &e.action {
            Some(EntryAction::Recreate { size, .. }) => *size,
            _ => e.fingerprint.size,
        };
        total = total.saturating_add(size);
        let dir = Path::new(&e.path)
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_default();
        let m = largest.entry(dir).or_default();
        *m = (*m).max(size);
    }
    if largest.is_empty() {
        return Ok(()); // nothing will be written
    }
    let backups = store.data_dir().join("backups");
    let need = total.saturating_add(total / 20).saturating_add(reserve);
    let free = mm_fs::volume_space(&backups)?.free;
    if free < need {
        return Err(CoreError::InsufficientSpace(format!(
            "backup volume ({}) needs {need} bytes free, has {free}",
            backups.display()
        )));
    }
    for (dir, size) in largest {
        let need = size.saturating_add(size / 10);
        let free = mm_fs::volume_space(&dir)?.free;
        if free < need {
            return Err(CoreError::InsufficientSpace(format!(
                "volume of {} needs {need} bytes free for temporary output, has {free}",
                dir.display()
            )));
        }
    }
    Ok(())
}

/// After ExifTool failed to produce the output: was the target volume full?
fn target_volume_full(temp: &Path, size: u64) -> bool {
    temp.parent()
        .and_then(|d| mm_fs::volume_space(d).ok())
        .is_some_and(|s| s.free < size + size / 10)
}

fn names(
    path: &Path,
    backup_dir: &Path,
    seq: u32,
) -> Result<(PathBuf, PathBuf, PathBuf), CoreError> {
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().into_owned())
        .unwrap_or_else(|| "bin".into());
    let t = mm_fs::random_token()?;
    Ok((
        mm_fs::sibling_name(path, ".mmtmp-", &t),
        mm_fs::sibling_name(path, ".mmbak-", &t),
        backup_dir.join(format!("{:08}-{}.{ext}", seq, &t[..8])),
    ))
}

/// The ExifTool version of the engines (all the same program).
fn engine_version(engines: &[Engine]) -> Result<String, CoreError> {
    engines
        .first()
        .map(|e| e.version().to_owned())
        .ok_or_else(|| CoreError::Internal("no ExifTool engine".into()))
}

/// Refuse to write while an interrupted Operation awaits recovery (SAFETY_MODEL §10).
pub fn require_no_pending_recovery(store: &Store) -> Result<(), CoreError> {
    let pending = store.unfinished()?;
    if pending.is_empty() {
        Ok(())
    } else {
        Err(CoreError::RecoveryPending(pending))
    }
}

/// Start an Operation for every executable entry of `plan`.
pub fn start(
    store: &mut Store,
    engines: &mut [Engine],
    plan: &Plan,
    opts: &ExecOptions,
) -> Result<OpReport, CoreError> {
    require_no_pending_recovery(store)?;
    let version = engine_version(engines)?;
    if plan.exiftool_version != version {
        return Err(CoreError::VersionMismatch(format!(
            "plan made with ExifTool {}, running {version}",
            plan.exiftool_version,
        )));
    }
    check_space(
        store,
        plan.executable(),
        opts.space_reserve.unwrap_or(SPACE_RESERVE),
    )?;
    let op_id = new_id("op")?;
    let backup_dir = store.backup_dir(&op_id);
    let mut files = Vec::new();
    for e in plan.executable() {
        let (temp, bak, backup) = names(Path::new(&e.path), &backup_dir, e.seq)?;
        files.push(NewFile {
            seq: e.seq,
            path: e.path.clone(),
            role: match e.action {
                Some(EntryAction::Recreate { .. }) => ROLE_RECREATE,
                Some(EntryAction::MoveToBackupStore { .. }) => ROLE_REMOVE,
                Some(EntryAction::CreateFile { .. }) => ROLE_CREATE,
                _ => ROLE_EMBEDDED,
            }
            .into(),
            temp_path: temp.to_string_lossy().into_owned(),
            bak_path: bak.to_string_lossy().into_owned(),
            backup_path: backup.to_string_lossy().into_owned(),
        });
    }
    let (kind, undo_of) = match &plan.kind {
        PlanKind::Apply => ("apply".to_owned(), None),
        PlanKind::Undo { of } => ("undo".to_owned(), Some(of.clone())),
    };
    store.begin_operation(
        &NewOperation {
            id: op_id.clone(),
            kind,
            title: plan.title.clone(),
            plan_json: serde_json::to_string(plan)
                .map_err(|e| CoreError::Internal(e.to_string()))?,
            app_version: APP_VERSION.into(),
            exiftool_version: version.clone(),
            registry_version: plan.registry_version,
            undo_of,
        },
        &files,
    )?;
    let seqs: Vec<u32> = files.iter().map(|f| f.seq).collect();
    run(store, engines, &op_id, plan, &seqs, opts)
}

/// Continue an Operation after recovery or cancellation: retry every file in `not_started`.
pub fn resume(
    store: &mut Store,
    engines: &mut [Engine],
    op_id: &str,
    opts: &ExecOptions,
) -> Result<OpReport, CoreError> {
    require_no_pending_recovery(store)?;
    let version = engine_version(engines)?;
    let op = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if op.app_version != APP_VERSION || op.exiftool_version != version {
        return Err(CoreError::VersionMismatch(format!(
            "operation made with app {} / ExifTool {}; running {} / {} — undo it or plan again",
            op.app_version, op.exiftool_version, APP_VERSION, version
        )));
    }
    let plan: Plan = serde_json::from_str(&op.plan_json)
        .map_err(|e| CoreError::Internal(format!("persisted plan: {e}")))?;
    let backup_dir = PathBuf::from(&op.backup_dir);
    let retry: Vec<_> = store
        .files(op_id)?
        .into_iter()
        .filter(|f| f.state == FileState::NotStarted || f.state == FileState::Cancelled)
        .collect();
    check_space(
        store,
        plan.entries
            .iter()
            .filter(|e| retry.iter().any(|f| f.seq == e.seq)),
        opts.space_reserve.unwrap_or(SPACE_RESERVE),
    )?;
    // `running` first: if re-registering a file fails part-way, the files already set back to
    // `planned` stay visible to recovery instead of being stranded in a finished Operation
    store.set_status(op_id, OpStatus::Running)?;
    let mut seqs = Vec::new();
    for f in retry {
        let (temp, bak, backup) = names(Path::new(&f.path), &backup_dir, f.seq)?;
        store.set_paths(
            op_id,
            f.seq,
            &temp.to_string_lossy(),
            &bak.to_string_lossy(),
            &backup.to_string_lossy(),
        )?;
        seqs.push(f.seq);
    }
    run(store, engines, op_id, &plan, &seqs, opts)
}

/// The Journal shared by the workers: one SQLite connection, one writer at a time. Every write
/// still returns only after SQLite has committed it (I-8); the workers only wait for each other.
struct Journal<'s>(Mutex<&'s mut Store>);

impl<'s> Journal<'s> {
    fn lock(&self) -> std::sync::MutexGuard<'_, &'s mut Store> {
        // a worker that panicked cannot leave SQLite half-written: every write is a transaction
        self.0.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn set_state(
        &self,
        op_id: &str,
        seq: u32,
        state: FileState,
        u: &FileUpdate,
    ) -> mm_store::Result<()> {
        self.lock().set_state(op_id, seq, state, u)
    }

    fn row(&self, op_id: &str, seq: u32) -> Result<mm_store::FileRow, CoreError> {
        self.lock()
            .files(op_id)?
            .into_iter()
            .find(|r| r.seq == seq)
            .ok_or_else(|| CoreError::Internal(format!("missing file row {seq}")))
    }
}

/// Per-volume IO permits (ARCHITECTURE §8.2): a file holds the permit of its volume for its whole
/// transaction, so an HDD or removable volume is written one file at a time whatever the number
/// of workers.
#[derive(Default)]
struct VolumeGate {
    state: Mutex<HashMap<PathBuf, (usize, usize)>>,
    freed: Condvar,
}

struct Permit<'g> {
    gate: &'g VolumeGate,
    root: PathBuf,
}

impl VolumeGate {
    fn acquire(&self, path: &Path) -> Permit<'_> {
        let root = mm_fs::volume_root(path).unwrap_or_default();
        let mut s = self.state.lock().unwrap_or_else(|p| p.into_inner());
        loop {
            let e = s.entry(root.clone()).or_insert_with(|| {
                let limit = if root.as_os_str().is_empty() {
                    1
                } else {
                    mm_fs::volume_kind(&root).io_limit()
                };
                (0, limit)
            });
            if e.0 < e.1 {
                e.0 += 1;
                return Permit { gate: self, root };
            }
            s = self.freed.wait(s).unwrap_or_else(|p| p.into_inner());
        }
    }
}

impl Drop for Permit<'_> {
    fn drop(&mut self) {
        let mut s = self.gate.state.lock().unwrap_or_else(|p| p.into_inner());
        if let Some(e) = s.get_mut(&self.root) {
            e.0 -= 1;
        }
        self.gate.freed.notify_all();
    }
}

/// Work shared by the workers: the files not yet started, the circuit breaker (SAFETY_MODEL
/// §8.16, counted in completion order) and the first fatal (journal) error.
struct Sched {
    queue: VecDeque<u32>,
    completed: usize,
    failures_first20: usize,
    consecutive_verify_failures: usize,
    tripped: Option<String>,
    fatal: Option<CoreError>,
    progress: ExecProgress,
}

/// What became of one file.
struct Settled {
    state: FileState,
    verify_fail: bool,
    /// Stop starting new files (a full volume).
    stop: Option<String>,
}

impl Sched {
    fn record(&mut self, s: &Settled) {
        let n = self.completed;
        self.completed += 1;
        if n < 20 && s.state == FileState::Failed {
            self.failures_first20 += 1;
        }
        self.consecutive_verify_failures = if s.verify_fail {
            self.consecutive_verify_failures + 1
        } else {
            0
        };
        if let Some(why) = &s.stop {
            self.tripped = Some(why.clone());
        }
        let sample = (n + 1).min(20);
        if (sample >= 4 && self.failures_first20 * 2 >= sample && n < 20)
            || self.consecutive_verify_failures >= 10
        {
            self.tripped = Some(
                "stopped: too many failures (circuit breaker); check ExifTool and the files".into(),
            );
        }
    }
}

/// Run the file transactions of `seqs` on one worker per engine (ARCHITECTURE §7.5, §8). Files are
/// started in `seqs` order; once the Operation is stopped (full volume, circuit breaker) files not
/// yet started become Cancelled and files in progress finish. A journal error stops every worker
/// and leaves the Operation `running` for recovery.
fn run(
    store: &mut Store,
    engines: &mut [Engine],
    op_id: &str,
    plan: &Plan,
    seqs: &[u32],
    opts: &ExecOptions,
) -> Result<OpReport, CoreError> {
    let rows = store.files(op_id)?;
    // Best effort: an Operation that cannot hold the system awake still runs.
    let _awake = mm_fs::KeepAwake::new().ok();
    let journal = Journal(Mutex::new(store));
    let gate = VolumeGate::default();
    let sched = Mutex::new(Sched {
        queue: seqs.iter().copied().collect(),
        completed: 0,
        failures_first20: 0,
        consecutive_verify_failures: 0,
        tripped: None,
        fatal: None,
        progress: ExecProgress {
            total: seqs.len(),
            ..Default::default()
        },
    });
    let lock = || sched.lock().unwrap_or_else(|p| p.into_inner());
    std::thread::scope(|scope| {
        for engine in engines.iter_mut() {
            let (journal, gate, rows) = (&journal, &gate, &rows);
            scope.spawn(move || {
                loop {
                    let next = {
                        let mut s = lock();
                        if s.fatal.is_some() {
                            return;
                        }
                        if s.tripped.is_none() && opts.cancelled() {
                            s.tripped = Some(USER_CANCEL.into());
                        }
                        s.queue.pop_front().map(|q| (q, s.tripped.clone()))
                    };
                    let Some((seq, tripped)) = next else {
                        return;
                    };
                    let r = match tripped {
                        Some(why) => journal
                            .set_state(
                                op_id,
                                seq,
                                FileState::Cancelled,
                                &FileUpdate {
                                    error: Some(why),
                                    ..Default::default()
                                },
                            )
                            .map(|()| None)
                            .map_err(CoreError::from),
                        None => {
                            settle(journal, gate, engine, op_id, plan, rows, seq, opts).map(Some)
                        }
                    };
                    let mut s = lock();
                    let state = match r {
                        Ok(Some(settled)) => {
                            s.record(&settled);
                            settled.state
                        }
                        Ok(None) => FileState::Cancelled,
                        Err(e) => {
                            s.fatal.get_or_insert(e);
                            return;
                        }
                    };
                    let p = &mut s.progress;
                    p.done += 1;
                    match state {
                        FileState::Done => p.ok += 1,
                        FileState::Failed => p.failed += 1,
                        _ => p.skipped += 1,
                    }
                    p.last = Some((seq, state));
                    if let Some(sink) = &opts.progress {
                        (sink.0)(p);
                    }
                }
            });
        }
    });
    let sched = sched.into_inner().unwrap_or_else(|p| p.into_inner());
    if let Some(e) = sched.fatal {
        return Err(e);
    }
    let store = journal.0.into_inner().unwrap_or_else(|p| p.into_inner());
    let tripped = sched.tripped;
    let files = store.files(op_id)?;
    let status = if tripped.is_some() {
        OpStatus::Cancelled
    } else if files.iter().all(|f| f.state == FileState::Done) {
        OpStatus::Completed
    } else {
        OpStatus::CompletedWithErrors
    };
    store.finish_operation(op_id, status)?;
    Ok(OpReport {
        op_id: op_id.to_owned(),
        status,
        files: files
            .into_iter()
            .map(|f| FileOutcome {
                seq: f.seq,
                path: f.path,
                state: f.state,
                reason: f.error,
            })
            .collect(),
        note: tripped,
    })
}

/// One file's transaction under its volume permit, settled into a terminal journal state.
#[allow(clippy::too_many_arguments)]
fn settle(
    journal: &Journal,
    gate: &VolumeGate,
    engine: &mut Engine,
    op_id: &str,
    plan: &Plan,
    rows: &[mm_store::FileRow],
    seq: u32,
    opts: &ExecOptions,
) -> Result<Settled, CoreError> {
    let row = rows
        .iter()
        .find(|r| r.seq == seq)
        .ok_or_else(|| CoreError::Internal(format!("missing file row {seq}")))?;
    let entry = plan
        .entries
        .iter()
        .find(|e| e.seq == seq)
        .ok_or_else(|| CoreError::Internal(format!("missing plan entry {seq}")))?;
    let files = FilePaths {
        path: PathBuf::from(&row.path),
        temp: PathBuf::from(&row.temp_path),
        bak: PathBuf::from(&row.bak_path),
        backup: PathBuf::from(&row.backup_path),
    };
    let permit = gate.acquire(&files.path);
    let result = one_file(journal, engine, op_id, entry, &files, opts);
    drop(permit);
    let mut stop = None;
    let outcome = match result {
        Ok(o) => o,
        // An error in the middle of the transaction (IO, engine, journal): the lock is released;
        // settle this file from the journal and the disk with the recovery table (SAFETY_MODEL §10).
        // If the journal itself is failing, `?` stops the Operation in `running` state so that
        // recovery handles it at the next start.
        Err(e) => {
            let now = journal.row(op_id, seq)?;
            let (to, action) = crate::recovery::decide(&now);
            if e.is_disk_full() {
                stop = Some(DISK_FULL_STOP.to_string()); // whatever became of this file
            }
            match to {
                FileState::NotStarted if e.is_disk_full() => {
                    Outcome::DiskFull(format!("{e}; original unchanged"))
                }
                FileState::NotStarted => Outcome::Failed(format!("{e}; original unchanged")),
                FileState::Done => {
                    // keep the error visible: the commit was confirmed from the disk, not
                    // from a complete journal
                    journal.set_state(
                        op_id,
                        seq,
                        FileState::Done,
                        &FileUpdate {
                            error: Some(format!("{e}; commit confirmed on disk")),
                            ..Default::default()
                        },
                    )?;
                    Outcome::Done
                }
                _ => Outcome::Attention(format!("{e}; {action}")),
            }
        }
    };
    let (state, reason, verify_fail) = match outcome {
        Outcome::Done => (FileState::Done, None, false),
        Outcome::Failed(r) => {
            let v = r.starts_with("verification");
            (FileState::Failed, Some(r), v)
        }
        Outcome::Skipped(r) => (FileState::Skipped, Some(r), false),
        Outcome::Conflict(r) => (FileState::Conflict, Some(r), false),
        Outcome::Attention(r) => (FileState::Attention, Some(r), false),
        Outcome::DiskFull(r) => {
            stop = Some(DISK_FULL_STOP.into());
            (FileState::Cancelled, Some(r), false)
        }
        Outcome::Cancelled(r) => (FileState::Cancelled, Some(r), false),
    };
    if state != FileState::Done {
        journal.set_state(
            op_id,
            seq,
            state,
            &FileUpdate {
                error: reason,
                ..Default::default()
            },
        )?;
    }
    Ok(Settled {
        state,
        verify_fail,
        stop,
    })
}

struct FilePaths {
    path: PathBuf,
    temp: PathBuf,
    bak: PathBuf,
    backup: PathBuf,
}

fn remove_if_exists(p: &Path) {
    let _ = std::fs::remove_file(p);
}

fn one_file(
    store: &Journal,
    engine: &mut Engine,
    op_id: &str,
    entry: &PlanEntry,
    f: &FilePaths,
    opts: &ExecOptions,
) -> Result<Outcome, CoreError> {
    let seq = entry.seq;
    if let Some(EntryAction::Recreate { backup, h0, .. }) = entry.action.as_ref() {
        return recreate(store, op_id, seq, Path::new(backup), h0, f, opts);
    }
    if let Some(EntryAction::MoveToBackupStore { h }) = entry.action.as_ref() {
        return move_to_backup_store(store, op_id, entry, h, f, opts);
    }
    if let Some(EntryAction::CreateFile { ops, expect }) = entry.action.as_ref() {
        return create_file(store, engine, op_id, seq, ops, expect, f, opts);
    }
    fault(opts, seq, 1)?;
    // 1 lock + fingerprint
    let mut lock = match mm_fs::open_lock(&f.path) {
        Ok(l) => l,
        Err(e) if e.raw_os_error() == Some(32) || e.raw_os_error() == Some(33) => {
            return Ok(Outcome::Skipped("file is in use by another program".into()));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Outcome::Conflict("file no longer exists".into()));
        }
        Err(e) => return Ok(Outcome::Failed(format!("cannot open: {e}"))),
    };
    let fp = fingerprint_of_handle(&lock)?;
    if !crate::planner::same_file(&fp, &entry.fingerprint) {
        return Ok(Outcome::Conflict(
            "file changed since the preview (size, time or identity)".into(),
        ));
    }
    let probe = mm_fs::probe(&f.path)?;
    if probe.read_only {
        return Ok(Outcome::Skipped("read-only attribute is set".into()));
    }
    if probe.links > 1 || probe.reparse_point {
        return Ok(Outcome::Skipped("hard link or reparse point".into()));
    }
    fault(opts, seq, 2)?;
    // 2 backup through the lock handle
    lock.rewind()?;
    let h0 = match mm_fs::copy_new_hashing(&mut lock, &f.backup) {
        Ok(h) => h,
        Err(e) if mm_fs::is_disk_full(&e) => {
            return Ok(Outcome::DiskFull(format!(
                "backup volume is full ({e}); original unchanged"
            )));
        }
        Err(e) => return Ok(Outcome::Failed(format!("backup failed: {e}"))),
    };
    fault(opts, seq, 3)?;
    if mm_fs::hash_path(&f.backup)? != h0 {
        remove_if_exists(&f.backup);
        return Ok(Outcome::Failed("backup verification failed".into()));
    }
    let h0s = mm_fs::hex(&h0);
    store.set_state(
        op_id,
        seq,
        FileState::BackedUp,
        &FileUpdate {
            h0: Some(h0s.clone()),
            ..Default::default()
        },
    )?;
    fault(opts, seq, 4)?;
    // 3 produce the temp file
    match entry.action.as_ref() {
        Some(EntryAction::Write { ops, expect }) => {
            let out = engine.write(ops, &f.backup, &f.temp);
            // ExifTool reports a failed write only as text; ask the volume whether it is full
            let engine_failed = |why: String| {
                remove_if_exists(&f.temp);
                if target_volume_full(&f.temp, fp.size) {
                    Outcome::DiskFull(format!("{why}; photo volume is full; original unchanged"))
                } else {
                    Outcome::Failed(why)
                }
            };
            let out = match out {
                Ok(o) => o,
                Err(e) => return Ok(engine_failed(format!("ExifTool: {e}"))),
            };
            if let Err(e) = verify::check_write_output(&out) {
                return Ok(engine_failed(format!("verification V1: {e}")));
            }
            if !f.temp.exists() {
                return Ok(engine_failed("ExifTool produced no output".into()));
            }
            fault(opts, seq, 5)?;
            // 4 verify against the verified backup (the actual source)
            let reads = engine.read_full(&[f.backup.as_path(), f.temp.as_path()])?;
            let (Some(src), Some(tmp)) = (reads[0].as_ref(), reads[1].as_ref()) else {
                remove_if_exists(&f.temp);
                return Ok(Outcome::Failed(
                    "verification V5: output not readable".into(),
                ));
            };
            if let Err(e) = before_matches(entry, src) {
                remove_if_exists(&f.temp);
                return Ok(Outcome::Conflict(e.to_string()));
            }
            if let Err(e) = verify::check_output(src, tmp, ops, expect) {
                remove_if_exists(&f.temp);
                return Ok(Outcome::Failed(format!("verification: {e}")));
            }
        }
        Some(EntryAction::Restore {
            backup,
            h0: orig_h0,
            h1: orig_h1,
        }) => {
            // undo: the current content must still be the post-operation content
            if &h0s != orig_h1 {
                return Ok(Outcome::Conflict(
                    "file changed after the operation; not restored".into(),
                ));
            }
            let mut src = match File::open(backup) {
                Ok(s) => s,
                Err(e) => return Ok(Outcome::Failed(format!("backup unavailable: {e}"))),
            };
            let hr = mm_fs::copy_new_hashing(&mut src, &f.temp)?;
            if &mm_fs::hex(&hr) != orig_h0 {
                remove_if_exists(&f.temp);
                return Ok(Outcome::Failed(
                    "backup content does not match its recorded hash".into(),
                ));
            }
            fault(opts, seq, 5)?;
        }
        Some(
            EntryAction::Recreate { .. }
            | EntryAction::MoveToBackupStore { .. }
            | EntryAction::CreateFile { .. },
        ) => {
            return Err(CoreError::Internal(
                "recreate / move reached the in-place path".into(),
            ));
        }
        None => return Ok(Outcome::Failed("plan entry has no action".into())),
    }
    mm_fs::flush_path(&f.temp)?;
    let h1s = mm_fs::hex(&mm_fs::hash_path(&f.temp)?);
    fault(opts, seq, 6)?;
    store.set_state(
        op_id,
        seq,
        FileState::Ready,
        &FileUpdate {
            h1: Some(h1s.clone()),
            ..Default::default()
        },
    )?;
    fault(opts, seq, 7)?;
    // steps 1–7 are abandoned on Cancel; from the commit on, the file finishes (SAFETY_MODEL §11)
    if opts.cancelled() {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Cancelled(
            "cancelled before the commit; original unchanged".into(),
        ));
    }
    // 5 identity and bak-name checks, then commit
    if mm_fs::file_id_of_path(&f.path).ok() != Some(mm_fs::file_id(&lock)?) {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Conflict(
            "file was renamed or replaced during the operation".into(),
        ));
    }
    if mm_fs::ensure_absent(&f.bak).is_err() {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Failed(
            "backup name already exists next to the file".into(),
        ));
    }
    let mut result = mm_fs::replace_file(&f.path, &f.temp, &f.bak);
    for delay in [200u64, 800, 2000] {
        match result {
            Err(mm_fs::Win32Error(32)) => {
                std::thread::sleep(Duration::from_millis(delay));
                result = mm_fs::replace_file(&f.path, &f.temp, &f.bak);
            }
            _ => break,
        }
    }
    if let Err(code) = result {
        return Ok(commit_failed(f, &h0s, code));
    }
    fault(opts, seq, 8)?;
    let new_id = mm_fs::file_id_of_path(&f.path)
        .ok()
        .map(|i| crate::file_id_hex(&i));
    store.set_state(
        op_id,
        seq,
        FileState::Committed,
        &FileUpdate {
            new_file_id: new_id,
            ..Default::default()
        },
    )?;
    fault(opts, seq, 9)?;
    drop(lock);
    // 6 post-check and cleanup
    if hash_opt(&f.path).as_deref() != Some(h1s.as_str()) {
        return Ok(Outcome::Attention(
            "content after commit differs from the verified output".into(),
        ));
    }
    if hash_opt(&f.bak).as_deref() == Some(h0s.as_str()) {
        remove_if_exists(&f.bak);
    }
    fault(opts, seq, 10)?;
    store.set_state(op_id, seq, FileState::Done, &FileUpdate::default())?;
    Ok(Outcome::Done)
}

/// Undo of a file the undone Operation created (SAFETY_MODEL §4.3, §7.2): never deleted, moved
/// into the backup store. The content is copied through the lock handle into the backup store and
/// verified (it is the pre-image H0 of this Operation), recorded, and only then is the path renamed
/// to its registered bak name (the commit) and the bak removed after a hash check (I-9). Fault
/// points 1–4 and 7–10 as in the file header.
fn move_to_backup_store(
    store: &Journal,
    op_id: &str,
    entry: &PlanEntry,
    want: &str,
    f: &FilePaths,
    opts: &ExecOptions,
) -> Result<Outcome, CoreError> {
    let seq = entry.seq;
    fault(opts, seq, 1)?;
    let mut lock = match mm_fs::open_lock(&f.path) {
        Ok(l) => l,
        Err(e) if e.raw_os_error() == Some(32) || e.raw_os_error() == Some(33) => {
            return Ok(Outcome::Skipped("file is in use by another program".into()));
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Outcome::Conflict("file no longer exists".into()));
        }
        Err(e) => return Ok(Outcome::Failed(format!("cannot open: {e}"))),
    };
    let fp = fingerprint_of_handle(&lock)?;
    if !crate::planner::same_file(&fp, &entry.fingerprint) {
        return Ok(Outcome::Conflict(
            "file changed since the preview (size, time or identity)".into(),
        ));
    }
    let probe = mm_fs::probe(&f.path)?;
    if probe.read_only {
        return Ok(Outcome::Skipped("read-only attribute is set".into()));
    }
    if probe.links > 1 || probe.reparse_point {
        return Ok(Outcome::Skipped("hard link or reparse point".into()));
    }
    fault(opts, seq, 2)?;
    lock.rewind()?;
    let h0 = match mm_fs::copy_new_hashing(&mut lock, &f.backup) {
        Ok(h) => mm_fs::hex(&h),
        Err(e) if mm_fs::is_disk_full(&e) => {
            return Ok(Outcome::DiskFull(format!(
                "backup volume is full ({e}); file unchanged"
            )));
        }
        Err(e) => return Ok(Outcome::Failed(format!("backup failed: {e}"))),
    };
    fault(opts, seq, 3)?;
    if h0 != want {
        remove_if_exists(&f.backup);
        return Ok(Outcome::Conflict(
            "file changed after the operation; not removed".into(),
        ));
    }
    if hash_opt(&f.backup).as_deref() != Some(h0.as_str()) {
        remove_if_exists(&f.backup);
        return Ok(Outcome::Failed("backup verification failed".into()));
    }
    store.set_state(
        op_id,
        seq,
        FileState::BackedUp,
        &FileUpdate {
            h0: Some(h0.clone()),
            ..Default::default()
        },
    )?;
    fault(opts, seq, 4)?;
    store.set_state(op_id, seq, FileState::Ready, &FileUpdate::default())?;
    fault(opts, seq, 7)?;
    if mm_fs::file_id_of_path(&f.path).ok() != Some(mm_fs::file_id(&lock)?) {
        return Ok(Outcome::Conflict(
            "file was renamed or replaced during the operation".into(),
        ));
    }
    // the commit: a rename that never replaces; the lock handle shares DELETE, so it is allowed
    match mm_fs::move_no_replace(&f.path, &f.bak) {
        Ok(()) => {}
        Err(mm_fs::Win32Error(80 | 183)) => {
            return Ok(Outcome::Failed(
                "backup name already exists next to the file".into(),
            ));
        }
        Err(mm_fs::Win32Error(c @ (32 | 33))) => {
            return Ok(Outcome::Skipped(format!(
                "file is in use by another program (Win32 {c})"
            )));
        }
        Err(code) => {
            return Ok(Outcome::Failed(format!(
                "could not move the file ({code}); file unchanged"
            )));
        }
    }
    fault(opts, seq, 8)?;
    store.set_state(op_id, seq, FileState::Committed, &FileUpdate::default())?;
    fault(opts, seq, 9)?;
    drop(lock);
    if hash_opt(&f.bak).as_deref() == Some(h0.as_str()) {
        remove_if_exists(&f.bak);
    } else {
        return Ok(Outcome::Attention(
            "moved file differs from its backup; nothing deleted".into(),
        ));
    }
    fault(opts, seq, 10)?;
    store.set_state(op_id, seq, FileState::Done, &FileUpdate::default())?;
    Ok(Outcome::Done)
}

/// Undo of a deleted or moved file (SAFETY_MODEL §7.2, committed like §4.3): the verified backup
/// is copied next to the path, recorded as Ready, then renamed onto the path without ever
/// replacing a file that has appeared there. Fault points 1 and 5–10 as in the file header.
fn recreate(
    store: &Journal,
    op_id: &str,
    seq: u32,
    backup: &Path,
    h0: &str,
    f: &FilePaths,
    opts: &ExecOptions,
) -> Result<Outcome, CoreError> {
    fault(opts, seq, 1)?;
    if mm_fs::ensure_absent(&f.path).is_err() {
        return Ok(Outcome::Conflict(
            "a file exists at this path again; not recreated".into(),
        ));
    }
    let mut src = match File::open(backup) {
        Ok(s) => s,
        Err(e) => return Ok(Outcome::Failed(format!("backup unavailable: {e}"))),
    };
    // copy_new_hashing never overwrites, flushes the copy and removes it on error
    let h1 = match mm_fs::copy_new_hashing(&mut src, &f.temp) {
        Ok(h) => mm_fs::hex(&h),
        Err(e) if mm_fs::is_disk_full(&e) => {
            return Ok(Outcome::DiskFull(format!(
                "photo volume is full ({e}); nothing created"
            )));
        }
        Err(e) => {
            return Ok(Outcome::Failed(format!(
                "cannot write next to the path: {e}"
            )));
        }
    };
    if h1 != h0 {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Failed(
            "backup content does not match its recorded hash".into(),
        ));
    }
    fault(opts, seq, 5)?;
    fault(opts, seq, 6)?;
    commit_new(store, op_id, seq, f, h1, opts)
}

/// A new file written from nothing (a new XMP sidecar, SAFETY_MODEL §3.1, §4.3): ExifTool writes
/// only the planned tags to a registered temporary name next to the path, the output is verified
/// against an empty source (V1–V3, V5; there is no image data), and it is committed like a
/// recreate: never replacing a file that has appeared at the path. Fault points 1 and 5–10.
#[allow(clippy::too_many_arguments)]
fn create_file(
    store: &Journal,
    engine: &mut Engine,
    op_id: &str,
    seq: u32,
    ops: &[mm_domain::plan::TagOp],
    expect: &[mm_domain::plan::Expect],
    f: &FilePaths,
    opts: &ExecOptions,
) -> Result<Outcome, CoreError> {
    fault(opts, seq, 1)?;
    if mm_fs::ensure_absent(&f.path).is_err() {
        return Ok(Outcome::Conflict(
            "a file exists at this path now; not created".into(),
        ));
    }
    let out = match engine.write_new(ops, &f.temp) {
        Ok(o) => o,
        Err(e) => {
            remove_if_exists(&f.temp);
            return Ok(Outcome::Failed(format!("ExifTool: {e}")));
        }
    };
    if let Err(e) = verify::check_write_output(&out) {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Failed(format!("verification V1: {e}")));
    }
    if !f.temp.exists() {
        return Ok(Outcome::Failed("ExifTool produced no output".into()));
    }
    fault(opts, seq, 5)?;
    let reads = engine.read_full(&[f.temp.as_path()])?;
    let Some(tmp) = reads[0].as_ref() else {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Failed(
            "verification V5: output not readable".into(),
        ));
    };
    if let Err(e) = verify::check_output(&Snapshot::default(), tmp, ops, expect) {
        remove_if_exists(&f.temp);
        return Ok(Outcome::Failed(format!("verification: {e}")));
    }
    mm_fs::flush_path(&f.temp)?;
    let h1 = mm_fs::hex(&mm_fs::hash_path(&f.temp)?);
    fault(opts, seq, 6)?;
    commit_new(store, op_id, seq, f, h1, opts)
}

/// Commit of a file that did not exist (recreate, create): Ready with its hash, then a rename
/// that never replaces (I-6), Committed, post-check, Done. Fault points 7–10.
fn commit_new(
    store: &Journal,
    op_id: &str,
    seq: u32,
    f: &FilePaths,
    h1: String,
    opts: &ExecOptions,
) -> Result<Outcome, CoreError> {
    store.set_state(
        op_id,
        seq,
        FileState::Ready,
        &FileUpdate {
            h1: Some(h1.clone()),
            ..Default::default()
        },
    )?;
    fault(opts, seq, 7)?;
    match mm_fs::move_no_replace(&f.temp, &f.path) {
        Ok(()) => {}
        // ERROR_FILE_EXISTS / ERROR_ALREADY_EXISTS: something appeared at the path (I-6)
        Err(mm_fs::Win32Error(80 | 183)) => {
            remove_if_exists(&f.temp);
            return Ok(Outcome::Conflict(
                "a file appeared at this path; nothing written there".into(),
            ));
        }
        Err(code) => {
            remove_if_exists(&f.temp);
            return Ok(Outcome::Failed(format!(
                "could not put the file in place ({code}); nothing created"
            )));
        }
    }
    fault(opts, seq, 8)?;
    let new_id = mm_fs::file_id_of_path(&f.path)
        .ok()
        .map(|i| crate::file_id_hex(&i));
    store.set_state(
        op_id,
        seq,
        FileState::Committed,
        &FileUpdate {
            new_file_id: new_id,
            ..Default::default()
        },
    )?;
    fault(opts, seq, 9)?;
    if hash_opt(&f.path).as_deref() != Some(h1.as_str()) {
        return Ok(Outcome::Attention(
            "content after creating differs from the verified output".into(),
        ));
    }
    fault(opts, seq, 10)?;
    store.set_state(op_id, seq, FileState::Done, &FileUpdate::default())?;
    Ok(Outcome::Done)
}

/// ReplaceFileW failed: classify what is on disk (SAFETY_MODEL §4.5) and restore if needed.
fn commit_failed(f: &FilePaths, h0: &str, code: mm_fs::Win32Error) -> Outcome {
    let cur = hash_opt(&f.path);
    if cur.as_deref() == Some(h0) {
        remove_if_exists(&f.temp);
        return match code.0 {
            32 | 33 => Outcome::Skipped("file is in use by another program".into()),
            5 => Outcome::Failed("access denied when replacing the file".into()),
            c => Outcome::Failed(format!("replace failed (Win32 {c}); original unchanged")),
        };
    }
    if cur.is_none() && hash_opt(&f.bak).as_deref() == Some(h0) {
        return match mm_fs::move_no_replace(&f.bak, &f.path) {
            Ok(()) => {
                remove_if_exists(&f.temp);
                Outcome::Failed(format!(
                    "replace failed (Win32 {}); original put back",
                    code.0
                ))
            }
            Err(e) => Outcome::Attention(format!(
                "replace failed (Win32 {}); original is at {} ({e})",
                code.0,
                f.bak.display()
            )),
        };
    }
    Outcome::Attention(format!(
        "replace failed (Win32 {}); unexpected state, nothing deleted",
        code.0
    ))
}

/// The field values shown in the Preview must still be the source's values. A sidecar's Preview
/// shows values that may come from its RAW, so for XMP targets the fingerprint of the sidecar
/// (checked under the lock) is what guards against a change since the Preview.
fn before_matches(entry: &PlanEntry, src: &Snapshot) -> Result<(), VerifyError> {
    if entry.raw.is_some() || entry.path.to_ascii_lowercase().ends_with(".xmp") {
        return Ok(());
    }
    for ch in &entry.changes {
        let now = match ch.field.as_str() {
            creator::FIELD => creator::read(src).effective,
            copyright::FIELD => copyright::read(src).effective.map(|v| vec![v]),
            gps::FIELD => gps::read(src).map(|p| vec![p.display()]),
            capture::FIELD => capture::read(src)
                .ok()
                .flatten()
                .map(|t| vec![capture::display(&t)]),
            // a field without this check must not be written unchecked
            other => {
                return Err(VerifyError::Value(format!(
                    "no preview check for field {other}"
                )));
            }
        };
        if now != ch.before {
            return Err(VerifyError::ChangedSincePreview(format!(
                "{} is now {now:?}, preview showed {:?}",
                ch.field, ch.before
            )));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::Duration;

    #[test]
    fn volume_gate_caps_concurrency_at_the_volume_limit() {
        let path = std::env::temp_dir().join("any.jpg");
        let root = mm_fs::volume_root(&path).unwrap();
        let limit = mm_fs::volume_kind(&root).io_limit();
        let gate = VolumeGate::default();
        let mut held: Vec<Permit<'_>> = (0..limit).map(|_| gate.acquire(&path)).collect();
        let got_one_more = AtomicBool::new(false);
        std::thread::scope(|s| {
            s.spawn(|| {
                let _p = gate.acquire(&path);
                got_one_more.store(true, Ordering::SeqCst);
            });
            std::thread::sleep(Duration::from_millis(200));
            assert!(
                !got_one_more.load(Ordering::SeqCst),
                "a permit beyond the limit {limit} was granted"
            );
            held.pop(); // one transaction finishes
            for _ in 0..50 {
                if got_one_more.load(Ordering::SeqCst) {
                    break;
                }
                std::thread::sleep(Duration::from_millis(20));
            }
            assert!(
                got_one_more.load(Ordering::SeqCst),
                "the freed permit was not handed on"
            );
        });
    }

    #[test]
    fn circuit_breaker_counts_in_completion_order() {
        let mut s = Sched {
            queue: VecDeque::new(),
            completed: 0,
            failures_first20: 0,
            consecutive_verify_failures: 0,
            tripped: None,
            fatal: None,
            progress: ExecProgress::default(),
        };
        let failed = Settled {
            state: FileState::Failed,
            verify_fail: false,
            stop: None,
        };
        let done = Settled {
            state: FileState::Done,
            verify_fail: false,
            stop: None,
        };
        for x in [&done, &failed, &done] {
            s.record(x);
        }
        assert!(s.tripped.is_none()); // fewer than 4 samples
        s.record(&failed); // 2 of 4 failed
        assert!(s.tripped.is_some());
    }
}
