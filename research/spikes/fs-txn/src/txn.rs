//! Crash-injectable single-file transaction + journal + recovery + invariant checker (S2 prototype).
//!
//! Per file (candidate protocol for SAFETY_MODEL v0.3 §4.1):
//!   J planned → lock(original: READ, deny WRITE) → fingerprint check → copy original→backup via
//!   the lock handle (hash H0) → re-read backup == H0 → J backed_up → ExifTool writes temp from
//!   <source> (backup copy or original path) → verify temp → flush temp, H1 → J ready (fsync) →
//!   identity check (path File ID == lock File ID) → commit (ReplaceFileW or POSIX rename) →
//!   J committed → close lock → post-check original == H1 → remove bak → J done.

use std::collections::BTreeMap;
use std::fs::OpenOptions;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use exiftool_session::{Session, arg_path, encode};
use serde_json::{Value, json};

use crate::*;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Source {
    Backup,
    Original,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Commit {
    ReplaceFileW,
    PosixRename,
}

pub struct Journal {
    path: PathBuf,
}

impl Journal {
    pub fn new(path: PathBuf) -> Journal {
        Journal { path }
    }
    /// Append one JSON line and FlushFileBuffers (stand-in for SQLite synchronous=FULL).
    pub fn append(&self, v: &Value) -> io::Result<()> {
        let mut f = OpenOptions::new().create(true).append(true).open(&self.path)?;
        let mut line = serde_json::to_string(v).unwrap();
        line.push('\n');
        f.write_all(line.as_bytes())?;
        flush(&f)
    }
    pub fn read(&self) -> Vec<Value> {
        std::fs::read_to_string(&self.path)
            .unwrap_or_default()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok()) // a torn last line is ignored
            .collect()
    }
}

#[derive(Clone)]
pub struct PlanItem {
    pub seq: usize,
    pub path: PathBuf,
    pub temp: PathBuf,
    pub bak: PathBuf,
    pub backup: PathBuf,
    pub size: u64,
    pub file_id: FileId,
    pub value: String,
}

/// Crash hook: terminate the process immediately (no destructors, no unwinding, no WER dialog).
pub fn crash_here() -> ! {
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};
    unsafe {
        TerminateProcess(GetCurrentProcess(), 77);
    }
    unreachable!()
}

pub struct Ctx<'a> {
    pub session: &'a mut Session,
    pub journal: &'a Journal,
    pub source: Source,
    pub commit: Commit,
    /// (seq, crash point) at which to terminate the process
    pub crash_at: Option<(usize, u8)>,
}

impl Ctx<'_> {
    fn cp(&self, seq: usize, point: u8) {
        if self.crash_at == Some((seq, point)) {
            crash_here();
        }
    }
}

pub fn plan(files: &[PathBuf], backup_dir: &Path, value: &str) -> io::Result<Vec<PlanItem>> {
    let mut out = vec![];
    for (i, p) in files.iter().enumerate() {
        let f = open_attr(p)?;
        let size = std::fs::metadata(p)?.len();
        let r: u32 = rand::random();
        out.push(PlanItem {
            seq: i,
            path: p.clone(),
            temp: sibling(p, &format!(".mmtmp-{r:08x}")),
            bak: sibling(p, &format!(".mmbak-{r:08x}")),
            backup: backup_dir.join(format!("{:08}.{}", i + 1, p.extension().unwrap().to_string_lossy())),
            size,
            file_id: file_id(&f)?,
            value: value.to_owned(),
        });
    }
    Ok(out)
}

fn json_first(v: &[u8]) -> Value {
    serde_json::from_slice::<Value>(v).ok().and_then(|a| a.get(0).cloned()).unwrap_or(Value::Null)
}

