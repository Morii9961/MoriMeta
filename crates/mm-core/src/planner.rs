//! Planning (ARCHITECTURE §6.2): environment pre-checks, fingerprints, metadata snapshot, and the
//! pure field planner. Produces an immutable `Plan`; nothing is written.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mm_domain::capture;
use mm_domain::copyright::{self, CopyrightEdit};
use mm_domain::creator::{self, CreatorEdit};
use mm_domain::gps::{self, GpsEdit};
use mm_domain::plan::{
    EntryAction, EntryStatus, FieldPlan, Fingerprint, Plan, PlanEntry, PlanKind, Target,
};
use mm_domain::rules::{self, Preset};
use mm_domain::snapshot::Snapshot;
use mm_domain::template::{Template, TemplateCtx};
use mm_domain::time::{
    self, NaiveDateTime, SequenceOrder, TimeDelta, TimeItem, TimeOp, TimeOpError,
};

use mm_fs::VolumeKind;

use crate::engine::Engine;
use crate::{CoreError, fingerprint, new_id, normalize};

/// Which part of planning a [`PlanProgress`] is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanStage {
    /// Fingerprints, environment checks, sidecar pairing.
    Files,
    /// Reading metadata with ExifTool, in chunks.
    Metadata,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlanProgress {
    pub stage: PlanStage,
    pub done: usize,
    pub total: usize,
}

pub type PlanProgressFn = Arc<dyn Fn(&PlanProgress) + Send + Sync>;

/// Progress and cancellation of Plan creation (ARCHITECTURE §5.3 `PlanProgress`): the adapter
/// runs planning in the background and may cancel it; nothing has been written either way.
#[derive(Clone, Default)]
pub struct PlanCtl {
    pub progress: Option<PlanProgressFn>,
    pub cancel: Option<Arc<AtomicBool>>,
}

impl PlanCtl {
    pub(crate) fn report(
        &self,
        stage: PlanStage,
        done: usize,
        total: usize,
    ) -> Result<(), CoreError> {
        if self
            .cancel
            .as_ref()
            .is_some_and(|c| c.load(Ordering::SeqCst))
        {
            return Err(CoreError::Cancelled);
        }
        if let Some(p) = &self.progress {
            p(&PlanProgress { stage, done, total });
        }
        Ok(())
    }
}

/// Files between two progress reports while inspecting them.
const PROGRESS_EVERY: usize = 100;

/// Environment checks at planning time (SAFETY_MODEL §8). Repeated at execution.
fn precheck(p: &std::path::Path) -> Result<(), String> {
    let pr = mm_fs::probe(p).map_err(|e| format!("cannot inspect file: {e}"))?;
    if pr.reparse_point {
        return Err("symbolic link or reparse point (not written)".into());
    }
    if pr.links > 1 {
        return Err("file has more than one hard link (not written)".into());
    }
    if pr.read_only {
        return Err("read-only attribute is set (treated as locked by the user)".into());
    }
    if pr.cloud_placeholder {
        return Err("cloud placeholder that is not downloaded".into());
    }
    Ok(())
}

pub fn plan_creator(
    engine: &mut Engine,
    inputs: &[PathBuf],
    edit: &CreatorEdit,
    title: &str,
    ctl: &PlanCtl,
) -> Result<Plan, CoreError> {
    let CreatorEdit::Set(names) = edit else {
        return plan_field(engine, inputs, title, ctl, |t| {
            creator::plan_target(t, edit)
        });
    };
    let templates = names
        .iter()
        .map(|n| Template::parse(n))
        .collect::<Result<Vec<_>, _>>()
        .map_err(CoreError::Input)?;
    plan_templated(engine, inputs, title, ctl, |t, ctx| {
        match templates.iter().map(|tm| tm.render(ctx)).collect() {
            Ok(names) => creator::plan_target(t, &CreatorEdit::Set(names)),
            Err(why) => FieldPlan::blocked(why),
        }
    })
}

