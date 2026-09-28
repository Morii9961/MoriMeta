// SPDX-License-Identifier: GPL-3.0-or-later
//! Backend interface for the UI adapter (ARCHITECTURE §5.1, §5.1a, §5.2). The frontend never names
//! a path for a write: files enter a [`Session`] through the backend and are referred to by
//! [`AssetId`]; a write names a Plan id, its version and a single-use confirmation token handed
//! out when the user confirms that version in Preview; every write holds the [`OperationGate`].

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use mm_domain::plan::{EntryStatus, Plan, PlanEntry, PlanSummary};
use mm_store::Store;

use crate::engine::Engine;
use crate::executor::{self, ExecOptions, OpReport};
use crate::{CoreError, file_id_hex, normalize};

#[derive(Debug)]
pub enum ServiceError {
    UnknownAsset(AssetId),
    UnknownPlan(String),
    /// The request names an older version of the Plan; `current` is the one to show.
    StalePlan {
        current: u32,
    },
    /// No confirmation token for this version, or a different one (tokens are single use).
    NotConfirmed,
    /// A high-risk Plan needs these acknowledgements first (INTERACTION_SPEC §4–5).
    NotAcknowledged(Vec<String>),
    /// The OperationGate refused: a write or an update installation is in progress.
    Busy(String),
    /// Running with administrator rights: writes are disabled (SECURITY_MODEL §4.1).
    Elevated,
    Core(CoreError),
}

impl std::fmt::Display for ServiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ServiceError {}
impl From<CoreError> for ServiceError {
    fn from(e: CoreError) -> Self {
        ServiceError::Core(e)
    }
}
impl From<std::io::Error> for ServiceError {
    fn from(e: std::io::Error) -> Self {
        ServiceError::Core(e.into())
    }
}
impl From<mm_store::StoreError> for ServiceError {
    fn from(e: mm_store::StoreError) -> Self {
        ServiceError::Core(e.into())
    }
}

/// A file of the Session; opaque and only valid in that Session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AssetId(pub u64);

#[derive(Debug, Default)]
pub struct ImportReport {
    pub added: Vec<AssetId>,
    /// Those of `added` in a format this build reads but does not write (shown read-only).
    pub read_only: Vec<AssetId>,
    /// Already in the Session (same volume and File ID, SAFETY_MODEL §8.8), possibly by another
    /// path: the id it already has.
    pub duplicates: Vec<AssetId>,
    pub failed: Vec<(String, String)>,
    /// Folder import only: files that are not photos (and XMP files, which come with their RAW).
    pub other_files: usize,
    /// Folder import only: directory links not entered (§8.6).
    pub not_followed: Vec<String>,
    /// Folder import only: cloud files not on this computer, not read (§8.3).
    pub placeholders: Vec<String>,
    /// Folder import only: system and hidden folders not entered (PRODUCT_SPEC §6.1).
    pub skipped_folders: Vec<String>,
    /// Folder import only: XMP files without a main file next to them, taken in on their own
    /// (PRODUCT_SPEC §6.1 "orphan sidecars are listed separately").
    pub orphan_sidecars: Vec<AssetId>,
}

/// The files the user brought in. Paths come only from the backend's own dialogs and drop events;
/// the frontend receives them for display.
#[derive(Debug, Default)]
pub struct Session {
    next: u64,
    assets: BTreeMap<AssetId, Asset>,
    by_identity: HashMap<String, AssetId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Asset {
    pub path: PathBuf,
    /// False for photo formats this build does not write: shown and inspected, never planned.
    pub writable: bool,
    /// Size, modification time and identity when it was imported ("changed since import").
    pub fingerprint: Option<mm_domain::plan::Fingerprint>,
}

impl Session {
    /// Files the user picked (dialog, drop).
    pub fn import(&mut self, paths: &[PathBuf]) -> ImportReport {
        self.import_as(paths, true)
    }

