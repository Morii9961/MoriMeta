//! The backend interface end to end with the pinned ExifTool (skipped when it is not fetched):
//! Session → Plan → Preview page → exclusion → confirmation → execution → undo through the same
//! interface. Works on copies of the ExifTool test images in a temporary folder.

use std::path::{Path, PathBuf};

use mm_core::engine::Engine;
use mm_core::executor::ExecOptions;
use mm_core::planner;
use mm_core::service::{EntryFilter, OperationGate, PAGE_SIZE, PlanBook, ServiceError, Session};
use mm_core::undo;
use mm_domain::creator::CreatorEdit;
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

#[test]
fn preview_exclusion_confirmation_execution_and_undo() {
    let Some(v) = version() else {
        eprintln!("SKIP: no ExifTool lock file");
        return;
    };
    let pkg = repo().join(format!("research/.work/exiftool/{v}/win64/exiftool-{v}_64"));
    let images = repo().join(format!(
        "research/.work/exiftool/{v}/src/Image-ExifTool-{v}/t/images"
    ));
    if !pkg.join("exiftool_files").join("perl.exe").exists() || !images.exists() {
        eprintln!("SKIP: pinned ExifTool not found");
        return;
    }
    let dir = std::env::temp_dir().join(format!("mm-service-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let photos = dir.join("photos");
    std::fs::create_dir_all(&photos).unwrap();
    let files: Vec<PathBuf> = ["Writer.jpg", "Nikon.jpg", "Canon.jpg"]
        .iter()
        .map(|n| {
            let p = photos.join(n);
            std::fs::copy(images.join(n), &p).unwrap();
            p
        })
        .collect();
    let before: Vec<String> = files.iter().map(|p| hash(p)).collect();
    let run = dir.join("data").join("run");
    let cfg = EngineConfig {
        program: pkg.join("exiftool_files").join("perl.exe"),
        script: Some(pkg.join("exiftool_files").join("exiftool.pl")),
        cwd: run.join("exiftool-cwd"),
        temp: run.join("tmp"),
    };
    std::fs::create_dir_all(&cfg.cwd).unwrap();
    std::fs::create_dir_all(&cfg.temp).unwrap();
    let mut engines = vec![Engine::start(cfg).unwrap()];
    let mut store = Store::open(&dir.join("data")).unwrap();
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