pub fn plan_copyright(
    engine: &mut Engine,
    inputs: &[PathBuf],
    edit: &CopyrightEdit,
    title: &str,
    ctl: &PlanCtl,
) -> Result<Plan, CoreError> {
    let CopyrightEdit::Set(value) = edit else {
        return plan_field(engine, inputs, title, ctl, |t| {
            copyright::plan_target(t, edit)
        });
    };
    let template = Template::parse(value).map_err(CoreError::Input)?;
    plan_templated(engine, inputs, title, ctl, |t, ctx| {
        match template.render(ctx) {
            Ok(v) => copyright::plan_target(t, &CopyrightEdit::Set(v)),
            Err(why) => FieldPlan::blocked(why),
        }
    })
}

/// Apply a Preset (PRODUCT_SPEC §6.10–6.11): per file, the actions of the rules whose conditions
/// hold on its original snapshot, combined into one entry (blocked fields left out with a note).
pub fn plan_preset(
    engine: &mut Engine,
    inputs: &[PathBuf],
    preset: &Preset,
    ctl: &PlanCtl,
) -> Result<Plan, CoreError> {
    preset.validate().map_err(CoreError::Input)?;
    plan_with(engine, inputs, &preset.name, ctl, |targets, entries| {
        Ok(targets
            .iter()
            .map(|(idx, t)| {
                let e = &entries[*idx];
                let shown = e.raw.as_deref().unwrap_or(&e.path);
                rules::plan_target(preset, t, shown)
            })
            .collect())
    })
}

/// A field whose value is a template (PRODUCT_SPEC §6.9), rendered per file from its original
/// snapshot and the path the user sees (the RAW for a sidecar).
fn plan_templated(
    engine: &mut Engine,
    inputs: &[PathBuf],
    title: &str,
    ctl: &PlanCtl,
    field: impl Fn(&Target, &TemplateCtx) -> FieldPlan,
) -> Result<Plan, CoreError> {
    plan_with(engine, inputs, title, ctl, |targets, entries| {
        Ok(targets
            .iter()
            .map(|(idx, t)| {
                let e = &entries[*idx];
                let shown = e.raw.as_deref().unwrap_or(&e.path);
                vec![("", field(t, &TemplateCtx::from_target(t, shown)))]
            })
            .collect())
    })
}

pub fn plan_gps(
    engine: &mut Engine,
    inputs: &[PathBuf],
    edit: &GpsEdit,
    title: &str,
    ctl: &PlanCtl,
) -> Result<Plan, CoreError> {
    plan_field(engine, inputs, title, ctl, |t| gps::plan_target(t, edit))
}

/// A time tool as the user specified it (METADATA_MODEL §5.2); the anchor of Preserve Relative
/// Timing is one of the input files.
#[derive(Debug, Clone)]
pub enum TimeTool {
    Absolute(NaiveDateTime),
    Shift(TimeDelta),
    Sequence {
        start: NaiveDateTime,
        step: TimeDelta,
        order: SequenceOrder,
    },
    PreserveRelative {
        anchor: PathBuf,
        new_local: NaiveDateTime,
    },
}