    fn import_as(&mut self, paths: &[PathBuf], chosen: bool) -> ImportReport {
        let mut r = ImportReport::default();
        for p in paths {
            let Some(kind) = crate::planner::import_kind(p, chosen) else {
                r.failed.push((
                    p.display().to_string(),
                    "not a photo format MoriMeta reads".into(),
                ));
                continue;
            };
            let id = normalize(p).and_then(|abs| {
                let fid = mm_fs::file_id_of_path(&abs)?;
                Ok((abs, file_id_hex(&fid)))
            });
            match id {
                Ok((abs, identity)) => {
                    if let Some(&have) = self.by_identity.get(&identity) {
                        r.duplicates.push(have);
                        continue;
                    }
                    self.next += 1;
                    let a = AssetId(self.next);
                    let writable = kind == crate::planner::ImportKind::Writable;
                    let fingerprint = crate::fingerprint(&abs).ok();
                    self.assets.insert(
                        a,
                        Asset {
                            path: abs,
                            writable,
                            fingerprint,
                        },
                    );
                    self.by_identity.insert(identity, a);
                    r.added.push(a);
                    if !writable {
                        r.read_only.push(a);
                    }
                }
                Err(e) => r.failed.push((p.display().to_string(), e.to_string())),
            }
        }
        r
    }

    /// Every supported file under `dir` (ARCHITECTURE §6.1), without following directory links
    /// or reading cloud placeholders.
    pub fn import_folder(&mut self, dir: &Path) -> ImportReport {
        let w = mm_fs::walk(dir);
        // an XMP file whose folder holds no other file of the same name is an orphan sidecar;
        // `<name>.<ext>.xmp` (darktable) is never taken in
        let stems: std::collections::HashSet<(PathBuf, String)> = w
            .files
            .iter()
            .filter(|p| !is_xmp(p))
            .map(|p| stem_key(p))
            .collect();
        let (orphans, rest): (Vec<PathBuf>, Vec<PathBuf>) = w
            .files
            .into_iter()
            .partition(|p| is_xmp(p) && !is_darktable(p) && !stems.contains(&stem_key(p)));
        let (take, other): (Vec<PathBuf>, Vec<PathBuf>) = rest
            .into_iter()
            .partition(|p| crate::planner::import_kind(p, false).is_some());
        let mut r = self.import_as(&take, false);
        let o = self.import_as(&orphans, true);
        r.orphan_sidecars = o.added.clone();
        r.added.extend(o.added);
        r.duplicates.extend(o.duplicates);
        r.failed.extend(o.failed);
        r.skipped_folders = w
            .skipped_folders
            .iter()
            .map(|p| p.display().to_string())
            .collect();
        let shown = |v: Vec<PathBuf>| v.iter().map(|p| p.display().to_string()).collect();
        r.other_files = other.len();
        r.not_followed = shown(w.not_followed);
        r.placeholders = shown(w.placeholders);
        r.failed.extend(
            w.errors
                .into_iter()
                .map(|(p, e)| (p.display().to_string(), e)),
        );
        r
    }

    pub fn path(&self, id: AssetId) -> Result<&Path, ServiceError> {
        self.asset(id).map(|a| a.path.as_path())
    }

    pub fn asset(&self, id: AssetId) -> Result<&Asset, ServiceError> {
        self.assets.get(&id).ok_or(ServiceError::UnknownAsset(id))
    }

    /// The paths of a selection, for the planner.
    pub fn paths(&self, ids: &[AssetId]) -> Result<Vec<PathBuf>, ServiceError> {
        ids.iter()
            .map(|&i| self.path(i).map(Path::to_path_buf))
            .collect()
    }

    /// Assets whose file changed (or vanished) since it was imported: they need a rescan.
    pub fn changed_since_import(&self) -> Vec<AssetId> {
        self.assets
            .iter()
            .filter(|(_, a)| {
                let now = crate::fingerprint(&a.path).ok();
                match (&a.fingerprint, now) {
                    (Some(then), Some(now)) => !crate::planner::same_file(&now, then),
                    _ => true,
                }
            })
            .map(|(&id, _)| id)
            .collect()
    }

