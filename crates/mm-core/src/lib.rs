// SPDX-License-Identifier: GPL-3.0-or-later
//! Application services (ARCHITECTURE §4.1 `mm-core`): planning, execution of the single-file
//! transaction, crash recovery, resume, undo and offline consistency checks.
//!
//! Phase 1b scope: Embedded JPEG targets and the `creator` field.

pub mod engine;
pub mod executor;
pub mod fsck;
pub mod history;
pub mod inspect;
pub mod planner;
pub mod presets;
pub mod recovery;
pub mod retention;
pub mod service;
pub mod undo;
pub mod verify;

use std::path::{Path, PathBuf};

use mm_domain::plan::Fingerprint;

#[derive(Debug)]
pub enum CoreError {
    Input(String),
    Engine(String),
    Store(mm_store::StoreError),
    Io(std::io::Error),
    /// Writes are refused until recovery has handled these operations (SAFETY_MODEL §10).
    RecoveryPending(Vec<String>),
    /// The persisted plan was made by a different app / ExifTool / registry version.
    VersionMismatch(String),
    /// The space pre-check failed (SAFETY_MODEL §6.2); nothing was registered or written.
    InsufficientSpace(String),
    /// Cancelled by the user before anything was written (Plan creation).
    Cancelled,
    Internal(String),
}

impl std::fmt::Display for CoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

impl CoreError {
    /// A volume (photos, backups or journal) is full.
    pub fn is_disk_full(&self) -> bool {
        match self {
            CoreError::Io(e) => mm_fs::is_disk_full(e),
            CoreError::Store(e) => e.is_disk_full(),
            _ => false,
        }
    }
}
impl std::error::Error for CoreError {}
impl From<mm_store::StoreError> for CoreError {
    fn from(e: mm_store::StoreError) -> Self {
        CoreError::Store(e)
    }
}
impl From<std::io::Error> for CoreError {
    fn from(e: std::io::Error) -> Self {
        CoreError::Io(e)
    }
}

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Journal role of a file written in place (SAFETY_MODEL §4.1).
pub const ROLE_EMBEDDED: &str = "embedded";
/// Journal role of a file an Undo creates again at its path because it was deleted or moved
/// (SAFETY_MODEL §7.2). Its pre-image is "absent": the row has no H0.
pub const ROLE_RECREATE: &str = "recreate";
/// Journal role of a file an Undo moves into the backup store because the undone Operation
/// created it (SAFETY_MODEL §4.3, §7.2). Its post-image is "absent": the row has no H1.
pub const ROLE_REMOVE: &str = "remove";
/// Journal role of a new file an Operation writes from nothing (a new XMP sidecar, SAFETY_MODEL
/// §4.3). Like `recreate`, its pre-image is "absent"; undoing it moves it into the backup store.
pub const ROLE_CREATE: &str = "create";

/// Absolute path without the verbatim prefix (`\\?\C:\…` → `C:\…`, `\\?\UNC\h\s` → `\\h\s`).
pub fn normalize(p: &Path) -> Result<PathBuf, CoreError> {
    let abs =
        std::fs::canonicalize(p).map_err(|e| CoreError::Input(format!("{}: {e}", p.display())))?;
    let s = abs.to_string_lossy();
    let s = if let Some(r) = s.strip_prefix(r"\\?\UNC\") {
        format!(r"\\{r}")
    } else if let Some(r) = s.strip_prefix(r"\\?\") {
        r.to_owned()
    } else {
        s.into_owned()
    };
    Ok(PathBuf::from(s))
}

pub fn file_id_hex(id: &mm_fs::FileId) -> String {
    format!(
        "{:016x}:{}",
        id.volume,
        id.id.iter().map(|b| format!("{b:02x}")).collect::<String>()
    )
}

/// Size, File ID and last-write time of the file at `p` (never follows reparse points).
pub fn fingerprint(p: &Path) -> Result<Fingerprint, CoreError> {
    use std::os::windows::fs::MetadataExt;
    let f = mm_fs::open_attr(p)?;
    let m = f.metadata()?;
    Ok(Fingerprint {
        size: m.len(),
        file_id: file_id_hex(&mm_fs::file_id(&f)?),
        mtime: m.last_write_time(),
    })
}

pub fn fingerprint_of_handle(f: &std::fs::File) -> Result<Fingerprint, CoreError> {
    use std::os::windows::fs::MetadataExt;
    let m = f.metadata()?;
    Ok(Fingerprint {
        size: m.len(),
        file_id: file_id_hex(&mm_fs::file_id(f)?),
        mtime: m.last_write_time(),
    })
}

pub fn new_id(prefix: &str) -> Result<String, CoreError> {
    Ok(format!(
        "{prefix}-{:012x}-{}",
        mm_store::now_ms(),
        mm_fs::random_token()?
    ))
}

pub fn hash_opt(p: &Path) -> Option<String> {
    mm_fs::hash_path(p).ok().map(|h| mm_fs::hex(&h))
}