/// Capture time for the whole selection at once: Sequence orders the files and Preserve Relative
/// Timing measures from its anchor, so no file can be planned on its own. `digitized` also sets
/// EXIF CreateDate (on by default, METADATA_MODEL §5.3).
pub fn plan_capture_time(
    engine: &mut Engine,
    inputs: &[PathBuf],
    tool: &TimeTool,
    digitized: bool,
    title: &str,
    ctl: &PlanCtl,
) -> Result<Plan, CoreError> {
    let keep_subsec = matches!(tool, TimeTool::Shift(_) | TimeTool::PreserveRelative { .. });
    plan_with(engine, inputs, title, ctl, |readable, entries| {
        // a sidecar is ordered and anchored by the name of its RAW
        let shown = |i: usize| {
            entries[i]
                .raw
                .clone()
                .unwrap_or_else(|| entries[i].path.clone())
        };
        let items: Vec<TimeItem> = readable
            .iter()
            .map(|(idx, t)| TimeItem {
                id: *idx as u64,
                file_name: Path::new(&shown(*idx))
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                time: capture::read_target(t).ok().flatten(),
            })
            .collect();
        let name = |id: u64| shown(id as usize);
        let op = match tool {
            TimeTool::Absolute(l) => TimeOp::Absolute(*l),
            TimeTool::Shift(d) => TimeOp::Shift(*d),
            TimeTool::Sequence { start, step, order } => TimeOp::Sequence {
                start: *start,
                step: *step,
                order: *order,
            },
            TimeTool::PreserveRelative { anchor, new_local } => {
                let a = normalize(anchor)?.to_string_lossy().into_owned();
                let id = items
                    .iter()
                    .find(|it| entries[it.id as usize].path == a || shown(it.id as usize) == a)
                    .ok_or_else(|| {
                        CoreError::Input(format!(
                            "anchor {a} is not a writable file of this selection"
                        ))
                    })?
                    .id;
                TimeOp::PreserveRelative {
                    anchor: id,
                    new_local: *new_local,
                }
            }
        };
        let results = time::apply(&op, &items).map_err(|e| match e {
            TimeOpError::OrderNeedsValidTimes(ids) => CoreError::Input(format!(
                "ordering by capture time needs a valid time on every file; missing: {}",
                ids.into_iter().map(name).collect::<Vec<_>>().join(", ")
            )),
            other => CoreError::Input(other.to_string()),
        })?;
        let n = results.len();
        Ok(readable
            .iter()
            .map(|(idx, t)| {
                let r = results.iter().find(|r| r.id == *idx as u64);
                let after = r
                    .map(|r| r.after.clone())
                    .unwrap_or(Err(TimeOpError::EmptySelection));
                let mut fp = capture::plan_target(t, &after, keep_subsec, digitized);
                if let (TimeTool::Sequence { .. }, Some(r)) = (tool, r) {
                    fp.notes.insert(
                        0,
                        format!("position {} of {n} in the sequence", r.index + 1),
                    );
                }
                vec![("capture_time", fp)]
            })
            .collect())
    })
}

/// Pre-checks, fingerprints and the snapshot of every input, then the pure field planner.
fn plan_field(
    engine: &mut Engine,
    inputs: &[PathBuf],
    title: &str,
    ctl: &PlanCtl,
    field: impl Fn(&Target) -> FieldPlan,
) -> Result<Plan, CoreError> {
    plan_with(engine, inputs, title, ctl, |targets, _| {
        Ok(targets.iter().map(|(_, t)| vec![("", field(t))]).collect())
    })
}