    pub fn assets(&self) -> impl Iterator<Item = (AssetId, &Asset)> {
        self.assets.iter().map(|(&a, x)| (a, x))
    }
}

fn is_xmp(p: &Path) -> bool {
    p.extension()
        .is_some_and(|e| e.to_string_lossy().eq_ignore_ascii_case("xmp"))
}

/// `<name>.<ext>.xmp`: a darktable sidecar (read-only for MoriMeta).
fn is_darktable(p: &Path) -> bool {
    p.file_stem()
        .is_some_and(|s| Path::new(s).extension().is_some())
}

fn stem_key(p: &Path) -> (PathBuf, String) {
    (
        p.parent().map(Path::to_path_buf).unwrap_or_default(),
        p.file_stem()
            .map(|s| s.to_string_lossy().to_lowercase())
            .unwrap_or_default(),
    )
}

/// Which entries of a Plan a Preview page shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryFilter {
    All,
    /// Will be written.
    Ready,
    NoChange,
    Blocked,
    Unsupported,
    Excluded,
}

impl EntryFilter {
    fn matches(self, e: &PlanEntry) -> bool {
        if e.excluded {
            return matches!(self, EntryFilter::All | EntryFilter::Excluded);
        }
        match self {
            EntryFilter::All => true,
            EntryFilter::Ready => e.status == EntryStatus::Ready,
            EntryFilter::NoChange => e.status == EntryStatus::NoChange,
            EntryFilter::Blocked => matches!(e.status, EntryStatus::Blocked(_)),
            EntryFilter::Unsupported => matches!(e.status, EntryStatus::Unsupported(_)),
            EntryFilter::Excluded => false,
        }
    }
}

/// Preview rows per page (ARCHITECTURE §8.3).
pub const PAGE_SIZE: usize = 200;

#[derive(Debug)]
pub struct PlanPage<'a> {
    pub version: u32,
    pub summary: PlanSummary,
    /// Entries matching the filter, over all pages.
    pub matching: usize,
    pub entries: Vec<&'a PlanEntry>,
}

struct Stored {
    /// Every version; the last is current. Older ones stay readable for the history of a Preview.
    versions: Vec<Plan>,
    /// The confirmation token of the current version, once the user confirmed it, with the
    /// acknowledgements given then.
    token: Option<(String, Vec<String>)>,
}

/// Plans awaiting Preview and confirmation. A Plan is immutable; excluding entries makes a new
/// version, and only the current version can be confirmed and executed.
#[derive(Default)]
pub struct PlanBook {
    plans: HashMap<String, Stored>,
}

impl PlanBook {
    /// Keep a Plan the planner made (or an undo Plan); returns its id and version.
    pub fn insert(&mut self, plan: Plan) -> (String, u32) {
        let key = (plan.id.clone(), plan.version);
        self.plans.insert(
            plan.id.clone(),
            Stored {
                versions: vec![plan],
                token: None,
            },
        );
        key
    }

    fn stored(&self, id: &str) -> Result<&Stored, ServiceError> {
        self.plans
            .get(id)
            .ok_or_else(|| ServiceError::UnknownPlan(id.to_owned()))
    }

    fn current_mut(&mut self, id: &str, version: u32) -> Result<&mut Stored, ServiceError> {
        let s = self
            .plans
            .get_mut(id)
            .ok_or_else(|| ServiceError::UnknownPlan(id.to_owned()))?;
        let current = s.versions.last().map_or(0, |p| p.version);
        if version != current {
            return Err(ServiceError::StalePlan { current });
        }
        Ok(s)
    }

    pub fn current(&self, id: &str) -> Result<&Plan, ServiceError> {
        self.stored(id)?
            .versions
            .last()
            .ok_or_else(|| ServiceError::UnknownPlan(id.to_owned()))
    }

