//! Operation execution: the single-file transaction of SAFETY_MODEL §4.1 for every executable
//! plan entry, with journal records before every irreversible step (I-8).
//!
//! Fault points (for the fault-injection tests only) terminate the process immediately:
//!   1 before lock · 2 after lock/fingerprint · 3 after backup copy · 4 after BackedUp recorded ·
//!   5 after temp written · 6 after verification · 7 after Ready recorded · 8 after ReplaceFileW ·
//!   9 after Committed recorded · 10 after bak removed (before Done recorded)
//!
//! A full volume (SAFETY_MODEL §8.13) pauses the Operation: the file in progress is settled with
//! the recovery table, it and every later file become `Cancelled` (original unchanged, retried
//! by resume once space is freed), and the Operation ends `Cancelled`.

use std::collections::BTreeMap;
use std::fs::File;
use std::io::Seek;
use std::path::{Path, PathBuf};
use std::time::Duration;

use mm_domain::creator;
use mm_domain::plan::{EntryAction, Plan, PlanEntry, PlanKind};
use mm_domain::snapshot::Snapshot;
use mm_store::{FileState, FileUpdate, NewFile, NewOperation, OpStatus, Store};

use crate::engine::Engine;
use crate::verify::{self, VerifyError};
use crate::{APP_VERSION, CoreError, fingerprint_of_handle, hash_opt, new_id};

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
}

/// Backup-volume reserve on top of the backups themselves (SAFETY_MODEL §6.2).
pub const SPACE_RESERVE: u64 = 1 << 30;

const DISK_FULL_STOP: &str = "paused: a volume is full; free space, then resume";

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
        let size = e.fingerprint.size;
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
    engine: &mut Engine,
    plan: &Plan,
    opts: &ExecOptions,
) -> Result<OpReport, CoreError> {
    require_no_pending_recovery(store)?;
    if plan.exiftool_version != engine.version() {
        return Err(CoreError::VersionMismatch(format!(
            "plan made with ExifTool {}, running {}",
            plan.exiftool_version,
            engine.version()
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
            role: "embedded".into(),
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
            exiftool_version: engine.version().into(),
            registry_version: plan.registry_version,
            undo_of,
        },
        &files,
    )?;
    let seqs: Vec<u32> = files.iter().map(|f| f.seq).collect();
    run(store, engine, &op_id, plan, &seqs, opts)
}

/// Continue an Operation after recovery or cancellation: retry every file in `not_started`.
pub fn resume(
    store: &mut Store,
    engine: &mut Engine,
    op_id: &str,
    opts: &ExecOptions,
) -> Result<OpReport, CoreError> {
    require_no_pending_recovery(store)?;
    let op = store
        .operation(op_id)?
        .ok_or_else(|| CoreError::Input(format!("no operation {op_id}")))?;
    if op.app_version != APP_VERSION || op.exiftool_version != engine.version() {
        return Err(CoreError::VersionMismatch(format!(
            "operation made with app {} / ExifTool {}; running {} / {} — undo it or plan again",
            op.app_version,
            op.exiftool_version,
            APP_VERSION,
            engine.version()
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
    run(store, engine, op_id, &plan, &seqs, opts)
}

fn run(
    store: &mut Store,
    engine: &mut Engine,
    op_id: &str,
    plan: &Plan,
    seqs: &[u32],
    opts: &ExecOptions,
) -> Result<OpReport, CoreError> {
    let rows = store.files(op_id)?;
    let mut failures_first20 = 0usize;
    let mut consecutive_verify_failures = 0usize;
    let mut tripped: Option<String> = None;
    for (n, seq) in seqs.iter().enumerate() {
        let row = rows
            .iter()
            .find(|r| r.seq == *seq)
            .cloned()
            .ok_or_else(|| CoreError::Internal(format!("missing file row {seq}")))?;
        if tripped.is_some() {
            store.set_state(
                op_id,
                *seq,
                FileState::Cancelled,
                &FileUpdate {
                    error: tripped.clone(),
                    ..Default::default()
                },
            )?;
            continue;
        }
        let entry = plan
            .entries
            .iter()
            .find(|e| e.seq == *seq)
            .ok_or_else(|| CoreError::Internal(format!("missing plan entry {seq}")))?;
        let files = FilePaths {
            path: PathBuf::from(&row.path),
            temp: PathBuf::from(&row.temp_path),
            bak: PathBuf::from(&row.bak_path),
            backup: PathBuf::from(&row.backup_path),
        };
        let outcome = match one_file(store, engine, op_id, entry, &files, opts) {
            Ok(o) => o,
            // An error in the middle of the transaction (IO, engine, journal): the lock is released;
            // settle this file from the journal and the disk with the recovery table (SAFETY_MODEL §10).
            // If the journal itself is failing, `?` stops the Operation in `running` state so that
            // recovery handles it at the next start.
            Err(e) => {
                let now = store
                    .files(op_id)?
                    .into_iter()
                    .find(|r| r.seq == *seq)
                    .ok_or_else(|| CoreError::Internal(format!("missing file row {seq}")))?;
                let (to, action) = crate::recovery::decide(&now);
                if e.is_disk_full() {
                    tripped = Some(DISK_FULL_STOP.into()); // whatever became of this file
                }
                match to {
                    FileState::NotStarted if e.is_disk_full() => {
                        Outcome::DiskFull(format!("{e}; original unchanged"))
                    }
                    FileState::NotStarted => Outcome::Failed(format!("{e}; original unchanged")),
                    FileState::Done => {
                        // keep the error visible: the commit was confirmed from the disk, not
                        // from a complete journal
                        store.set_state(
                            op_id,
                            *seq,
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
                tripped = Some(DISK_FULL_STOP.into());
                (FileState::Cancelled, Some(r), false)
            }
        };
        if state != FileState::Done {
            store.set_state(
                op_id,
                *seq,
                state,
                &FileUpdate {
                    error: reason,
                    ..Default::default()
                },
            )?;
        }
        // circuit breaker (SAFETY_MODEL §8.16)
        if n < 20 && state == FileState::Failed {
            failures_first20 += 1;
        }
        consecutive_verify_failures = if verify_fail {
            consecutive_verify_failures + 1
        } else {
            0
        };
        let sample = (n + 1).min(20);
        if (sample >= 4 && failures_first20 * 2 >= sample && n < 20)
            || consecutive_verify_failures >= 10
        {
            tripped = Some(
                "stopped: too many failures (circuit breaker); check ExifTool and the files".into(),
            );
        }
    }
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
    store: &mut Store,
    engine: &mut Engine,
    op_id: &str,
    entry: &PlanEntry,
    f: &FilePaths,
    opts: &ExecOptions,
) -> Result<Outcome, CoreError> {
    let seq = entry.seq;
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

/// The field values shown in the Preview must still be the source's values.
fn before_matches(entry: &PlanEntry, src: &Snapshot) -> Result<(), VerifyError> {
    for ch in &entry.changes {
        if ch.field == creator::FIELD {
            let now = creator::read(src).effective;
            if now != ch.before {
                return Err(VerifyError::ChangedSincePreview(format!(
                    "creator is now {now:?}, preview showed {:?}",
                    ch.before
                )));
            }
        }
    }
    Ok(())
}
