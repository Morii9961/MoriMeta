//! Operation Journal (SAFETY_MODEL §9) on SQLite (WAL, synchronous=FULL), plus the backup
//! store layout and the self-describing `manifest.json` written next to each Operation's backups.
//!
//! Every state change is its own committed transaction; with `synchronous=FULL` a commit returns
//! only after the WAL is flushed, which is what I-8 requires ("record before irreversible action").

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, OptionalExtension, params};
use serde_json::json;

pub const SCHEMA_VERSION: i32 = 1;

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
        Ok(Store {
            conn,
            data_dir: data_dir.to_path_buf(),
            fault: None,
            blocker: None,
            manifest_blocker: None,
        })
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

    pub fn backup_dir(&self, op_id: &str) -> PathBuf {
        self.data_dir.join("backups").join(op_id)
    }

    /// Step 0: register the Operation, its executable plan and every file, in one transaction.
    pub fn begin_operation(&mut self, op: &NewOperation, files: &[NewFile]) -> Result<PathBuf> {
        let dir = self.backup_dir(&op.id);
        std::fs::create_dir_all(&dir)?;
        self.write(WriteTarget::Begin, |conn| {
            let tx = conn.transaction()?;
            let now = now_ms();
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
        Ok(())
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
        Ok(())
    }

    pub fn finish_operation(&mut self, op_id: &str, status: OpStatus) -> Result<()> {
        self.write(WriteTarget::Finish, |conn| {
            conn.execute(
                "UPDATE operations SET status = ?2, finished_ms = ?3 WHERE id = ?1",
                params![op_id, status.as_str(), now_ms()],
            )?;
            Ok(())
        })?;
        self.write_manifest(op_id)?;
        Ok(())
    }

    pub fn set_status(&mut self, op_id: &str, status: OpStatus) -> Result<()> {
        self.conn.execute(
            "UPDATE operations SET status = ?2 WHERE id = ?1",
            params![op_id, status.as_str()],
        )?;
        Ok(())
    }

    pub fn operation(&self, op_id: &str) -> Result<Option<OperationRow>> {
        Ok(self
            .conn
            .query_row(
                "SELECT id, kind, title, status, created_ms, finished_ms, plan_json, exiftool_version, app_version, registry_version, undo_of, backup_dir
                 FROM operations WHERE id = ?1",
                params![op_id],
                row_to_op,
            )
            .optional()?)
    }

    pub fn operations(&self) -> Result<Vec<OperationRow>> {
        let mut st = self.conn.prepare(
            "SELECT id, kind, title, status, created_ms, finished_ms, plan_json, exiftool_version, app_version, registry_version, undo_of, backup_dir
             FROM operations ORDER BY created_ms, id",
        )?;
        let rows = st
            .query_map([], row_to_op)?
            .collect::<std::result::Result<Vec<_>, _>>()?;
        Ok(rows)
    }

    /// Operations left in `running` (crash) or `interrupted` (recovery not finished).
    pub fn unfinished(&self) -> Result<Vec<String>> {
        let mut st = self.conn.prepare("SELECT id FROM operations WHERE status IN ('running', 'interrupted') ORDER BY created_ms, id")?;
        let rows = st
            .query_map([], |r| r.get(0))?
            .collect::<std::result::Result<Vec<String>, _>>()?;
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
        if armed.is_some_and(|x| !x.persistent) {
            if let Some(d) = self.manifest_blocker.take() {
                let _ = std::fs::remove_dir(d);
            }
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
                          "exiftool_version": op.exiftool_version, "app_version": op.app_version},
            "files": files.iter().map(|f| json!({
                "seq": f.seq, "path": f.path, "role": f.role, "backup": f.backup_path,
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
            temp_path: "t".into(),
            bak_path: "b".into(),
            backup_path: "k".into(),
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
    fn newer_schema_is_refused() {
        let d = dir("newer");
        {
            let s = Store::open(&d).unwrap();
            s.conn.pragma_update(None, "user_version", 99).unwrap();
        }
        assert!(matches!(Store::open(&d), Err(StoreError::NewerSchema(99))));
    }
}
