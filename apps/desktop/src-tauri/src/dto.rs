// SPDX-License-Identifier: GPL-3.0-or-later
//! What crosses the IPC boundary (ARCHITECTURE §5). Paths go to the frontend for display only;
//! files are referred to by `AssetId`, Plans by id + version (§5.1 Handle model). The frontend's
//! mirror of these shapes is `src/ipc/types.ts`.

use std::path::Path;

use mm_core::inspect::{FieldView, Row};
use mm_core::service::{AssetId, ImportReport, Session};
use mm_domain::plan::{EntryAction, Plan, PlanEntry, PlanSummary};
use serde::{Deserialize, Serialize};

/// Pushed through the channel the frontend registers with `subscribe`.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum AppEvent {
    /// Launch state, ExifTool or the write gate changed: fetch `app_info` again.
    Status,
    /// Files came in (dialog or drop).
    Imported {
        assets: Vec<AssetDto>,
        summary: ImportSummary,
    },
    /// A batch of Library rows, as soon as it is read.
    Rows {
        rows: Vec<RowDto>,
    },
    ScanProgress {
        done: usize,
        total: usize,
    },
    ScanDone {
        cancelled: bool,
        error: Option<String>,
    },
    PlanProgress {
        stage: &'static str,
        done: usize,
        total: usize,
    },
    ExecProgress(ExecProgressDto),
    /// The window was asked to close while an Operation runs; it stayed open.
    CloseBlocked,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetDto {
    pub id: u64,
    pub path: String,
    pub name: String,
    pub folder: String,
    pub ext: String,
    pub writable: bool,
    pub size: u64,
}

