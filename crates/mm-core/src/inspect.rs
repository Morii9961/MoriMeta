// SPDX-License-Identifier: GPL-3.0-or-later
//! Read-only views for the UI (ARCHITECTURE §5.2): `asset_detail` for the Inspector (each field's
//! effective value, where it comes from, conflicts, and every raw tag) and `selection_aggregate`
//! for the batch editor (how many files hold which value, so mixed values are visible), and
//! `scan_rows` for the Library table.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use mm_domain::capture;
use mm_domain::copyright;
use mm_domain::creator;
use mm_domain::gps;
use mm_domain::plan::Target;
use mm_domain::snapshot::Snapshot;
use serde::Serialize;

use crate::engine::{BinaryTag, Engine};
use crate::planner::{PlanCtl, Policy, pair_sidecar, policy};
use crate::{CoreError, normalize};

#[derive(Debug, Clone, Serialize)]
pub struct FieldView {
    pub field: &'static str,
    /// Effective value as the Preview shows it; None when empty.
    pub value: Option<String>,
    /// Every location holding a value: (tag, value).
    pub sources: Vec<(String, String)>,
    pub conflicting: bool,
    /// The value is present but not valid (e.g. an unparsable time).
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetDetail {
    pub path: String,
    /// The XMP sidecar that MoriMeta writes for a RAW (it may not exist yet).
    pub sidecar: Option<String>,
    pub fields: Vec<FieldView>,
    /// Every tag of the file (`Group1:Tag` → value), read only; for a RAW, the RAW's own tags.
    pub tags: BTreeMap<String, String>,
    /// The sidecar's tags, when it exists.
    pub sidecar_tags: Option<BTreeMap<String, String>>,
}

fn fields_of(t: &Target) -> Vec<FieldView> {
    let c = creator::read_target(t);
    let r = copyright::read_target(t);
    let (time, time_err) = match capture::read_target(t) {
        Ok(v) => (v.map(|x| capture::display(&x)), None),
        Err(e) => (None, Some(e)),
    };
    vec![
        FieldView {
            field: "creator",
            value: c.effective.as_ref().map(|v| v.join("; ")),
            sources: c
                .sources
                .iter()
                .map(|(k, v)| (k.clone(), v.join("; ")))
                .collect(),
            conflicting: c.conflicting,
            error: None,
        },
        FieldView {
            field: "copyright",
            value: r.effective.clone(),
            sources: r.sources.clone(),
            conflicting: r.conflicting,
            error: None,
        },
        FieldView {
            field: "capture_time",
            value: time,
            sources: vec![],
            conflicting: false,
            error: time_err,
        },
        FieldView {
            field: "gps",
            value: gps::read_target(t).map(|p| p.display()),
            sources: vec![],
            conflicting: false,
            error: None,
        },
    ]
}

fn flat(s: &Snapshot) -> BTreeMap<String, String> {
    s.iter()
        .map(|(k, v)| (k.clone(), mm_domain::snapshot::value_text(v)))
        .collect()
}

/// A file's RAW-or-image snapshot and its sidecar (path, snapshot if it exists).
type Views = (Snapshot, Option<(PathBuf, Option<Snapshot>)>);

fn read_views(engine: &mut Engine, path: &Path) -> Result<Views, CoreError> {
    let sidecar = match policy(path) {
        Policy::RawSidecar => {
            let (sc, exists, _) = pair_sidecar(path).map_err(CoreError::Input)?;
            Some((sc, exists))
        }
        _ => None,
    };
    let mut paths: Vec<&Path> = vec![path];
    if let Some((sc, true)) = &sidecar {
        paths.push(sc);
    }
    if let Some(p) = paths.iter().find(|p| crate::is_placeholder(p)) {
        return Err(CoreError::NotDownloaded(p.display().to_string()));
    }
    let mut snaps = engine.read_all_tags(&paths)?.into_iter();
    let main = snaps
        .next()
        .flatten()
        .ok_or_else(|| CoreError::Engine(format!("{}: no metadata result", path.display())))?;
    let sidecar =
        sidecar.map(|(sc, exists)| (sc, if exists { snaps.next().flatten() } else { None }));
    Ok((main, sidecar))
}

/// The Inspector's data for one file.
pub fn asset_detail(engine: &mut Engine, path: &Path) -> Result<AssetDetail, CoreError> {
    let path = normalize(path)?;
    let (main, sidecar) = read_views(engine, &path)?;
    let t = match &sidecar {
        Some((_, sc)) => Target::Sidecar {
            raw: &main,
            sidecar: sc.as_ref(),
        },
        None => Target::Embedded(&main),
    };
    Ok(AssetDetail {
        path: path.to_string_lossy().into_owned(),
        fields: fields_of(&t),
        tags: flat(&main),
        sidecar: sidecar
            .as_ref()
            .map(|(p, _)| p.to_string_lossy().into_owned()),
        sidecar_tags: sidecar.as_ref().and_then(|(_, s)| s.as_ref().map(flat)),
    })
}

/// The largest embedded preview the Inspector shows.
const MAX_PREVIEW: usize = 1 << 20;

/// The embedded JPEG preview of a file for the Inspector (SCREEN_SPEC §2, 112 px): its
/// thumbnail, or a RAW's preview when it has none. Only JPEG data of at most 1 MiB is returned,
/// for the WebView to show as an image. A cloud placeholder is not read.
pub fn embedded_preview(engine: &mut Engine, path: &Path) -> Result<Option<Vec<u8>>, CoreError> {
    let path = normalize(path)?;
    if crate::is_placeholder(&path) {
        return Err(CoreError::NotDownloaded(path.display().to_string()));
    }
    for tag in [BinaryTag::Thumbnail, BinaryTag::Preview] {
        if let Some(b) = engine.read_binary(&path, tag)?
            && b.len() <= MAX_PREVIEW
            && b.starts_with(&[0xff, 0xd8, 0xff])
        {
            return Ok(Some(b));
        }
    }
    Ok(None)
}

/// What the Session summary lists under "Needs attention" (SCREEN_SPEC Library): indexes into
/// the paths asked about.
#[derive(Debug, Clone, Default, Serialize)]
pub struct Attention {
    pub read_only: Vec<usize>,
    /// Creator or copyright locations that disagree.
    pub conflicts: Vec<usize>,
    pub cloud_placeholders: Vec<usize>,
    /// Downloaded files of a cloud sync client: not written in this build (SAFETY_MODEL §8.3).
    pub cloud_files: Vec<usize>,
    /// RAWs with a darktable `<name>.<ext>.xmp` sidecar (read-only for MoriMeta).
    pub darktable_sidecars: Vec<usize>,
    pub c2pa: Vec<usize>,
    /// Links (hard links, symbolic links): never written.
    pub links: Vec<usize>,
    pub unreadable: Vec<usize>,
    /// Paths longer than 260 characters (they work, S0; shown as a risk marker).
    pub long_paths: Vec<usize>,
    /// On removable media (writing there is blocked by default, SAFETY_MODEL §8.4).
    pub removable: Vec<usize>,
    /// On a network drive (§8.5).
    pub network: Vec<usize>,
    /// On a local drive whose file system is not NTFS (exFAT, FAT32, …; blocked like removable
    /// media, §8.4).
    pub other_file_system: Vec<usize>,
}

/// One metadata read of the paths plus their file attributes.
pub fn attention(
    engine: &mut Engine,
    paths: &[PathBuf],
    ctl: &PlanCtl,
) -> Result<Attention, CoreError> {
    let mut a = Attention::default();
    let mut to_read = Vec::new();
    let mut idx = Vec::new();
    let mut volumes: std::collections::HashMap<PathBuf, (mm_fs::VolumeKind, Option<String>)> =
        std::collections::HashMap::new();
    for (i, p) in paths.iter().enumerate() {
        if p.as_os_str().len() > 260 {
            a.long_paths.push(i);
        }
        let root = mm_fs::volume_root(p).unwrap_or_default();
        let (kind, fs) = volumes
            .entry(root.clone())
            .or_insert_with(|| (mm_fs::volume_kind(&root), mm_fs::file_system(&root)));
        match kind {
            mm_fs::VolumeKind::Removable => a.removable.push(i),
            mm_fs::VolumeKind::Network => a.network.push(i),
            _ if fs
                .as_deref()
                .is_some_and(|f| !crate::planner::is_verified_fs(f)) =>
            {
                a.other_file_system.push(i)
            }
            _ => {}
        }
        match mm_fs::probe(p) {
            Ok(pr) => {
                if pr.read_only {
                    a.read_only.push(i);
                }
                match pr.reparse {
                    mm_fs::Reparse::Cloud if !pr.cloud_placeholder => a.cloud_files.push(i),
                    mm_fs::Reparse::Link | mm_fs::Reparse::Other => a.links.push(i),
                    _ if pr.links > 1 => a.links.push(i),
                    _ => {}
                }
                if pr.cloud_placeholder {
                    a.cloud_placeholders.push(i);
                    continue; // not read, so not downloaded
                }
            }
            Err(_) => {
                a.unreadable.push(i);
                continue;
            }
        }
        if matches!(policy(p), Policy::RawSidecar) {
            let mut dt = p.as_os_str().to_owned();
            dt.push(".xmp");
            if Path::new(&dt).exists() {
                a.darktable_sidecars.push(i);
            }
        }
        to_read.push(p.clone());
        idx.push(i);
    }
    let snaps = engine.read_snapshots_parallel(&to_read, ctl.readers, &mut |done, total| {
        ctl.report(crate::planner::PlanStage::Metadata, done, total)
    })?;
    for (i, s) in idx.into_iter().zip(snaps) {
        let Ok(s) = s else {
            a.unreadable.push(i);
            continue;
        };
        let t = Target::Embedded(&s);
        if creator::read_target(&t).conflicting || copyright::read_target(&t).conflicting {
            a.conflicts.push(i);
        }
        if mm_domain::risk::has_c2pa(&s) {
            a.c2pa.push(i);
        }
    }
    a.unreadable.sort_unstable();
    Ok(a)
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct FieldAggregate {
    pub field: &'static str,
    pub files: usize,
    /// Distinct effective values with their file counts, most frequent first (at most 20).
    pub values: Vec<(String, usize)>,
    /// More distinct values than listed.
    pub more_values: usize,
    pub empty: usize,
    pub conflicting: usize,
    /// Present but not valid, or the file could not be read.
    pub unreadable: usize,
    /// Cloud placeholders (the file or its sidecar) that are not on this computer: counted in
    /// `files`, not read (SAFETY_MODEL §8.3).
    pub not_downloaded: usize,
}

const TOP_VALUES: usize = 20;

/// The batch editor's view of a selection: per field, which values how many files hold.
pub fn selection_aggregate(
    engine: &mut Engine,
    paths: &[PathBuf],
    ctl: &PlanCtl,
) -> Result<Vec<FieldAggregate>, CoreError> {
    let names = ["creator", "copyright", "capture_time", "gps"];
    let mut counts: Vec<BTreeMap<String, usize>> = vec![BTreeMap::new(); names.len()];
    let mut aggs: Vec<FieldAggregate> = names
        .iter()
        .map(|n| FieldAggregate {
            field: n,
            ..Default::default()
        })
        .collect();
    // image files and RAWs in one batched read, sidecars after them
    let mut main: Vec<PathBuf> = Vec::new();
    let mut sidecars: Vec<Option<(PathBuf, bool)>> = Vec::new();
    let mut not_downloaded = 0;
    for p in paths {
        let p = normalize(p)?;
        let sc = match policy(&p) {
            Policy::RawSidecar => pair_sidecar(&p).ok().map(|(sc, exists, _)| (sc, exists)),
            _ => None,
        };
        // SAFETY_MODEL §8.3: reading a placeholder downloads it; its values are not known here
        if crate::is_placeholder(&p)
            || sc
                .as_ref()
                .is_some_and(|(s, exists)| *exists && crate::is_placeholder(s))
        {
            not_downloaded += 1;
            continue;
        }
        sidecars.push(sc);
        main.push(p);
    }
    for a in aggs.iter_mut() {
        a.files += not_downloaded;
        a.not_downloaded = not_downloaded;
    }
    let existing: Vec<PathBuf> = sidecars
        .iter()
        .flatten()
        .filter(|(_, e)| *e)
        .map(|(p, _)| p.clone())
        .collect();
    let all: Vec<PathBuf> = main
        .iter()
        .cloned()
        .chain(existing.iter().cloned())
        .collect();
    let snaps = engine.read_snapshots_parallel(&all, ctl.readers, &mut |done, total| {
        ctl.report(crate::planner::PlanStage::Metadata, done, total)
    })?;
    let (main_snaps, sc_snaps) = snaps.split_at(main.len());
    let mut sc_iter = sc_snaps.iter();
    for (i, snap) in main_snaps.iter().enumerate() {
        let sc = match &sidecars[i] {
            Some((_, true)) => sc_iter.next().and_then(|r| r.as_ref().ok()),
            _ => None,
        };
        let Ok(snap) = snap else {
            for a in aggs.iter_mut() {
                a.files += 1;
                a.unreadable += 1;
            }
            continue;
        };
        let t = if sidecars[i].is_some() {
            Target::Sidecar {
                raw: snap,
                sidecar: sc,
            }
        } else {
            Target::Embedded(snap)
        };
        for (k, v) in fields_of(&t).into_iter().enumerate() {
            let a = &mut aggs[k];
            a.files += 1;
            if v.conflicting {
                a.conflicting += 1;
            }
            match (v.value, v.error) {
                (_, Some(_)) => a.unreadable += 1,
                (Some(val), None) => *counts[k].entry(val).or_insert(0) += 1,
                (None, None) => a.empty += 1,
            }
        }
    }
    for (a, c) in aggs.iter_mut().zip(counts) {
        let mut v: Vec<(String, usize)> = c.into_iter().collect();
        v.sort_by(|x, y| y.1.cmp(&x.1).then_with(|| x.0.cmp(&y.0)));
        a.more_values = v.len().saturating_sub(TOP_VALUES);
        v.truncate(TOP_VALUES);
        a.values = v;
    }
    Ok(aggs)
}

/// One Library row (ARCHITECTURE §6.1 "Emit rows … Stream row updates"; SCREEN_SPEC Library):
/// what the table, its facets, filters and sorting show for a file, from a batched read.
#[derive(Debug, Clone, Serialize)]
pub struct Row {
    /// Index into the paths asked about.
    pub index: usize,
    /// Where an edit of this file goes: `in_file`, `sidecar`, `new_sidecar`, or `read_only` (a
    /// format this build does not write).
    pub writes_to: &'static str,
    /// Creator, copyright, capture time, GPS: effective values as the Inspector shows them.
    pub fields: Vec<FieldView>,
    pub make: Option<String>,
    pub model: Option<String>,
    pub lens: Option<String>,
    /// A cloud placeholder (the file or its sidecar) that is not on this computer: not read
    /// (SAFETY_MODEL §8.3); its values are unknown.
    pub not_downloaded: bool,
    /// The file or its sidecar could not be read.
    pub error: Option<String>,
}

/// Files per streamed batch of rows.
const ROW_CHUNK: usize = 400;

/// Library scan (ARCHITECTURE §6.1, SCREEN_SPEC 1#large): reads every file (and a RAW's sidecar)
/// in batches, hands each batch of rows to `emit` as soon as it is read, reports progress and
/// stops on cancel through `ctl`. Read-only: placeholders are never read.
pub fn scan_rows(
    engine: &mut Engine,
    paths: &[PathBuf],
    ctl: &PlanCtl,
    emit: &mut dyn FnMut(Vec<Row>),
) -> Result<(), CoreError> {
    let total = paths.len();
    for (c, chunk) in paths.chunks(ROW_CHUNK).enumerate() {
        let offset = c * ROW_CHUNK;
        ctl.report(crate::planner::PlanStage::Metadata, offset, total)?;
        let mut rows: Vec<Row> = Vec::with_capacity(chunk.len());
        let mut to_read: Vec<PathBuf> = Vec::new();
        // (row, main read index, sidecar read index)
        let mut plan: Vec<(usize, Option<usize>, Option<usize>)> = Vec::new();
        for (k, p) in chunk.iter().enumerate() {
            let mut row = Row {
                index: offset + k,
                writes_to: "in_file",
                fields: vec![],
                make: None,
                model: None,
                lens: None,
                not_downloaded: false,
                error: None,
            };
            let p = match normalize(p) {
                Ok(p) => p,
                Err(e) => {
                    row.error = Some(e.to_string());
                    rows.push(row);
                    plan.push((rows.len() - 1, None, None));
                    continue;
                }
            };
            let sc = match policy(&p) {
                Policy::RawSidecar => pair_sidecar(&p).ok().map(|(sc, exists, _)| (sc, exists)),
                _ => None,
            };
            row.writes_to = match (policy(&p), &sc) {
                (Policy::Unsupported, _) => "read_only",
                (Policy::RawSidecar, Some((_, true))) => "sidecar",
                (Policy::RawSidecar, _) => "new_sidecar",
                _ => "in_file",
            };
            let sc_exists = sc.filter(|(_, e)| *e).map(|(s, _)| s);
            if crate::is_placeholder(&p) || sc_exists.as_deref().is_some_and(crate::is_placeholder)
            {
                row.not_downloaded = true;
                rows.push(row);
                plan.push((rows.len() - 1, None, None));
                continue;
            }
            to_read.push(p);
            let main = to_read.len() - 1;
            let side = sc_exists.map(|s| {
                to_read.push(s);
                to_read.len() - 1
            });
            rows.push(row);
            plan.push((rows.len() - 1, Some(main), side));
        }
        let snaps = engine.read_snapshots_parallel(&to_read, ctl.readers, &mut |done, _| {
            ctl.report(
                crate::planner::PlanStage::Metadata,
                offset + done.min(chunk.len()),
                total,
            )
        })?;
        for (r, main, side) in plan {
            let Some(main) = main else { continue };
            let row = &mut rows[r];
            let snap = match &snaps[main] {
                Ok(s) => s,
                Err(e) => {
                    row.error = Some(e.clone());
                    continue;
                }
            };
            row.error = crate::planner::content_mismatch(&to_read[main], snap).or_else(|| {
                side.and_then(|i| match &snaps[i] {
                    Ok(s) => crate::planner::content_mismatch(&to_read[i], s),
                    Err(_) => None,
                })
            });
            let sidecar = match side.map(|i| &snaps[i]) {
                Some(Err(e)) => {
                    row.error = Some(format!("sidecar: {e}"));
                    continue;
                }
                Some(Ok(s)) => Some(s),
                None => None,
            };
            let t = if row.writes_to == "sidecar" || row.writes_to == "new_sidecar" {
                Target::Sidecar { raw: snap, sidecar }
            } else {
                Target::Embedded(snap)
            };
            row.fields = fields_of(&t);
            let first = |keys: &[&str]| keys.iter().find_map(|k| snap.text(k));
            row.make = first(&["IFD0:Make"]);
            row.model = first(&["IFD0:Model"]);
            row.lens = first(&["ExifIFD:LensModel", "Composite:LensID", "XMP-aux:Lens"]);
        }
        emit(rows);
    }
    ctl.report(crate::planner::PlanStage::Metadata, total, total)
}
