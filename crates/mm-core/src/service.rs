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
    /// The OperationGate refused: a write or an update installation is in progress.
    Busy(String),
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
    /// Already in the Session (same volume and File ID, SAFETY_MODEL §8.8), possibly by another
    /// path: the id it already has.
    pub duplicates: Vec<AssetId>,
    pub failed: Vec<(String, String)>,
    /// Folder import only: files of other formats (and XMP files, which come with their RAW).
    pub other_files: usize,
    /// Folder import only: directory links not entered (§8.6).
    pub not_followed: Vec<String>,
    /// Folder import only: cloud files not on this computer, not read (§8.3).
    pub placeholders: Vec<String>,
}

/// The files the user brought in. Paths come only from the backend's own dialogs and drop events;
/// the frontend receives them for display.
#[derive(Debug, Default)]
pub struct Session {
    next: u64,
    assets: BTreeMap<AssetId, PathBuf>,
    by_identity: HashMap<String, AssetId>,
}

impl Session {
    pub fn import(&mut self, paths: &[PathBuf]) -> ImportReport {
        let mut r = ImportReport::default();
        for p in paths {
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
                    self.assets.insert(a, abs);
                    self.by_identity.insert(identity, a);
                    r.added.push(a);
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
        let (take, other): (Vec<PathBuf>, Vec<PathBuf>) = w
            .files
            .into_iter()
            .partition(|p| crate::planner::importable(p));
        let mut r = self.import(&take);
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
        self.assets
            .get(&id)
            .map(PathBuf::as_path)
            .ok_or(ServiceError::UnknownAsset(id))
    }

    /// The paths of a selection, for the planner.
    pub fn paths(&self, ids: &[AssetId]) -> Result<Vec<PathBuf>, ServiceError> {
        ids.iter()
            .map(|&i| self.path(i).map(Path::to_path_buf))
            .collect()
    }

    pub fn assets(&self) -> impl Iterator<Item = (AssetId, &Path)> {
        self.assets.iter().map(|(&a, p)| (a, p.as_path()))
    }
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
    /// The confirmation token of the current version, once the user confirmed it.
    token: Option<String>,
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

    /// The user confirmed `version` in Preview: a token that allows executing exactly it, once.
    pub fn confirm(&mut self, id: &str, version: u32) -> Result<String, ServiceError> {
        let s = self.current_mut(id, version)?;
        let t = format!("{}{}", mm_fs::random_token()?, mm_fs::random_token()?);
        s.token = Some(t.clone());
        Ok(t)
    }

    /// Hand out the confirmed version for execution and use up its token.
    fn take(&mut self, id: &str, version: u32, token: &str) -> Result<Plan, ServiceError> {
        let s = self.current_mut(id, version)?;
        if s.token.as_deref() != Some(token) {
            return Err(ServiceError::NotConfirmed);
        }
        s.token = None;
        Ok(s.versions
            .last()
            .cloned()
            .expect("a stored plan has a version"))
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
        let plan = self.take(id, version, token)?;
        Ok(executor::start(store, engines, &plan, opts)?)
    }
}

#[derive(Debug, Default)]
struct GateState {
    writers: usize,
    exclusive: bool,
}

/// Shared by writes (Apply, Undo, Recovery, Export) and the update installer (ARCHITECTURE
/// §5.1a): writes run side by side; installing takes it exclusively, only when no write runs and
/// no Operation awaits recovery, and no write starts until it is released.
#[derive(Debug, Default)]
pub struct OperationGate {
    state: Mutex<GateState>,
}

pub struct WritePermit<'a>(&'a OperationGate);
pub struct ExclusivePermit<'a>(&'a OperationGate);

impl OperationGate {
    fn lock(&self) -> std::sync::MutexGuard<'_, GateState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    pub fn write(&self) -> Result<WritePermit<'_>, ServiceError> {
        let mut s = self.lock();
        if s.exclusive {
            return Err(ServiceError::Busy("an update is being installed".into()));
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
        assert_eq!(b.take(&id, v2, &t2).unwrap().version, 2);
        assert!(matches!(
            b.take(&id, v2, &t2),
            Err(ServiceError::NotConfirmed)
        ));
    }

    #[test]
    fn gate_writes_share_and_installing_excludes() {
        let dir = std::env::temp_dir().join(format!("mm-gate-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = Store::open(&dir).unwrap();
        let g = OperationGate::default();
        let w1 = g.write().unwrap();
        let w2 = g.write().unwrap();
        assert!(matches!(g.exclusive(&store), Err(ServiceError::Busy(_))));
        drop((w1, w2));
        let x = g.exclusive(&store).unwrap();
        assert!(matches!(g.write(), Err(ServiceError::Busy(_))));
        assert!(matches!(g.exclusive(&store), Err(ServiceError::Busy(_))));
        drop(x);
        drop(g.write().unwrap());
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn folder_import_takes_supported_formats_only() {
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
        ] {
            std::fs::write(dir.join(n), n).unwrap();
        }
        let mut s = Session::default();
        let r = s.import_folder(&dir);
        let names: Vec<String> = s
            .assets()
            .map(|(_, p)| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["a.JPG", "b.jpeg", "c.NEF"], "{r:?}");
        assert_eq!(r.other_files, 5);
        // importing the folder again adds nothing
        let again = s.import_folder(&dir);
        assert!(again.added.is_empty());
        assert_eq!(again.duplicates.len(), 3);
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