    pub fn version(&self, id: &str, version: u32) -> Result<&Plan, ServiceError> {
        let s = self.stored(id)?;
        s.versions
            .iter()
            .find(|p| p.version == version)
            .ok_or(ServiceError::StalePlan {
                current: s.versions.last().map_or(0, |p| p.version),
            })
    }

    /// Page `page` (from 0) of the entries of `version` that match `filter`, in plan order.
    pub fn page(
        &self,
        id: &str,
        version: u32,
        filter: EntryFilter,
        page: usize,
        size: usize,
    ) -> Result<PlanPage<'_>, ServiceError> {
        let plan = self.version(id, version)?;
        let size = size.max(1);
        let matching: Vec<&PlanEntry> = plan.entries.iter().filter(|e| filter.matches(e)).collect();
        Ok(PlanPage {
            version,
            summary: plan.summary(),
            matching: matching.len(),
            entries: matching.into_iter().skip(page * size).take(size).collect(),
        })
    }

    /// Exclude (or include again) entries of the current version; returns the new version.
    /// Unknown sequence numbers are an error so that a stale selection cannot silently do nothing.
    pub fn exclude(
        &mut self,
        id: &str,
        version: u32,
        seqs: &[u32],
        excluded: bool,
    ) -> Result<u32, ServiceError> {
        let s = self.current_mut(id, version)?;
        let mut next = s
            .versions
            .last()
            .cloned()
            .expect("a stored plan has a version");
        for &q in seqs {
            let e = next
                .entries
                .iter_mut()
                .find(|e| e.seq == q)
                .ok_or_else(|| CoreError::Input(format!("plan has no entry {q}")))?;
            e.excluded = excluded;
        }
        next.version += 1;
        let v = next.version;
        s.versions.push(next);
        s.token = None;
        Ok(v)
    }

    /// Pre-flight of a version (`plan_preflight`): files changed since the Preview, backup
    /// location, space, ExifTool. Apply stays blocked while it is not ok (INTERACTION_SPEC §4).
    pub fn preflight(
        &self,
        store: &Store,
        id: &str,
        version: u32,
    ) -> Result<crate::preflight::Preflight, ServiceError> {
        Ok(crate::preflight::preflight(
            store,
            self.version(id, version)?,
        )?)
    }

    /// The user confirmed `version` in Preview: a token that allows executing exactly it, once.
    pub fn confirm(&mut self, id: &str, version: u32) -> Result<String, ServiceError> {
        self.confirm_with(id, version, &[])
    }

    /// Confirm with the acknowledgements the dialog collected; every one the Plan requires
    /// ([`Plan::required_acks`]) must be among them. They are recorded with the Operation.
    pub fn confirm_with(
        &mut self,
        id: &str,
        version: u32,
        acks: &[String],
    ) -> Result<String, ServiceError> {
        let s = self.current_mut(id, version)?;
        let plan = s.versions.last().expect("a stored plan has a version");
        let missing: Vec<String> = plan
            .required_acks()
            .into_iter()
            .filter(|a| !acks.contains(a))
            .collect();
        if !missing.is_empty() {
            return Err(ServiceError::NotAcknowledged(missing));
        }
        let t = format!("{}{}", mm_fs::random_token()?, mm_fs::random_token()?);
        s.token = Some((t.clone(), acks.to_vec()));
        Ok(t)
    }

    /// Hand out the confirmed version for execution and use up its token.
    fn take(
        &mut self,
        id: &str,
        version: u32,
        token: &str,
    ) -> Result<(Plan, Vec<String>), ServiceError> {
        let s = self.current_mut(id, version)?;
        let acks = match &s.token {
            Some((t, acks)) if t == token => acks.clone(),
            _ => return Err(ServiceError::NotConfirmed),
        };
        s.token = None;
        Ok((
            s.versions
                .last()
                .cloned()
                .expect("a stored plan has a version"),
            acks,
        ))
    }

    /// Execute the confirmed version under the write permit of `gate` (`op_execute`).
    #[allow(clippy::too_many_arguments)]
    pub fn execute(
        &mut self,
        gate: &OperationGate,
        store: &mut Store,
        engines: &mut [Engine],
        id: &str,
        version: u32,
        token: &str,
        opts: &ExecOptions,
    ) -> Result<OpReport, ServiceError> {
        let _permit = gate.write()?;
        let (plan, acks) = self.take(id, version, token)?;
        let opts = ExecOptions {
            acks,
            ..opts.clone()
        };
        Ok(executor::start(store, engines, &plan, &opts)?)
    }
}

