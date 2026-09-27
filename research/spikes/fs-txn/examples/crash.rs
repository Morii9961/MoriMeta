//! S2-C: crash injection. The transaction runs in a child process that terminates itself at a
//! given (file, step) or is killed at a random moment by the parent. The parent then checks I-3
//! before recovery, runs recovery, and checks the post-recovery invariants.
//!
//!   crash sweep  <root> <backup|original>      every crash point x several files
//!   crash random <root> <iterations>           random TerminateProcess during a 24-file batch
//!   crash roundtrip <root> <n>                 apply all, then undo from backup: byte-identical?
//!   (internal) crash child <dir> <source> <seq> <cp>

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use exiftool_session::{EngineConfig, Session};
use fs_txn::txn::{self, Commit, Ctx, Journal, Source};
use fs_txn::*;
use serde_json::json;

fn research() -> PathBuf {
    EngineConfig::research("launcher").cwd.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn samples() -> Vec<PathBuf> {
    let d = research().join(".work/exiftool/13.59/src/Image-ExifTool-13.59/t/images");
    ["Writer.jpg", "Canon.jpg", "Nikon.jpg", "Sony.jpg", "Olympus.jpg", "Pentax.jpg", "GPS.jpg", "IPTC.jpg", "XMP.jpg", "ExifTool.jpg"]
        .iter()
        .map(|n| d.join(n))
        .collect()
}

fn setup(dir: &Path, n: usize) -> (Vec<PathBuf>, BTreeMap<usize, String>) {
    let _ = std::fs::remove_dir_all(dir);
    std::fs::create_dir_all(dir.join("photos")).unwrap();
    std::fs::create_dir_all(dir.join("backup")).unwrap();
    let s = samples();
    let mut files = vec![];
    let mut truth = BTreeMap::new();
    for i in 0..n {
        let p = dir.join("photos").join(format!("IMG_{i:04}.jpg"));
        std::fs::copy(&s[i % s.len()], &p).unwrap();
        truth.insert(i, hex(&hash_path(&p).unwrap()));
        files.push(p);
    }
    (files, truth)
}

fn child(dir: &Path, source: Source, crash_at: Option<(usize, u8)>) {
    let mut files: Vec<PathBuf> = std::fs::read_dir(dir.join("photos")).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    let journal = Journal::new(dir.join("journal.jsonl"));
    let items = txn::plan(&files, &dir.join("backup"), "Morii 森").unwrap();
    txn::write_plan(&journal, &items).unwrap();
    let mut sess = Session::spawn(&EngineConfig::research("launcher")).unwrap();
    let mut ctx = Ctx { session: &mut sess, journal: &journal, source, commit: Commit::ReplaceFileW, crash_at };
    for it in &items {
        let st = txn::run_one(&mut ctx, it).unwrap();
        if st != "done" {
            eprintln!("seq {} -> {st}", it.seq);
        }
    }
}

fn run_child(dir: &Path, source: &str, crash: Option<(usize, u8)>) -> std::process::Child {
    let exe = std::env::current_exe().unwrap();
    let (seq, cp) = crash.map(|(s, c)| (s.to_string(), c.to_string())).unwrap_or(("-".into(), "-".into()));
    Command::new(exe).args(["child", &dir.to_string_lossy(), source, &seq, &cp]).spawn().unwrap()
}

fn verify(dir: &Path, truth: &BTreeMap<usize, String>) -> serde_json::Value {
    let journal = Journal::new(dir.join("journal.jsonl"));
    let items = txn::load_plan(&journal.read());
    if items.is_empty() {
        // crashed before the plan record was durable: nothing may have changed
        let changed: Vec<usize> = truth.iter().filter(|(i, h)| {
            let p = dir.join("photos").join(format!("IMG_{i:04}.jpg"));
            hash_path(&p).map(|x| &hex(&x) != *h).unwrap_or(true)
        }).map(|(i, _)| *i).collect();
        return json!({"plan_durable": false, "violations_before": [], "violations_after": changed.iter().map(|i| format!("seq {i} changed without plan")).collect::<Vec<_>>()});
    }
    let before = txn::check_invariants(&items, truth, &journal, "before");
    let outcomes = txn::recover(&journal);
    let after = txn::check_invariants(&items, truth, &journal, "after");
    let mut hist: BTreeMap<String, usize> = BTreeMap::new();
    for o in outcomes.values() {
        *hist.entry(o.clone()).or_default() += 1;
    }
    json!({"plan_durable": true, "violations_before": before, "violations_after": after, "recovery": hist})
}

fn main() {
    let a: Vec<String> = std::env::args().collect();
    match a[1].as_str() {
        "child" => {
            let dir = PathBuf::from(&a[2]);
            let source = if a[3] == "original" { Source::Original } else { Source::Backup };
            let crash = if a[4] == "-" { None } else { Some((a[4].parse().unwrap(), a[5].parse().unwrap())) };
            child(&dir, source, crash);
        }
        "sweep" => {
            let root = PathBuf::from(a[2].replace('/', "\\"));
            let source = a[3].clone();
            let mut rows = vec![];
            let mut total_viol = 0;
            for seq in [0usize, 3, 5] {
                for cp in 1u8..=10 {
                    let dir = root.join(format!("sweep-{source}-{seq}-{cp}"));
                    let (_, truth) = setup(&dir, 6);
                    let st = run_child(&dir, &source, Some((seq, cp))).wait().unwrap();
                    let v = verify(&dir, &truth);
                    total_viol += v["violations_before"].as_array().unwrap().len() + v["violations_after"].as_array().unwrap().len();
                    rows.push(json!({"seq": seq, "cp": cp, "exit": st.code(), "result": v}));
                }
            }
            let out = json!({"mode": "sweep", "source": source, "root": root, "cases": rows.len(), "total_violations": total_viol, "rows": rows});
            finish(&root, &format!("crash-sweep-{source}"), out);
        }
        "random" => {
            let root = PathBuf::from(a[2].replace('/', "\\"));
            let iters: usize = a[3].parse().unwrap();
            // calibrate one uninterrupted run
            let dir = root.join("random-cal");
            setup(&dir, 24);
            let t0 = Instant::now();
            run_child(&dir, "backup", None).wait().unwrap();
            let full = t0.elapsed();
            let mut rng = exiftool_session::valuegen::SplitMix64(42);
            let mut total_viol = 0;
            let mut hist: BTreeMap<String, usize> = BTreeMap::new();
            let mut bad = vec![];
            for i in 0..iters {
                let dir = root.join("random");
                let (_, truth) = setup(&dir, 24);
                let delay = Duration::from_millis(rng.below(full.as_millis() as u64 + 1));
                let mut c = run_child(&dir, "backup", None);
                std::thread::sleep(delay);
                let _ = c.kill();
                let _ = c.wait();
                std::thread::sleep(Duration::from_millis(50)); // let the job object reap ExifTool
                let v = verify(&dir, &truth);
                let n = v["violations_before"].as_array().unwrap().len() + v["violations_after"].as_array().unwrap().len();
                total_viol += n;
                if let Some(r) = v["recovery"].as_object() {
                    for (k, c) in r {
                        *hist.entry(k.clone()).or_default() += c.as_u64().unwrap() as usize;
                    }
                }
                if n > 0 && bad.len() < 10 {
                    bad.push(json!({"iteration": i, "delay_ms": delay.as_millis() as u64, "result": v}));
                }
            }
            let out = json!({"mode": "random", "root": root, "iterations": iters, "files_per_iteration": 24,
                "uninterrupted_run_ms": full.as_millis() as u64, "total_violations": total_viol, "recovery_outcomes": hist, "failures": bad});
            finish(&root, "crash-random", out);
        }
        "roundtrip" => {
            let root = PathBuf::from(a[2].replace('/', "\\"));
            let n: usize = a[3].parse().unwrap();
            let dir = root.join("roundtrip");
            let (files, truth) = setup(&dir, n);
            run_child(&dir, "backup", None).wait().unwrap();
            let journal = Journal::new(dir.join("journal.jsonl"));
            let recs = journal.read();
            let items = txn::load_plan(&recs);
            let done = recs.iter().filter(|r| r["t"] == "done").count();
            let changed = files.iter().enumerate().filter(|(i, p)| hex(&hash_path(p).unwrap()) != truth[i]).count();
            // undo: backup -> same-dir temp (verified == H0) -> ReplaceFileW
            let mut identical = 0;
            for it in &items {
                let tmp = sibling(&it.path, ".mmundo");
                let mut b = std::fs::File::open(&it.backup).unwrap();
                let h = copy_new_hashing(&mut b, &tmp).unwrap();
                assert_eq!(hex(&h), truth[&it.seq]);
                let bak = sibling(&it.path, ".mmundobak");
                replace_file(&it.path, &tmp, &bak).unwrap();
                std::fs::remove_file(&bak).unwrap();
                if hex(&hash_path(&it.path).unwrap()) == truth[&it.seq] {
                    identical += 1;
                }
            }
            let out = json!({"mode": "roundtrip", "files": n, "journal_done": done, "files_changed_by_apply": changed, "byte_identical_after_undo": identical});
            finish(&root, "apply-undo-roundtrip", out);
        }
        _ => panic!("mode"),
    }
}

fn finish(root: &Path, name: &str, out: serde_json::Value) {
    let tag = if root.to_string_lossy().starts_with(r"\\") { "smb" } else { "ntfs" };
    let dir = research().join("results/s2");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{name}-{tag}.json")), serde_json::to_string_pretty(&out).unwrap()).unwrap();
    let mut short = out.clone();
    if let Some(o) = short.as_object_mut() {
        o.remove("rows");
    }
    println!("{}", serde_json::to_string(&short).unwrap().chars().take(1500).collect::<String>());
}
