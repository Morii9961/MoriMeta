//! End-to-end tests of the JPEG + Creator chain through mm-cli, with fault injection.
//! Needs the pinned ExifTool (research/scripts/fetch_exiftool.py); skipped otherwise.
//!
//! Invariants checked after every injected crash (SAFETY_MODEL §1, §10):
//!   before recovery: every file's original content still exists somewhere (path, bak or backup)
//!   after recovery : every path holds H0 or the recorded H1; no temp/bak leftovers; fsck clean
//!   then resume + undo: every file is byte-identical to the original.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::time::Duration;

use serde_json::Value;

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn pkg() -> Option<PathBuf> {
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("research/exiftool.lock.json")).ok()?,
    )
    .ok()?;
    let v = lock["version"].as_str()?;
    let p = repo().join(format!("research/.work/exiftool/{v}/win64/exiftool-{v}_64"));
    if !p.join("exiftool.exe").exists() && p.join("exiftool(-k).exe").exists() {
        std::fs::copy(p.join("exiftool(-k).exe"), p.join("exiftool.exe")).ok()?;
    }
    p.join("exiftool.exe").exists().then_some(p)
}

fn timages() -> PathBuf {
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("research/exiftool.lock.json")).unwrap(),
    )
    .unwrap();
    let v = lock["version"].as_str().unwrap();
    repo().join(format!(
        "research/.work/exiftool/{v}/src/Image-ExifTool-{v}/t/images"
    ))
}

macro_rules! require {
    () => {
        match pkg() {
            Some(p) => p,
            None => {
                eprintln!("SKIP: pinned ExifTool not found");
                return;
            }
        }
    };
}

const SAMPLES: &[&str] = &[
    "Writer.jpg",
    "Nikon.jpg",
    "Canon.jpg",
    "XMP.jpg",
    "Sony.jpg",
    "Olympus.jpg",
    "Pentax.jpg",
    "GPS.jpg",
];

struct Lab {
    dir: PathBuf,
    data: PathBuf,
    pkg: PathBuf,
    photos: Vec<PathBuf>,
    truth: BTreeMap<PathBuf, String>,
}

fn blake(p: &Path) -> Option<String> {
    std::fs::read(p)
        .ok()
        .map(|b| blake3::hash(&b).to_hex().to_string())
}

