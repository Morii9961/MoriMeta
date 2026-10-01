// SPDX-License-Identifier: GPL-3.0-or-later
//! Clean Export (D-15 (c), PRODUCT_SPEC §6.8.3, METADATA_MODEL §10.1): copies of JPEG files that
//! keep only whitelisted tags and segments, written into a folder the user picks.
//!
//! - [`plan`] reads every source (full tag inventory, marker segments, image data hash) and
//!   predicts each tag and segment removed. Nothing is written.
//! - [`export`] writes each copy to a temporary name in the output folder with ExifTool, checks it
//!   (segment whitelist, tag whitelist, image data unchanged, removals exactly as predicted) and
//!   only then gives it its name, never replacing a file. A copy that fails any check is removed
//!   and the file is reported as not exported.
//!
//! The sources are only read. The guarantee is the S7 one: the exported JPEG holds only
//! whitelisted segments and tags, each file checked; not the picture content itself.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use mm_domain::clean::{self, KeepSpec, Prediction};
use mm_domain::jpeg;
use mm_domain::plan::Fingerprint;
use serde::Serialize;

use crate::engine::Engine;
use crate::planner::{PlanCtl, PlanStage, same_file};
use crate::{CoreError, fingerprint, new_id, normalize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "status", content = "reason", rename_all = "snake_case")]
pub enum CleanStatus {
    Ready,
    /// Not exported, with the reason (not a JPEG, unreadable structure, …).
    Blocked(String),
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanEntry {
    pub seq: u32,
    pub source: String,
    /// The name the copy gets (before numbering).
    pub name: String,
    pub status: CleanStatus,
    pub prediction: Prediction,
    #[serde(skip)]
    fingerprint: Option<Fingerprint>,
    #[serde(skip)]
    tags: BTreeMap<String, String>,
    #[serde(skip)]
    image_hash: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct CleanPlan {
    pub id: String,
    pub spec: KeepSpec,
    pub entries: Vec<CleanEntry>,
}

fn is_jpeg_name(p: &Path) -> bool {
    p.extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .is_some_and(|e| e == "jpg" || e == "jpeg")
}

/// Read the sources and predict what each copy loses. Nothing is written.
pub fn plan(
    engine: &mut Engine,
    inputs: &[PathBuf],
    spec: KeepSpec,
    ctl: &PlanCtl,
) -> Result<CleanPlan, CoreError> {
    let mut entries = Vec::with_capacity(inputs.len());
    for (i, input) in inputs.iter().enumerate() {
        ctl.report(PlanStage::Metadata, i, inputs.len())?;
        let path = normalize(input)?;
        let mut e = CleanEntry {
            seq: i as u32,
            source: path.display().to_string(),
            name: path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            status: CleanStatus::Ready,
            prediction: Prediction::default(),
            fingerprint: None,
            tags: BTreeMap::new(),
            image_hash: None,
        };
        let blocked = |why: String| CleanStatus::Blocked(why);
        if !is_jpeg_name(&path) {
            e.status = blocked("only JPEG files are exported in this version".into());
        } else if crate::is_placeholder(&path) {
            e.status = blocked("cloud placeholder that is not downloaded".into());
        } else {
            match std::fs::read(&path) {
                Err(err) => e.status = blocked(format!("cannot open: {err}")),
                Ok(bytes) => match jpeg::parse(&bytes) {
                    Err(err) => {
                        e.status = blocked(format!("cannot read the JPEG structure: {err}"))
                    }
                    Ok(segments) => match engine.read_inventory(&path) {
                        Err(err) => e.status = blocked(format!("metadata unreadable: {err}")),
                        Ok(inv) => {
                            e.prediction = clean::predict(&spec, &inv.tags, &segments);
                            e.fingerprint = Some(fingerprint(&path)?);
                            e.tags = inv.tags;
                            e.image_hash = inv.image_hash;
                            if e.image_hash.is_none() {
                                e.status = blocked("the image data cannot be hashed, so the copy could not be checked".into());
                            }
                        }
                    },
                },
            }
        }
        entries.push(e);
    }
    ctl.report(PlanStage::Metadata, inputs.len(), inputs.len())?;
    Ok(CleanPlan {
        id: new_id("clean")?,
        spec,
        entries,
    })
}

/// What happens when a copy's name is taken in the output folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OnConflict {
    /// `name (2).jpg`, `name (3).jpg`, …
    Number,
    Skip,
}

#[derive(Debug, Clone, Serialize)]
pub struct Exported {
    pub seq: u32,
    pub source: String,
    /// The new file.
    pub output: Option<String>,
    /// `exported`, `skipped` (name taken), `refused` (a check failed), `blocked` (in the plan),
    /// `failed`, `cancelled`.
    pub status: String,
    pub reasons: Vec<String>,
}

fn numbered(dir: &Path, name: &str, n: u32) -> PathBuf {
    if n < 2 {
        return dir.join(name);
    }
    let p = Path::new(name);
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    match p.extension() {
        Some(ext) => dir.join(format!("{stem} ({n}).{}", ext.to_string_lossy())),
        None => dir.join(format!("{stem} ({n})")),
    }
}

/// Export the Ready entries into `dir`. `progress(done, total)` after each file.
pub fn export(
    engine: &mut Engine,
    plan: &CleanPlan,
    dir: &Path,
    on_conflict: OnConflict,
    progress: &dyn Fn(usize, usize),
    cancel: Option<Arc<AtomicBool>>,
) -> Result<Vec<Exported>, CoreError> {
    let dir = normalize(dir)?;
    if !dir.is_dir() {
        return Err(CoreError::Input(format!("{}: not a folder", dir.display())));
    }
    if crate::in_backup_folder(&dir.join("x.jpg")) {
        return Err(CoreError::Input(crate::IN_BACKUP_FOLDER.into()));
    }
    let copy = plan.spec.copy_tags();
    let total = plan.entries.len();
    let mut out = Vec::with_capacity(total);
    for (i, e) in plan.entries.iter().enumerate() {
        let mut r = Exported {
            seq: e.seq,
            source: e.source.clone(),
            output: None,
            status: String::new(),
            reasons: vec![],
        };
        if cancel.as_ref().is_some_and(|c| c.load(Ordering::SeqCst)) {
            r.status = "cancelled".into();
        } else if let CleanStatus::Blocked(why) = &e.status {
            r.status = "blocked".into();
            r.reasons.push(why.clone());
        } else {
            export_one(engine, plan, e, &dir, on_conflict, &copy, &mut r);
        }
        crate::log::event(
            if r.status == "exported" {
                "info"
            } else {
                "warn"
            },
            "clean export file",
            &[("status", &r.status)],
        );
        out.push(r);
        progress(i + 1, total);
    }
    Ok(out)
}

fn export_one(
    engine: &mut Engine,
    plan: &CleanPlan,
    e: &CleanEntry,
    dir: &Path,
    on_conflict: OnConflict,
    copy: &[String],
    r: &mut Exported,
) {
    let source = Path::new(&e.source);
    // the source must still be what the Preview read
    match (fingerprint(source), &e.fingerprint) {
        (Ok(now), Some(then)) if same_file(&now, then) => {}
        _ => {
            r.status = "refused".into();
            r.reasons
                .push("file changed since the preview (size, time or identity)".into());
            return;
        }
    }
    let token = match mm_fs::random_token() {
        Ok(t) => t,
        Err(err) => {
            r.status = "failed".into();
            r.reasons.push(err.to_string());
            return;
        }
    };
    let temp = dir.join(format!(".mmexport-{token}.jpg"));
    let discard = |p: &Path| {
        let _ = std::fs::remove_file(p);
    };
    let ran = engine.clean_copy(source, &temp, copy);
    if !temp.exists() {
        r.status = "failed".into();
        r.reasons.push(match ran {
            Ok(o) => format!("ExifTool: {}", o.stderr_text().trim()),
            Err(err) => format!("ExifTool: {err}"),
        });
        return;
    }
    // the four checks (METADATA_MODEL §10.1)
    let segment_problems = match std::fs::read(&temp).map(|b| jpeg::parse(&b)) {
        Ok(Ok(j)) => jpeg::check_clean(&j),
        Ok(Err(err)) => vec![format!("the copy's JPEG structure: {err}")],
        Err(err) => vec![format!("the copy cannot be read: {err}")],
    };
    let inventory = match engine.read_inventory(&temp) {
        Ok(i) => i,
        Err(err) => {
            discard(&temp);
            r.status = "refused".into();
            r.reasons
                .push(format!("the copy's metadata cannot be read: {err}"));
            return;
        }
    };
    let why = clean::check(
        &plan.spec,
        &e.tags,
        &e.prediction,
        &inventory.tags,
        &segment_problems,
        e.image_hash.as_deref(),
        inventory.image_hash.as_deref(),
    );
    if !why.is_empty() {
        discard(&temp);
        r.status = "refused".into();
        r.reasons = why;
        return;
    }
    if let Err(err) = mm_fs::flush_path(&temp) {
        discard(&temp);
        r.status = "failed".into();
        r.reasons.push(format!("cannot flush the copy: {err}"));
        return;
    }
    // its name, never replacing a file
    for n in 1..1000 {
        let target = numbered(dir, &e.name, n);
        if target.exists() {
            if on_conflict == OnConflict::Skip {
                discard(&temp);
                r.status = "skipped".into();
                r.reasons.push(format!(
                    "{}: a file with this name exists",
                    target.display()
                ));
                return;
            }
            continue;
        }
        match mm_fs::move_no_replace(&temp, &target) {
            Ok(()) => {
                r.status = "exported".into();
                r.output = Some(target.display().to_string());
                return;
            }
            // taken in the meantime: try the next number
            Err(_) if target.exists() && on_conflict == OnConflict::Number => continue,
            Err(err) => {
                discard(&temp);
                r.status = "failed".into();
                r.reasons.push(format!("cannot name the copy: {err:?}"));
                return;
            }
        }
    }
    discard(&temp);
    r.status = "failed".into();
    r.reasons.push("no free file name".into());
}