/// The planned fields of one file, each with its field name (one for a single edit, several for a
/// Preset); combined by `plan::merge`.
type FileFields = Vec<(&'static str, FieldPlan)>;

/// RAW formats whose sidecar this build writes (SAFETY_MODEL §3: NEF, NRW; the RAW stays
/// read-only, I-10).
const SIDECAR_RAW: &[&str] = &["nef", "nrw"];
/// Every RAW extension that can claim a `<stem>.xmp` (for the ambiguity rule of §3.1).
const ANY_RAW: &[&str] = &[
    "nef", "nrw", "cr2", "cr3", "crw", "arw", "srf", "sr2", "raf", "orf", "rw2", "pef", "dng",
    "srw", "x3f", "iiq", "3fr", "erf", "kdc", "mrw", "raw", "rwl",
];

fn ext_of(p: &Path) -> String {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// FormatPolicy (SAFETY_MODEL §3).
pub(crate) enum Policy {
    /// Written in place (JPEG).
    Embedded,
    /// An XMP file selected on its own: written in place, XMP tags only.
    OwnSidecar,
    /// A RAW: its XMP sidecar is written.
    RawSidecar,
    Unsupported,
}

pub(crate) fn policy(p: &Path) -> Policy {
    let ext = ext_of(p);
    match ext.as_str() {
        "jpg" | "jpeg" => Policy::Embedded,
        "xmp" => Policy::OwnSidecar,
        e if SIDECAR_RAW.contains(&e) => Policy::RawSidecar,
        _ => Policy::Unsupported,
    }
}

/// Whether a folder import takes the file in: a format this build plans for, except XMP files
/// (they come with their RAW, §3.1) and the transaction's own temporary and backup names
/// (`IMG_1.mmtmp-….JPG`), which recovery handles.
pub fn importable(p: &Path) -> bool {
    let name = p
        .file_name()
        .map(|n| n.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    if name.contains(".mmtmp-") || name.contains(".mmbak-") {
        return false;
    }
    matches!(policy(p), Policy::Embedded | Policy::RawSidecar)
}

/// The sidecar of `raw` (SAFETY_MODEL §3.1): `<stem>.xmp` in any letter case (the existing name
/// is used), unless another RAW with the same stem makes the ownership ambiguous. A darktable
/// `<file>.<ext>.xmp` is recognised and left alone. Returns (path, exists, notes).
pub(crate) fn pair_sidecar(raw: &Path) -> Result<(PathBuf, bool, Vec<String>), String> {
    let dir = raw.parent().ok_or("no folder")?;
    let stem = raw
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let own = raw
        .file_name()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let mut existing = None;
    let mut notes = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("cannot list the folder: {e}"))? {
        let name = e
            .map_err(|e| format!("cannot list the folder: {e}"))?
            .file_name()
            .to_string_lossy()
            .into_owned();
        let lower = name.to_lowercase();
        let (s, x) = lower.rsplit_once('.').unwrap_or((&lower, ""));
        if s == stem && x == "xmp" {
            existing = Some(dir.join(&name));
        } else if s == stem && ANY_RAW.contains(&x) && lower != own {
            return Err(format!(
                "another RAW file ({name}) has the same name, so the sidecar's owner is ambiguous"
            ));
        } else if lower == format!("{own}.xmp") {
            notes.push(format!(
                "darktable sidecar {name} is recognised and not modified"
            ));
        }
    }
    Ok(match existing {
        Some(p) => (p, true, notes),
        None => (
            dir.join(format!(
                "{}.xmp",
                raw.file_stem().unwrap_or_default().to_string_lossy()
            )),
            false,
            notes,
        ),
    })
}

/// What planning read for one writable entry.
struct Reads {
    sidecar_mode: bool,
    /// The RAW (sidecar mode) — empty for an XMP selected on its own.
    raw: Snapshot,
    /// The file that is written, if it exists.
    own: Option<Snapshot>,
}

impl Reads {
    fn target(&self) -> Option<Target<'_>> {
        if self.sidecar_mode {
            Some(Target::Sidecar {
                raw: &self.raw,
                sidecar: self.own.as_ref(),
            })
        } else {
            self.own.as_ref().map(Target::Embedded)
        }
    }
}

/// A file planned for writing: its entry index, the RAW to read (sidecar mode) and whether the
/// target file exists.
struct Pending {
    idx: usize,
    raw: Option<PathBuf>,
    own: Option<PathBuf>,
    sidecar_mode: bool,
}

fn absent() -> Fingerprint {
    Fingerprint {
        size: 0,
        file_id: String::new(),
        mtime: 0,
    }
}

/// The shared planning pipeline: FormatPolicy and sidecar pairing, pre-checks and fingerprints
/// of the file that will be written, one batched read, then `field_all` with every readable
/// target together (one field plan per target, same order).
fn plan_with(
    engine: &mut Engine,
    inputs: &[PathBuf],
    title: &str,
    ctl: &PlanCtl,
    field_all: impl FnOnce(&[(usize, Target)], &[PlanEntry]) -> Result<Vec<FileFields>, CoreError>,
) -> Result<Plan, CoreError> {
    let mut entries: Vec<PlanEntry> = Vec::new();
    let mut seen = HashSet::new();
    let mut pending: Vec<Pending> = Vec::new();
    let mut volumes: HashMap<PathBuf, VolumeKind> = HashMap::new();
    let sync_roots = mm_fs::sync_roots();
    for (i, input) in inputs.iter().enumerate() {
        if i % PROGRESS_EVERY == 0 {
            ctl.report(PlanStage::Files, i, inputs.len())?;
        }
        let path = normalize(input)?;
        let fp_in = fingerprint(&path)?;
        let mut entry = PlanEntry {
            seq: 0,
            path: path.to_string_lossy().into_owned(),
            raw: None,
            fingerprint: fp_in.clone(),
            status: EntryStatus::Ready,
            changes: vec![],
            action: None,
            notes: vec![],
            excluded: false,
        };
        let mut job = None;
        match policy(&path) {
            Policy::Unsupported => {
                entry.status = EntryStatus::Unsupported("format is read-only in this build".into());
                if !seen.insert(fp_in.file_id.clone()) {
                    continue;
                }
            }
            Policy::Embedded | Policy::OwnSidecar => {
                if !seen.insert(fp_in.file_id.clone()) {
                    continue; // same file through another path, or already a RAW's sidecar
                }
                if let Err(why) = precheck(&path) {
                    entry.status = EntryStatus::Blocked(why);
                } else {
                    job = Some(Pending {
                        idx: 0,
                        raw: None,
                        own: Some(path.clone()),
                        sidecar_mode: matches!(policy(&path), Policy::OwnSidecar),
                    });
                }
            }
            Policy::RawSidecar => {
                entry.raw = Some(entry.path.clone());
                let cloud = mm_fs::probe(&path)
                    .map(|p| p.cloud_placeholder)
                    .unwrap_or(false);
                match pair_sidecar(&path) {
                    _ if cloud => {
                        entry.status =
                            EntryStatus::Blocked("cloud placeholder that is not downloaded".into());
                        if !seen.insert(fp_in.file_id.clone()) {
                            continue;
                        }
                    }
                    Err(why) => {
                        entry.status = EntryStatus::Blocked(why);
                        if !seen.insert(fp_in.file_id.clone()) {
                            continue;
                        }
                    }
                    Ok((sc, exists, notes)) => {
                        entry.path = sc.to_string_lossy().into_owned();
                        entry.notes = notes;
                        if exists {
                            let fp = fingerprint(&sc)?;
                            if !seen.insert(fp.file_id.clone()) {
                                continue; // its sidecar was selected too
                            }
                            entry.fingerprint = fp;
                            if let Err(why) = precheck(&sc) {
                                entry.status = EntryStatus::Blocked(format!("sidecar: {why}"));
                            }
                        } else {
                            if !seen.insert(format!("new:{}", entry.path.to_lowercase())) {
                                continue;
                            }
                            entry.fingerprint = absent();
                        }
                        if entry.status == EntryStatus::Ready {
                            job = Some(Pending {
                                idx: 0,
                                raw: Some(path.clone()),
                                own: exists.then_some(sc),
                                sidecar_mode: true,
                            });
                        }
                    }
                }
            }
        }
        // SAFETY_MODEL §8.4 / §8.5: where the file that is written lives
        if job.is_some() {
            let root = mm_fs::volume_root(Path::new(&entry.path)).unwrap_or_default();
            let kind = *volumes
                .entry(root.clone())
                .or_insert_with(|| mm_fs::volume_kind(&root));
            match kind {
                VolumeKind::Removable => {
                    entry.status = EntryStatus::Blocked(
                        "on removable media (memory card, USB drive): writing there is off by default; copy the files to the computer first"
                            .into(),
                    );
                    job = None;
                }
                VolumeKind::Network => entry.notes.push(
                    "on a network drive: allowed, but not verified on real NAS devices".into(),
                ),
                _ => {}
            }
            if job.is_some() {
                if let Some(provider) = mm_fs::sync_provider(Path::new(&entry.path), &sync_roots) {
                    entry.notes.push(format!(
                        "in a {provider} folder: the change will be uploaded, and if the file is \
                         edited elsewhere at the same time the sync client may keep a conflicted copy"
                    ));
                }
            }
        }
        entry.seq = entries.len() as u32;
        if let Some(mut j) = job {
            j.idx = entries.len();
            pending.push(j);
        }
        entries.push(entry);
    }
    // one batched read of every RAW and every existing target
    let mut to_read: Vec<PathBuf> = Vec::new();
    for p in &pending {
        to_read.extend(p.raw.iter().cloned());
        to_read.extend(p.own.iter().cloned());
    }
    ctl.report(PlanStage::Files, inputs.len(), inputs.len())?;
    let mut snaps = engine
        .read_snapshots_with(&to_read, &mut |done, total| {
            ctl.report(PlanStage::Metadata, done, total)
        })?
        .into_iter();
    let mut reads: Vec<(usize, Reads)> = Vec::new();
    for p in pending {
        let raw = p
            .raw
            .as_ref()
            .map(|_| snaps.next().unwrap_or(Err("missing".into())));
        let own = p
            .own
            .as_ref()
            .map(|_| snaps.next().unwrap_or(Err("missing".into())));
        let failed = [raw.as_ref(), own.as_ref()]
            .into_iter()
            .flatten()
            .find_map(|r| r.as_ref().err().cloned());
        if let Some(why) = failed {
            entries[p.idx].status = EntryStatus::Blocked(format!("metadata unreadable: {why}"));
            continue;
        }
        reads.push((
            p.idx,
            Reads {
                sidecar_mode: p.sidecar_mode,
                raw: raw.and_then(Result::ok).unwrap_or_default(),
                own: own.and_then(Result::ok),
            },
        ));
    }
    let targets: Vec<(usize, Target)> = reads
        .iter()
        .filter_map(|(i, r)| r.target().map(|t| (*i, t)))
        .collect();
    let plans = field_all(&targets, &entries)?;
    for ((idx, t), fields) in targets.iter().zip(plans) {
        let cp = mm_domain::plan::merge(fields);
        let e = &mut entries[*idx];
        e.status = cp.status;
        e.notes.extend(cp.notes);
        // SAFETY_MODEL §8.12: writing in place would invalidate Content Credentials
        let c2pa = match t {
            Target::Embedded(s) => mm_domain::risk::has_c2pa(s),
            Target::Sidecar { .. } => false, // the RAW is not written
        };
        if c2pa && e.status == EntryStatus::Ready {
            e.status = EntryStatus::Blocked(
                "carries C2PA Content Credentials: any metadata change invalidates their signature, so the file is excluded by default"
                    .into(),
            );
            continue;
        }
        e.changes.extend(cp.changes);
        if e.status == EntryStatus::Ready {
            let (ops, expect) = (cp.ops, cp.expect);
            e.action = Some(if e.fingerprint.file_id.is_empty() {
                EntryAction::CreateFile { ops, expect }
            } else {
                EntryAction::Write { ops, expect }
            });
        }
    }
    Ok(Plan {
        id: new_id("plan")?,
        version: 1,
        kind: PlanKind::Apply,
        title: title.to_owned(),
        registry_version: creator::REGISTRY_VERSION,
        exiftool_version: engine.version().to_owned(),
        entries,
    })
}

/// True when the file still matches the planning-time fingerprint.
pub fn same_file(now: &Fingerprint, planned: &Fingerprint) -> bool {
    now == planned
}