/// What the app learns at launch, before any write (SAFETY_MODEL §10, INTERACTION_SPEC §13).
#[derive(Debug)]
pub struct Startup {
    /// Files settled by crash recovery, per Operation.
    pub recovered: Vec<crate::recovery::RecoveryReport>,
    /// Prunes that were interrupted and are now finished.
    pub prunes_finished: Vec<String>,
    /// Operations that still ask for a decision (the Recovery dialog).
    pub needs_decision: Vec<crate::recovery::RecoverySummary>,
    /// Running with administrator rights: every write is refused.
    pub elevated: bool,
    /// The backup location is not usable: every write is refused until it is.
    pub backup_problem: Option<String>,
}

/// Launch sequence: crash recovery, unfinished prunes, then what the user must decide. Recovery
/// itself writes (it may put an original back from its bak name), so it is skipped when running
/// elevated; it then happens at the next normal launch.
pub fn startup(store: &mut Store) -> Result<Startup, ServiceError> {
    let elevated = mm_fs::is_elevated().unwrap_or(true);
    let (recovered, prunes_finished) = if elevated {
        (vec![], vec![])
    } else {
        (
            crate::recovery::recover(store)?,
            crate::retention::finish_interrupted(store)?,
        )
    };
    Ok(Startup {
        recovered,
        prunes_finished,
        needs_decision: crate::recovery::summary(store)?,
        elevated,
        backup_problem: crate::executor::check_backup_location(store)
            .err()
            .map(|e| e.to_string()),
    })
}

/// "Clear read-only attribute…" (INTERACTION_SPEC §7, SAFETY_MODEL §8.1): MoriMeta never clears
/// the attribute by itself; this is the user's explicit, separate action, taken under the write
/// gate and written to the log. Returns whether the attribute was set.
pub fn clear_read_only(gate: &OperationGate, path: &Path) -> Result<bool, ServiceError> {
    let _permit = gate.write()?;
    let meta = std::fs::metadata(path)?;
    let mut perm = meta.permissions();
    if !perm.readonly() {
        return Ok(false);
    }
    #[allow(clippy::permissions_set_readonly_false)]
    // Windows: clears FILE_ATTRIBUTE_READONLY only
    perm.set_readonly(false);
    std::fs::set_permissions(path, perm)?;
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    crate::log::event(
        "info",
        "read-only attribute cleared by the user",
        &[("ext", &ext)],
    );
    Ok(true)
}

#[derive(Debug, Default)]
struct GateState {
    writers: usize,
    exclusive: bool,
}

/// Shared by writes (Apply, Undo, Recovery, Export) and the update installer (ARCHITECTURE
/// §5.1a): one write runs at a time; installing takes it exclusively, only when no write runs and
/// no Operation awaits recovery, and no write starts until it is released.
#[derive(Debug, Default)]
pub struct OperationGate {
    state: Mutex<GateState>,
    /// The process has administrator rights: every write is refused.
    elevated: bool,
}

pub struct WritePermit<'a>(&'a OperationGate);
pub struct ExclusivePermit<'a>(&'a OperationGate);

impl OperationGate {
    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// The gate of this process: writes are refused if it runs with administrator rights (or
    /// if that cannot be determined).
    pub fn for_this_process() -> OperationGate {
        OperationGate {
            elevated: mm_fs::is_elevated().unwrap_or(true),
            ..Default::default()
        }
    }

