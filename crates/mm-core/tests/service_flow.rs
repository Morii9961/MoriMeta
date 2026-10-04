// SPDX-License-Identifier: GPL-3.0-or-later
//! The backend interface end to end with the pinned ExifTool (skipped when it is not fetched):
//! Session → Plan → Preview page → exclusion → confirmation → execution → undo through the same
//! interface. Works on copies of the ExifTool test images in a temporary folder.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mm_core::engine::Engine;
use mm_core::executor::{self, ExecOptions, ExecProgress, FaultPoint, ProgressSink};
use mm_core::planner::{self, PlanCtl, PlanProgress, PlanStage};
use mm_core::service::{EntryFilter, OperationGate, PAGE_SIZE, PlanBook, ServiceError, Session};
use mm_core::{inspect, undo};
use mm_domain::creator::CreatorEdit;
use mm_domain::plan::{EntryAction, Plan};
use mm_exiftool::EngineConfig;
use mm_store::{FileState, OpStatus, Store};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .to_path_buf()
}

fn version() -> Option<String> {
    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("research/exiftool.lock.json")).ok()?,
    )
    .ok()?;
    lock["version"].as_str().map(str::to_owned)
}

fn hash(p: &Path) -> String {
    mm_fs::hex(&mm_fs::hash_path(p).unwrap())
}

/// Copies of ExifTool test images in a temporary folder, one ExifTool session and a Store.
struct Lab {
    dir: PathBuf,
    files: Vec<PathBuf>,
    before: Vec<String>,
    engines: Vec<Engine>,
    store: Store,
}