/// Returns Ok(state) where state is "done" or a failure/skip reason (original untouched).
pub fn run_one(ctx: &mut Ctx, it: &PlanItem) -> io::Result<String> {
    let seq = it.seq;
    ctx.cp(seq, 1);
    // lock + fingerprint
    let mut lock = match open_lock(&it.path) {
        Ok(f) => f,
        Err(e) => {
            ctx.journal.append(&json!({"t":"failed","seq":seq,"reason":format!("lock: {e}")}))?;
            return Ok(format!("skipped: lock {e}"));
        }
    };
    if file_id(&lock)? != it.file_id || lock.metadata()?.len() != it.size {
        ctx.journal.append(&json!({"t":"failed","seq":seq,"reason":"fingerprint"}))?;
        return Ok("conflict: fingerprint".into());
    }
    ctx.cp(seq, 2);
    let h0 = copy_new_hashing(&mut lock, &it.backup)?;
    ctx.cp(seq, 3);
    if hash_path(&it.backup)? != h0 {
        ctx.journal.append(&json!({"t":"failed","seq":seq,"reason":"backup verify"}))?;
        return Ok("failed: backup verify".into());
    }
    ctx.journal.append(&json!({"t":"backed_up","seq":seq,"h0":hex(&h0)}))?;
    ctx.cp(seq, 4);
    let src = match ctx.source {
        Source::Backup => &it.backup,
        Source::Original => &it.path,
    };
    let w = ctx
        .session
        .execute(
            &[
                "-ex".into(),
                encode::assign_xml("XMP-dc:Creator", &it.value).unwrap(),
                "-o".into(),
                arg_path(&it.temp),
                arg_path(src),
            ],
            Duration::from_secs(60),
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
    ctx.cp(seq, 5);
    if w.status != 0 || !exists(&it.temp) {
        let _ = std::fs::remove_file(&it.temp);
        ctx.journal.append(&json!({"t":"failed","seq":seq,"reason":format!("engine: {}", w.err().trim())}))?;
        return Ok("failed: engine".into());
    }
    // verify (simplified V1/V2/V4): target value present; ImageDataHash unchanged vs source
    let r = ctx
        .session
        .execute(
            &[
                "-json".into(), "-api".into(), "StructFormat=JSONQ".into(), "-api".into(), "ImageHashType=SHA256".into(),
                "-G1".into(), "-XMP-dc:Creator".into(), "-ImageDataHash".into(), arg_path(&it.temp), arg_path(src),
            ],
            Duration::from_secs(60),
        )
        .map_err(|e| io::Error::other(e.to_string()))?;
    let v: Value = serde_json::from_slice(&r.stdout).unwrap_or(Value::Null);
    let (t, s) = (&v[0], &v[1]);
    let creator_ok = t.get("XMP-dc:Creator").map(|c| c == &json!(it.value) || c == &json!([it.value])).unwrap_or(false);
    let hash_of = |o: &Value| o.as_object().and_then(|m| m.iter().find(|(k, _)| k.ends_with("ImageDataHash")).map(|(_, v)| v.clone()));
    let pixels_ok = hash_of(t).is_some() && hash_of(t) == hash_of(s);
    if !(creator_ok && pixels_ok) {
        let _ = std::fs::remove_file(&it.temp);
        ctx.journal.append(&json!({"t":"failed","seq":seq,"reason":"verify"}))?;
        return Ok("failed: verify".into());
    }
    flush_path(&it.temp)?;
    let h1 = hash_path(&it.temp)?;
    ctx.cp(seq, 6);
    ctx.journal.append(&json!({"t":"ready","seq":seq,"h0":hex(&h0),"h1":hex(&h1)}))?;
    ctx.cp(seq, 7);
    // identity: the path must still name the file we hold
    if file_id_of_path(&it.path)? != file_id(&lock)? {
        let _ = std::fs::remove_file(&it.temp);
        ctx.journal.append(&json!({"t":"failed","seq":seq,"reason":"identity changed"}))?;
        return Ok("conflict: identity".into());
    }
    let res = match ctx.commit {
        Commit::ReplaceFileW => replace_file(&it.path, &it.temp, &it.bak),
        Commit::PosixRename => posix_replace(&it.temp, &it.path),
    };
    if let Err(code) = res {
        // leave recovery to classify on-disk state (it is exactly what the journal predicts)
        ctx.journal.append(&json!({"t":"commit_error","seq":seq,"code":code}))?;
        let st = recover_item(it, &ctx.journal.read());
        return Ok(format!("failed: commit {code} -> {st}"));
    }
    ctx.cp(seq, 8);
    ctx.journal.append(&json!({"t":"committed","seq":seq}))?;
    ctx.cp(seq, 9);
    drop(lock);
    if hash_path(&it.path)? != h1 {
        ctx.journal.append(&json!({"t":"attention","seq":seq,"reason":"post-check"}))?;
        return Ok("attention: post-check".into());
    }
    if exists(&it.bak) {
        if hash_path(&it.bak)? == h0 {
            std::fs::remove_file(&it.bak)?;
        }
    }
    ctx.cp(seq, 10);
    ctx.journal.append(&json!({"t":"done","seq":seq}))?;
    Ok("done".into())
}

pub fn write_plan(journal: &Journal, items: &[PlanItem]) -> io::Result<()> {
    // one record for the whole plan (single flush), like the single SQLite transaction in §9.3
    let v: Vec<Value> = items
        .iter()
        .map(|it| {
            json!({"seq":it.seq,"path":it.path,"temp":it.temp,"bak":it.bak,"backup":it.backup,
                   "size":it.size,"fid":it.file_id.hex(),"value":it.value})
        })
        .collect();
    journal.append(&json!({"t":"plan","items":v}))
}

pub fn load_plan(records: &[Value]) -> Vec<PlanItem> {
    let Some(p) = records.iter().find(|r| r["t"] == "plan") else { return vec![] };
    p["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| PlanItem {
            seq: i["seq"].as_u64().unwrap() as usize,
            path: PathBuf::from(i["path"].as_str().unwrap()),
            temp: PathBuf::from(i["temp"].as_str().unwrap()),
            bak: PathBuf::from(i["bak"].as_str().unwrap()),
            backup: PathBuf::from(i["backup"].as_str().unwrap()),
            size: i["size"].as_u64().unwrap(),
            file_id: FileId { volume: 0, id: [0; 16] },
            value: i["value"].as_str().unwrap().to_owned(),
        })
        .collect()
}

