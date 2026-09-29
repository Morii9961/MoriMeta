// SPDX-License-Identifier: GPL-3.0-or-later
//! Operation Journal (SAFETY_MODEL §9) on SQLite (WAL, synchronous=FULL), plus the backup
//! store layout and the self-describing `manifest.json` written next to each Operation's backups.
//!
//! Every state change is its own committed transaction; with `synchronous=FULL` a commit returns
//! only after the WAL is flushed, which is what I-8 requires ("record before irreversible action").

mod mlog;

use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::{Value, json};

pub use mlog::{ImportReport, MANIFEST_LOG, PLAN_FILE};

pub const SCHEMA_VERSION: i32 = 5;

/// Every backup location an Operation has used, one per line, in the data folder but outside the
/// database: the location setting lives in the database, so a lost database would otherwise be
/// rebuilt from the default location only, and the Operations kept elsewhere would vanish from
/// History and Undo.
pub const BACKUP_LOCATIONS: &str = "backup-locations.txt";

/// The same folder, as Windows compares names (case-insensitive, trailing separators ignored).
fn same_dir(a: &Path, b: &Path) -> bool {
    let k = |p: &Path| {
        p.to_string_lossy()
            .trim_end_matches(['\\', '/'])
            .replace('/', "\\")
            .to_lowercase()
    };
    k(a) == k(b)
}

pub type Result<T> = std::result::Result<T, StoreError>;

#[derive(Debug)]
pub enum StoreError {
    Sql(rusqlite::Error),
    Io(std::io::Error),
    /// The database was written by a newer MoriMeta (ARCHITECTURE §11: never downgrade).
    NewerSchema(i32),
    NotFound(String),
}

impl std::fmt::Display for StoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for StoreError {}

impl StoreError {
    /// The journal or manifest could not be written because a volume is full (`SQLITE_FULL`,
    /// Win32 112/39).
    pub fn is_disk_full(&self) -> bool {
        match self {
            StoreError::Sql(e) => e.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull),
            StoreError::Io(e) => matches!(e.raw_os_error(), Some(112 | 39)),
            _ => false,
        }
    }
}
impl From<rusqlite::Error> for StoreError {
    fn from(e: rusqlite::Error) -> Self {
        StoreError::Sql(e)
    }
}
impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

/// File states (SAFETY_MODEL §4.1, §10). Terminal: Done, Failed, Skipped, Conflict, Cancelled,
/// NotStarted (after recovery), Attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileState {
    Planned,
    BackedUp,
    Ready,
    Committed,
    Done,
    Failed,
    Skipped,
    Conflict,
    Cancelled,
    NotStarted,
    Attention,
}