impl Lab {
    fn new(name: &str, copies: usize) -> Option<Lab> {
        let Some(v) = version() else {
            eprintln!("SKIP: no ExifTool lock file");
            return None;
        };
        let pkg = repo().join(format!("research/.work/exiftool/{v}/win64/exiftool-{v}_64"));
        let images = repo().join(format!(
            "research/.work/exiftool/{v}/src/Image-ExifTool-{v}/t/images"
        ));
        if !pkg.join("exiftool_files").join("perl.exe").exists() || !images.exists() {
            eprintln!("SKIP: pinned ExifTool not found");
            return None;
        }
        let dir = std::env::temp_dir().join(format!("mm-service-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let photos = dir.join("photos");
        std::fs::create_dir_all(&photos).unwrap();
        let samples = ["Writer.jpg", "Nikon.jpg", "Canon.jpg"];
        let files: Vec<PathBuf> = (0..copies)
            .map(|i| {
                let n = samples[i % samples.len()];
                let p = photos.join(format!("{i:02}-{n}"));
                std::fs::copy(images.join(n), &p).unwrap();
                p
            })
            .collect();
        let before = files.iter().map(|p| hash(p)).collect();
        let run = dir.join("data").join("run");
        let cfg = EngineConfig {
            program: pkg.join("exiftool_files").join("perl.exe"),
            script: Some(pkg.join("exiftool_files").join("exiftool.pl")),
            cwd: run.join("exiftool-cwd"),
            temp: run.join("tmp"),
            max_output: mm_exiftool::MAX_OUTPUT,
        };
        std::fs::create_dir_all(&cfg.cwd).unwrap();
        std::fs::create_dir_all(&cfg.temp).unwrap();
        Some(Lab {
            engines: vec![Engine::start(cfg).unwrap()],
            store: Store::open(&dir.join("data")).unwrap(),
            dir,
            files,
            before,
        })
    }

    fn hashes(&self) -> Vec<String> {
        self.files.iter().map(|p| hash(p)).collect()
    }

    fn plan_creator(&mut self) -> Plan {
        planner::plan_creator(
            &mut self.engines[0],
            &self.files,
            &CreatorEdit::Set(vec!["Morii".into()]),
            "Creator",
            &Default::default(),
        )
        .unwrap()
    }

    fn close(self) {
        for e in self.engines {
            e.close();
        }
        drop(self.store);
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn preview_exclusion_confirmation_execution_and_undo() {
    let Some(lab) = Lab::new("flow", 3) else {
        return;
    };
    let Lab {
        dir,
        files,
        before,
        mut engines,
        mut store,
    } = lab;
    let gate = OperationGate::default();
    let mut book = PlanBook::default();

    // import: the same file twice is one asset
    let mut session = Session::default();
    let r = session.import(&[files.clone(), vec![files[0].clone()]].concat());
    assert_eq!((r.added.len(), r.duplicates.len()), (3, 1), "{r:?}");

    let plan = planner::plan_creator(
        &mut engines[0],
        &session.paths(&r.added).unwrap(),
        &CreatorEdit::Set(vec!["Morii".into()]),
        "Creator",
        &Default::default(),
    )
    .unwrap();
    let (id, v1) = book.insert(plan);
    let page = book
        .page(&id, v1, EntryFilter::Ready, 0, PAGE_SIZE)
        .unwrap();
    assert_eq!(page.matching, 3, "{page:?}");
    let excluded = page
        .entries
        .iter()
        .find(|e| e.path.ends_with("Nikon.jpg"))
        .unwrap()
        .seq;

    // a token for v1 cannot run v2, and v2 cannot run without its own token
    let t1 = book.confirm(&id, v1).unwrap();
    let v2 = book.exclude(&id, v1, &[excluded], true).unwrap();
    let opts = ExecOptions::default();
    let mut exec = |book: &mut PlanBook, ver: u32, token: &str| {
        book.execute(&gate, &mut store, &mut engines, &id, ver, token, &opts)
    };
    assert!(matches!(
        exec(&mut book, v1, &t1),
        Err(ServiceError::StalePlan { current: 2 })
    ));
    assert!(matches!(
        exec(&mut book, v2, &t1),
        Err(ServiceError::NotConfirmed)
    ));
    assert_eq!(files.iter().map(|p| hash(p)).collect::<Vec<_>>(), before);

    let t2 = book.confirm(&id, v2).unwrap();
    let rep = exec(&mut book, v2, &t2).unwrap();
    assert_eq!(rep.status, OpStatus::Completed, "{rep:?}");
    assert_eq!(rep.files.len(), 2);
    assert!(rep.files.iter().all(|f| f.state == FileState::Done));
    assert_eq!(hash(&files[1]), before[1], "the excluded file is untouched");
    assert_ne!(hash(&files[0]), before[0]);
    assert!(matches!(
        exec(&mut book, v2, &t2),
        Err(ServiceError::NotConfirmed)
    ));

    // undo is a Plan too, through the same interface
    let up = undo::plan_undo(&store, &rep.op_id, engines[0].version()).unwrap();
    let (uid, uv) = book.insert(up);
    let ut = book.confirm(&uid, uv).unwrap();
    let urep = book
        .execute(&gate, &mut store, &mut engines, &uid, uv, &ut, &opts)
        .unwrap();
    assert_eq!(urep.status, OpStatus::Completed, "{urep:?}");
    assert_eq!(files.iter().map(|p| hash(p)).collect::<Vec<_>>(), before);

    // with nothing running or pending, the installer may take the gate
    drop(gate.exclusive(&store).unwrap());
    for e in engines {
        e.close();
    }
    drop(store);
    let _ = std::fs::remove_dir_all(&dir);
}

/// ARCHITECTURE §5.3 and `op_cancel`: progress arrives once per file in completion order; Cancel
/// stops new files, the Operation ends cancelled and resumes to completion; undo restores all.
#[test]
fn progress_and_cooperative_cancel() {
    let Some(mut lab) = Lab::new("cancel", 6) else {
        return;
    };
    let plan = lab.plan_creator();
    let events = Arc::new(Mutex::new(Vec::<ExecProgress>::new()));
    let cancel = Arc::new(AtomicBool::new(false));
    let (ev, c) = (events.clone(), cancel.clone());
    let opts = ExecOptions {
        progress: Some(ProgressSink(Arc::new(move |p: &ExecProgress| {
            ev.lock().unwrap().push(p.clone());
            if p.ok == 2 {
                c.store(true, Ordering::SeqCst);
            }
        }))),
        cancel: Some(cancel.clone()),
        ..Default::default()
    };
    let rep = executor::start(&mut lab.store, &mut lab.engines, &plan, &opts).unwrap();
    assert_eq!(rep.status, OpStatus::Cancelled, "{rep:?}");
    let states: Vec<FileState> = rep.files.iter().map(|f| f.state).collect();
    assert_eq!(states.iter().filter(|s| **s == FileState::Done).count(), 2);
    assert_eq!(
        states
            .iter()
            .filter(|s| **s == FileState::Cancelled)
            .count(),
        4
    );
    let ev = events.lock().unwrap().clone();
    assert_eq!(ev.len(), 6, "{ev:?}");
    assert!(
        ev.iter()
            .enumerate()
            .all(|(i, p)| p.done == i + 1 && p.total == 6)
    );
    let last = ev.last().unwrap();
    assert_eq!((last.ok, last.failed, last.skipped), (2, 0, 4));

    let rest = executor::resume(
        &mut lab.store,
        &mut lab.engines,
        &rep.op_id,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(rest.status, OpStatus::Completed, "{rest:?}");
    assert!(lab.hashes().iter().zip(&lab.before).all(|(a, b)| a != b));

    let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
    let urep = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &up,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(urep.status, OpStatus::Completed, "{urep:?}");
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// SAFETY_MODEL §11: Cancel during a file's steps 1–7 abandons it (temp removed, original
/// unchanged, `cancelled`, resumable); Cancel after its commit lets it finish.
#[test]
fn cancel_abandons_before_the_commit_and_finishes_after() {
    let Some(mut lab) = Lab::new("cancel-mid", 3) else {
        return;
    };
    let photos = lab.files[0].parent().unwrap().to_path_buf();
    let listing = || std::fs::read_dir(&photos).unwrap().count();
    for (step, done) in [(4u8, 0usize), (8, 1)] {
        // planned again each round: the undo gave the files new identities
        let plan = lab.plan_creator();
        let first = plan.executable().next().unwrap().seq;
        let opts = ExecOptions {
            cancel: Some(Arc::new(AtomicBool::new(false))),
            cancel_at: Some(FaultPoint { seq: first, step }),
            ..Default::default()
        };
        let rep = executor::start(&mut lab.store, &mut lab.engines, &plan, &opts).unwrap();
        assert_eq!(rep.status, OpStatus::Cancelled, "step {step}: {rep:?}");
        let n_done = rep
            .files
            .iter()
            .filter(|f| f.state == FileState::Done)
            .count();
        assert_eq!(n_done, done, "step {step}: {rep:?}");
        // the completion summary: rolled back (stopped mid-write) vs. never started
        let summary = mm_core::history::list(&lab.store, 0, 1).unwrap().remove(0);
        assert_eq!(summary.rolled_back, 1 - done, "step {step}: {summary:?}");
        assert_eq!(listing(), 3, "no temporary file left (step {step})");
        let changed = lab
            .hashes()
            .iter()
            .zip(&lab.before)
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(changed, done, "step {step}");

        let rest = executor::resume(
            &mut lab.store,
            &mut lab.engines,
            &rep.op_id,
            &ExecOptions::default(),
        )
        .unwrap();
        assert_eq!(rest.status, OpStatus::Completed, "{rest:?}");
        let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
        let urep = executor::start(
            &mut lab.store,
            &mut lab.engines,
            &up,
            &ExecOptions::default(),
        )
        .unwrap();
        assert_eq!(urep.status, OpStatus::Completed, "{urep:?}");
        assert_eq!(lab.hashes(), lab.before, "step {step}");
    }
    lab.close();
}

/// Cancel before the commit of a recreate (undo of a deleted file, SAFETY_MODEL §7.2): nothing is
/// created and no temporary file is left; resuming recreates it.
#[test]
fn cancel_before_a_recreate_creates_nothing() {
    let Some(mut lab) = Lab::new("cancel-recreate", 3) else {
        return;
    };
    let plan = lab.plan_creator();
    let rep = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &plan,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.status, OpStatus::Completed, "{rep:?}");
    std::fs::remove_file(&lab.files[0]).unwrap();
    let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
    let recreate = up
        .entries
        .iter()
        .find(|e| matches!(e.action, Some(EntryAction::Recreate { .. })))
        .expect("a recreate entry")
        .seq;
    let opts = ExecOptions {
        cancel: Some(Arc::new(AtomicBool::new(false))),
        cancel_at: Some(FaultPoint {
            seq: recreate,
            step: 5,
        }),
        ..Default::default()
    };
    let photos = lab.files[0].parent().unwrap().to_path_buf();
    // run the recreate first so that the cancel lands inside it
    let mut first = up.clone();
    first.entries.sort_by_key(|e| e.seq != recreate);
    let urep = executor::start(&mut lab.store, &mut lab.engines, &first, &opts).unwrap();
    assert_eq!(urep.status, OpStatus::Cancelled, "{urep:?}");
    assert!(!lab.files[0].exists());
    assert_eq!(
        std::fs::read_dir(&photos).unwrap().count(),
        2,
        "no temp left"
    );
    let rest = executor::resume(
        &mut lab.store,
        &mut lab.engines,
        &urep.op_id,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(rest.status, OpStatus::Completed, "{rest:?}");
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// ARCHITECTURE §5.3 `PlanProgress`: planning reports the file inspection every 100 files and the
/// metadata read per chunk of 100; Cancel stops it with nothing written.
#[test]
fn plan_progress_and_cancel() {
    let Some(mut lab) = Lab::new("plan-progress", 250) else {
        return;
    };
    let events = Arc::new(Mutex::new(Vec::<PlanProgress>::new()));
    let ev = events.clone();
    let ctl = PlanCtl {
        progress: Some(Arc::new(move |p: &PlanProgress| {
            ev.lock().unwrap().push(*p)
        })),
        cancel: None,
        ..Default::default()
    };
    let edit = CreatorEdit::Set(vec!["Morii".into()]);
    let plan = planner::plan_creator(&mut lab.engines[0], &lab.files, &edit, "C", &ctl).unwrap();
    assert_eq!(plan.executable().count(), 250);
    let ev = events.lock().unwrap().clone();
    let of = |stage: PlanStage| -> Vec<usize> {
        ev.iter()
            .filter(|p| p.stage == stage && p.total == 250)
            .map(|p| p.done)
            .collect()
    };
    assert_eq!(of(PlanStage::Files), [0, 100, 200, 250], "{ev:?}");
    assert_eq!(of(PlanStage::Metadata), [0, 100, 200, 250], "{ev:?}");

    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let ctl = PlanCtl {
        progress: Some(Arc::new(move |p: &PlanProgress| {
            if p.stage == PlanStage::Metadata && p.done >= 100 {
                c.store(true, Ordering::SeqCst);
            }
        })),
        cancel: Some(cancel),
        ..Default::default()
    };
    let r = planner::plan_creator(&mut lab.engines[0], &lab.files, &edit, "C", &ctl);
    assert!(matches!(r, Err(mm_core::CoreError::Cancelled)), "{r:?}");
    // the engine is still usable after a cancelled plan
    assert_eq!(lab.plan_creator().executable().count(), 250);
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// SCREEN_SPEC §2: the Inspector's embedded preview is the file's own JPEG thumbnail, read as
/// data; a file without one, or whose thumbnail is not a JPEG, gives none.
#[test]
fn embedded_preview_is_only_ever_a_jpeg() {
    let Some(mut lab) = Lab::new("preview", 1) else {
        return;
    };
    let images = repo()
        .join("research/.work/exiftool")
        .join(version().unwrap())
        .join("src")
        .join(format!("Image-ExifTool-{}", version().unwrap()))
        .join("t/images");
    let photos = lab.files[0].parent().unwrap().to_path_buf();
    for n in ["ExifTool.jpg", "Nikon.jpg"] {
        std::fs::copy(images.join(n), photos.join(n)).unwrap();
    }
    let thumb = inspect::embedded_preview(&mut lab.engines[0], &photos.join("ExifTool.jpg"))
        .unwrap()
        .expect("ExifTool.jpg carries a JPEG thumbnail");
    assert!(
        thumb.starts_with(&[0xff, 0xd8, 0xff]) && thumb.len() < 4096,
        "{}",
        thumb.len()
    );
    // Writer.jpg has none; Nikon.jpg's is not a valid JPEG (ExifTool warns about it)
    assert!(
        inspect::embedded_preview(&mut lab.engines[0], &lab.files[0])
            .unwrap()
            .is_none()
    );
    assert!(
        inspect::embedded_preview(&mut lab.engines[0], &photos.join("Nikon.jpg"))
            .unwrap()
            .is_none()
    );
    // reading changed nothing
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// ARCHITECTURE §5.2 `asset_detail` and `selection_aggregate`: field views with their sources,
/// every raw tag, and per-field value counts that show a mixed selection.
#[test]
fn inspector_and_selection_aggregate() {
    let Some(mut lab) = Lab::new("inspect", 6) else {
        return;
    };
    let d = inspect::asset_detail(&mut lab.engines[0], &lab.files[0]).unwrap();
    let names: Vec<&str> = d.fields.iter().map(|f| f.field).collect();
    assert_eq!(names, ["creator", "copyright", "capture_time", "gps"]);
    assert!(d.tags.len() > 10, "{:?}", d.tags);
    assert!(d.sidecar.is_none());
    // the read-only attribute is reported for the Inspector's explicit "Clear read-only attribute…"
    assert!(!d.read_only);
    let mut perm = std::fs::metadata(&lab.files[0]).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&lab.files[0], perm.clone()).unwrap();
    assert!(
        inspect::asset_detail(&mut lab.engines[0], &lab.files[0])
            .unwrap()
            .read_only
    );
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&lab.files[0], perm).unwrap();

    let agg = |lab: &mut Lab| {
        inspect::selection_aggregate(&mut lab.engines[0], &lab.files, &Default::default()).unwrap()
    };
    for a in agg(&mut lab) {
        let listed: usize = a.values.iter().map(|(_, n)| n).sum();
        assert_eq!(listed + a.empty + a.unreadable, 6, "{a:?}");
    }
    // write one creator to half of the files: the selection is now mixed
    let half = lab.files[..3].to_vec();
    let plan = planner::plan_creator(
        &mut lab.engines[0],
        &half,
        &CreatorEdit::Set(vec!["Morii".into()]),
        "C",
        &Default::default(),
    )
    .unwrap();
    let rep = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &plan,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.status, OpStatus::Completed, "{rep:?}");
    let creator = agg(&mut lab).remove(0);
    assert_eq!(creator.field, "creator");
    assert!(
        creator.values.contains(&("Morii".to_string(), 3)),
        "{creator:?}"
    );
    assert!(
        creator.values.len() >= 2 || creator.empty > 0,
        "{creator:?}"
    );
    let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
    executor::start(
        &mut lab.store,
        &mut lab.engines,
        &up,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// SAFETY_MODEL §11: Cancel ends the ExifTool process that is writing a large file's temporary
/// output; the file is settled as cancelled with the original unchanged and nothing left behind,
/// and the next use of the engine starts a new process.
#[test]
fn cancel_ends_a_long_exiftool_write() {
    let Some(mut lab) = Lab::new("cancel-kill", 1) else {
        return;
    };
    // 150 MB after the JPEG's end: ExifTool copies it into the temporary output
    {
        use std::io::Write;
        let mut f = std::fs::OpenOptions::new()
            .append(true)
            .open(&lab.files[0])
            .unwrap();
        let block = vec![0x5au8; 1 << 20];
        for _ in 0..150 {
            f.write_all(&block).unwrap();
        }
    }
    lab.before = lab.hashes();
    let plan = lab.plan_creator();
    let seq = plan.executable().next().unwrap().seq;
    let opts = ExecOptions {
        cancel: Some(Arc::new(AtomicBool::new(false))),
        cancel_at: Some(FaultPoint { seq, step: 4 }),
        space_reserve: Some(0),
        ..Default::default()
    };
    let t = std::time::Instant::now();
    let rep = executor::start(&mut lab.store, &mut lab.engines, &plan, &opts).unwrap();
    assert_eq!(rep.status, OpStatus::Cancelled, "{rep:?}");
    assert_eq!(rep.files[0].state, FileState::Cancelled, "{rep:?}");
    assert_eq!(lab.hashes(), lab.before);
    let dir = lab.files[0].parent().unwrap();
    assert_eq!(
        std::fs::read_dir(dir).unwrap().count(),
        1,
        "no temporary file left"
    );
    eprintln!("cancelled after {:?}", t.elapsed());

    let rest = executor::resume(
        &mut lab.store,
        &mut lab.engines,
        &rep.op_id,
        &ExecOptions {
            space_reserve: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(rest.status, OpStatus::Completed, "{rest:?}");
    assert!(
        lab.engines[0].restarts() >= 1,
        "the ExifTool process was ended"
    );
    let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
    executor::start(
        &mut lab.store,
        &mut lab.engines,
        &up,
        &ExecOptions {
            space_reserve: Some(0),
            ..Default::default()
        },
    )
    .unwrap();
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// Planning with several ExifTool readers gives exactly the Plan of one reader (entries in the
/// input order), reports progress up to the total, and stops on Cancel.
#[test]
fn parallel_metadata_reads_match_one_reader() {
    let Some(mut lab) = Lab::new("parallel-read", 450) else {
        return;
    };
    let edit = CreatorEdit::Set(vec!["Morii".into()]);
    let one = planner::plan_creator(
        &mut lab.engines[0],
        &lab.files,
        &edit,
        "C",
        &PlanCtl::default(),
    )
    .unwrap();
    let events = Arc::new(Mutex::new(Vec::<PlanProgress>::new()));
    let ev = events.clone();
    let ctl = PlanCtl {
        progress: Some(Arc::new(move |p: &PlanProgress| {
            ev.lock().unwrap().push(*p)
        })),
        readers: 4,
        ..Default::default()
    };
    let t = std::time::Instant::now();
    let four = planner::plan_creator(&mut lab.engines[0], &lab.files, &edit, "C", &ctl).unwrap();
    eprintln!("4 readers: {:?}", t.elapsed());
    assert_eq!(one.entries, four.entries);
    let meta: Vec<usize> = events
        .lock()
        .unwrap()
        .iter()
        .filter(|p| p.stage == PlanStage::Metadata)
        .map(|p| p.done)
        .collect();
    assert_eq!(meta.first(), Some(&0));
    assert_eq!(meta.last(), Some(&450), "{meta:?}");
    assert!(meta.windows(2).all(|w| w[0] <= w[1]), "{meta:?}");

    let cancel = Arc::new(AtomicBool::new(false));
    let c = cancel.clone();
    let ctl = PlanCtl {
        progress: Some(Arc::new(move |p: &PlanProgress| {
            if p.stage == PlanStage::Metadata && p.done >= 100 {
                c.store(true, Ordering::SeqCst);
            }
        })),
        cancel: Some(cancel),
        readers: 4,
    };
    let r = planner::plan_creator(&mut lab.engines[0], &lab.files, &edit, "C", &ctl);
    assert!(matches!(r, Err(mm_core::CoreError::Cancelled)), "{r:?}");
    assert_eq!(lab.plan_creator().executable().count(), 450);
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// Photo formats this build does not write enter the Session read-only (user decision
/// 2026-09-28): the Inspector reads them, every Plan marks them Unsupported, nothing is written.
#[test]
fn read_only_formats_are_shown_not_written() {
    let Some(mut lab) = Lab::new("read-only", 1) else {
        return;
    };
    let v = version().unwrap();
    let images = repo().join(format!(
        "research/.work/exiftool/{v}/src/Image-ExifTool-{v}/t/images"
    ));
    let photos = lab.files[0].parent().unwrap().to_path_buf();
    let mut read_only = Vec::new();
    for n in ["PNG.png", "ExifTool.tif", "DNG.dng", "CanonRaw.cr3"] {
        let p = photos.join(n);
        std::fs::copy(images.join(n), &p).unwrap();
        read_only.push((p.clone(), hash(&p)));
    }
    let mut session = Session::default();
    let r = session.import_folder(&photos);
    assert_eq!((r.added.len(), r.read_only.len()), (5, 4), "{r:?}");
    for &a in &r.read_only {
        let asset = session.asset(a).unwrap();
        assert!(!asset.writable);
        let d = inspect::asset_detail(&mut lab.engines[0], &asset.path).unwrap();
        assert!(!d.tags.is_empty(), "{}", asset.path.display());
    }
    let paths = session.paths(&r.added).unwrap();
    let plan = planner::plan_creator(
        &mut lab.engines[0],
        &paths,
        &CreatorEdit::Set(vec!["Morii".into()]),
        "C",
        &Default::default(),
    )
    .unwrap();
    let unsupported = plan
        .entries
        .iter()
        .filter(|e| matches!(e.status, mm_domain::plan::EntryStatus::Unsupported(_)))
        .count();
    assert_eq!(unsupported, 4, "{:?}", plan.entries);
    assert_eq!(plan.executable().count(), 1);
    let rep = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &plan,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.files.len(), 1);
    for (p, h) in &read_only {
        assert_eq!(&hash(p), h, "{} was written", p.display());
    }
    let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
    executor::start(
        &mut lab.store,
        &mut lab.engines,
        &up,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// SAFETY_MODEL §10 launch sequence: an Operation stopped by a lasting journal failure is
/// recovered at startup, and the Recovery dialog data lists it with its remaining files.
#[test]
fn startup_recovers_and_reports_what_needs_a_decision() {
    let Some(mut lab) = Lab::new("startup", 3) else {
        return;
    };
    let plan = lab.plan_creator();
    let second = plan.executable().nth(1).unwrap().seq;
    lab.store.arm_write_fault(mm_store::WriteFault {
        target: mm_store::WriteTarget::File {
            seq: second,
            state: FileState::BackedUp,
        },
        persistent: true,
    });
    let r = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &plan,
        &ExecOptions::default(),
    );
    assert!(r.is_err(), "the journal failed for good");
    // a new process: the old connection (and the lock it holds) goes away, the database stays
    let spare = Store::open(&lab.dir.join("spare")).unwrap();
    drop(std::mem::replace(&mut lab.store, spare));
    let mut store = Store::open(&lab.dir.join("data")).unwrap();
    let s = mm_core::service::startup(&mut store).unwrap();
    if s.elevated {
        eprintln!("SKIP: elevated (CI runner); recovery waits for a normal launch");
    } else {
        assert_eq!(s.recovered.len(), 1, "{s:?}");
        assert_eq!(s.needs_decision.len(), 1, "{s:?}");
        assert!(s.needs_decision[0].remaining >= 1, "{s:?}");
        assert!(s.backup_problem.is_none());
    }
    drop(store);
    lab.close();
}

/// The batch editor stages several fields at once (INTERACTION_SPEC §3): field edits as an
/// unconditional Preset plus a capture-time tool over the whole selection make one entry per
/// file; it is written, verified and undone like any Plan.
#[test]
fn batch_edits_make_one_entry_per_file() {
    use mm_domain::rules::{Action, Preset, Rule};
    use mm_domain::time::{self, SequenceOrder};
    let Some(mut lab) = Lab::new("batch", 3) else {
        return;
    };
    let preset = Preset {
        schema_version: mm_domain::rules::PRESET_SCHEMA_VERSION,
        name: "Batch".into(),
        rules: vec![
            Rule {
                name: String::new(),
                enabled: true,
                when: vec![],
                then: vec![Action::SetCreator {
                    names: vec!["Morii".into()],
                }],
            },
            Rule {
                name: String::new(),
                enabled: true,
                when: vec![],
                then: vec![Action::SetCopyright {
                    value: "© {creator|Morii} {year|2026}".into(),
                }],
            },
        ],
    };
    let tool = planner::TimeTool::Sequence {
        start: time::parse_local("2026:09:30 10:00:00").unwrap(),
        step: time::parse_shift("+00:01:00").unwrap(),
        order: SequenceOrder::NaturalFileName,
    };
    // nothing staged, or the time staged twice, is refused
    let empty = Preset {
        rules: vec![],
        ..preset.clone()
    };
    assert!(
        planner::plan_batch(
            &mut lab.engines[0],
            &lab.files,
            &empty,
            None,
            "x",
            &Default::default()
        )
        .is_err()
    );
    let mut twice = preset.clone();
    twice.rules[0].then.push(Action::ShiftTime {
        by: "+01:00:00".into(),
        digitized: true,
    });
    assert!(
        planner::plan_batch(
            &mut lab.engines[0],
            &lab.files,
            &twice,
            Some((&tool, true)),
            "x",
            &Default::default()
        )
        .is_err()
    );

    let plan = planner::plan_batch(
        &mut lab.engines[0],
        &lab.files,
        &preset,
        Some((&tool, true)),
        "Batch",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(plan.entries.len(), 3);
    for (i, e) in plan.entries.iter().enumerate() {
        let fields: Vec<&str> = e.changes.iter().map(|c| c.field.as_str()).collect();
        assert!(
            fields.contains(&"creator")
                && fields.contains(&"copyright")
                && fields.contains(&"capture_time"),
            "{e:?}"
        );
        let t = e
            .changes
            .iter()
            .find(|c| c.field == "capture_time")
            .unwrap();
        assert!(
            t.after.as_ref().unwrap()[0].starts_with(&format!("2026:09:30 10:0{i}:00")),
            "{t:?}"
        );
    }
    let rep = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &plan,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(rep.status, OpStatus::Completed, "{rep:?}");
    let d = inspect::asset_detail(&mut lab.engines[0], &lab.files[1]).unwrap();
    let value = |f: &str| {
        d.fields
            .iter()
            .find(|v| v.field == f)
            .and_then(|v| v.value.clone())
    };
    assert_eq!(value("creator").as_deref(), Some("Morii"));
    assert!(value("copyright").unwrap().starts_with("© "));
    let up = undo::plan_undo(&lab.store, &rep.op_id, lab.engines[0].version()).unwrap();
    let urep = executor::start(
        &mut lab.store,
        &mut lab.engines,
        &up,
        &ExecOptions::default(),
    )
    .unwrap();
    assert_eq!(urep.status, OpStatus::Completed, "{urep:?}");
    assert_eq!(lab.hashes(), lab.before);
    lab.close();
}

/// Clean Export (D-15 (c), METADATA_MODEL §10.1) with the pinned ExifTool: a JPEG with GPS, an
/// unknown APP5 segment, a JUMBF APP11 segment and data after the end of the image. The Preview
/// predicts each removal; the copy holds only whitelisted segments and tags, the same image
/// data, and nothing else; the source is not touched; a taken name is numbered or skipped; a
/// file that is not a JPEG is not exported.
#[test]
fn clean_export_keeps_only_the_whitelist() {
    use mm_core::clean_export::{self, CleanStatus, OnConflict};
    use mm_domain::clean::KeepSpec;
    use mm_domain::jpeg;
    let Some(mut lab) = Lab::new("clean", 1) else {
        return;
    };
    let images = lab.files[0].parent().unwrap().to_path_buf();
    // GPS.jpg of the ExifTool test images, with segments a metadata reader may not decode
    let src = images.join("hazard.jpg");
    let gps = repo()
        .join("research/.work/exiftool")
        .join(version().unwrap())
        .join("src")
        .join(format!("Image-ExifTool-{}", version().unwrap()))
        .join("t/images/GPS.jpg");
    let mut b = std::fs::read(&gps).unwrap();
    let seg = |m: u8, payload: &[u8]| {
        let mut v = vec![0xFF, m];
        v.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
        v.extend_from_slice(payload);
        v
    };
    b.splice(2..2, seg(0xE5, b"MMTEST\0hidden payload"));
    b.splice(2..2, seg(0xEB, b"JP\0\x01\0\0\0\x01jumb"));
    b.extend_from_slice(b"SECRET AFTER EOI");
    std::fs::write(&src, &b).unwrap();
    let not_jpeg = images.join("notes.png");
    std::fs::write(&not_jpeg, b"x").unwrap();
    let before = hash(&src);

    let plan = clean_export::plan(
        &mut lab.engines[0],
        &[src.clone(), not_jpeg.clone()],
        KeepSpec::default(),
        &Default::default(),
    )
    .unwrap();
    let e = &plan.entries[0];
    assert_eq!(e.status, CleanStatus::Ready, "{e:?}");
    assert!(
        e.prediction.remove.iter().any(|r| r.category == "gps"),
        "{:?}",
        e.prediction
    );
    let segs: Vec<&str> = e
        .prediction
        .remove_segments
        .iter()
        .map(|s| s.label.as_str())
        .collect();
    assert!(segs.contains(&"APP5:unknown:MMTEST"), "{segs:?}");
    assert!(segs.contains(&"APP11:JUMBF"), "{segs:?}");
    assert!(segs.contains(&"after the end of the image"), "{segs:?}");
    assert!(matches!(plan.entries[1].status, CleanStatus::Blocked(_)));

    let out = lab.dir.join("out");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("hazard.jpg"), b"taken").unwrap();
    let r = clean_export::export(
        &mut lab.engines[0],
        &plan,
        &out,
        OnConflict::Number,
        &|_, _| {},
        None,
    )
    .unwrap();
    assert_eq!(r[0].status, "exported", "{r:?}");
    assert_eq!(r[1].status, "blocked");
    let copy = std::path::PathBuf::from(r[0].output.as_ref().unwrap());
    assert!(copy.ends_with("hazard (2).jpg"), "{copy:?}");
    let j = jpeg::parse(&std::fs::read(&copy).unwrap()).unwrap();
    assert!(
        jpeg::check_clean(&j).is_empty(),
        "{:?}",
        jpeg::check_clean(&j)
    );
    let inv = lab.engines[0].read_inventory(&copy).unwrap();
    assert!(
        !inv.tags.keys().any(|k| k.contains("GPS")),
        "{:?}",
        inv.tags.keys()
    );
    assert_eq!(hash(&src), before, "the source is only read");
    assert_eq!(
        std::fs::read(out.join("hazard.jpg")).unwrap(),
        b"taken",
        "never replaced"
    );
    // no temporary copies left
    assert!(!std::fs::read_dir(&out).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .contains(".mmexport-")
    }));
    // Skip leaves the name to the existing file
    let r = clean_export::export(
        &mut lab.engines[0],
        &plan,
        &out,
        OnConflict::Skip,
        &|_, _| {},
        None,
    )
    .unwrap();
    assert_eq!(r[0].status, "skipped", "{r:?}");
    // a source changed after the Preview is refused
    std::fs::write(&src, [b.as_slice(), b"more"].concat()).unwrap();
    let r = clean_export::export(
        &mut lab.engines[0],
        &plan,
        &out,
        OnConflict::Number,
        &|_, _| {},
        None,
    )
    .unwrap();
    assert_eq!(r[0].status, "refused", "{r:?}");
    lab.close();
}