fn hash_opt(p: &Path) -> Option<String> {
    hash_path(p).ok().map(|h| hex(&h))
}

/// Recovery for one file, from journal + observed disk state. Only registered names are touched,
/// and a registered file is removed only after its hash is checked.
pub fn recover_item(it: &PlanItem, records: &[Value]) -> String {
    let mine: Vec<&Value> = records.iter().filter(|r| r["seq"].as_u64() == Some(it.seq as u64)).collect();
    let last = |t: &str| mine.iter().rev().find(|r| r["t"] == t).cloned();
    let ready = last("ready");
    let h0 = ready.and_then(|r| r["h0"].as_str()).or_else(|| last("backed_up").and_then(|r| r["h0"].as_str())).map(str::to_owned);
    let h1 = ready.and_then(|r| r["h1"].as_str()).map(str::to_owned);
    let cur = hash_opt(&it.path);
    let bak = hash_opt(&it.bak);
    let rm_if = |p: &Path, want: &Option<String>| {
        if let (Some(w), Some(h)) = (want, hash_opt(p)) {
            if &h == w {
                let _ = std::fs::remove_file(p);
            }
        }
    };
    if mine.iter().any(|r| r["t"] == "done") {
        rm_if(&it.bak, &h0);
        return "done".into();
    }
    match (&h1, &cur) {
        (Some(h1v), Some(c)) if c == h1v => {
            // committed
            rm_if(&it.bak, &h0);
            let _ = std::fs::remove_file(&it.temp); // temp name no longer exists after commit
            "recovered: committed".into()
        }
        (_, Some(c)) if Some(c) == h0.as_ref() || h0.is_none() => {
            // not committed (h0 unknown only before backup verification: original untouched by protocol)
            if exists(&it.temp) {
                let _ = std::fs::remove_file(&it.temp); // temp holds no user data
            }
            "recovered: not started".into()
        }
        (_, None) if bak.is_some() && bak == h0 => {
            // mid-replace: original moved to bak, replacement not moved in
            match move_no_replace(&it.bak, &it.path) {
                Ok(()) => {
                    let _ = std::fs::remove_file(&it.temp);
                    "recovered: restored from bak".into()
                }
                Err(e) => format!("attention: bak restore failed {e}"),
            }
        }
        _ => "attention: unexpected state".into(),
    }
}

pub fn recover(journal: &Journal) -> BTreeMap<usize, String> {
    let recs = journal.read();
    let items = load_plan(&recs);
    items.iter().map(|it| (it.seq, recover_item(it, &recs))).collect()
}

/// I-3: at least one intact pre-image exists (orig, bak, or verified backup) — checked BEFORE recovery.
/// Post-recovery: original is H0 or journal H1; no temp/bak leftovers; original path exists.
pub fn check_invariants(items: &[PlanItem], truth_h0: &BTreeMap<usize, String>, journal: &Journal, phase: &str) -> Vec<String> {
    let recs = journal.read();
    let mut v = vec![];
    for it in items {
        let h0 = &truth_h0[&it.seq];
        let cands = [hash_opt(&it.path), hash_opt(&it.bak), hash_opt(&it.backup)];
        if !cands.iter().any(|c| c.as_ref() == Some(h0)) {
            v.push(format!("I-3 seq {}: no intact pre-image", it.seq));
        }
        if phase == "after" {
            let h1 = recs.iter().rev().find(|r| r["seq"].as_u64() == Some(it.seq as u64) && r["t"] == "ready").and_then(|r| r["h1"].as_str()).map(str::to_owned);
            match hash_opt(&it.path) {
                None => v.push(format!("seq {}: original path missing after recovery", it.seq)),
                Some(c) if &c == h0 || Some(&c) == h1.as_ref() => {}
                Some(_) => v.push(format!("seq {}: original is neither H0 nor H1", it.seq)),
            }
            if exists(&it.temp) {
                v.push(format!("seq {}: temp leftover", it.seq));
            }
            if exists(&it.bak) {
                v.push(format!("seq {}: bak leftover", it.seq));
            }
        }
    }
    v
}