impl FileState {
    pub fn as_str(self) -> &'static str {
        match self {
            FileState::Planned => "planned",
            FileState::BackedUp => "backed_up",
            FileState::Ready => "ready",
            FileState::Committed => "committed",
            FileState::Done => "done",
            FileState::Failed => "failed",
            FileState::Skipped => "skipped",
            FileState::Conflict => "conflict",
            FileState::Cancelled => "cancelled",
            FileState::NotStarted => "not_started",
            FileState::Attention => "attention",
        }
    }
    pub fn parse(s: &str) -> Option<FileState> {
        Some(match s {
            "planned" => FileState::Planned,
            "backed_up" => FileState::BackedUp,
            "ready" => FileState::Ready,
            "committed" => FileState::Committed,
            "done" => FileState::Done,
            "failed" => FileState::Failed,
            "skipped" => FileState::Skipped,
            "conflict" => FileState::Conflict,
            "cancelled" => FileState::Cancelled,
            "not_started" => FileState::NotStarted,
            "attention" => FileState::Attention,
            _ => return None,
        })
    }
    pub fn is_terminal(self) -> bool {
        !matches!(
            self,
            FileState::Planned | FileState::BackedUp | FileState::Ready | FileState::Committed
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpStatus {
    Running,
    Completed,
    CompletedWithErrors,
    Cancelled,
    Interrupted,
    Recovered,
}

impl OpStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            OpStatus::Running => "running",
            OpStatus::Completed => "completed",
            OpStatus::CompletedWithErrors => "completed_with_errors",
            OpStatus::Cancelled => "cancelled",
            OpStatus::Interrupted => "interrupted",
            OpStatus::Recovered => "recovered",
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewOperation {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub plan_json: String,
    pub app_version: String,
    pub exiftool_version: String,
    pub registry_version: u32,
    pub undo_of: Option<String>,
}

/// One file of an Operation, registered before anything happens to it (step 0).
#[derive(Debug, Clone)]
pub struct NewFile {
    pub seq: u32,
    pub path: String,
    pub role: String,
    pub temp_path: String,
    pub bak_path: String,
    pub backup_path: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRow {
    pub op_id: String,
    pub seq: u32,
    pub path: String,
    pub role: String,
    pub temp_path: String,
    pub bak_path: String,
    pub backup_path: String,
    pub state: FileState,
    pub h0: Option<String>,
    pub h1: Option<String>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperationRow {
    pub id: String,
    pub kind: String,
    pub title: String,
    pub status: String,
    pub created_ms: i64,
    pub finished_ms: Option<i64>,
    pub plan_json: String,
    pub exiftool_version: String,
    pub app_version: String,
    pub registry_version: u32,
    pub undo_of: Option<String>,
    pub backup_dir: String,
    /// Marked by the user: never pruned automatically (SAFETY_MODEL §6.3).
    pub keep: bool,
    /// When the backups were pruned; the Operation can no longer be undone.
    pub pruned_ms: Option<i64>,
}

/// A user Preset as stored (PRODUCT_SPEC §6.11).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PresetRow {
    pub id: String,
    pub name: String,
    pub json: String,
    pub created_ms: i64,
    pub updated_ms: i64,
    pub last_used_ms: Option<i64>,
    /// Came from a file (SECURITY_MODEL §9).
    pub imported: bool,
}

#[derive(Debug, Default, Clone)]
pub struct FileUpdate {
    pub h0: Option<String>,
    pub h1: Option<String>,
    pub new_file_id: Option<String>,
    pub error: Option<String>,
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Which journal write fails (fault-injection tests only; SAFETY_MODEL §12).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WriteTarget {
    /// The single transaction that registers the Operation and all its files (step 0).
    Begin,
    /// The state change of file `seq` to `state`.
    File { seq: u32, state: FileState },
    /// The final status of the Operation.
    Finish,
    /// A rewrite of an Operation's `manifest.json`, after skipping this many successful ones.
    Manifest(u32),
    /// An append to `manifest.jsonl`, after skipping this many successful ones. The append
    /// fails with an IO error before writing (simulated; the database write it follows has
    /// already been committed).
    Log(u32),
}

/// A journal write failure produced by SQLite itself: right before the target write, a second
/// connection takes the database write lock (`BEGIN IMMEDIATE`), so the write fails with a real
/// `SQLITE_BUSY`. `persistent` keeps the lock until the process ends, so every later write fails
/// too (a journal that stays unavailable); otherwise only the target write fails.
///
/// For [`WriteTarget::Manifest`], a directory is created at the manifest's temporary name, so the
/// file system refuses to create the file (access denied); `persistent` keeps that directory
/// until the process ends, so every later manifest write of that Operation fails too.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WriteFault {
    pub target: WriteTarget,
    pub persistent: bool,
}

const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

pub struct Store {
    conn: Connection,
    data_dir: PathBuf,
    fault: Option<WriteFault>,
    blocker: Option<Connection>,
    manifest_blocker: Option<PathBuf>,
    /// A persistent `Log` fault fired: every later append fails.
    log_blocked: bool,
    /// Where new Operations keep their backups (SAFETY_MODEL §6: configurable; existing
    /// Operations keep the folder recorded with them).
    backup_root: PathBuf,
    /// Open `manifest.jsonl` of each Operation written by this process.
    logs: HashMap<String, std::fs::File>,
}

impl Drop for Store {
    fn drop(&mut self) {
        if let Some(d) = self.manifest_blocker.take() {
            let _ = std::fs::remove_dir(d);
        }
    }
}

impl Store {
    /// Open (or create) `<data_dir>/db/morimeta.sqlite` and `<data_dir>/backups`.
    pub fn open(data_dir: &Path) -> Result<Store> {
        std::fs::create_dir_all(data_dir.join("db"))?;
        std::fs::create_dir_all(data_dir.join("backups"))?;
        let conn = Connection::open(data_dir.join("db").join("morimeta.sqlite"))?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        let v: i32 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if v > SCHEMA_VERSION {
            return Err(StoreError::NewerSchema(v));
        }
        if v < 1 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE operations(
                   id TEXT PRIMARY KEY, kind TEXT NOT NULL, title TEXT NOT NULL, status TEXT NOT NULL,
                   created_ms INTEGER NOT NULL, finished_ms INTEGER,
                   plan_json TEXT NOT NULL, app_version TEXT NOT NULL, exiftool_version TEXT NOT NULL,
                   registry_version INTEGER NOT NULL, undo_of TEXT, backup_dir TEXT NOT NULL);
                 CREATE TABLE op_files(
                   op_id TEXT NOT NULL REFERENCES operations(id), seq INTEGER NOT NULL,
                   path TEXT NOT NULL, role TEXT NOT NULL,
                   temp_path TEXT NOT NULL, bak_path TEXT NOT NULL, backup_path TEXT NOT NULL,
                   state TEXT NOT NULL, h0 TEXT, h1 TEXT, new_file_id TEXT, error TEXT,
                   updated_ms INTEGER NOT NULL, PRIMARY KEY(op_id, seq));
                 CREATE INDEX op_files_state ON op_files(state);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )?;
        }
        if v < 2 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE operations ADD COLUMN keep INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE operations ADD COLUMN pruned_ms INTEGER;
                 PRAGMA user_version = 2;
                 COMMIT;",
            )?;
        }
        if v < 3 {
            conn.execute_batch(
                "BEGIN;
                 CREATE TABLE presets(
                   id TEXT PRIMARY KEY, name TEXT NOT NULL, json TEXT NOT NULL,
                   created_ms INTEGER NOT NULL, updated_ms INTEGER NOT NULL, last_used_ms INTEGER);
                 CREATE TABLE settings(key TEXT PRIMARY KEY, value TEXT NOT NULL);
                 PRAGMA user_version = 3;
                 COMMIT;",
            )?;
        }
        if v < 4 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE presets ADD COLUMN imported INTEGER NOT NULL DEFAULT 0;
                 PRAGMA user_version = 4;
                 COMMIT;",
            )?;
        }
        if v < 5 {
            conn.execute_batch(
                "BEGIN;
                 ALTER TABLE operations ADD COLUMN acks TEXT;
                 PRAGMA user_version = 5;
                 COMMIT;",
            )?;
        }
        Ok(Store {
            conn,
            data_dir: data_dir.to_path_buf(),
            fault: None,
            blocker: None,
            manifest_blocker: None,
            log_blocked: false,
            backup_root: data_dir.join("backups"),
            logs: HashMap::new(),
        })
    }

    /// Append one record to the Operation's `manifest.jsonl`; `durable` flushes it to disk.
    fn log(&mut self, op_id: &str, line: Value, durable: bool) -> Result<()> {
        match self.fault {
            Some(WriteFault {
                target: WriteTarget::Log(0),
                persistent,
            }) => {
                self.fault = None;
                self.log_blocked = persistent;
                return Err(injected_log_failure());
            }
            Some(WriteFault {
                target: WriteTarget::Log(n),
                persistent,
            }) => {
                self.fault = Some(WriteFault {
                    target: WriteTarget::Log(n - 1),
                    persistent,
                });
            }
            _ => {}
        }
        if self.log_blocked {
            return Err(injected_log_failure());
        }
        if !self.logs.contains_key(op_id) {
            let f = std::fs::OpenOptions::new()
                .append(true)
                .create(true)
                .open(self.recorded_backup_dir(op_id)?.join(MANIFEST_LOG))?;
            self.logs.insert(op_id.to_owned(), f);
        }
        let f = self
            .logs
            .get_mut(op_id)
            .ok_or_else(|| StoreError::NotFound(op_id.into()))?;
        let mut s = line.to_string();
        s.push('\n');
        f.write_all(s.as_bytes())?;
        if durable {
            f.sync_data()?;
        }
        Ok(())
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    /// Arm a journal write failure (fault-injection tests only).
    pub fn arm_write_fault(&mut self, fault: WriteFault) {
        self.fault = Some(fault);
    }

    /// Run a journal write; if it is the armed target, run it while another connection holds the
    /// write lock so that SQLite itself fails it.
    fn write<T>(
        &mut self,
        target: WriteTarget,
        f: impl FnOnce(&mut Connection) -> Result<T>,
    ) -> Result<T> {
        let armed = self.fault.filter(|x| x.target == target);
        if let Some(x) = armed {
            self.fault = None;
            let blocker = Connection::open(self.data_dir.join("db").join("morimeta.sqlite"))?;
            blocker.execute_batch("BEGIN IMMEDIATE")?;
            self.blocker = Some(blocker);
            self.conn
                .busy_timeout(std::time::Duration::from_millis(50))?;
            let r = f(&mut self.conn);
            if !x.persistent {
                self.blocker = None; // closing the connection rolls back and releases the lock
                self.conn.busy_timeout(BUSY_TIMEOUT)?;
            }
            return r;
        }
        f(&mut self.conn)
    }

    /// The folder a new Operation `op_id` gets under the current backup location.
    pub fn backup_dir(&self, op_id: &str) -> PathBuf {
        self.backup_root.join(op_id)
    }

    /// The folder recorded with an existing Operation (or, before it is registered, the one it
    /// will get).
    pub fn recorded_backup_dir(&self, op_id: &str) -> Result<PathBuf> {
        Ok(self
            .conn
            .query_row(
                "SELECT backup_dir FROM operations WHERE id = ?1",
                params![op_id],
                |r| r.get::<_, String>(0),
            )
            .optional()?
            .map_or_else(|| self.backup_dir(op_id), PathBuf::from))
    }

    /// Where new Operations keep their backups (the setting `backup.root`; default
    /// `<data>/backups`). Changing it affects only later Operations.
    pub fn set_backup_root(&mut self, root: PathBuf) {
        self.backup_root = root;
    }

    pub fn backup_root(&self) -> &Path {
        &self.backup_root
    }

    /// The backup location must exist (it is created if its parent does) and accept a new file;
    /// otherwise nothing may be written (INTERACTION_SPEC: backup unavailable → no writes).
    pub fn check_backup_root(&self) -> Result<()> {
        std::fs::create_dir_all(&self.backup_root)?;
        let probe = self
            .backup_root
            .join(format!(".mm-probe-{}", std::process::id()));
        std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe)?;
        std::fs::remove_file(&probe)?;
        Ok(())
    }

    /// The backup locations to look in: the current one and the default.
    pub fn backup_roots(&self) -> Vec<PathBuf> {
        let mut roots = vec![self.backup_root.clone(), self.data_dir.join("backups")];
        for r in self.recorded_backup_roots() {
            if !roots.iter().any(|k| same_dir(k, &r)) {
                roots.push(r);
            }
        }
        if same_dir(&roots[0], &roots[1]) {
            roots.remove(1);
        }
        roots
    }

    /// The locations listed in [`BACKUP_LOCATIONS`].
    fn recorded_backup_roots(&self) -> Vec<PathBuf> {
        std::fs::read_to_string(self.data_dir.join(BACKUP_LOCATIONS))
            .map(|s| {
                s.lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(PathBuf::from)
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Add the current backup location to [`BACKUP_LOCATIONS`] (flushed) before an Operation
    /// puts its first file there; the default location is always searched and not listed.
    fn record_backup_root(&self) -> Result<()> {
        let root = &self.backup_root;
        if same_dir(root, &self.data_dir.join("backups"))
            || self
                .recorded_backup_roots()
                .iter()
                .any(|r| same_dir(r, root))
        {
            return Ok(());
        }
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.data_dir.join(BACKUP_LOCATIONS))?;
        writeln!(f, "{}", root.display())?;
        f.sync_all()?;
        Ok(())
    }

    /// Step 0: register the Operation, its executable plan and every file, in one transaction.
    pub fn begin_operation(&mut self, op: &NewOperation, files: &[NewFile]) -> Result<PathBuf> {
        let dir = self.backup_dir(&op.id);
        self.record_backup_root()?;
        std::fs::create_dir_all(&dir)?;
        let now = now_ms();
        self.write(WriteTarget::Begin, |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO operations(id, kind, title, status, created_ms, plan_json, app_version, exiftool_version, registry_version, undo_of, backup_dir)
                 VALUES(?1, ?2, ?3, 'running', ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                params![op.id, op.kind, op.title, now, op.plan_json, op.app_version, op.exiftool_version, op.registry_version, op.undo_of, dir.to_string_lossy()],
            )?;
            for f in files {
                tx.execute(
                    "INSERT INTO op_files(op_id, seq, path, role, temp_path, bak_path, backup_path, state, updated_ms)
                     VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, 'planned', ?8)",
                    params![op.id, f.seq, f.path, f.role, f.temp_path, f.bak_path, f.backup_path, now],
                )?;
            }
            tx.commit()?;
            Ok(())
        })?;
        // the backup folder describes itself: executable plan, then the append-only record
        {
            let mut p = std::fs::File::create(dir.join(PLAN_FILE))?;
            p.write_all(op.plan_json.as_bytes())?;
            p.sync_data()?;
        }
        self.log(
            &op.id,
            json!({"t": "op", "manifest_version": 1, "id": op.id, "kind": op.kind, "title": op.title,
                   "created_ms": now, "app_version": op.app_version,
                   "exiftool_version": op.exiftool_version, "registry_version": op.registry_version,
                   "undo_of": op.undo_of}),
            false,
        )?;
        for f in files {
            self.log(
                &op.id,
                json!({"t": "file", "seq": f.seq, "path": f.path, "role": f.role,
                       "temp": f.temp_path, "bak": f.bak_path, "backup": f.backup_path}),
                false,
            )?;
        }
        if let Some(f) = self.logs.get(&op.id) {
            f.sync_data()?;
        }
        self.write_manifest(&op.id)?;
        Ok(dir)
    }

    /// Record a state change (durable when this returns).
    pub fn set_state(
        &mut self,
        op_id: &str,
        seq: u32,
        state: FileState,
        u: &FileUpdate,
    ) -> Result<()> {
        let n = self.write(WriteTarget::File { seq, state }, |conn| {
            Ok(conn.execute(
                "UPDATE op_files SET state = ?3,
                     h0 = COALESCE(?4, h0), h1 = COALESCE(?5, h1), new_file_id = COALESCE(?6, new_file_id),
                     error = COALESCE(?7, error), updated_ms = ?8
                 WHERE op_id = ?1 AND seq = ?2",
                params![op_id, seq, state.as_str(), u.h0, u.h1, u.new_file_id, u.error, now_ms()],
            )?)
        })?;
        if n != 1 {
            return Err(StoreError::NotFound(format!("{op_id}#{seq}")));
        }
        // H0 and H1 must be on disk outside the database before a commit can follow
        let durable = matches!(state, FileState::BackedUp | FileState::Ready);
        self.log(
            op_id,
            json!({"t": "state", "seq": seq, "state": state.as_str(), "h0": u.h0, "h1": u.h1,
                   "new_file_id": u.new_file_id, "error": u.error}),
            durable,
        )
    }

    /// Register fresh temp/bak/backup names for a file that is about to be retried (resume).
    pub fn set_paths(
        &mut self,
        op_id: &str,
        seq: u32,
        temp: &str,
        bak: &str,
        backup: &str,
    ) -> Result<()> {
        let target = WriteTarget::File {
            seq,
            state: FileState::Planned,
        };
        let n = self.write(target, |conn| {
            Ok(conn.execute(
                "UPDATE op_files SET temp_path = ?3, bak_path = ?4, backup_path = ?5, state = 'planned', h0 = NULL, h1 = NULL, error = NULL, updated_ms = ?6
                 WHERE op_id = ?1 AND seq = ?2",
                params![op_id, seq, temp, bak, backup, now_ms()],
            )?)
        })?;
        if n != 1 {
            return Err(StoreError::NotFound(format!("{op_id}#{seq}")));
        }
        // the new names are registered before anything is created under them
        self.log(
            op_id,
            json!({"t": "paths", "seq": seq, "temp": temp, "bak": bak, "backup": backup}),
            true,
        )
    }

    pub fn finish_operation(&mut self, op_id: &str, status: OpStatus) -> Result<()> {
        let now = now_ms();
        self.write(WriteTarget::Finish, |conn| {
            conn.execute(
                "UPDATE operations SET status = ?2, finished_ms = ?3 WHERE id = ?1",
                params![op_id, status.as_str(), now],
            )?;
            Ok(())
        })?;
        self.log(
            op_id,
            json!({"t": "status", "status": status.as_str(), "finished_ms": now}),
            true,
        )?;
        self.write_manifest(op_id)?;
        Ok(())
    }

    pub fn set_status(&mut self, op_id: &str, status: OpStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET status = ?2 WHERE id = ?1",
            params![op_id, status.as_str()],
        )?;
        self.log(
            op_id,
            json!({"t": "status", "status": status.as_str()}),
            false,
        )
    }

    pub fn operation(&self, op_id: &str) -> Result<Option<OperationRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, kind, title, status, created_ms, finished_ms, plan_json, exiftool_version, app_version, registry_version, undo_of, backup_dir, keep, pruned_ms
                 FROM operations WHERE id = ?1",
                params![op_id],
                row_to_op,
            )
            .optional()?)
    }

    pub fn operations(&self) -> Result<Vec<OperationRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, kind, title, status, created_ms, finished_ms, plan_json, exiftool_version, app_version, registry_version, undo_of, backup_dir, keep, pruned_ms
             FROM operations ORDER BY created_ms, id",
        )?;
        let rows = st
            .query_map([], row_to_op)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// The acknowledgements given for an Operation (INTERACTION_SPEC §5), in the database and the
    /// append-only record.
    pub fn set_acks(&mut self, op_id: &str, acks: &[String]) -> Result<()> {
        let text = serde_json::to_string(acks).unwrap_or_default();
        self.conn.execute(
            "UPDATE operations SET acks = ?2 WHERE id = ?1",
            params![op_id, text],
        )?;
        self.log(op_id, json!({"t": "acks", "acks": acks}), true)
    }

    pub fn acks(&self, op_id: &str) -> Result<Vec<String>> {
        let text: Option<String> = self
            .conn
            .query_row(
                "SELECT acks FROM operations WHERE id = ?1",
                params![op_id],
                |r| r.get(0),
            )
            .optional()?
            .flatten();
        Ok(text
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default())
    }

    /// Mark (or unmark) an Operation as kept: its backups are never pruned automatically.
    pub fn set_keep(&mut self, op_id: &str, keep: bool) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET keep = ?2 WHERE id = ?1",
            params![op_id, keep],
        )?;
        Ok(())
    }

    /// Insert or replace a user Preset (its JSON is checked by the caller).
    /// `imported`: it came from a file (untrusted until first applied, SECURITY_MODEL §9); a later
    /// save by the user keeps the flag.
    pub fn save_preset(&mut self, id: &str, name: &str, json: &str, imported: bool) -> Result<()> {
        let now = now_ms();
        self.conn.execute(
            "INSERT INTO presets(id, name, json, created_ms, updated_ms, imported)
             VALUES(?1, ?2, ?3, ?4, ?4, ?5)
             ON CONFLICT(id) DO UPDATE SET name = ?2, json = ?3, updated_ms = ?4",
            params![id, name, json, now, imported],
        )?;
        Ok(())
    }

    pub fn presets(&self) -> Result<Vec<PresetRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, name, json, created_ms, updated_ms, last_used_ms, imported FROM presets
             ORDER BY name COLLATE NOCASE, id",
        )?;
        let rows = st
            .query_map([], |r| {
                Ok(PresetRow {
                    id: r.get(0)?,
                    name: r.get(1)?,
                    json: r.get(2)?,
                    created_ms: r.get(3)?,
                    updated_ms: r.get(4)?,
                    last_used_ms: r.get(5)?,
                    imported: r.get(6)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Whether it existed.
    pub fn delete_preset(&mut self, id: &str) -> Result<bool> {
        Ok(self
            .conn
            .execute("DELETE FROM presets WHERE id = ?1", params![id])?
            > 0)
    }

    pub fn touch_preset(&mut self, id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE presets SET last_used_ms = ?2 WHERE id = ?1",
            params![id, now_ms()],
        )?;
        Ok(())
    }

    pub fn setting(&self, key: &str) -> Result<Option<String>> {
        Ok(self
            .conn
            .query_row(
                "SELECT value FROM settings WHERE key = ?1",
                params![key],
                |r| r.get(0),
            )
            .optional()?)
    }

    pub fn set_setting(&mut self, key: &str, value: &str) -> Result<()> {
        self.conn.execute(
            "INSERT INTO settings(key, value) VALUES(?1, ?2)
             ON CONFLICT(key) DO UPDATE SET value = ?2",
            params![key, value],
        )?;
        Ok(())
    }

    pub fn clear_setting(&mut self, key: &str) -> Result<()> {
        self.conn
            .execute("DELETE FROM settings WHERE key = ?1", params![key])?;
        Ok(())
    }

    /// Record that the backups of an Operation are being removed (before they are deleted, so
    /// that an interrupted prune is finished rather than leaving an Operation that looks
    /// undoable without its backups).
    pub fn mark_pruned(&mut self, op_id: &str) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET pruned_ms = COALESCE(pruned_ms, ?2) WHERE id = ?1",
            params![op_id, now_ms()],
        )?;
        Ok(())
    }

    /// Operations left in `running` (crash) or `interrupted` (recovery not finished).
    pub fn unfinished(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT id FROM operations WHERE status IN ('running', 'interrupted') ORDER BY created_ms, id")?;
        let rows = st
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
        Ok(rows)
    }

    /// Operations registered after `op_id` that finished writing `path` (id, title), oldest first:
    /// who changed a file after it (SAFETY_MODEL §7.2).
    pub fn later_writers(&self, op_id: &str, path: &str) -> Result<Vec<(String, String)>> {
        let mut st = self.conn.prepare(
            "SELECT o.id, o.title FROM op_files f JOIN operations o ON o.id = f.op_id
             WHERE f.path = ?2 COLLATE NOCASE AND f.state = 'done'
               AND o.rowid > (SELECT rowid FROM operations WHERE id = ?1)
             ORDER BY o.rowid",
        )?;
        let rows = st
            .query_map(params![op_id, path], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    pub fn files(&self, op_id: &str) -> Result<Vec<FileRow>> {
        let mut st = self.conn.prepare(
            "SELECT op_id, seq, path, role, temp_path, bak_path, backup_path, state, h0, h1, error
             FROM op_files WHERE op_id = ?1 ORDER BY seq",
        )?;
        let rows = st
            .query_map(params![op_id], |r| {
                let state: String = r.get(7)?;
                Ok(FileRow {
                    op_id: r.get(0)?,
                    seq: r.get(1)?,
                    path: r.get(2)?,
                    role: r.get(3)?,
                    temp_path: r.get(4)?,
                    bak_path: r.get(5)?,
                    backup_path: r.get(6)?,
                    state: FileState::parse(&state).unwrap_or(FileState::Attention),
                    h0: r.get(8)?,
                    h1: r.get(9)?,
                    error: r.get(10)?,
                })
            })?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// `backups/<op>/manifest.json`: enough to understand and restore the backups without the DB.
    pub fn write_manifest(&mut self, op_id: &str) -> Result<()> {
        let armed = match self.fault {
            Some(WriteFault {
                target: WriteTarget::Manifest(0),
                ..
            }) => self.fault.take(),
            Some(WriteFault {
                target: WriteTarget::Manifest(n),
                persistent,
            }) => {
                self.fault = Some(WriteFault {
                    target: WriteTarget::Manifest(n - 1),
                    persistent,
                });
                None
            }
            _ => None,
        };
        let r = self.write_manifest_file(op_id, armed.is_some());
        if armed.is_some_and(|x| !x.persistent)
            && let Some(d) = self.manifest_blocker.take()
        {
            let _ = std::fs::remove_dir(d);
        }
        r
    }

    fn write_manifest_file(&mut self, op_id: &str, block: bool) -> Result<()> {
        let op = self
            .operation(op_id)?
            .ok_or_else(|| StoreError::NotFound(op_id.into()))?;
        let files = self.files(op_id)?;
        let doc = json!({
            "manifest_version": 1,
            "operation": {"id": op.id, "kind": op.kind, "title": op.title, "status": op.status,
                          "created_ms": op.created_ms, "finished_ms": op.finished_ms, "undo_of": op.undo_of,
                          "exiftool_version": op.exiftool_version, "app_version": op.app_version,
                          "registry_version": op.registry_version},
            "files": files.iter().map(|f| json!({
                "seq": f.seq, "path": f.path, "role": f.role, "backup": f.backup_path,
                "temp": f.temp_path, "bak": f.bak_path,
                "state": f.state.as_str(), "h0": f.h0, "h1": f.h1})).collect::<Vec<_>>(),
        });
        let dir = PathBuf::from(&op.backup_dir);
        std::fs::create_dir_all(&dir)?;
        let tmp = dir.join("manifest.json.tmp");
        if block {
            std::fs::create_dir(&tmp)?; // the file below can no longer be created
            self.manifest_blocker = Some(tmp.clone());
        }
        {
            use std::io::Write;
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(
                serde_json::to_string_pretty(&doc)
                    .unwrap_or_default()
                    .as_bytes(),
            )?;
            f.sync_all()?;
        }
        std::fs::rename(&tmp, dir.join("manifest.json"))?;
        Ok(())
    }
}

fn injected_log_failure() -> StoreError {
    StoreError::Io(std::io::Error::other(
        "injected manifest.jsonl append failure",
    ))
}

fn row_to_op(r: &rusqlite::Row<'_>) -> rusqlite::Result<OperationRow> {
    Ok(OperationRow {
        id: r.get(0)?,
        kind: r.get(1)?,
        title: r.get(2)?,
        status: r.get(3)?,
        created_ms: r.get(4)?,
        finished_ms: r.get(5)?,
        plan_json: r.get(6)?,
        exiftool_version: r.get(7)?,
        app_version: r.get(8)?,
        registry_version: r.get(9)?,
        undo_of: r.get(10)?,
        backup_dir: r.get(11)?,
        keep: r.get(12)?,
        pruned_ms: r.get(13)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("mm-store-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    }

    fn op(id: &str) -> NewOperation {
        NewOperation {
            id: id.into(),
            kind: "apply".into(),
            title: "t".into(),
            plan_json: "{}".into(),
            app_version: "0".into(),
            exiftool_version: "13.59".into(),
            registry_version: 0,
            undo_of: None,
        }
    }

    fn file(seq: u32) -> NewFile {
        NewFile {
            seq,
            path: format!("C:/p/{seq}.jpg"),
            role: "embedded".into(),
            temp_path: format!("C:/p/{seq}.mmtmp-00000000000000aa.jpg"),
            bak_path: format!("C:/p/{seq}.mmbak-00000000000000aa.jpg"),
            backup_path: String::new(),
        }
    }

    #[test]
    fn lifecycle_is_persisted_and_reopened() {
        let d = dir("life");
        {
            let mut s = Store::open(&d).unwrap();
            let bdir = s.begin_operation(&op("op1"), &[file(0), file(1)]).unwrap();
            assert!(bdir.join("manifest.json").exists());
            s.set_state(
                "op1",
                0,
                FileState::BackedUp,
                &FileUpdate {
                    h0: Some("aa".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            s.set_state(
                "op1",
                0,
                FileState::Ready,
                &FileUpdate {
                    h1: Some("bb".into()),
                    ..Default::default()
                },
            )
            .unwrap();
            assert_eq!(s.unfinished().unwrap(), vec!["op1".to_string()]);
        }
        let mut s = Store::open(&d).unwrap();
        let f = s.files("op1").unwrap();
        assert_eq!(
            (f[0].state, f[0].h0.as_deref(), f[0].h1.as_deref()),
            (FileState::Ready, Some("aa"), Some("bb"))
        );
        assert_eq!(f[1].state, FileState::Planned);
        s.finish_operation("op1", OpStatus::Completed).unwrap();
        assert!(s.unfinished().unwrap().is_empty());
        assert_eq!(s.operation("op1").unwrap().unwrap().status, "completed");
        assert!(matches!(
            s.set_state("op1", 9, FileState::Done, &FileUpdate::default()),
            Err(StoreError::NotFound(_))
        ));
    }

    fn busy(r: Result<()>) -> bool {
        matches!(r, Err(StoreError::Sql(e)) if e.sqlite_error_code() == Some(rusqlite::ErrorCode::DatabaseBusy))
    }

    #[test]
    fn armed_write_fails_in_sqlite_once_or_persistently() {
        let d = dir("fault");
        let mut s = Store::open(&d).unwrap();
        s.begin_operation(&op("op1"), &[file(0), file(1)]).unwrap();
        let backed = |h: &str| FileUpdate {
            h0: Some(h.into()),
            ..Default::default()
        };
        // once: only the target write fails and changes nothing; the next write succeeds
        s.arm_write_fault(WriteFault {
            target: WriteTarget::File {
                seq: 0,
                state: FileState::BackedUp,
            },
            persistent: false,
        });
        s.set_state("op1", 1, FileState::BackedUp, &backed("x"))
            .unwrap(); // not the target
        assert!(busy(s.set_state(
            "op1",
            0,
            FileState::BackedUp,
            &backed("a")
        )));
        assert_eq!(s.files("op1").unwrap()[0].state, FileState::Planned);
        s.set_state("op1", 0, FileState::BackedUp, &backed("a"))
            .unwrap();
        // persistent: every later write fails, reads still work
        s.arm_write_fault(WriteFault {
            target: WriteTarget::File {
                seq: 0,
                state: FileState::Ready,
            },
            persistent: true,
        });
        assert!(busy(s.set_state(
            "op1",
            0,
            FileState::Ready,
            &FileUpdate::default()
        )));
        assert!(busy(s.set_state(
            "op1",
            1,
            FileState::Ready,
            &FileUpdate::default()
        )));
        assert!(busy(s.finish_operation("op1", OpStatus::Completed)));
        let f = s.files("op1").unwrap();
        assert_eq!(
            (f[0].state, f[0].h0.as_deref(), f[1].state),
            (FileState::BackedUp, Some("a"), FileState::BackedUp)
        );
        drop(s);
        // a new process (connection) is not blocked
        let mut s = Store::open(&d).unwrap();
        s.set_state("op1", 0, FileState::Ready, &FileUpdate::default())
            .unwrap();
        assert_eq!(s.unfinished().unwrap(), vec!["op1".to_string()]);
    }

    #[test]
    fn journal_is_rebuilt_from_manifest_log_after_database_loss() {
        let d = dir("rebuild");
        let mut o = op("op1");
        o.plan_json = r#"{"plan":"executable"}"#.into();
        o.undo_of = Some("op0".into());
        let (before_op, before_files) = {
            let mut s = Store::open(&d).unwrap();
            s.begin_operation(&o, &[file(0), file(1), file(2)]).unwrap();
            let up = |h0: Option<&str>, h1: Option<&str>| FileUpdate {
                h0: h0.map(Into::into),
                h1: h1.map(Into::into),
                ..Default::default()
            };
            s.set_state("op1", 0, FileState::BackedUp, &up(Some("a0"), None))
                .unwrap();
            s.set_state("op1", 0, FileState::Ready, &up(None, Some("a1")))
                .unwrap();
            s.set_state("op1", 0, FileState::Committed, &FileUpdate::default())
                .unwrap();
            s.set_state("op1", 1, FileState::Cancelled, &FileUpdate::default())
                .unwrap();
            let backup = d.join("backups").join("op1").join("00000001-0123abcd.jpg");
            s.set_paths(
                "op1",
                1,
                "C:/p/1.mmtmp-00000000000000bb.jpg",
                "C:/p/1.mmbak-00000000000000bb.jpg",
                &backup.to_string_lossy(),
            )
            .unwrap();
            s.set_state("op1", 1, FileState::BackedUp, &up(Some("c0"), None))
                .unwrap();
            (
                s.operation("op1").unwrap().unwrap(),
                s.files("op1").unwrap(),
            )
        };
        // the database is lost; the backup folder remains
        std::fs::remove_dir_all(d.join("db")).unwrap();
        let log = d.join("backups").join("op1").join(MANIFEST_LOG);
        std::fs::OpenOptions::new()
            .append(true)
            .open(&log)
            .unwrap()
            .write_all(br#"{"t":"state","seq":2,"sta"#) // torn by a crash while appending
            .unwrap();
        let mut s = Store::open(&d).unwrap();
        assert!(s.operations().unwrap().is_empty());
        let r = s.import_from_backups().unwrap();
        assert_eq!(r.imported, vec!["op1".to_string()]);
        assert!(r.skipped.is_empty(), "{:?}", r.skipped);
        assert_eq!(s.operation("op1").unwrap().unwrap(), before_op);
        assert_eq!(s.files("op1").unwrap(), before_files);
        assert_eq!(s.unfinished().unwrap(), vec!["op1".to_string()]);
        // already present: nothing imported twice
        assert!(s.import_from_backups().unwrap().imported.is_empty());
        drop(s);

        // an Operation in another backup location: the setting is lost with the database, the
        // location list outside it is not
        let elsewhere = d.join("备份 📦");
        {
            let mut s = Store::open(&d).unwrap();
            s.set_backup_root(elsewhere.clone());
            s.begin_operation(&op("op2"), &[file(0)]).unwrap();
            s.begin_operation(&op("op3"), &[file(0)]).unwrap();
        }
        let listed = std::fs::read_to_string(d.join(BACKUP_LOCATIONS)).unwrap();
        assert_eq!(listed.lines().count(), 1, "{listed}");
        std::fs::remove_dir_all(d.join("db")).unwrap();
        let mut s = Store::open(&d).unwrap();
        let r = s.import_from_backups().unwrap();
        assert_eq!(r.imported, ["op1", "op2", "op3"]);
        assert_eq!(s.recorded_backup_dir("op2").unwrap(), elsewhere.join("op2"));
        // the list itself lost too (the whole data folder): the user names the folder
        drop(s);
        std::fs::remove_dir_all(d.join("db")).unwrap();
        std::fs::remove_file(d.join(BACKUP_LOCATIONS)).unwrap();
        let mut s = Store::open(&d).unwrap();
        assert_eq!(s.import_from_backups().unwrap().imported, ["op1"]);
        let r = s
            .import_from_backups_in(std::slice::from_ref(&elsewhere))
            .unwrap();
        assert_eq!(r.imported, ["op2", "op3"]);

        // SECURITY_MODEL §7: a record whose names recovery would act on are not MoriMeta's is
        // not imported (recovery removes a registered temporary name when its hash matches)
        for (id, temp, backup) in [
            ("op-evil1", "D:/Photos/elsewhere/important.jpg", ""),
            ("op-evil2", "C:/p/0.mmtmp-zz.jpg", ""),
            ("op-evil3", "", "D:/Photos/elsewhere/important.jpg"),
        ] {
            let evil = d.join("backups").join(id);
            std::fs::create_dir_all(&evil).unwrap();
            std::fs::write(
                evil.join(MANIFEST_LOG),
                format!(
                    "{}\n{}\n",
                    json!({"t": "op", "manifest_version": 1, "id": id, "kind": "apply", "title": "x"}),
                    json!({"t": "file", "seq": 0, "path": "C:/p/0.jpg", "role": "embedded",
                           "temp": temp, "bak": "", "backup": backup})
                ),
            )
            .unwrap();
        }
        let r = s.import_from_backups().unwrap();
        assert!(r.imported.is_empty(), "{:?}", r.imported);
        let why: Vec<_> = r
            .skipped
            .iter()
            .filter(|(id, _)| id.starts_with("op-evil"))
            .map(|(_, w)| w.as_str())
            .collect();
        assert_eq!(why.len(), 3, "{:?}", r.skipped);
        assert!(why[0].contains("not a temporary name"), "{why:?}");
        assert!(why[1].contains("not a temporary name"), "{why:?}");
        assert!(why[2].contains("not a backup MoriMeta makes"), "{why:?}");
        for id in ["op-evil1", "op-evil2", "op-evil3"] {
            std::fs::remove_dir_all(d.join("backups").join(id)).unwrap();
        }
        // a damaged line before the end makes the record unusable instead of guessed
        drop(s);
        std::fs::remove_dir_all(d.join("db")).unwrap();
        let text = std::fs::read_to_string(&log).unwrap();
        std::fs::write(&log, text.replacen("\"t\":\"state\"", "\"t\":\"sta", 1)).unwrap();
        let mut s = Store::open(&d).unwrap();
        let r = s.import_from_backups().unwrap();
        assert!(r.imported.is_empty());
        assert_eq!(r.skipped.len(), 1, "{:?}", r.skipped);
    }

    #[test]
    fn disk_full_is_recognised() {
        assert!(StoreError::Io(std::io::Error::from_raw_os_error(112)).is_disk_full());
        assert!(
            StoreError::Sql(rusqlite::Error::SqliteFailure(
                rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL),
                None
            ))
            .is_disk_full()
        );
        assert!(!StoreError::Io(std::io::Error::from_raw_os_error(5)).is_disk_full());
    }

    #[test]
    fn version_1_database_is_upgraded_to_the_current_one() {
        let d = dir("v1");
        {
            let s = Store::open(&d).unwrap();
            s.conn
                .execute_batch(
                    "ALTER TABLE operations DROP COLUMN keep;
                     ALTER TABLE operations DROP COLUMN pruned_ms;
                     ALTER TABLE operations DROP COLUMN acks;
                     DROP TABLE presets;
                     DROP TABLE settings;
                     PRAGMA user_version = 1;",
                )
                .unwrap();
        }
        let mut s = Store::open(&d).unwrap();
        let v: i32 = s
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, SCHEMA_VERSION);
        s.begin_operation(
            &NewOperation {
                id: "op-1".into(),
                kind: "apply".into(),
                title: "t".into(),
                plan_json: "{}".into(),
                app_version: "0".into(),
                exiftool_version: "0".into(),
                registry_version: 0,
                undo_of: None,
            },
            &[],
        )
        .unwrap();
        s.set_keep("op-1", true).unwrap();
        let o = s.operation("op-1").unwrap().unwrap();
        assert!(o.keep && o.pruned_ms.is_none());
        s.set_setting("k", "v").unwrap();
        assert_eq!(s.setting("k").unwrap().as_deref(), Some("v"));
        s.save_preset("p1", "P", "{}", true).unwrap();
        s.save_preset("p1", "Q", "{}", false).unwrap();
        assert!(
            s.presets().unwrap()[0].imported,
            "a later save keeps the flag"
        );
        assert_eq!(s.presets().unwrap()[0].name, "Q");
    }

    #[test]
    fn newer_schema_is_refused() {
        let d = dir("newer");
        {
            let s = Store::open(&d).unwrap();
            s.conn.pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(matches!(Store::open(&d), Err(StoreError::NewerSchema(99))));
    }
}