impl AssetDto {
    pub fn of(session: &Session, id: AssetId) -> Option<AssetDto> {
        let a = session.asset(id).ok()?;
        let p: &Path = &a.path;
        Some(AssetDto {
            id: id.0,
            path: p.display().to_string(),
            name: p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            folder: p
                .parent()
                .map(|d| d.display().to_string())
                .unwrap_or_default(),
            ext: p
                .extension()
                .map(|e| e.to_string_lossy().to_uppercase())
                .unwrap_or_default(),
            writable: a.writable,
            size: a.fingerprint.as_ref().map_or(0, |f| f.size),
        })
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ImportSummary {
    pub added: usize,
    pub read_only: usize,
    pub duplicates: usize,
    pub failed: Vec<(String, String)>,
    pub other_files: usize,
    pub not_followed: usize,
    pub placeholders: usize,
    pub skipped_folders: usize,
    pub orphan_sidecars: usize,
    pub backup_files: usize,
}

impl From<&ImportReport> for ImportSummary {
    fn from(r: &ImportReport) -> Self {
        ImportSummary {
            added: r.added.len(),
            read_only: r.read_only.len(),
            duplicates: r.duplicates.len(),
            failed: r.failed.clone(),
            other_files: r.other_files,
            not_followed: r.not_followed.len(),
            placeholders: r.placeholders.len(),
            skipped_folders: r.skipped_folders.len(),
            orphan_sidecars: r.orphan_sidecars.len(),
            backup_files: r.backup_files,
        }
    }
}

/// One Library row: the four fields as the Inspector shows them, camera and lens, where an edit
/// goes (SCREEN_SPEC 1#default).
#[derive(Debug, Clone, Serialize)]
pub struct RowDto {
    pub id: u64,
    pub writes_to: &'static str,
    pub creator: Option<String>,
    pub copyright: Option<String>,
    pub capture_time: Option<String>,
    pub gps: Option<String>,
    /// Fields whose locations disagree (CONF flag).
    pub conflicts: Vec<&'static str>,
    /// Fields holding a value that is not valid.
    pub invalid: Vec<&'static str>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    pub not_downloaded: bool,
    pub error: Option<String>,
}

impl RowDto {
    pub fn of(id: u64, r: &Row) -> RowDto {
        let value = |f: &str| -> Option<String> {
            r.fields
                .iter()
                .find(|v| v.field == f)
                .and_then(|v: &FieldView| v.value.clone())
        };
        RowDto {
            id,
            writes_to: r.writes_to,
            creator: value("creator"),
            copyright: value("copyright"),
            capture_time: value("capture_time"),
            gps: value("gps"),
            conflicts: r
                .fields
                .iter()
                .filter(|v| v.conflicting)
                .map(|v| v.field)
                .collect(),
            invalid: r
                .fields
                .iter()
                .filter(|v| v.error.is_some())
                .map(|v| v.field)
                .collect(),
            make: r.make.clone(),
            model: r.model.clone(),
            lens: r.lens.clone(),
            not_downloaded: r.not_downloaded,
            error: r.error.clone(),
        }
    }
}

/// What the batch editor staged (INTERACTION_SPEC §1): each field left out is "Leave".
#[derive(Debug, Clone, Default, Deserialize)]
pub struct BatchEditDto {
    pub creator: Option<ListEdit>,
    pub copyright: Option<TextEdit>,
    pub gps: Option<GpsEditDto>,
    pub time: Option<TimeEditDto>,
    /// Also change the digitized time (EXIF CreateDate), on by default.
    #[serde(default = "yes")]
    pub digitized: bool,
    /// Shown in the Preview banner and History.
    #[serde(default)]
    pub title: String,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum ListEdit {
    Set { values: Vec<String> },
    Clear,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum TextEdit {
    Set { value: String },
    Clear,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum GpsEditDto {
    /// `lat,lon[,alt]` in decimal degrees and metres.
    Set {
        position: String,
    },
    Remove,
}

/// The four MVP time tools (PRODUCT_SPEC §6.5.1). Times are `YYYY:MM:DD HH:MM:SS`, shifts
/// `[+|-][Nd]HH:MM:SS`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum TimeEditDto {
    Absolute {
        to: String,
    },
    Shift {
        by: String,
    },
    Sequence {
        start: String,
        step: String,
        /// `time` or `name`
        order: String,
    },
    Preserve {
        anchor: u64,
        to: String,
    },
}

/// A Plan as the Preview first shows it; entries come page by page.
#[derive(Debug, Clone, Serialize)]
pub struct PlanView {
    pub id: String,
    pub version: u32,
    pub title: String,
    /// `apply` or `undo`.
    pub kind: String,
    pub summary: PlanSummary,
    pub required_acks: Vec<String>,
    /// Every field an entry changes (for "Edits in plan").
    pub fields: Vec<String>,
    /// Entries per field that carry that field's change (still in the Plan).
    pub field_counts: Vec<(String, usize)>,
    pub excluded_fields: Vec<String>,
    /// Kind counts over entries that will be written: add / modify / remove.
    pub kinds: KindCounts,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct KindCounts {
    pub add: usize,
    pub modify: usize,
    pub remove: usize,
    pub warnings: usize,
    pub unsupported: usize,
    pub blocked: usize,
    pub no_change: usize,
}

impl PlanView {
    pub fn of(p: &Plan) -> PlanView {
        use mm_domain::plan::{ChangeKind, EntryStatus};
        let mut fields: Vec<String> = Vec::new();
        let mut counts: Vec<(String, usize)> = Vec::new();
        let mut excluded_fields: Vec<String> = Vec::new();
        let mut kinds = KindCounts::default();
        for e in &p.entries {
            for c in &e.changes {
                if !fields.contains(&c.field) {
                    fields.push(c.field.clone());
                }
                if !e.excluded {
                    match counts.iter_mut().find(|(f, _)| *f == c.field) {
                        Some((_, n)) => *n += 1,
                        None => counts.push((c.field.clone(), 1)),
                    }
                }
            }
            for c in &e.excluded_changes {
                if !fields.contains(&c.field) {
                    fields.push(c.field.clone());
                }
            }
            if e.excluded {
                continue;
            }
            match &e.status {
                EntryStatus::Ready => {
                    for c in &e.changes {
                        match c.kind {
                            ChangeKind::Add => kinds.add += 1,
                            ChangeKind::Modify => kinds.modify += 1,
                            ChangeKind::Remove => kinds.remove += 1,
                        }
                    }
                    if e.warnings().next().is_some() {
                        kinds.warnings += 1;
                    }
                }
                EntryStatus::Unsupported(_) => kinds.unsupported += 1,
                EntryStatus::Blocked(_) => kinds.blocked += 1,
                EntryStatus::NoChange => kinds.no_change += 1,
            }
        }
        // a field left out everywhere is an excluded edit
        for f in &fields {
            let present = p
                .entries
                .iter()
                .any(|e| e.changes.iter().any(|c| &c.field == f));
            let left_out = p
                .entries
                .iter()
                .any(|e| e.excluded_changes.iter().any(|c| &c.field == f));
            if left_out && !present {
                excluded_fields.push(f.clone());
            }
        }
        PlanView {
            id: p.id.clone(),
            version: p.version,
            title: p.title.clone(),
            kind: serde_json::to_value(&p.kind)
                .ok()
                .and_then(|v| v["kind"].as_str().map(str::to_owned))
                .unwrap_or_default(),
            summary: p.summary(),
            required_acks: p.required_acks(),
            fields,
            field_counts: counts,
            excluded_fields,
            kinds,
        }
    }
}

/// One Preview row.
#[derive(Debug, Clone, Serialize)]
pub struct EntryDto<'a> {
    #[serde(flatten)]
    pub entry: &'a PlanEntry,
    /// `in_file`, `sidecar`, `new_sidecar` (SCREEN_SPEC: Writes to).
    pub target: &'static str,
    pub warnings: Vec<String>,
    /// The name the user knows the file by (the RAW for a sidecar).
    pub name: String,
}

impl<'a> EntryDto<'a> {
    pub fn of(e: &'a PlanEntry) -> EntryDto<'a> {
        let target = match &e.action {
            Some(EntryAction::CreateFile { .. }) => "new_sidecar",
            _ if e.raw.is_some() => "sidecar",
            _ if e.path.to_lowercase().ends_with(".xmp") => "sidecar",
            _ => "in_file",
        };
        let shown = e.raw.as_deref().unwrap_or(&e.path);
        EntryDto {
            entry: e,
            target,
            warnings: e.warnings().map(str::to_owned).collect(),
            name: Path::new(shown)
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct PageDto<'a> {
    pub version: u32,
    pub matching: usize,
    pub entries: Vec<EntryDto<'a>>,
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct ExecProgressDto {
    pub total: usize,
    pub done: usize,
    pub ok: usize,
    pub failed: usize,
    pub skipped: usize,
    pub last_seq: Option<u32>,
    pub last_state: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct FileOutcomeDto {
    pub seq: u32,
    pub path: String,
    pub name: String,
    pub state: String,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OpReportDto {
    pub op_id: String,
    pub status: String,
    pub note: Option<String>,
    pub files: Vec<FileOutcomeDto>,
}

impl From<&mm_core::executor::OpReport> for OpReportDto {
    fn from(r: &mm_core::executor::OpReport) -> Self {
        OpReportDto {
            op_id: r.op_id.clone(),
            status: r.status.as_str().to_owned(),
            note: r.note.clone(),
            files: r
                .files
                .iter()
                .map(|f| FileOutcomeDto {
                    seq: f.seq,
                    path: f.path.clone(),
                    name: Path::new(&f.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    state: f.state.as_str().to_owned(),
                    reason: f.reason.clone(),
                })
                .collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct SettingDto {
    pub name: &'static str,
    pub value: String,
    pub default: &'static str,
    pub about: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AttentionDto {
    pub read_only: Vec<u64>,
    pub conflicts: Vec<u64>,
    pub cloud_placeholders: Vec<u64>,
    pub cloud_files: Vec<u64>,
    pub darktable_sidecars: Vec<u64>,
    pub c2pa: Vec<u64>,
    pub links: Vec<u64>,
    pub unreadable: Vec<u64>,
    pub long_paths: Vec<u64>,
    pub removable: Vec<u64>,
    pub network: Vec<u64>,
    pub other_file_system: Vec<u64>,
    pub changed_since_import: Vec<u64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct BackupDto {
    pub root: String,
    pub bytes: u64,
    pub operations: usize,
    pub free: Option<u64>,
    pub sync_warning: Option<String>,
    pub problem: Option<String>,
}