    pub fn write(&self) -> Result<WritePermit<'_>, ServiceError> {
        if self.elevated {
            return Err(ServiceError::Elevated);
        }
        let mut s = self.lock();
        if s.exclusive {
            return Err(ServiceError::Busy("an update is being installed".into()));
        }
        // one write Operation at a time (PRODUCT_SPEC §6.13: no new write while one runs)
        if s.writers > 0 {
            return Err(ServiceError::Busy("an Operation is already running".into()));
        }
        s.writers += 1;
        Ok(WritePermit(self))
    }

    pub fn exclusive(&self, store: &Store) -> Result<ExclusivePermit<'_>, ServiceError> {
        let mut s = self.lock();
        if s.exclusive {
            return Err(ServiceError::Busy("an update is being installed".into()));
        }
        if s.writers > 0 {
            return Err(ServiceError::Busy("an Operation is running".into()));
        }
        let pending = store.unfinished()?;
        if !pending.is_empty() {
            return Err(ServiceError::Core(CoreError::RecoveryPending(pending)));
        }
        s.exclusive = true;
        Ok(ExclusivePermit(self))
    }
}

impl Drop for WritePermit<'_> {
    fn drop(&mut self) {
        self.0.lock().writers -= 1;
    }
}

impl Drop for ExclusivePermit<'_> {
    fn drop(&mut self) {
        self.0.lock().exclusive = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mm_domain::plan::{Fingerprint, PlanKind};

    fn plan(n: u32) -> Plan {
        let entry = |seq: u32| PlanEntry {
            seq,
            path: format!("C:\\p\\{seq}.jpg"),
            raw: None,
            fingerprint: Fingerprint {
                size: 1,
                file_id: String::new(),
                mtime: 0,
            },
            status: match seq % 3 {
                0 => EntryStatus::Ready,
                1 => EntryStatus::NoChange,
                _ => EntryStatus::Blocked("x".into()),
            },
            changes: vec![],
            action: None,
            notes: vec![],
            excluded: false,
        };
        Plan {
            id: "plan-t".into(),
            version: 1,
            kind: PlanKind::Apply,
            title: "t".into(),
            registry_version: 0,
            exiftool_version: "13.59".into(),
            entries: (0..n).map(entry).collect(),
            source: None,
        }
    }

    #[test]
    fn pages_follow_the_filter() {
        let mut b = PlanBook::default();
        let (id, v) = b.insert(plan(10));
        let p = b.page(&id, v, EntryFilter::Ready, 0, 2).unwrap();
        assert_eq!(p.matching, 4); // 0, 3, 6, 9
        assert_eq!(p.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), [0, 3]);
        let p = b.page(&id, v, EntryFilter::Ready, 1, 2).unwrap();
        assert_eq!(p.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), [6, 9]);
        assert!(
            b.page(&id, v, EntryFilter::Ready, 2, 2)
                .unwrap()
                .entries
                .is_empty()
        );
        assert_eq!(
            b.page(&id, v, EntryFilter::All, 0, 200).unwrap().matching,
            10
        );
        assert!(matches!(
            b.page("nope", 1, EntryFilter::All, 0, 1),
            Err(ServiceError::UnknownPlan(_))
        ));
    }

    #[test]
    fn exclusion_makes_a_new_version_and_old_ones_go_stale() {
        let mut b = PlanBook::default();
        let (id, v1) = b.insert(plan(6));
        let v2 = b.exclude(&id, v1, &[0, 3], true).unwrap();
        assert_eq!(v2, 2);
        let p = b.page(&id, v2, EntryFilter::Excluded, 0, 10).unwrap();
        assert_eq!(p.entries.iter().map(|e| e.seq).collect::<Vec<_>>(), [0, 3]);
        assert_eq!(p.summary.excluded, 2);
        assert_eq!(p.summary.ready, 0);
        assert_eq!(b.current(&id).unwrap().executable().count(), 0);
        // v1 still reads as it was, but can no longer be changed or confirmed
        assert_eq!(
            b.page(&id, v1, EntryFilter::Ready, 0, 10).unwrap().matching,
            2
        );
        assert!(matches!(
            b.exclude(&id, v1, &[1], true),
            Err(ServiceError::StalePlan { current: 2 })
        ));
        assert!(matches!(
            b.confirm(&id, v1),
            Err(ServiceError::StalePlan { current: 2 })
        ));
        assert!(b.exclude(&id, v2, &[99], true).is_err());
        let v3 = b.exclude(&id, v2, &[3], false).unwrap();
        assert_eq!(
            b.page(&id, v3, EntryFilter::Ready, 0, 10).unwrap().matching,
            1
        );
    }

    #[test]
    fn tokens_are_bound_to_one_version_and_used_once() {
        let mut b = PlanBook::default();
        let (id, v1) = b.insert(plan(3));
        let t1 = b.confirm(&id, v1).unwrap();
        assert!(matches!(
            b.take(&id, v1, "forged"),
            Err(ServiceError::NotConfirmed)
        ));
        // a new version invalidates the token of the previous one
        let v2 = b.exclude(&id, v1, &[1], true).unwrap();
        assert!(matches!(
            b.take(&id, v1, &t1),
            Err(ServiceError::StalePlan { .. })
        ));
        assert!(matches!(
            b.take(&id, v2, &t1),
            Err(ServiceError::NotConfirmed)
        ));
        let t2 = b.confirm(&id, v2).unwrap();
        assert_eq!(b.take(&id, v2, &t2).unwrap().0.version, 2);
        assert!(matches!(
            b.take(&id, v2, &t2),
            Err(ServiceError::NotConfirmed)
        ));
    }

    #[test]
    fn high_risk_plans_need_their_acknowledgements() {
        use mm_domain::plan::{ChangeKind, EntryAction, FieldChange};
        let mut p = plan(3);
        p.entries[0].action = Some(EntryAction::Write {
            ops: vec![],
            expect: vec![],
        });
        p.entries[0].changes.push(FieldChange {
            field: "gps".into(),
            before: Some(vec!["1, 2".into()]),
            after: None,
            kind: ChangeKind::Remove,
        });
        assert_eq!(p.required_acks(), ["remove:gps"]);
        let mut b = PlanBook::default();
        let (id, v) = b.insert(p);
        assert!(matches!(
            b.confirm(&id, v),
            Err(ServiceError::NotAcknowledged(m)) if m == ["remove:gps"]
        ));
        let t = b.confirm_with(&id, v, &["remove:gps".into()]).unwrap();
        assert_eq!(b.take(&id, v, &t).unwrap().1, ["remove:gps"]);
    }

    #[test]
    fn gate_writes_share_and_installing_excludes() {
        let dir = std::env::temp_dir().join(format!("mm-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let g = OperationGate::default();
        let w1 = g.write().unwrap();
        assert!(
            matches!(g.write(), Err(ServiceError::Busy(_))),
            "one write at a time"
        );
        assert!(matches!(g.exclusive(&store), Err(ServiceError::Busy(_))));
        drop(w1);
        let x = g.exclusive(&store).unwrap();
        assert!(matches!(g.write(), Err(ServiceError::Busy(_))));
        assert!(matches!(g.exclusive(&store), Err(ServiceError::Busy(_))));
        drop(x);
        drop(g.write().unwrap());
        let elevated = OperationGate {
            elevated: true,
            ..Default::default()
        };
        assert!(matches!(elevated.write(), Err(ServiceError::Elevated)));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_import_takes_photos_and_marks_read_only_formats() {
        let dir = std::env::temp_dir().join(format!("mm-folder-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("raw")).unwrap();
        for n in [
            "a.JPG",
            "b.jpeg",
            "notes.txt",
            "a.mmtmp-0123456789abcdef.JPG",
            "a.mmbak-0123456789abcdef.JPG",
            "raw/c.NEF",
            "raw/c.xmp",
            "raw/d.png",
            "raw/e.CR3",
        ] {
            std::fs::write(dir.join(n), n).unwrap();
        }
        let mut s = Session::default();
        let r = s.import_folder(&dir);
        let names: Vec<(String, bool)> = s
            .assets()
            .map(|(_, a)| {
                let n = a.path.file_name().unwrap().to_string_lossy().into_owned();
                (n, a.writable)
            })
            .collect();
        let want = [
            ("a.JPG", true),
            ("b.jpeg", true),
            ("c.NEF", true),
            ("d.png", false),
            ("e.CR3", false),
        ];
        assert_eq!(names, want.map(|(n, w)| (n.to_string(), w)), "{r:?}");
        assert_eq!(r.read_only.len(), 2);
        assert!(r.orphan_sidecars.is_empty(), "c.xmp belongs to c.NEF");
        assert_eq!(r.other_files, 4); // notes.txt, c.xmp and the two transaction names
        // importing the folder again adds nothing
        let again = s.import_folder(&dir);
        assert!(again.added.is_empty());
        assert_eq!(again.duplicates.len(), 5);
        // picked on its own, an XMP file is writable; a text file is refused
        let picked = s.import(&[dir.join("raw/c.xmp"), dir.join("notes.txt")]);
        assert_eq!(picked.added.len(), 1);
        assert!(s.asset(picked.added[0]).unwrap().writable);
        assert_eq!(picked.failed.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn orphan_sidecars_and_system_folders() {
        let dir = std::env::temp_dir().join(format!("mm-orphan-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("$RECYCLE.BIN")).unwrap();
        for n in [
            "a.NEF",
            "a.xmp",
            "lonely.xmp",
            "b.NEF.xmp",
            "$RECYCLE.BIN/x.jpg",
        ] {
            std::fs::write(dir.join(n), n).unwrap();
        }
        let mut s = Session::default();
        let r = s.import_folder(&dir);
        let names: Vec<String> = r
            .orphan_sidecars
            .iter()
            .map(|&a| {
                s.path(a)
                    .unwrap()
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, ["lonely.xmp"], "{r:?}");
        assert!(s.asset(r.orphan_sidecars[0]).unwrap().writable);
        assert_eq!(r.added.len(), 2, "a.NEF and lonely.xmp: {r:?}");
        assert_eq!(r.skipped_folders.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changed_since_import() {
        let dir = std::env::temp_dir().join(format!("mm-changed-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for n in ["a.jpg", "b.jpg", "c.jpg"] {
            std::fs::write(dir.join(n), n).unwrap();
        }
        let mut s = Session::default();
        let r = s.import_folder(&dir);
        assert!(s.changed_since_import().is_empty());
        std::fs::write(dir.join("b.jpg"), "b, changed").unwrap();
        std::fs::remove_file(dir.join("c.jpg")).unwrap();
        assert_eq!(s.changed_since_import(), [r.added[1], r.added[2]]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn session_dedups_by_file_identity() {
        let dir = std::env::temp_dir().join(format!("mm-session-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        let a = dir.join("a.jpg");
        std::fs::write(&a, b"a").unwrap();
        std::fs::write(dir.join("b.jpg"), b"b").unwrap();
        let mut s = Session::default();
        let r = s.import(&[a.clone(), dir.join("b.jpg"), dir.join("missing.jpg")]);
        assert_eq!(r.added.len(), 2);
        assert_eq!(r.failed.len(), 1);
        // the same file by another spelling of its path
        let r2 = s.import(&[dir.join("sub").join("..").join("a.jpg")]);
        assert_eq!(r2.duplicates, [r.added[0]]);
        assert!(r2.added.is_empty());
        assert_eq!(s.paths(&r.added).unwrap().len(), 2);
        assert!(matches!(
            s.path(AssetId(99)),
            Err(ServiceError::UnknownAsset(_))
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
