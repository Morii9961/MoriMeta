//! Plan model (METADATA_MODEL §9). A Plan is immutable once created; the executable part of each
//! entry (`action`) is persisted when an Operation starts so that "continue" after a crash
//! executes exactly what was previewed (SAFETY_MODEL §9).

use serde::{Deserialize, Serialize};

/// Identity of a file at planning time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Fingerprint {
    pub size: u64,
    /// Volume serial + 128-bit file ID, hex.
    pub file_id: String,
    /// Last-write time (100 ns units since 1601, as reported by Windows).
    pub mtime: u64,
}

/// One ExifTool tag operation. Tag names are family-1 qualified (`IFD0:Artist`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TagOp {
    /// Set a tag; several values write several list items.
    Set {
        tag: String,
        values: Vec<String>,
    },
    Delete {
        tag: String,
    },
    /// `-Photoshop:IPTCDigest=new` (digest of the IPTC written in the same command).
    UpdateIptcDigest,
}

impl TagOp {
    pub fn tag(&self) -> &str {
        match self {
            TagOp::Set { tag, .. } | TagOp::Delete { tag } => tag,
            TagOp::UpdateIptcDigest => "Photoshop:IPTCDigest",
        }
    }
}

/// The key under which ExifTool reads back what `write_tag` writes. A language-alternative
/// write names the language (`XMP-dc:Rights-x-default`, so that other languages are kept),
/// while the default language reads back without the suffix (`XMP-dc:Rights`).
pub fn read_key(write_tag: &str) -> &str {
    write_tag.strip_suffix("-x-default").unwrap_or(write_tag)
}

/// Result of planning one field for one file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FieldPlan {
    pub status: EntryStatus,
    pub change: Option<FieldChange>,
    pub ops: Vec<TagOp>,
    pub expect: Vec<Expect>,
    pub notes: Vec<String>,
}

impl FieldPlan {
    pub fn blocked(why: String) -> FieldPlan {
        FieldPlan {
            status: EntryStatus::Blocked(why),
            change: None,
            ops: vec![],
            expect: vec![],
            notes: vec![],
        }
    }
}

/// What verification (SAFETY_MODEL §5 V2) must find in the temporary output.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "expect", rename_all = "snake_case")]
pub enum Expect {
    Equals {
        tag: String,
        values: Vec<String>,
    },
    Absent {
        tag: String,
    },
    /// IPTCDigest present and matching the IPTC block (no "not current" warning).
    IptcDigestCurrent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChangeKind {
    Add,
    Modify,
    Remove,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FieldChange {
    pub field: String,
    pub before: Option<Vec<String>>,
    pub after: Option<Vec<String>>,
    pub kind: ChangeKind,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", content = "reason", rename_all = "snake_case")]
pub enum EntryStatus {
    Ready,
    NoChange,
    Blocked(String),
    Unsupported(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum EntryAction {
    /// Metadata write through ExifTool (Embedded target).
    Write {
        ops: Vec<TagOp>,
        expect: Vec<Expect>,
    },
    /// Undo: put back the verified backup `backup` (content hash `h0`), only if the file still
    /// has the post-operation hash `h1` (otherwise Conflict).
    Restore {
        backup: String,
        h0: String,
        h1: String,
    },
    /// Undo of a file that was deleted or moved after the operation (SAFETY_MODEL §7.2): create
    /// it again at its path from the verified backup `backup` (content hash `h0`, `size` bytes).
    /// Never replaces a file that exists at the path by then.
    Recreate {
        backup: String,
        h0: String,
        size: u64,
    },
    /// Undo of a file that the undone Operation created (SAFETY_MODEL §4.3, §7.2): move it into
    /// the backup store instead of deleting it, only if its content is still `h`.
    MoveToBackupStore { h: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanEntry {
    pub seq: u32,
    pub path: String,
    pub fingerprint: Fingerprint,
    pub status: EntryStatus,
    pub changes: Vec<FieldChange>,
    pub action: Option<EntryAction>,
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlanKind {
    Apply,
    Undo { of: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub id: String,
    pub version: u32,
    pub kind: PlanKind,
    pub title: String,
    pub registry_version: u32,
    pub exiftool_version: String,
    pub entries: Vec<PlanEntry>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanSummary {
    pub files: usize,
    pub ready: usize,
    pub no_change: usize,
    pub blocked: usize,
    pub unsupported: usize,
    pub changes: usize,
}

impl Plan {
    pub fn summary(&self) -> PlanSummary {
        let mut s = PlanSummary {
            files: self.entries.len(),
            ..Default::default()
        };
        for e in &self.entries {
            match e.status {
                EntryStatus::Ready => s.ready += 1,
                EntryStatus::NoChange => s.no_change += 1,
                EntryStatus::Blocked(_) => s.blocked += 1,
                EntryStatus::Unsupported(_) => s.unsupported += 1,
            }
            s.changes += e.changes.len();
        }
        s
    }

    /// Entries that an Operation will execute.
    pub fn executable(&self) -> impl Iterator<Item = &PlanEntry> {
        self.entries
            .iter()
            .filter(|e| e.status == EntryStatus::Ready && e.action.is_some())
    }
}
