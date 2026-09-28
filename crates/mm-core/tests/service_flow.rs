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