impl Lab {
    fn new(name: &str, pkg: &Path) -> Lab {
        let dir = std::env::temp_dir().join(format!("mm-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Lab::at(pkg, dir.clone(), dir.join("photos"), dir.join("data"))
    }

    /// Photos and application data in chosen places (e.g. on a small test volume).
    fn at(pkg: &Path, dir: PathBuf, photo_dir: PathBuf, data: PathBuf) -> Lab {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&photo_dir).unwrap();
        let mut photos = Vec::new();
        let mut truth = BTreeMap::new();
        for s in SAMPLES {
            let p = photo_dir.join(s);
            std::fs::copy(timages().join(s), &p).unwrap();
            truth.insert(p.clone(), blake(&p).unwrap());
            photos.push(p);
        }
        Lab {
            data,
            dir,
            pkg: pkg.to_path_buf(),
            photos,
            truth,
        }
    }

    /// Current content hash of every photo (the pre-images of the next Operation).
    fn snapshot(&self) -> BTreeMap<PathBuf, String> {
        self.snapshot_where(|_| true)
    }

    /// Same, for the photos whose index passes `keep`.
    fn snapshot_where(&self, keep: impl Fn(usize) -> bool) -> BTreeMap<PathBuf, String> {
        self.photos
            .iter()
            .enumerate()
            .filter(|(i, _)| keep(*i))
            .map(|(_, p)| (p.clone(), blake(p).unwrap()))
            .collect()
    }

    fn cli(&self, args: &[&str]) -> Output {
        self.cli_env(args, false)
    }

    fn cli_env(&self, args: &[&str], faults: bool) -> Output {
        let mut c = Command::new(env!("CARGO_BIN_EXE_mm-cli"));
        c.arg("--data")
            .arg(&self.data)
            .arg("--exiftool")
            .arg(&self.pkg)
            .args(args);
        if faults {
            c.env("MM_FAULT_INJECTION", "1");
        }
        c.output().unwrap()
    }

    fn json(o: &Output) -> Value {
        serde_json::from_slice(&o.stdout)
            .unwrap_or_else(|_| panic!("not JSON: {}", String::from_utf8_lossy(&o.stdout)))
    }

    fn plan(&self, name: &str, file: &str) -> PathBuf {
        let out = self.dir.join(file);
        let mut args = vec![
            "plan-creator",
            "--set",
            name,
            "--out",
            out.to_str().unwrap(),
        ];
        let ps: Vec<String> = self
            .photos
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        args.extend(ps.iter().map(String::as_str));
        let o = self.cli(&args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        out
    }

    fn last_op(&self) -> String {
        let h = Lab::json(&self.cli(&["history"]));
        h.as_array().unwrap().last().unwrap()["id"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    fn show(&self, op: &str) -> Value {
        Lab::json(&self.cli(&["show", op]))
    }

    fn leftovers(&self) -> Vec<String> {
        std::fs::read_dir(self.photos[0].parent().unwrap())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".mmtmp-") || n.contains(".mmbak-"))
            .collect()
    }

    /// Before recovery: the original content of every file exists at the path, a bak name or a backup.
    fn assert_preimages(&self, op: &str) {
        self.assert_preimages_of(op, &self.truth);
    }

    /// Same, for an Operation whose pre-images are `before` (e.g. an Undo).
    fn assert_preimages_of(&self, op: &str, before: &BTreeMap<PathBuf, String>) {
        let s = self.show(op);
        for (path, h0) in before {
            let mut cands = vec![blake(path)];
            for f in s["files"].as_array().unwrap() {
                if Path::new(f["path"].as_str().unwrap()) == path.as_path() {
                    for k in ["bak", "backup"] {
                        cands.push(blake(Path::new(f[k].as_str().unwrap())));
                    }
                }
            }
            assert!(
                cands.iter().any(|c| c.as_deref() == Some(h0.as_str())),
                "I-3 violated for {}",
                path.display()
            );
        }
    }

    /// After recovery: each path is H0 or the recorded H1, no leftovers, fsck clean.
    fn assert_recovered(&self, op: &str) {
        self.assert_recovered_of(op, &self.truth);
    }

    fn assert_recovered_of(&self, op: &str, before: &BTreeMap<PathBuf, String>) {
        let s = self.show(op);
        for (path, h0) in before {
            let cur = blake(path).unwrap_or_else(|| panic!("missing {}", path.display()));
            let h1 = s["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| Path::new(f["path"].as_str().unwrap()) == path.as_path())
                .and_then(|f| f["h1"].as_str().map(str::to_owned));
            assert!(
                &cur == h0 || Some(&cur) == h1.as_ref(),
                "{} is neither H0 nor H1",
                path.display()
            );
        }
        assert!(
            self.leftovers().is_empty(),
            "leftovers: {:?}",
            self.leftovers()
        );
        let f = self.cli(&["fsck", op]);
        assert!(
            f.status.success(),
            "fsck: {}",
            String::from_utf8_lossy(&f.stdout)
        );
    }

    fn assert_all_original(&self) {
        for (p, h) in &self.truth {
            assert_eq!(
                blake(p).as_deref(),
                Some(h.as_str()),
                "{} not restored",
                p.display()
            );
        }
    }

    fn undo(&self, op: &str) {
        let u = self.dir.join(format!("undo-{op}.json"));
        let o = self.cli(&["plan-undo", op, "--out", u.to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        let a = self.cli(&["apply", u.to_str().unwrap()]);
        assert!(
            a.status.success(),
            "undo apply: {}",
            String::from_utf8_lossy(&a.stdout)
        );
    }
}

fn creators(lab: &Lab) -> Vec<Value> {
    let ps: Vec<String> = lab
        .photos
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    let mut args = vec!["scan"];
    args.extend(ps.iter().map(String::as_str));
    Lab::json(&lab.cli(&args))
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["creator"].clone())
        .collect()
}

#[test]
fn apply_then_undo_is_byte_identical() {
    let pkg = require!();
    let lab = Lab::new("roundtrip", &pkg);
    let plan = lab.plan("森 Morii", "p.json");
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    // files with Latin IPTC (GPS.jpg) are blocked for a CJK name and must stay untouched
    let p: Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
    let statuses: Vec<String> = p["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["status"]["status"].as_str().unwrap().to_owned())
        .collect();
    assert!(
        statuses.iter().any(|s| s == "blocked")
            && statuses.iter().filter(|s| *s == "ready").count() >= 6
    );
    for ((c, st), photo) in creators(&lab).iter().zip(&statuses).zip(&lab.photos) {
        if st == "ready" {
            assert_eq!(c, &serde_json::json!(["森 Morii"]), "{}", photo.display());
        } else {
            assert_eq!(blake(photo).as_deref(), Some(lab.truth[photo].as_str()));
        }
    }
    assert!(lab.cli(&["fsck", &op]).status.success());
    // re-planning the same value is a no-op
    let again = lab.plan("森 Morii", "p2.json");
    let s: Value = serde_json::from_slice(&std::fs::read(&again).unwrap()).unwrap();
    assert!(
        s["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["status"]["status"] == "no_change" || e["status"]["status"] == "blocked")
    );
    lab.undo(&op);
    lab.assert_all_original();
    assert!(lab.leftovers().is_empty());
}

#[test]
fn crash_at_every_step_recovers_resumes_and_undoes() {
    let pkg = require!();
    for seq in [0u32, 3] {
        for step in 1u8..=10 {
            let lab = Lab::new(&format!("crash-{seq}-{step}"), &pkg);
            let plan = lab.plan("Morii", "p.json");
            let o = lab.cli_env(
                &[
                    "apply",
                    plan.to_str().unwrap(),
                    "--crash-at",
                    &format!("{seq}:{step}"),
                ],
                true,
            );
            assert_eq!(
                o.status.code(),
                Some(77),
                "crash {seq}:{step} did not happen: {}",
                String::from_utf8_lossy(&o.stdout)
            );
            let op = lab.last_op();
            lab.assert_preimages(&op);
            // no new write may start while recovery is pending
            let other = lab.plan("Someone", "p-other.json");
            let refused = lab.cli(&["apply", other.to_str().unwrap()]);
            assert!(
                !refused.status.success()
                    && String::from_utf8_lossy(&refused.stdout).contains("RecoveryPending")
            );
            let r = lab.cli(&["recover"]);
            assert!(r.status.success());
            lab.assert_recovered(&op);
            let res = lab.cli(&["resume", &op]);
            assert!(
                res.status.success(),
                "resume {seq}:{step}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
            assert!(lab.cli(&["fsck", &op]).status.success());
            assert!(
                creators(&lab)
                    .iter()
                    .all(|c| c == &serde_json::json!(["Morii"])),
                "after resume {seq}:{step}"
            );
            lab.undo(&op);
            lab.assert_all_original();
            let _ = std::fs::remove_dir_all(&lab.dir);
        }
    }
}

#[test]
fn random_kills_recover() {
    let pkg = require!();
    let iterations: u64 = std::env::var("MM_E2E_KILLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut rng: u64 = 0x2026_0927;
    for i in 0..iterations {
        let lab = Lab::new(&format!("kill-{i}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let delay = Duration::from_millis(150 + (rng >> 33) % 900);
        let mut child = Command::new(env!("CARGO_BIN_EXE_mm-cli"))
            .arg("--data")
            .arg(&lab.data)
            .arg("--exiftool")
            .arg(&lab.pkg)
            .args(["apply", plan.to_str().unwrap()])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(delay);
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(100));
        let h = Lab::json(&lab.cli(&["history"]));
        let Some(op) = h
            .as_array()
            .unwrap()
            .last()
            .map(|o| o["id"].as_str().unwrap().to_owned())
        else {
            lab.assert_all_original(); // killed before the operation was registered
            continue;
        };
        lab.assert_preimages(&op);
        assert!(lab.cli(&["recover"]).status.success());
        lab.assert_recovered(&op);
        let res = lab.cli(&["resume", &op]);
        assert!(
            res.status.success(),
            "{}",
            String::from_utf8_lossy(&res.stdout)
        );
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

#[test]
#[allow(clippy::permissions_set_readonly_false)] // Windows: clears FILE_ATTRIBUTE_READONLY
fn changed_locked_readonly_and_hardlinked_files_are_not_written() {
    let pkg = require!();
    let lab = Lab::new("guards", &pkg);
    let ro = &lab.photos[1];
    let mut perm = std::fs::metadata(ro).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(ro, perm.clone()).unwrap();
    std::fs::hard_link(&lab.photos[2], lab.dir.join("second-link.jpg")).unwrap();
    let plan = lab.plan("Morii", "p.json");
    let p: Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
    let st = |i: usize| {
        p["entries"][i]["status"]["status"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(st(1), "blocked");
    assert_eq!(st(2), "blocked");
    // file 0 changes after the preview; file 3 is held open exclusively during apply
    std::thread::sleep(Duration::from_millis(20));
    let bytes = std::fs::read(&lab.photos[0]).unwrap();
    std::fs::write(&lab.photos[0], &bytes).unwrap();
    let changed_hash = blake(&lab.photos[0]).unwrap();
    let held = {
        use std::os::windows::fs::OpenOptionsExt;
        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&lab.photos[3])
            .unwrap()
    };
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    drop(held);
    assert_eq!(a.status.code(), Some(3));
    let r = Lab::json(&a);
    let state = |i: usize| {
        r["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["seq"] == i)
            .unwrap()["state"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(state(0), "conflict");
    assert_eq!(state(3), "skipped");
    assert_eq!(state(4), "done");
    assert_eq!(blake(&lab.photos[0]).unwrap(), changed_hash);
    assert_eq!(
        blake(&lab.photos[3]).as_deref(),
        Some(lab.truth[&lab.photos[3]].as_str())
    );
    assert!(
        lab.cli(&["fsck", r["op_id"].as_str().unwrap()])
            .status
            .success()
    );
    perm.set_readonly(false);
    std::fs::set_permissions(ro, perm).unwrap();
}

#[test]
fn latin_iptc_blocks_cjk_and_updates_digest_for_latin_names() {
    let pkg = require!();
    let lab = Lab::new("iptc", &pkg);
    let iptc = lab.dir.join("photos").join("iptc.jpg");
    // IPTC without CodedCharacterSet, with XMP and a current IPTCDigest
    let mk = Command::new(pkg.join("exiftool.exe"))
        .args([
            "-config",
            "",
            "-IPTC:By-line=Cafe",
            "-XMP-dc:Creator=Cafe",
            "-Photoshop:IPTCDigest=new",
            "-o",
        ])
        .arg(&iptc)
        .arg(timages().join("Writer.jpg"))
        .output()
        .unwrap();
    assert!(mk.status.success());
    let out = lab.dir.join("cjk.json");
    let o = lab.cli(&[
        "plan-creator",
        "--set",
        "森",
        "--out",
        out.to_str().unwrap(),
        iptc.to_str().unwrap(),
    ]);
    assert!(String::from_utf8_lossy(&o.stdout).contains("Latin character set"));
    let out2 = lab.dir.join("latin.json");
    lab.cli(&[
        "plan-creator",
        "--set",
        "Zoë Morii",
        "--out",
        out2.to_str().unwrap(),
        iptc.to_str().unwrap(),
    ]);
    let a = lab.cli(&["apply", out2.to_str().unwrap()]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    lab.undo(&op);
}

#[test]
fn injected_io_errors_are_settled_without_recovery() {
    let pkg = require!();
    for step in 1u8..=10 {
        let lab = Lab::new(&format!("ioerr-{step}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--fail-at",
                &format!("2:{step}"),
            ],
            true,
        );
        let r = Lab::json(&o);
        let op = r["op_id"]
            .as_str()
            .unwrap_or_else(|| panic!("step {step}: {r}"))
            .to_owned();
        assert!(
            r["status"] != "running",
            "step {step} left the operation running"
        );
        let f2 = r["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["seq"] == 2)
            .unwrap()
            .clone();
        let target = &lab.photos[2];
        if step <= 7 {
            // before the commit: the file is reported failed and was not changed
            assert_eq!(f2["state"], "failed", "step {step}: {f2}");
            assert_eq!(
                blake(target).as_deref(),
                Some(lab.truth[target].as_str()),
                "step {step}"
            );
        } else {
            // the commit had happened: the file is done
            assert_eq!(f2["state"], "done", "step {step}: {f2}");
        }
        for f in r["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["seq"] != 2)
        {
            assert_eq!(f["state"], "done", "step {step}: {f}");
        }
        assert!(
            lab.leftovers().is_empty(),
            "step {step}: {:?}",
            lab.leftovers()
        );
        assert!(lab.cli(&["fsck", &op]).status.success(), "step {step}");
        assert!(lab.cli(&["recover"]).status.success());
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

fn state_of(report: &Value, seq: u64) -> String {
    report["files"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["seq"] == seq)
        .unwrap_or_else(|| panic!("no file {seq} in {report}"))["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

fn op_status(lab: &Lab, op: &str) -> String {
    lab.show(op)["status"].as_str().unwrap().to_owned()
}

/// G-1: journal writes fail inside SQLite (another connection holds the write lock), once or for
/// the rest of the process, at every journal write of the file transaction and at the Operation's
/// begin and end. Nothing irreversible may happen without its record (I-8), and every outcome must
/// recover to H0/H1 and undo byte-identically.
#[test]
fn journal_write_failures_are_settled_or_recovered() {
    let pkg = require!();
    let targets = [
        "begin",
        "2:backed_up",
        "2:ready",
        "2:committed",
        "2:done",
        "finish",
    ];
    for target in targets {
        for persist in [false, true] {
            let case = format!("{target}{}", if persist { " persistent" } else { " once" });
            let lab = Lab::new(
                &format!("journal-{}-{persist}", target.replace(':', "-")),
                &pkg,
            );
            let plan = lab.plan("Morii", "p.json");
            let mut args = vec!["apply", plan.to_str().unwrap(), "--journal-fail-at", target];
            if persist {
                args.push("--journal-fail-persist");
            }
            let o = lab.cli_env(&args, true);
            let out = String::from_utf8_lossy(&o.stdout).into_owned();
            if target == "begin" {
                // the registering transaction failed: no Operation, no file touched
                assert!(
                    !o.status.success() && out.contains("DatabaseBusy"),
                    "{case}: {out}"
                );
                let h = Lab::json(&lab.cli(&["history"]));
                assert!(h.as_array().unwrap().is_empty(), "{case}: {h}");
                lab.assert_all_original();
                assert!(lab.leftovers().is_empty(), "{case}");
                let _ = std::fs::remove_dir_all(&lab.dir);
                continue;
            }
            assert!(
                out.contains("DatabaseBusy"),
                "{case}: failure not seen: {out}"
            );
            let op = lab.last_op();
            lab.assert_preimages(&op);
            let target_file = &lab.photos[2];
            let before_commit = target == "2:backed_up" || target == "2:ready";
            if before_commit {
                // the file was never committed without its Ready record (I-8)
                assert_eq!(
                    blake(target_file).as_deref(),
                    Some(lab.truth[target_file].as_str()),
                    "{case}"
                );
            }
            if !persist && target != "finish" {
                // settled in-process: the Operation ended, file 2 has a truthful state
                let r = Lab::json(&o);
                assert_ne!(r["status"], "running", "{case}");
                let want = if before_commit { "failed" } else { "done" };
                assert_eq!(state_of(&r, 2), want, "{case}: {r}");
            } else {
                // the journal stayed unavailable: the Operation is left for recovery
                assert_eq!(op_status(&lab, &op), "running", "{case}");
                let other = lab.plan("Someone", "p-other.json");
                let refused = lab.cli(&["apply", other.to_str().unwrap()]);
                assert!(
                    String::from_utf8_lossy(&refused.stdout).contains("RecoveryPending"),
                    "{case}"
                );
            }
            assert!(lab.cli(&["recover"]).status.success(), "{case}");
            lab.assert_recovered(&op);
            let res = lab.cli(&["resume", &op]);
            assert!(
                res.status.success() || res.status.code() == Some(3),
                "{case}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
            assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
            lab.undo(&op);
            lab.assert_all_original();
            assert!(lab.leftovers().is_empty(), "{case}");
            let _ = std::fs::remove_dir_all(&lab.dir);
        }
    }
}

/// The Undo Operation writes JPEGs through the same transaction (Restore branch): crash at every
/// fault point and inject an IO error at every point, then recover / re-undo to the originals.
#[test]
fn undo_path_crashes_and_io_errors_recover() {
    let pkg = require!();
    for (kind, step) in (1u8..=10)
        .map(|s| ("crash", s))
        .chain((1u8..=10).map(|s| ("fail", s)))
    {
        let case = format!("undo {kind} at 2:{step}");
        let lab = Lab::new(&format!("undo-{kind}-{step}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let a = lab.cli(&["apply", plan.to_str().unwrap()]);
        assert!(a.status.success(), "{case}");
        let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
        let applied = lab.snapshot();
        let u = lab.dir.join("undo.json");
        assert!(
            lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()])
                .status
                .success()
        );
        let flag = if kind == "crash" {
            "--crash-at"
        } else {
            "--fail-at"
        };
        let o = lab.cli_env(
            &["apply", u.to_str().unwrap(), flag, &format!("2:{step}")],
            true,
        );
        let undo_op = lab.last_op();
        assert_ne!(undo_op, op, "{case}: undo operation not registered");
        if kind == "crash" {
            assert_eq!(o.status.code(), Some(77), "{case}");
            lab.assert_preimages_of(&undo_op, &applied);
            assert!(lab.cli(&["recover"]).status.success(), "{case}");
            lab.assert_recovered_of(&undo_op, &applied);
            let res = lab.cli(&["resume", &undo_op]);
            assert!(
                res.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
            assert!(lab.cli(&["fsck", &undo_op]).status.success(), "{case}");
        } else {
            let r = Lab::json(&o);
            assert_ne!(r["status"], "running", "{case}");
            let want = if step <= 7 { "failed" } else { "done" };
            assert_eq!(state_of(&r, 2), want, "{case}: {r}");
            lab.assert_recovered_of(&undo_op, &applied);
            // a failed restore leaves the file as it was; undoing the original operation again
            // restores whatever is still changed (afterwards this Undo's fsck would rightly
            // report file 2 as changed later, so it is checked by assert_recovered_of above)
            if step <= 7 {
                assert_eq!(
                    blake(&lab.photos[2]).as_deref(),
                    Some(applied[&lab.photos[2]].as_str()),
                    "{case}"
                );
                lab.undo(&op);
            }
        }
        lab.assert_all_original();
        assert!(lab.leftovers().is_empty(), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// SAFETY_MODEL §8.13 with a simulated ERROR_DISK_FULL at every fault point: the Operation pauses
/// (the file in progress and all later files are Cancelled, originals unchanged), and resume
/// completes it once space is available.
#[test]
fn disk_full_pauses_the_operation_and_resume_completes_it() {
    let pkg = require!();
    for step in 1u8..=10 {
        let case = format!("disk full at 2:{step}");
        let lab = Lab::new(&format!("diskfull-{step}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--disk-full-at",
                &format!("2:{step}"),
            ],
            true,
        );
        assert_eq!(o.status.code(), Some(3), "{case}");
        let r = Lab::json(&o);
        let op = r["op_id"].as_str().unwrap().to_owned();
        assert_eq!(r["status"], "cancelled", "{case}: {r}");
        assert!(r["note"].as_str().unwrap().contains("full"), "{case}: {r}");
        for seq in 0..SAMPLES.len() as u64 {
            let want = match seq {
                0 | 1 => "done",
                2 if step >= 8 => "done",
                _ => "cancelled",
            };
            assert_eq!(state_of(&r, seq), want, "{case} file {seq}: {r}");
        }
        for (i, p) in lab.photos.iter().enumerate().skip(2) {
            if i > 2 || step <= 7 {
                assert_eq!(blake(p).as_deref(), Some(lab.truth[p].as_str()), "{case}");
            }
        }
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
        let res = lab.cli(&["resume", &op]);
        assert!(
            res.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        assert!(
            creators(&lab)
                .iter()
                .all(|c| c == &serde_json::json!(["Morii"])),
            "{case}"
        );
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// SAFETY_MODEL §6.2: without enough free space the Operation is refused before anything is
/// registered or written.
#[test]
fn space_precheck_refuses_before_any_write() {
    let pkg = require!();
    let lab = Lab::new("space", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let o = lab.cli_env(
        &[
            "apply",
            plan.to_str().unwrap(),
            "--space-reserve",
            &(1u64 << 62).to_string(),
        ],
        true,
    );
    let out = String::from_utf8_lossy(&o.stdout);
    assert!(
        !o.status.success() && out.contains("InsufficientSpace"),
        "{out}"
    );
    assert!(
        Lab::json(&lab.cli(&["history"]))
            .as_array()
            .unwrap()
            .is_empty()
    );
    lab.assert_all_original();
    // the normal reserve (1 GiB) passes on the test volume
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// G-1, real disk full: needs `MM_E2E_SMALL_VOLUME` = a directory on a volume of at most 2 GiB
/// (e.g. a VHDX made with `tests/fault-lab/small_volume.ps1`, as administrator). The volume is
/// really filled at a fault point; afterwards the filler is deleted and the Operation resumed.
/// Scenario "photos": the photo volume fills before ExifTool writes the temporary file.
/// Scenario "data": the volume of backups and journal fills before the backup copy.
#[test]
fn real_disk_full_on_small_volume() {
    let pkg = require!();
    let Some(small) = std::env::var_os("MM_E2E_SMALL_VOLUME").map(PathBuf::from) else {
        eprintln!("SKIP: MM_E2E_SMALL_VOLUME not set (needs a small test volume)");
        return;
    };
    for (scenario, point) in [("photos", "2:4"), ("data", "2:2")] {
        let case = format!("real disk full: {scenario} at {point}");
        let base =
            std::env::temp_dir().join(format!("mm-e2e-realfull-{scenario}-{}", std::process::id()));
        let on_small = small.join(format!("mm-e2e-{scenario}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&on_small);
        let fill = on_small.join("fill");
        std::fs::create_dir_all(&fill).unwrap();
        let (photos, data) = if scenario == "photos" {
            (on_small.join("photos"), base.join("data"))
        } else {
            (base.join("photos"), on_small.join("data"))
        };
        let lab = Lab::at(&pkg, base.clone(), photos, data);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--space-reserve",
                "0",
                "--fill-at",
                point,
                "--fill-dir",
                fill.to_str().unwrap(),
            ],
            true,
        );
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        eprintln!("{case}: exit {:?}: {out}", o.status.code());
        let fillers: Vec<PathBuf> = std::fs::read_dir(&fill)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert!(!fillers.is_empty(), "{case}: the volume was not filled");
        let op = lab.last_op();
        lab.assert_preimages(&op);
        // while the volume is still full, recovery may fail but must not lose anything
        let r = lab.cli(&["recover"]);
        eprintln!(
            "{case}: recover while full: exit {:?}: {}",
            r.status.code(),
            String::from_utf8_lossy(&r.stdout)
        );
        lab.assert_preimages(&op);
        for f in fillers {
            std::fs::remove_file(f).unwrap();
        }
        assert!(lab.cli(&["recover"]).status.success(), "{case}");
        lab.assert_recovered(&op);
        eprintln!("{case}: after recovery: {}", lab.show(&op));
        // a small test volume can never hold the normal 1 GiB backup reserve
        let res = lab.cli_env(&["resume", &op, "--space-reserve", "0"], true);
        assert!(
            res.status.success() || res.status.code() == Some(3),
            "{case}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
        let u = lab.dir.join("undo.json");
        let p = lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()]);
        assert!(p.status.success(), "{case}");
        let a = lab.cli_env(
            &["apply", u.to_str().unwrap(), "--space-reserve", "0"],
            true,
        );
        assert!(
            a.status.success(),
            "{case}: undo: {}",
            String::from_utf8_lossy(&a.stdout)
        );
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&base);
        let _ = std::fs::remove_dir_all(&on_small);
    }
}

/// manifest.json (the redundant record next to the backups) cannot be written: at Operation
/// begin, at its end, and during recovery. It must never lead to an unrecorded irreversible step.
#[test]
fn manifest_write_failures_keep_invariants() {
    let pkg = require!();
    // (where, fault, persistent)
    let cases = [
        ("begin", "manifest:0", false),
        ("begin", "manifest:0", true),
        ("finish", "manifest:1", false),
        ("finish", "manifest:1", true),
        ("recover", "manifest:0", true),
    ];
    for (at, fault, persist) in cases {
        let case = format!("manifest at {at} ({fault}, persistent {persist})");
        let lab = Lab::new(&format!("manifest-{at}-{persist}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let mut faults = vec!["--journal-fail-at", fault];
        if persist {
            faults.push("--journal-fail-persist");
        }
        let op = if at == "recover" {
            let o = lab.cli_env(
                &["apply", plan.to_str().unwrap(), "--crash-at", "2:7"],
                true,
            );
            assert_eq!(o.status.code(), Some(77), "{case}");
            let op = lab.last_op();
            let mut args = vec!["recover"];
            args.extend(&faults);
            let r = lab.cli_env(&args, true);
            let out = String::from_utf8_lossy(&r.stdout).into_owned();
            assert!(!r.status.success() && out.contains("Io("), "{case}: {out}");
            op
        } else {
            let mut args = vec!["apply", plan.to_str().unwrap()];
            args.extend(&faults);
            let o = lab.cli_env(&args, true);
            let out = String::from_utf8_lossy(&o.stdout).into_owned();
            assert!(!o.status.success() && out.contains("Io("), "{case}: {out}");
            let op = lab.last_op();
            if at == "begin" {
                // registered, nothing touched yet
                assert_eq!(op_status(&lab, &op), "running", "{case}");
                lab.assert_all_original();
            } else {
                // every file was done before the manifest failed
                assert_eq!(op_status(&lab, &op), "completed", "{case}");
                assert!(
                    creators(&lab)
                        .iter()
                        .all(|c| c == &serde_json::json!(["Morii"])),
                    "{case}"
                );
            }
            op
        };
        lab.assert_preimages(&op);
        assert!(lab.cli(&["recover"]).status.success(), "{case}");
        lab.assert_recovered(&op);
        let res = lab.cli(&["resume", &op]);
        assert!(
            res.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        lab.undo(&op);
        lab.assert_all_original();
        assert!(lab.leftovers().is_empty(), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// Recovery itself loses its journal: it has already put a file back from its bak name (a crash
/// inside ReplaceFileW) when recording that fails. Running recovery again must finish the job.
#[test]
fn recovery_journal_failure_is_retried_safely() {
    let pkg = require!();
    for persist in [false, true] {
        let case = format!("recover journal failure (persistent {persist})");
        let lab = Lab::new(&format!("recover-journal-{persist}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &["apply", plan.to_str().unwrap(), "--crash-at", "2:7"],
            true,
        );
        assert_eq!(o.status.code(), Some(77), "{case}");
        let op = lab.last_op();
        // the state a termination inside ReplaceFileW leaves (SAFETY_MODEL §4.5, S2: 14/300 kills):
        // the original path is gone, the original content is under the registered bak name
        let show = lab.show(&op);
        let row = show["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["seq"] == 2)
            .unwrap();
        assert_eq!(row["state"], "ready", "{case}");
        let bak = PathBuf::from(row["bak"].as_str().unwrap());
        std::fs::rename(&lab.photos[2], &bak).unwrap();
        lab.assert_preimages(&op);
        let mut args = vec!["recover", "--journal-fail-at", "2:not_started"];
        if persist {
            args.push("--journal-fail-persist");
        }
        let r = lab.cli_env(&args, true);
        let out = String::from_utf8_lossy(&r.stdout).into_owned();
        assert!(
            !r.status.success() && out.contains("DatabaseBusy"),
            "{case}: {out}"
        );
        // the disk action happened, its record did not: the original is back at its path
        assert_eq!(
            blake(&lab.photos[2]).as_deref(),
            Some(lab.truth[&lab.photos[2]].as_str()),
            "{case}"
        );
        lab.assert_preimages(&op);
        assert!(lab.cli(&["recover"]).status.success(), "{case}");
        lab.assert_recovered(&op);
        let res = lab.cli(&["resume", &op]);
        assert!(
            res.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// `resume` loses its journal while re-registering the files it will retry: no file may be left
/// in a non-terminal state inside an Operation that recovery no longer looks at.
#[test]
fn resume_journal_failure_leaves_operation_recoverable() {
    let pkg = require!();
    let lab = Lab::new("resume-journal", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let o = lab.cli_env(
        &["apply", plan.to_str().unwrap(), "--disk-full-at", "2:4"],
        true,
    );
    let op = Lab::json(&o)["op_id"].as_str().unwrap().to_owned();
    assert_eq!(op_status(&lab, &op), "cancelled");
    let r = lab.cli_env(
        &[
            "resume",
            &op,
            "--journal-fail-at",
            "4:planned",
            "--journal-fail-persist",
        ],
        true,
    );
    let out = String::from_utf8_lossy(&r.stdout).into_owned();
    assert!(!r.status.success() && out.contains("DatabaseBusy"), "{out}");
    assert_eq!(
        op_status(&lab, &op),
        "running",
        "files were re-registered but the Operation is not visible to recovery: {}",
        lab.show(&op)
    );
    lab.assert_preimages(&op);
    assert!(lab.cli(&["recover"]).status.success());
    lab.assert_recovered(&op);
    let res = lab.cli(&["resume", &op]);
    assert!(
        res.status.success(),
        "{}",
        String::from_utf8_lossy(&res.stdout)
    );
    assert!(
        creators(&lab)
            .iter()
            .all(|c| c == &serde_json::json!(["Morii"]))
    );
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// Random termination of the Undo Operation (the Restore branch of the transaction).
#[test]
fn random_kills_during_undo_recover() {
    let pkg = require!();
    let iterations: u64 = std::env::var("MM_E2E_KILLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(12);
    let mut rng: u64 = 0x2026_0928;
    for i in 0..iterations {
        let lab = Lab::new(&format!("undo-kill-{i}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let a = lab.cli(&["apply", plan.to_str().unwrap()]);
        assert!(a.status.success());
        let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
        let applied = lab.snapshot();
        let u = lab.dir.join("undo.json");
        assert!(
            lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()])
                .status
                .success()
        );
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let delay = Duration::from_millis(50 + (rng >> 33) % 700);
        let mut child = Command::new(env!("CARGO_BIN_EXE_mm-cli"))
            .arg("--data")
            .arg(&lab.data)
            .arg("--exiftool")
            .arg(&lab.pkg)
            .args(["apply", u.to_str().unwrap()])
            .stdout(std::process::Stdio::null())
            .spawn()
            .unwrap();
        std::thread::sleep(delay);
        let _ = child.kill();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(100));
        let undo_op = lab.last_op();
        if undo_op == op {
            // killed before the Undo Operation was registered
            assert_eq!(lab.snapshot(), applied, "undo kill {i}");
            lab.undo(&op);
        } else {
            lab.assert_preimages_of(&undo_op, &applied);
            assert!(lab.cli(&["recover"]).status.success(), "undo kill {i}");
            lab.assert_recovered_of(&undo_op, &applied);
            let res = lab.cli(&["resume", &undo_op]);
            assert!(
                res.status.success(),
                "undo kill {i}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
        }
        lab.assert_all_original();
        assert!(lab.leftovers().is_empty(), "undo kill {i}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

fn plan_entry(plan: &Value, seq: u64) -> Value {
    plan["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["seq"] == seq)
        .unwrap_or_else(|| panic!("no entry {seq} in {plan}"))
        .clone()
}

/// G-6 / SAFETY_MODEL §7.2: Undo recreates a file that was deleted or moved after the operation,
/// at its original path, from the verified backup; a moved copy elsewhere is not touched. Undoing
/// that Undo does not remove the recreated file (explicitly Blocked for now).
#[test]
fn undo_recreates_deleted_and_moved_files() {
    let pkg = require!();
    let lab = Lab::new("recreate", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success());
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    let applied = lab.snapshot();
    std::fs::remove_file(&lab.photos[2]).unwrap();
    std::fs::create_dir_all(lab.dir.join("moved")).unwrap();
    let moved = lab.dir.join("moved").join(SAMPLES[3]);
    std::fs::rename(&lab.photos[3], &moved).unwrap();

    let u = lab.dir.join("undo.json");
    let p = lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()]);
    assert!(p.status.success());
    let pj = Lab::json(&p);
    for seq in [2, 3] {
        let e = plan_entry(&pj, seq);
        assert_eq!(e["status"]["status"], "ready", "{e}");
        assert!(
            e["notes"][0].as_str().unwrap().contains("deleted or moved"),
            "{e}"
        );
    }
    let r = lab.cli(&["apply", u.to_str().unwrap()]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stdout));
    let undo_op = Lab::json(&r)["op_id"].as_str().unwrap().to_owned();
    lab.assert_all_original();
    assert_eq!(
        blake(&moved).as_deref(),
        Some(applied[&lab.photos[3]].as_str()),
        "the moved copy must stay as it was"
    );
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    assert!(lab.cli(&["fsck", &undo_op]).status.success());
    let states: Vec<String> = Lab::json(&lab.cli(&["show", &undo_op]))["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["state"].as_str().unwrap().to_owned())
        .collect();
    assert!(states.iter().all(|s| s == "done"), "{states:?}");

    // undoing the Undo: the recreated files are Blocked and left alone, the others change back
    let uu = lab.dir.join("undo2.json");
    let p2 = lab.cli(&["plan-undo", &undo_op, "--out", uu.to_str().unwrap()]);
    assert!(p2.status.success());
    let p2j = Lab::json(&p2);
    for seq in [2, 3] {
        let e = plan_entry(&p2j, seq);
        assert_eq!(e["status"]["status"], "blocked", "{e}");
        assert!(
            e["status"]["reason"]
                .as_str()
                .unwrap()
                .contains("recreated"),
            "{e}"
        );
    }
    let r2 = lab.cli(&["apply", uu.to_str().unwrap()]);
    assert!(
        r2.status.success(),
        "{}",
        String::from_utf8_lossy(&r2.stdout)
    );
    for (i, p) in lab.photos.iter().enumerate() {
        let want = if i == 2 || i == 3 {
            &lab.truth[p]
        } else {
            &applied[p]
        };
        assert_eq!(blake(p).as_deref(), Some(want.as_str()), "{}", p.display());
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// A file that appears at the path between the Undo preview and its execution is never replaced
/// (I-6); a folder that no longer exists is never created.
#[test]
fn undo_recreate_never_replaces_and_never_creates_folders() {
    let pkg = require!();
    let lab = Lab::new("recreate-guards", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success());
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    std::fs::remove_file(&lab.photos[2]).unwrap();
    let u = lab.dir.join("undo.json");
    assert!(
        lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()])
            .status
            .success()
    );
    std::fs::write(&lab.photos[2], b"a new file the user put here").unwrap();
    let r = lab.cli(&["apply", u.to_str().unwrap()]);
    assert_eq!(
        r.status.code(),
        Some(3),
        "{}",
        String::from_utf8_lossy(&r.stdout)
    );
    let rj = Lab::json(&r);
    assert_eq!(state_of(&rj, 2), "conflict", "{rj}");
    assert_eq!(
        std::fs::read(&lab.photos[2]).unwrap(),
        b"a new file the user put here"
    );
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    assert!(
        lab.cli(&["fsck", rj["op_id"].as_str().unwrap()])
            .status
            .success()
    );

    // the whole folder is gone: nothing is planned, nothing is created
    let lab2 = Lab::new("recreate-folder", &pkg);
    let plan2 = lab2.plan("Morii", "p.json");
    let a2 = lab2.cli(&["apply", plan2.to_str().unwrap()]);
    assert!(a2.status.success());
    let op2 = Lab::json(&a2)["op_id"].as_str().unwrap().to_owned();
    let folder = lab2.photos[0].parent().unwrap().to_path_buf();
    let elsewhere = lab2.dir.join("photos-renamed");
    std::fs::rename(&folder, &elsewhere).unwrap();
    let u2 = lab2.dir.join("undo.json");
    let p2 = lab2.cli(&["plan-undo", &op2, "--out", u2.to_str().unwrap()]);
    assert!(p2.status.success());
    for e in Lab::json(&p2)["entries"].as_array().unwrap() {
        assert_eq!(e["status"]["status"], "blocked", "{e}");
        assert!(
            e["status"]["reason"].as_str().unwrap().contains("folder"),
            "{e}"
        );
    }
    lab2.cli(&["apply", u2.to_str().unwrap()]); // nothing executable
    assert!(!folder.exists(), "a folder was created");
    let _ = std::fs::remove_dir_all(&lab.dir);
    let _ = std::fs::remove_dir_all(&lab2.dir);
}

/// Crash and IO error at every fault point of the recreate transaction (1, 5–10).
#[test]
fn recreate_crashes_and_io_errors_recover() {
    let pkg = require!();
    for (kind, step) in [1u8, 5, 6, 7, 8, 9, 10]
        .into_iter()
        .map(|s| ("crash", s))
        .chain([1u8, 5, 6, 7, 8, 9, 10].into_iter().map(|s| ("fail", s)))
    {
        let case = format!("recreate {kind} at 2:{step}");
        let lab = Lab::new(&format!("recreate-{kind}-{step}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let a = lab.cli(&["apply", plan.to_str().unwrap()]);
        assert!(a.status.success(), "{case}");
        let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
        std::fs::remove_file(&lab.photos[2]).unwrap();
        let others = lab.snapshot_where(|i| i != 2);
        let target = lab.photos[2].clone();
        let original = lab.truth[&target].clone();
        let u = lab.dir.join("undo.json");
        assert!(
            lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()])
                .status
                .success(),
            "{case}"
        );
        let flag = if kind == "crash" {
            "--crash-at"
        } else {
            "--fail-at"
        };
        let o = lab.cli_env(
            &["apply", u.to_str().unwrap(), flag, &format!("2:{step}")],
            true,
        );
        let undo_op = lab.last_op();
        assert_ne!(undo_op, op, "{case}");
        // the recreated path only ever holds nothing or the complete original
        let absent_or_original = |when: &str| {
            let cur = blake(&target);
            assert!(
                cur.is_none() || cur.as_deref() == Some(original.as_str()),
                "{case} {when}: path holds something else"
            );
        };
        if kind == "crash" {
            assert_eq!(o.status.code(), Some(77), "{case}");
            absent_or_original("before recovery");
            lab.assert_preimages_of(&undo_op, &others);
            assert!(lab.cli(&["recover"]).status.success(), "{case}");
            absent_or_original("after recovery");
            lab.assert_recovered_of(&undo_op, &others);
            let res = lab.cli(&["resume", &undo_op]);
            assert!(
                res.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
            assert!(lab.cli(&["fsck", &undo_op]).status.success(), "{case}");
        } else {
            let r = Lab::json(&o);
            assert_ne!(r["status"], "running", "{case}");
            let want = if step <= 7 { "failed" } else { "done" };
            assert_eq!(state_of(&r, 2), want, "{case}: {r}");
            absent_or_original("after the error");
            lab.assert_recovered_of(&undo_op, &others);
            if step <= 7 {
                assert!(blake(&target).is_none(), "{case}");
                lab.undo(&op); // plans the recreate again
            }
        }
        lab.assert_all_original();
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}
