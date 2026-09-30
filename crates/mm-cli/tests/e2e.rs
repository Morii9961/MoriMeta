// SPDX-License-Identifier: GPL-3.0-or-later
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

/// The temporary folder in the long form mm-cli reports paths in (a runner's TEMP can be an 8.3
/// short name such as `RUNNER~1`, which mm-cli normalizes).
fn tmp() -> PathBuf {
    mm_core::normalize(&std::env::temp_dir()).unwrap()
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

/// BLAKE3 of the pinned ExifTool 13.59 fixtures (their SHA-256 are in
/// docs/PHASE1B_SCALE_VALIDATION.md). Every test copies them; a changed fixture would silently
/// change what the tests prove, so it stops the test instead.
const FIXTURE_BLAKE3: &[(&str, &str)] = &[
    (
        "Writer.jpg",
        "e1814acf81acf70055b3f31acfb56c1e25c534420943da52fc4e85d84dcc979f",
    ),
    (
        "Nikon.jpg",
        "0e3dc126263b2f0da7787f98e9447f3d39bda3f6e9bee6307beca53fb331c7f2",
    ),
    (
        "Canon.jpg",
        "230c0981f567bc4ece255546a61d0ac96ee320f23663bdf5b5335468ecf4d0e3",
    ),
    (
        "XMP.jpg",
        "df0e53915a22893064161b34f0a4bae3f8280575e7cb0647db0469f0d7132d92",
    ),
    (
        "Sony.jpg",
        "85c14a041daa2d5fa5aba2f54fe1d965489fc1ea1cee0ea9929ab501475735d8",
    ),
    (
        "Olympus.jpg",
        "babeb761ad20c65dab557b6c982cf0695284336ee36eadce2909edc6dbef8a03",
    ),
    (
        "Pentax.jpg",
        "c48f9f59953b15e9f7eee76d632bd6f54efc20b0a556f287031fe30408e078b6",
    ),
    (
        "GPS.jpg",
        "eea1397afd8d9160d3adf3a2b4b3c0f271c0d448c81b4b4242719f65f64e86c7",
    ),
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
        let dir = tmp().join(format!("mm-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Lab::at(pkg, dir.clone(), dir.join("photos"), dir.join("data"), 1)
    }

    /// Photos and application data in chosen places (e.g. on a small test volume).
    /// `copies` of every fixture (named `<n>-<fixture>` when more than one).
    fn at(pkg: &Path, dir: PathBuf, photo_dir: PathBuf, data: PathBuf, copies: usize) -> Lab {
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::create_dir_all(&photo_dir).unwrap();
        let mut photos = Vec::new();
        let mut truth = BTreeMap::new();
        for (c, s) in (0..copies).flat_map(|c| SAMPLES.iter().map(move |s| (c, s))) {
            let p = if copies == 1 {
                photo_dir.join(s)
            } else {
                photo_dir.join(format!("{c}-{s}"))
            };
            let src = timages().join(s);
            let want = FIXTURE_BLAKE3.iter().find(|(n, _)| n == s).unwrap().1;
            assert_eq!(
                blake(&src).as_deref(),
                Some(want),
                "pinned fixture {s} changed; restore it (SHA-256 in docs/PHASE1B_SCALE_VALIDATION.md)"
            );
            std::fs::copy(&src, &p).unwrap();
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

    fn many(name: &str, pkg: &Path, copies: usize) -> Lab {
        let dir = tmp().join(format!("mm-e2e-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        Lab::at(
            pkg,
            dir.clone(),
            dir.join("photos"),
            dir.join("data"),
            copies,
        )
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
    for seq in [0u32, 3, 7] {
        for step in 1u8..=10 {
            let lab = Lab::new(&format!("crash-{seq}-{step}"), &pkg);
            let plan = lab.plan("Morii", "p.json");
            // planned while every path exists: with parallel workers the crash can land inside
            // another file's ReplaceFileW (original only under its bak name until recovery)
            let other = lab.plan("Someone", "p-other.json");
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
        // the first and the last file
        "0:ready",
        "0:committed",
        "7:backed_up",
        "7:done",
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
            let seq = target
                .split_once(':')
                .and_then(|(s, _)| s.parse::<usize>().ok())
                .unwrap_or(2);
            let target_file = &lab.photos[seq];
            let before_commit = target.ends_with(":backed_up") || target.ends_with(":ready");
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
                assert_eq!(state_of(&r, seq as u64), want, "{case}: {r}");
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
        // one worker: the exact "this file and every later one" pattern needs sequential order
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--disk-full-at",
                &format!("2:{step}"),
                "--workers",
                "1",
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
        let base = tmp().join(format!("mm-e2e-realfull-{scenario}-{}", std::process::id()));
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
        let lab = Lab::at(&pkg, base.clone(), photos, data, 1);
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
        &[
            "apply",
            plan.to_str().unwrap(),
            "--disk-full-at",
            "2:4",
            "--workers",
            "1",
        ],
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
/// that Undo moves the recreated files into the backup store (never deletes them), and undoing
/// that recreates them again: every Undo is itself undoable (PRODUCT_SPEC §6.14).
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

    // undoing the Undo: the recreated files go into the backup store, the others change back
    let uu = lab.dir.join("undo2.json");
    let p2 = lab.cli(&["plan-undo", &undo_op, "--out", uu.to_str().unwrap()]);
    assert!(p2.status.success());
    let p2j = Lab::json(&p2);
    for seq in [2, 3] {
        let e = plan_entry(&p2j, seq);
        assert_eq!(e["status"]["status"], "ready", "{e}");
        assert!(
            e["notes"][0].as_str().unwrap().contains("backup store"),
            "{e}"
        );
    }
    let r2 = lab.cli(&["apply", uu.to_str().unwrap()]);
    assert!(
        r2.status.success(),
        "{}",
        String::from_utf8_lossy(&r2.stdout)
    );
    let undo2 = Lab::json(&r2)["op_id"].as_str().unwrap().to_owned();
    for (i, p) in lab.photos.iter().enumerate() {
        if i == 2 || i == 3 {
            assert!(!p.exists(), "{} was not moved away", p.display());
        } else {
            assert_eq!(blake(p).as_deref(), Some(applied[p].as_str()));
        }
    }
    // their content is kept, byte for byte, in the backup store
    let s2 = lab.show(&undo2);
    for seq in [2, 3] {
        let row = s2["files"]
            .as_array()
            .unwrap()
            .iter()
            .find(|f| f["seq"] == seq)
            .unwrap();
        assert_eq!(
            blake(Path::new(row["backup"].as_str().unwrap())).as_deref(),
            Some(lab.truth[&lab.photos[seq as usize]].as_str())
        );
    }
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    assert!(lab.cli(&["fsck", &undo2]).status.success());
    // and undoing that recreates them: everything is back to the original
    lab.undo(&undo2);
    lab.assert_all_original();
    assert_eq!(
        blake(&moved).as_deref(),
        Some(applied[&lab.photos[3]].as_str())
    );
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

/// Crash and IO error at every fault point of the move-into-the-backup-store transaction
/// (1–4, 7–10), which undoes a recreate. The content is never lost: it is at the path, the bak
/// name or the backup store, and it ends in exactly one of "at the path" or "in the backup store".
#[test]
fn move_to_backup_store_crashes_and_io_errors_recover() {
    let pkg = require!();
    let steps = [1u8, 2, 3, 4, 7, 8, 9, 10];
    for (kind, step) in steps
        .into_iter()
        .map(|s| ("crash", s))
        .chain(steps.into_iter().map(|s| ("fail", s)))
    {
        let case = format!("move {kind} at 2:{step}");
        let lab = Lab::new(&format!("move-{kind}-{step}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let a = lab.cli(&["apply", plan.to_str().unwrap()]);
        assert!(a.status.success(), "{case}");
        let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
        let applied = lab.snapshot();
        std::fs::remove_file(&lab.photos[2]).unwrap();
        let u1 = lab.dir.join("undo1.json");
        assert!(
            lab.cli(&["plan-undo", &op, "--out", u1.to_str().unwrap()])
                .status
                .success()
        );
        let r1 = lab.cli(&["apply", u1.to_str().unwrap()]);
        assert!(r1.status.success(), "{case}");
        let undo1 = Lab::json(&r1)["op_id"].as_str().unwrap().to_owned();
        lab.assert_all_original();
        // undo the undo: file 2 goes into the backup store, the others back to `applied`
        let u2 = lab.dir.join("undo2.json");
        assert!(
            lab.cli(&["plan-undo", &undo1, "--out", u2.to_str().unwrap()])
                .status
                .success()
        );
        let flag = if kind == "crash" {
            "--crash-at"
        } else {
            "--fail-at"
        };
        let o = lab.cli_env(
            &["apply", u2.to_str().unwrap(), flag, &format!("2:{step}")],
            true,
        );
        let undo2 = lab.last_op();
        assert_ne!(undo2, undo1, "{case}");
        let target = lab.photos[2].clone();
        let others = lab.truth.iter().filter(|(p, _)| **p != target);
        let others: BTreeMap<PathBuf, String> =
            others.map(|(p, h)| (p.clone(), h.clone())).collect();
        let path_is_original_or_absent = |when: &str| {
            let cur = blake(&target);
            assert!(
                cur.is_none() || cur.as_deref() == Some(lab.truth[&target].as_str()),
                "{case} {when}: path holds something else"
            );
        };
        if kind == "crash" {
            assert_eq!(o.status.code(), Some(77), "{case}");
            lab.assert_preimages_of(&undo2, &lab.truth);
            path_is_original_or_absent("before recovery");
            assert!(lab.cli(&["recover"]).status.success(), "{case}");
            path_is_original_or_absent("after recovery");
            lab.assert_recovered_of(&undo2, &others);
            let res = lab.cli(&["resume", &undo2]);
            assert!(
                res.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
            assert!(lab.cli(&["fsck", &undo2]).status.success(), "{case}");
            assert!(!target.exists(), "{case}");
            // undoing it again restores everything
            lab.undo(&undo2);
            lab.assert_all_original();
        } else {
            let r = Lab::json(&o);
            assert_ne!(r["status"], "running", "{case}");
            let want = if step <= 7 { "failed" } else { "done" };
            assert_eq!(state_of(&r, 2), want, "{case}: {r}");
            path_is_original_or_absent("after the error");
            assert_eq!(target.exists(), step <= 7, "{case}");
            lab.assert_recovered_of(&undo2, &others);
            for (p, h) in &applied {
                if *p != target {
                    assert_eq!(blake(p).as_deref(), Some(h.as_str()), "{case}");
                }
            }
        }
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

impl Lab {
    /// Lose the Journal database (deleted, or moved away after corruption).
    fn lose_database(&self) {
        std::fs::remove_dir_all(self.data.join("db")).unwrap();
    }

    fn rebuild(&self) -> Value {
        let o = self.cli(&["rebuild-journal"]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        Lab::json(&o)
    }
}

/// G-7: the Journal database is lost after completed Operations (an apply and its undo); both
/// are rebuilt from their backup folders and the chain can still be undone.
#[test]
fn journal_rebuilt_after_database_loss_keeps_history_and_undo() {
    let pkg = require!();
    let lab = Lab::new("rebuild-done", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success());
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    let applied = lab.snapshot();
    lab.undo(&op);
    let undo_op = lab.last_op();
    lab.assert_all_original();
    let history = Lab::json(&lab.cli(&["history"]));

    lab.lose_database();
    assert!(
        Lab::json(&lab.cli(&["history"]))
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = lab.rebuild();
    assert_eq!(r["imported"], serde_json::json!([op, undo_op]), "{r}");
    assert_eq!(Lab::json(&lab.cli(&["history"])), history);
    assert_eq!(lab.rebuild()["imported"], serde_json::json!([]));
    assert!(lab.cli(&["fsck", &undo_op]).status.success());
    // undo the undo from the rebuilt journal
    lab.undo(&undo_op);
    assert_eq!(lab.snapshot(), applied);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// G-7: the database is lost after a crash at several points of the transaction, including
/// inside ReplaceFileW (original only under its bak name). Rebuild, recover, resume, undo.
#[test]
fn journal_rebuilt_after_crash_and_database_loss_recovers() {
    let pkg = require!();
    // (crash step, then simulate a termination inside ReplaceFileW)
    for (step, mid_replace) in [
        (2u8, false),
        (4, false),
        (7, false),
        (7, true),
        (8, false),
        (9, false),
    ] {
        let case = format!("crash at 2:{step} mid_replace {mid_replace}, database lost");
        let lab = Lab::new(&format!("rebuild-crash-{step}-{mid_replace}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        // planned while every path exists (a parallel crash can leave one inside ReplaceFileW)
        let other = lab.plan("Someone", "p-other.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--crash-at",
                &format!("2:{step}"),
            ],
            true,
        );
        assert_eq!(o.status.code(), Some(77), "{case}");
        let op = lab.last_op();
        if mid_replace {
            let show = lab.show(&op);
            let row = show["files"]
                .as_array()
                .unwrap()
                .iter()
                .find(|f| f["seq"] == 2)
                .unwrap();
            std::fs::rename(&lab.photos[2], row["bak"].as_str().unwrap()).unwrap();
        }
        lab.lose_database();
        let r = lab.rebuild();
        assert_eq!(r["imported"], serde_json::json!([op]), "{case}: {r}");
        assert_eq!(op_status(&lab, &op), "running", "{case}");
        lab.assert_preimages(&op);
        // no write before recovery
        let refused = lab.cli(&["apply", other.to_str().unwrap()]);
        assert!(
            String::from_utf8_lossy(&refused.stdout).contains("RecoveryPending"),
            "{case}"
        );
        assert!(lab.cli(&["recover"]).status.success(), "{case}");
        lab.assert_recovered(&op);
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
        assert!(lab.leftovers().is_empty(), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

impl Lab {
    fn plan_copyright(&self, value: &str, file: &str) -> (PathBuf, Value) {
        let out = self.dir.join(file);
        let mut args = vec![
            "plan-copyright",
            "--set",
            value,
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
        (out, Lab::json(&o))
    }

    fn copyrights(&self) -> Vec<Value> {
        let ps: Vec<String> = self
            .photos
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let mut args = vec!["scan"];
        args.extend(ps.iter().map(String::as_str));
        Lab::json(&self.cli(&args))
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["copyright"].clone())
            .collect()
    }
}

/// Scenario D's field (PRODUCT_SPEC §6.7): Copyright written to EXIF, XMP dc:rights (default
/// language) and existing IPTC; non-Latin text on Latin IPTC is Blocked and the file untouched;
/// undo is byte-identical.
#[test]
fn copyright_apply_then_undo_is_byte_identical() {
    let pkg = require!();
    let lab = Lab::new("copyright", &pkg);
    let (plan, pj) = lab.plan_copyright("© 森 Morii 2026", "p.json");
    let statuses: Vec<String> = pj["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["status"]["status"].as_str().unwrap().to_owned())
        .collect();
    let gps = SAMPLES.iter().position(|s| *s == "GPS.jpg").unwrap();
    assert_eq!(statuses[gps], "blocked", "{pj}"); // Latin IPTC CopyrightNotice
    assert!(
        statuses.iter().filter(|s| *s == "ready").count() >= 6,
        "{statuses:?}"
    );
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    for ((c, st), p) in lab.copyrights().iter().zip(&statuses).zip(&lab.photos) {
        if st == "ready" {
            assert_eq!(c["value"], "© 森 Morii 2026", "{}: {c}", p.display());
            assert_eq!(c["conflicting"], false, "{}: {c}", p.display());
        } else {
            assert_eq!(blake(p).as_deref(), Some(lab.truth[p].as_str()));
        }
    }
    assert!(lab.cli(&["fsck", &op]).status.success());
    // the same value again is no change
    let (_, again) = lab.plan_copyright("© 森 Morii 2026", "p2.json");
    assert!(
        again["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["status"]["status"] == "no_change" || e["status"]["status"] == "blocked"),
        "{again}"
    );
    lab.undo(&op);
    lab.assert_all_original();

    // a Latin value can be stored in the Latin IPTC copy: GPS.jpg is written, IPTC included
    let (plan2, pj2) = lab.plan_copyright("© Zoë Morii 2026", "p3.json");
    assert_eq!(pj2["entries"][gps]["status"]["status"], "ready", "{pj2}");
    let a2 = lab.cli(&["apply", plan2.to_str().unwrap()]);
    assert!(
        a2.status.success(),
        "{}",
        String::from_utf8_lossy(&a2.stdout)
    );
    let c = &lab.copyrights()[gps];
    assert_eq!(c["value"], "© Zoë Morii 2026", "{c}");
    assert!(
        c["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|s| s[0] == "IPTC:CopyrightNotice" && s[1] == "© Zoë Morii 2026"),
        "{c}"
    );
    lab.undo(Lab::json(&a2)["op_id"].as_str().unwrap());
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// dc:rights in another language is kept (ExifTool would drop it if the default language were
/// written without a language code), and verification would refuse the file if it were not.
#[test]
fn copyright_keeps_other_languages() {
    let pkg = require!();
    let lab = Lab::new("copyright-lang", &pkg);
    let target = lab.dir.join("photos").join("lang.jpg");
    let mk = Command::new(pkg.join("exiftool.exe"))
        .args([
            "-config",
            "",
            "-XMP-dc:Rights-de=Alle Rechte vorbehalten",
            "-o",
        ])
        .arg(&target)
        .arg(&lab.photos[0])
        .output()
        .unwrap();
    assert!(mk.status.success());
    let before = blake(&target).unwrap();
    let out = lab.dir.join("lang.json");
    let o = lab.cli(&[
        "plan-copyright",
        "--set",
        "© Morii",
        "--out",
        out.to_str().unwrap(),
        target.to_str().unwrap(),
    ]);
    assert!(o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("other languages is kept"),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let a = lab.cli(&["apply", out.to_str().unwrap()]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    let scan = Lab::json(&lab.cli(&["scan", target.to_str().unwrap()]));
    let c = &scan[0]["copyright"];
    assert_eq!(c["value"], "© Morii", "{c}");
    assert_eq!(
        c["other_languages"],
        serde_json::json!([["XMP-dc:Rights-de", "Alle Rechte vorbehalten"]]),
        "{c}"
    );
    lab.undo(Lab::json(&a)["op_id"].as_str().unwrap());
    assert_eq!(blake(&target).as_deref(), Some(before.as_str()));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// The preview check and resume work for the copyright field too (crash before the commit).
#[test]
fn copyright_crash_recovers_and_resumes() {
    let pkg = require!();
    for step in [6u8, 8] {
        let lab = Lab::new(&format!("copyright-crash-{step}"), &pkg);
        let (plan, _) = lab.plan_copyright("© Morii 2026", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--crash-at",
                &format!("0:{step}"),
            ],
            true,
        );
        assert_eq!(o.status.code(), Some(77));
        let op = lab.last_op();
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
            lab.copyrights()
                .iter()
                .all(|c| c["value"] == "© Morii 2026"),
            "step {step}"
        );
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// Worker pool (ARCHITECTURE §7.5, §8): random termination while four file transactions run at
/// the same time (24 files), so several files are in the middle of their transaction when the
/// process dies. The same invariants as with one worker must hold.
#[test]
fn random_kills_with_four_workers_recover() {
    let pkg = require!();
    let iterations: u64 = std::env::var("MM_E2E_KILLS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(8);
    let mut rng: u64 = 0x2026_0929;
    for i in 0..iterations {
        let lab = Lab::many(&format!("pkill-{i}"), &pkg, 3);
        let plan = lab.plan("Morii", "p.json");
        rng = rng
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let delay = Duration::from_millis(100 + (rng >> 33) % 1500);
        let mut child = Command::new(env!("CARGO_BIN_EXE_mm-cli"))
            .arg("--data")
            .arg(&lab.data)
            .arg("--exiftool")
            .arg(&lab.pkg)
            .args(["--workers", "4", "apply", plan.to_str().unwrap()])
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
            lab.assert_all_original();
            continue;
        };
        lab.assert_preimages(&op);
        assert!(lab.cli(&["recover"]).status.success(), "kill {i}");
        lab.assert_recovered(&op);
        let res = lab.cli(&["--workers", "4", "resume", &op]);
        assert!(
            res.status.success(),
            "kill {i}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        assert!(
            creators(&lab)
                .iter()
                .all(|c| c == &serde_json::json!(["Morii"])),
            "kill {i}"
        );
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// With four workers a full volume stops new files; files already in flight finish. Every file
/// ends done or cancelled (original unchanged), and resume completes the rest.
#[test]
fn disk_full_with_four_workers_pauses_and_resumes() {
    let pkg = require!();
    for step in [2u8, 4, 7] {
        let case = format!("disk full at 5:{step}, four workers");
        let lab = Lab::many(&format!("pdiskfull-{step}"), &pkg, 3);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--disk-full-at",
                &format!("5:{step}"),
                "--workers",
                "4",
            ],
            true,
        );
        assert_eq!(o.status.code(), Some(3), "{case}");
        let r = Lab::json(&o);
        let op = r["op_id"].as_str().unwrap().to_owned();
        assert_eq!(r["status"], "cancelled", "{case}: {r}");
        assert_eq!(state_of(&r, 5), "cancelled", "{case}: {r}");
        for f in r["files"].as_array().unwrap() {
            let st = f["state"].as_str().unwrap();
            assert!(st == "done" || st == "cancelled", "{case}: {f}");
            if st == "cancelled" {
                let p = PathBuf::from(f["path"].as_str().unwrap());
                assert_eq!(blake(&p).as_deref(), Some(lab.truth[&p].as_str()), "{case}");
            }
        }
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
        let res = lab.cli(&["--workers", "4", "resume", &op]);
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

impl Lab {
    fn plan_time(&self, tool: &[&str], file: &str) -> (PathBuf, Value) {
        let out = self.dir.join(file);
        let mut args = vec!["plan-time"];
        args.extend_from_slice(tool);
        args.extend(["--out", out.to_str().unwrap()]);
        let ps: Vec<String> = self
            .photos
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        args.extend(ps.iter().map(String::as_str));
        let o = self.cli(&args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        (out, Lab::json(&o))
    }

    fn times(&self) -> Vec<Option<String>> {
        let ps: Vec<String> = self
            .photos
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let mut args = vec!["scan"];
        args.extend(ps.iter().map(String::as_str));
        Lab::json(&self.cli(&args))
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["capture_time"].as_str().map(str::to_owned))
            .collect()
    }

    /// Tags of one file straight from ExifTool (read-only; lab copies only).
    fn tags(&self, p: &Path) -> Value {
        let o = Command::new(self.pkg.join("exiftool.exe"))
            .args([
                "-config",
                "",
                "-json",
                "-G1",
                "-a",
                "-time:all",
                "-SubSecTimeOriginal",
            ])
            .arg(p)
            .output()
            .unwrap();
        serde_json::from_slice::<Value>(&o.stdout).unwrap()[0].clone()
    }

    fn apply_ok(&self, plan: &Path) -> String {
        let a = self.cli(&["apply", plan.to_str().unwrap()]);
        assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
        Lab::json(&a)["op_id"].as_str().unwrap().to_owned()
    }
}

fn local(s: &str) -> mm_domain::time::NaiveDateTime {
    mm_domain::time::parse_local(&s[..19]).unwrap()
}

/// The four MVP time tools (METADATA_MODEL §5.2) through Plan → apply → undo on the fixtures.
#[test]
fn time_tools_apply_and_undo() {
    let pkg = require!();
    let lab = Lab::new("time", &pkg);
    let before = lab.times();
    let writer = SAMPLES.iter().position(|s| *s == "Writer.jpg").unwrap();
    assert_eq!(before[writer], None, "Writer.jpg has no capture time");

    // Shift: files without a time are blocked; the others move by exactly one hour
    let (p, pj) = lab.plan_time(&["--shift", "+01:00:00"], "shift.json");
    assert_eq!(pj["entries"][writer]["status"]["status"], "blocked", "{pj}");
    let op = lab.apply_ok(&p);
    for (i, (b, a)) in before.iter().zip(lab.times()).enumerate() {
        if i == writer {
            assert_eq!(a, None);
        } else {
            let d = local(a.as_deref().unwrap()) - local(b.as_deref().unwrap());
            assert_eq!(d.num_seconds(), 3600, "{}", SAMPLES[i]);
        }
    }
    // other locations keep their shape: XMP.jpg's date-only XMP value stays date-only,
    // GPS.jpg's IPTC date follows the new capture date
    let xmp = &lab.tags(&lab.photos[SAMPLES.iter().position(|s| *s == "XMP.jpg").unwrap()]);
    assert_eq!(xmp["XMP-photoshop:DateCreated"], "2001:05:19", "{xmp}");
    let gps = &lab.tags(&lab.photos[SAMPLES.iter().position(|s| *s == "GPS.jpg").unwrap()]);
    assert_eq!(gps["IPTC:DateCreated"], "2002:07:13", "{gps}");
    assert_eq!(
        gps["IFD0:ModifyDate"], "2002:07:19 13:28:10",
        "ModifyDate is never changed"
    );
    assert!(lab.cli(&["fsck", &op]).status.success());
    lab.undo(&op);
    lab.assert_all_original();

    // Absolute: every file, including the one without a time
    let (p, pj) = lab.plan_time(&["--absolute", "2026:09:27 10:00:00"], "abs.json");
    // INTERACTION_SPEC §17: a shared timestamp loses the order by time, and the Preview says so
    assert!(pj.to_string().contains("order by time is lost"), "{pj}");
    assert_eq!(
        pj["summary"]["warnings"], pj["summary"]["ready"],
        "{}",
        pj["summary"]
    );
    let (_, one) = lab.plan_on(
        &["plan-time", "--absolute", "2026:09:27 10:00:00"],
        &[&lab.photos[1]],
        "abs-one.json",
    );
    assert!(!one.to_string().contains("order by time"), "{one}");
    let op = lab.apply_ok(&p);
    assert!(
        lab.times()
            .iter()
            .all(|t| t.as_deref() == Some("2026:09:27 10:00:00"))
    );
    lab.undo(&op);
    lab.assert_all_original();

    // Sequence by natural file name, one minute apart
    let (p, _) = lab.plan_time(
        &[
            "--sequence",
            "2026:01:01 00:00:00",
            "--step",
            "00:01:00",
            "--order",
            "name",
        ],
        "seq.json",
    );
    let op = lab.apply_ok(&p);
    let mut by_name: Vec<(&str, String)> = SAMPLES
        .iter()
        .copied()
        .zip(lab.times().into_iter().map(Option::unwrap))
        .collect();
    by_name.sort_by(|a, b| mm_domain::time::natural_cmp(a.0, b.0));
    for (i, (_, t)) in by_name.iter().enumerate() {
        assert_eq!(t, &format!("2026:01:01 00:{i:02}:00"), "{by_name:?}");
    }
    lab.undo(&op);
    lab.assert_all_original();

    // Preserve relative timing: the anchor gets the new time, every file moves by the same amount
    let nikon = SAMPLES.iter().position(|s| *s == "Nikon.jpg").unwrap();
    let anchor = lab.photos[nikon].to_string_lossy().into_owned();
    let (p, _) = lab.plan_time(
        &["--preserve", &anchor, "--to", "2026:01:01 12:00:00"],
        "keep.json",
    );
    let op = lab.apply_ok(&p);
    let after = lab.times();
    assert_eq!(after[nikon].as_deref(), Some("2026:01:01 12:00:00"));
    let delta = local("2026:01:01 12:00:00") - local(before[nikon].as_deref().unwrap());
    for (i, (b, a)) in before.iter().zip(&after).enumerate() {
        if let (Some(b), Some(a)) = (b, a) {
            assert_eq!(local(a) - local(b), delta, "{}", SAMPLES[i]);
        }
    }
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// Offsets are never changed or invented; fractions are kept by Shift and dropped by Absolute;
/// IPTC time keeps its own offset (ExifTool would otherwise use the computer's time zone).
#[test]
fn time_tools_keep_offsets_and_handle_fractions() {
    let pkg = require!();
    let lab = Lab::new("time-offsets", &pkg);
    let f = lab.dir.join("photos").join("offsets.jpg");
    let mk = Command::new(pkg.join("exiftool.exe"))
        .args([
            "-config",
            "",
            "-ExifIFD:SubSecTimeOriginal=07",
            "-ExifIFD:OffsetTimeOriginal=+09:00",
            "-XMP-exif:DateTimeOriginal=2001:08:01 12:57:23.07+09:00",
            "-IPTC:DateCreated=2001:08:01",
            "-IPTC:TimeCreated=12:57:23+09:00",
            "-o",
        ])
        .arg(&f)
        .arg(&lab.photos[SAMPLES.iter().position(|s| *s == "Nikon.jpg").unwrap()])
        .output()
        .unwrap();
    assert!(
        mk.status.success(),
        "{}",
        String::from_utf8_lossy(&mk.stderr)
    );
    let original = blake(&f).unwrap();
    let plan = |tool: &[&str], name: &str| {
        let out = lab.dir.join(name);
        let mut args = vec!["plan-time"];
        args.extend_from_slice(tool);
        args.extend(["--out", out.to_str().unwrap(), f.to_str().unwrap()]);
        let o = lab.cli(&args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        out
    };
    let op = lab.apply_ok(&plan(&["--shift", "+2d01:00:00"], "s.json"));
    let t = lab.tags(&f);
    assert_eq!(t["ExifIFD:DateTimeOriginal"], "2001:08:03 13:57:23", "{t}");
    assert_eq!(t["ExifIFD:SubSecTimeOriginal"], "07", "{t}");
    assert_eq!(t["ExifIFD:OffsetTimeOriginal"], "+09:00", "{t}");
    assert_eq!(
        t["XMP-exif:DateTimeOriginal"], "2001:08:03 13:57:23.07+09:00",
        "{t}"
    );
    assert_eq!(t["IPTC:DateCreated"], "2001:08:03", "{t}");
    assert_eq!(t["IPTC:TimeCreated"], "13:57:23+09:00", "{t}");
    lab.undo(&op);
    assert_eq!(blake(&f).as_deref(), Some(original.as_str()));

    let op = lab.apply_ok(&plan(&["--absolute", "2026:09:27 08:00:00"], "a.json"));
    let t = lab.tags(&f);
    assert_eq!(t["ExifIFD:DateTimeOriginal"], "2026:09:27 08:00:00", "{t}");
    assert!(t.get("ExifIFD:SubSecTimeOriginal").is_none(), "{t}");
    assert_eq!(t["ExifIFD:OffsetTimeOriginal"], "+09:00", "{t}");
    assert_eq!(
        t["XMP-exif:DateTimeOriginal"], "2026:09:27 08:00:00+09:00",
        "{t}"
    );
    assert_eq!(t["IPTC:TimeCreated"], "08:00:00+09:00", "{t}");
    lab.undo(&op);
    assert_eq!(blake(&f).as_deref(), Some(original.as_str()));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// A crash in the middle of a Shift: recovery, and resume re-checks the capture time shown in
/// the Preview against the file before writing it.
#[test]
fn time_shift_crash_recovers_and_resumes() {
    let pkg = require!();
    let lab = Lab::new("time-crash", &pkg);
    let before = lab.times();
    let (p, _) = lab.plan_time(&["--shift", "-00:30:00"], "s.json");
    let o = lab.cli_env(&["apply", p.to_str().unwrap(), "--crash-at", "2:6"], true);
    assert_eq!(o.status.code(), Some(77));
    let op = lab.last_op();
    lab.assert_preimages(&op);
    assert!(lab.cli(&["recover"]).status.success());
    lab.assert_recovered(&op);
    let res = lab.cli(&["resume", &op]);
    assert!(
        res.status.success() || res.status.code() == Some(3),
        "{}",
        String::from_utf8_lossy(&res.stdout)
    );
    for (b, a) in before.iter().zip(lab.times()) {
        if let (Some(b), Some(a)) = (b, a) {
            assert_eq!((local(&a) - local(b)).num_seconds(), -1800);
        }
    }
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

impl Lab {
    fn plan_gps(&self, how: &[&str], files: &[&Path], name: &str) -> (PathBuf, Value) {
        let out = self.dir.join(name);
        let mut args = vec!["plan-gps"];
        args.extend_from_slice(how);
        args.extend(["--out", out.to_str().unwrap()]);
        let ps: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        args.extend(ps.iter().map(String::as_str));
        let o = self.cli(&args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        (out, Lab::json(&o))
    }

    fn gps_of(&self, files: &[&Path]) -> Vec<Value> {
        let ps: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        let mut args = vec!["scan"];
        args.extend(ps.iter().map(String::as_str));
        Lab::json(&self.cli(&args))
            .as_array()
            .unwrap()
            .iter()
            .map(|x| x["gps"].clone())
            .collect()
    }

    /// Every GPS tag of a file, numerically, straight from ExifTool (lab copies only).
    fn gps_tags(&self, p: &Path) -> serde_json::Map<String, Value> {
        let o = Command::new(self.pkg.join("exiftool.exe"))
            .args([
                "-config",
                "",
                "-json",
                "-G1",
                "-n",
                "-GPS:all",
                "-XMP-exif:GPS*",
            ])
            .arg(p)
            .output()
            .unwrap();
        let mut m = serde_json::from_slice::<Value>(&o.stdout).unwrap()[0]
            .as_object()
            .unwrap()
            .clone();
        m.remove("SourceFile");
        m
    }
}

/// GPS set and remove (METADATA_MODEL §7) through Plan → apply → undo on the fixtures: numbers
/// are verified numerically, the GPS time stamp survives a set, removal leaves no GPS tag.
#[test]
fn gps_set_and_remove_apply_and_undo() {
    let pkg = require!();
    let lab = Lab::new("gps", &pkg);
    let all: Vec<&Path> = lab.photos.iter().map(PathBuf::as_path).collect();
    let gps_jpg = lab.photos[SAMPLES.iter().position(|s| *s == "GPS.jpg").unwrap()].clone();
    let before = lab.gps_tags(&gps_jpg);
    assert!(before.contains_key("GPS:GPSTimeStamp"), "{before:?}");

    let (p, pj) = lab.plan_gps(&["--set", "35.6812345,139.7671234,40.5"], &all, "set.json");
    assert!(
        pj["entries"]
            .as_array()
            .unwrap()
            .iter()
            .all(|e| e["status"]["status"] == "ready"),
        "{pj}"
    );
    let op = lab.apply_ok(&p);
    assert!(
        lab.gps_of(&all)
            .iter()
            .all(|g| g == "35.6812345, 139.7671234, 40.50 m"),
        "{:?}",
        lab.gps_of(&all)
    );
    let after = lab.gps_tags(&gps_jpg);
    for kept in ["GPS:GPSTimeStamp", "GPS:GPSMapDatum"] {
        assert_eq!(after.get(kept), before.get(kept), "{kept} must be kept");
    }
    assert!(lab.cli(&["fsck", &op]).status.success());
    lab.undo(&op);
    lab.assert_all_original();

    // removal: only the file with GPS changes; afterwards it has no GPS tag at all
    let (p, pj) = lab.plan_gps(&["--remove"], &all, "remove.json");
    let statuses: Vec<&str> = pj["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["status"]["status"].as_str().unwrap())
        .collect();
    assert_eq!(
        statuses.iter().filter(|s| **s == "ready").count(),
        1,
        "{pj}"
    );
    let op = lab.apply_ok(&p);
    assert!(
        lab.gps_tags(&gps_jpg).is_empty(),
        "{:?}",
        lab.gps_tags(&gps_jpg)
    );
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// XMP GPS already present is updated with the GPS directory and removed with it; southern and
/// western coordinates and an altitude below sea level keep their signs.
#[test]
fn gps_xmp_copy_and_signs() {
    let pkg = require!();
    let lab = Lab::new("gps-xmp", &pkg);
    let f = lab.dir.join("photos").join("xmpgps.jpg");
    let mk = Command::new(pkg.join("exiftool.exe"))
        .args([
            "-config",
            "",
            "-XMP-exif:GPSLatitude=10.5",
            "-XMP-exif:GPSLongitude=20.25",
            "-o",
        ])
        .arg(&f)
        .arg(&lab.photos[SAMPLES.iter().position(|s| *s == "Nikon.jpg").unwrap()])
        .output()
        .unwrap();
    assert!(mk.status.success());
    let original = blake(&f).unwrap();
    let (p, _) = lab.plan_gps(&["--set", "-33.8688,-70.5,-10"], &[&f], "s.json");
    let op = lab.apply_ok(&p);
    assert_eq!(lab.gps_of(&[&f])[0], "-33.8688000, -70.5000000, -10.00 m");
    let t = lab.gps_tags(&f);
    assert_eq!(t["GPS:GPSLatitudeRef"], "S", "{t:?}");
    assert_eq!(t["GPS:GPSLongitudeRef"], "W", "{t:?}");
    assert_eq!(t["GPS:GPSAltitudeRef"], 1, "{t:?}");
    assert!(
        (t["XMP-exif:GPSLatitude"].as_f64().unwrap() + 33.8688).abs() < 1e-7,
        "{t:?}"
    );
    assert!(
        (t["XMP-exif:GPSLongitude"].as_f64().unwrap() + 70.5).abs() < 1e-7,
        "{t:?}"
    );
    lab.undo(&op);
    assert_eq!(blake(&f).as_deref(), Some(original.as_str()));

    let (p, _) = lab.plan_gps(&["--remove"], &[&f], "r.json");
    let op = lab.apply_ok(&p);
    assert!(lab.gps_tags(&f).is_empty(), "{:?}", lab.gps_tags(&f));
    lab.undo(&op);
    assert_eq!(blake(&f).as_deref(), Some(original.as_str()));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// ExifTool 13.59's `t/images/Nikon.nef`, verified identical to the copy inside the SHA-256-pinned
/// source archive (2026-09-27).
const NEF_BLAKE3: &str = "a869af4e74e002c31da32a76f96e67d5bda0a401a684f86850aa71ffc861aca7";

impl Lab {
    /// A copy of the pinned NEF fixture in the photo folder under `name`.
    fn add_nef(&self, name: &str) -> PathBuf {
        let src = timages().join("Nikon.nef");
        assert_eq!(
            blake(&src).as_deref(),
            Some(NEF_BLAKE3),
            "pinned fixture Nikon.nef changed"
        );
        let p = self.photos[0].parent().unwrap().join(name);
        std::fs::copy(&src, &p).unwrap();
        p
    }

    fn plan_on(&self, cmd: &[&str], files: &[&Path], name: &str) -> (PathBuf, Value) {
        let out = self.dir.join(name);
        let mut args: Vec<&str> = cmd.to_vec();
        args.extend(["--out", out.to_str().unwrap()]);
        let ps: Vec<String> = files
            .iter()
            .map(|p| p.to_string_lossy().into_owned())
            .collect();
        args.extend(ps.iter().map(String::as_str));
        let o = self.cli(&args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        (out, Lab::json(&o))
    }

    /// Every tag of an XMP file (lab copies only).
    fn xmp_tags(&self, p: &Path) -> serde_json::Map<String, Value> {
        let o = Command::new(self.pkg.join("exiftool.exe"))
            .args(["-config", "", "-json", "-G1", "-XMP:all"])
            .arg(p)
            .output()
            .unwrap();
        let mut m = serde_json::from_slice::<Value>(&o.stdout).unwrap()[0]
            .as_object()
            .unwrap()
            .clone();
        m.remove("SourceFile");
        m.remove("XMP-x:XMPToolkit");
        m
    }
}

/// SAFETY_MODEL §3, §4.2, §4.3: for a NEF only its XMP sidecar is written. A new sidecar holds
/// only MoriMeta's fields; an existing one keeps everything else; undo of the creation moves the
/// sidecar into the backup store; the NEF stays byte-identical throughout (I-10).
#[test]
fn nef_sidecar_is_created_updated_and_undone() {
    let pkg = require!();
    let lab = Lab::new("nef", &pkg);
    let nef = lab.add_nef("DSC_0001.NEF");
    let xmp = nef.with_file_name("DSC_0001.xmp");
    let (p, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&nef], "c.json");
    let e = &pj["entries"][0];
    assert_eq!(e["status"]["status"], "ready", "{pj}");
    assert_eq!(e["path"], xmp.to_str().unwrap(), "{pj}");
    // PRODUCT_SPEC §6.12: counts by write target
    assert_eq!(pj["summary"]["targets"]["new_sidecar"], 1, "{pj}");
    assert_eq!(pj["summary"]["targets"]["in_file"], 0, "{pj}");
    let op1 = lab.apply_ok(&p);
    assert_eq!(
        lab.xmp_tags(&xmp),
        serde_json::json!({"XMP-dc:Creator": "Morii"})
            .as_object()
            .unwrap()
            .clone(),
        "a new sidecar holds only what was written"
    );
    let after_creator = blake(&xmp).unwrap();

    let (p, _) = lab.plan_on(&["plan-copyright", "--set", "© Morii"], &[&nef], "r.json");
    let op2 = lab.apply_ok(&p);
    let t = lab.xmp_tags(&xmp);
    assert_eq!(t["XMP-dc:Creator"], "Morii", "{t:?}");
    assert_eq!(t["XMP-dc:Rights"], "© Morii", "{t:?}");
    assert_eq!(
        blake(&nef).as_deref(),
        Some(NEF_BLAKE3),
        "the NEF must not change"
    );
    assert!(lab.cli(&["fsck", &op2]).status.success());

    lab.undo(&op2); // the update is undone byte for byte
    assert_eq!(blake(&xmp).as_deref(), Some(after_creator.as_str()));
    lab.undo(&op1); // the created sidecar goes into the backup store
    assert!(!xmp.exists());
    let undo1 = lab.last_op();
    assert!(lab.cli(&["fsck", &undo1]).status.success());
    lab.undo(&undo1); // and comes back
    assert_eq!(blake(&xmp).as_deref(), Some(after_creator.as_str()));
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// Capture time and GPS for a NEF go to the sidecar; the NEF's own EXIF stays as it is.
#[test]
fn nef_time_and_gps_go_to_the_sidecar() {
    let pkg = require!();
    let lab = Lab::new("nef-time", &pkg);
    let nef = lab.add_nef("DSC_0001.NEF");
    let xmp = nef.with_file_name("DSC_0001.xmp");
    let (p, pj) = lab.plan_on(&["plan-time", "--shift", "+01:00:00"], &[&nef], "t.json");
    let before = pj["entries"][0]["changes"][0]["before"][0]
        .as_str()
        .unwrap()
        .to_owned();
    let op = lab.apply_ok(&p);
    let t = lab.xmp_tags(&xmp);
    let want = {
        let b = local(&before) + mm_domain::time::TimeDelta::try_hours(1).unwrap();
        b.format("%Y:%m:%d %H:%M:%S").to_string()
    };
    for tag in [
        "XMP-exif:DateTimeOriginal",
        "XMP-photoshop:DateCreated",
        "XMP-xmp:CreateDate",
    ] {
        assert!(
            t[tag].as_str().unwrap().starts_with(&want),
            "{tag}: {t:?} (want {want})"
        );
    }
    // a second Shift starts from the sidecar's time, not from the NEF's unchanged EXIF
    let (_, pj2) = lab.plan_on(&["plan-time", "--shift", "+01:00:00"], &[&nef], "t2.json");
    assert!(
        pj2["entries"][0]["changes"][0]["before"][0]
            .as_str()
            .unwrap()
            .starts_with(&want),
        "{pj2}"
    );
    let (p, _) = lab.plan_on(&["plan-gps", "--set", "35.5,139.25"], &[&nef], "g.json");
    let op_g = lab.apply_ok(&p);
    let t = lab.xmp_tags(&xmp);
    assert!(t.contains_key("XMP-exif:GPSLatitude"), "{t:?}");
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    lab.undo(&op_g);
    lab.undo(&op);
    assert!(!xmp.exists());
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §3.1 pairing: an existing sidecar is used whatever its letter case; a darktable
/// `<file>.NEF.xmp` is never touched; two RAW files with one name make ownership ambiguous; a JPEG
/// with the same name writes itself; a NEF and its sidecar selected together are one entry.
#[test]
fn nef_sidecar_pairing_rules() {
    let pkg = require!();
    let lab = Lab::new("nef-pair", &pkg);
    let a = lab.add_nef("DSC_0002.NEF");
    let a_xmp = a.with_file_name("DSC_0002.XMP");
    let mk = Command::new(pkg.join("exiftool.exe"))
        .args(["-config", "", "-XMP-dc:Title=Kept", "-o"])
        .arg(&a_xmp)
        .output()
        .unwrap();
    assert!(mk.status.success());
    let b = lab.add_nef("DSC_0003.NEF");
    let dt = b.with_file_name("DSC_0003.NEF.xmp");
    std::fs::write(&dt, b"<darktable/>").unwrap();
    let c = lab.add_nef("DSC_0004.NEF");
    std::fs::write(c.with_file_name("DSC_0004.CR2"), b"another raw").unwrap();
    let d = lab.add_nef("DSC_0005.NEF");
    let d_jpg = d.with_file_name("DSC_0005.JPG");
    std::fs::copy(&lab.photos[1], &d_jpg).unwrap();

    let files: Vec<&Path> = vec![&a, &a_xmp, &b, &c, &d, &d_jpg];
    let (p, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &files, "pair.json");
    let entries = pj["entries"].as_array().unwrap();
    assert_eq!(
        entries.len(),
        5,
        "a NEF and its sidecar are one entry: {pj}"
    );
    let by_path = |s: &Path| {
        entries
            .iter()
            .find(|e| e["path"] == s.to_str().unwrap())
            .unwrap_or_else(|| panic!("no entry for {}: {pj}", s.display()))
    };
    assert_eq!(by_path(&a_xmp)["status"]["status"], "ready");
    let b_entry = by_path(&b.with_file_name("DSC_0003.xmp"));
    assert!(
        b_entry["notes"].to_string().contains("darktable"),
        "{b_entry}"
    );
    assert_eq!(by_path(&c)["status"]["status"], "blocked", "{pj}");
    assert_eq!(by_path(&d_jpg)["status"]["status"], "ready");
    assert_eq!(
        by_path(&d.with_file_name("DSC_0005.xmp"))["status"]["status"],
        "ready"
    );
    let op = lab.apply_ok(&p);
    // the existing sidecar kept its other properties and its letter case
    let t = lab.xmp_tags(&a_xmp);
    assert_eq!(t["XMP-dc:Title"], "Kept", "{t:?}");
    assert_eq!(t["XMP-dc:Creator"], "Morii", "{t:?}");
    let names: Vec<String> = std::fs::read_dir(a.parent().unwrap())
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.to_lowercase().starts_with("dsc_0002"))
        .collect();
    assert!(
        names.contains(&"DSC_0002.XMP".to_string()) && names.len() == 2,
        "{names:?}"
    );
    assert_eq!(std::fs::read(&dt).unwrap(), b"<darktable/>");
    assert!(!c.with_file_name("DSC_0004.xmp").exists());
    for nef in [&a, &b, &c, &d] {
        assert_eq!(blake(nef).as_deref(), Some(NEF_BLAKE3));
    }
    lab.undo(&op);
    assert!(!b.with_file_name("DSC_0003.xmp").exists());
    assert_eq!(lab.xmp_tags(&a_xmp).get("XMP-dc:Creator"), None);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// Crash at every fault point of the create transaction (1, 5–10): the sidecar path only ever
/// holds nothing or the complete verified sidecar; recovery, resume and undo finish the job.
#[test]
fn nef_sidecar_creation_crashes_recover() {
    let pkg = require!();
    for step in [1u8, 5, 6, 7, 8, 9, 10] {
        let case = format!("create crash at 0:{step}");
        let lab = Lab::new(&format!("nef-crash-{step}"), &pkg);
        let nef = lab.add_nef("DSC_0001.NEF");
        let xmp = nef.with_file_name("DSC_0001.xmp");
        let (p, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&nef], "c.json");
        let o = lab.cli_env(
            &[
                "apply",
                p.to_str().unwrap(),
                "--crash-at",
                &format!("0:{step}"),
            ],
            true,
        );
        assert_eq!(o.status.code(), Some(77), "{case}");
        let op = lab.last_op();
        let complete = |when: &str| {
            if xmp.exists() {
                assert_eq!(
                    lab.xmp_tags(&xmp).get("XMP-dc:Creator"),
                    Some(&serde_json::json!("Morii")),
                    "{case} {when}"
                );
            }
        };
        complete("before recovery");
        assert!(lab.cli(&["recover"]).status.success(), "{case}");
        complete("after recovery");
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
        let res = lab.cli(&["resume", &op]);
        assert!(
            res.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        assert!(xmp.exists(), "{case}");
        lab.undo(&op);
        assert!(!xmp.exists(), "{case}");
        assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// Real camera files (research/corpus.lock.json: Nikon Z8 ×2, D850; CC0, SHA-256-pinned; skipped
/// when not fetched): every field through the sidecar, then undone. The NEFs stay byte-identical;
/// the shifted time keeps the camera's sub-seconds and offset.
#[test]
fn real_nef_corpus_through_the_sidecar() {
    let pkg = require!();
    let corpus = repo().join("research/.work/corpus/nikon");
    let names = [
        "Nikon_Z8_high_efficiency_low.NEF",
        "Nikon_Z8_raw_14_bit_lossless_compression.NEF",
        "Nikon-D850-14bit-lossless-compressed.NEF",
    ];
    if !names.iter().all(|n| corpus.join(n).exists()) {
        eprintln!("SKIP: research/.work/corpus not fetched");
        return;
    }
    let lab = Lab::new("real-nef", &pkg);
    let dir = lab.photos[0].parent().unwrap().to_path_buf();
    let nefs: Vec<PathBuf> = names
        .iter()
        .map(|n| {
            let p = dir.join(n);
            std::fs::copy(corpus.join(n), &p).unwrap();
            p
        })
        .collect();
    let refs: Vec<&Path> = nefs.iter().map(PathBuf::as_path).collect();
    let before: Vec<String> = nefs.iter().map(|p| blake(p).unwrap()).collect();

    let (p, pj) = lab.plan_on(&["plan-time", "--shift", "+01:00:00"], &refs, "t.json");
    let shown: Vec<String> = pj["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| {
            assert_eq!(e["status"]["status"], "ready", "{e}");
            e["changes"][0]["before"][0].as_str().unwrap().to_owned()
        })
        .collect();
    let mut ops = vec![lab.apply_ok(&p)];
    for (nef, b) in nefs.iter().zip(&shown) {
        let t = lab.xmp_tags(&nef.with_extension("xmp"));
        let a = t["XMP-exif:DateTimeOriginal"].as_str().unwrap();
        assert_eq!(
            (local(a) - local(b)).num_seconds(),
            3600,
            "{}: {b} -> {a}",
            nef.display()
        );
        assert_eq!(
            &a[19..],
            &b[19..],
            "sub-seconds and offset kept: {b} -> {a}"
        );
    }
    for (i, cmd) in [
        vec!["plan-creator", "--set", "森 Morii"],
        vec!["plan-copyright", "--set", "© 2026 Morii"],
        vec!["plan-gps", "--set", "35.6812345,139.7671234,40"],
    ]
    .iter()
    .enumerate()
    {
        let (p, _) = lab.plan_on(cmd, &refs, &format!("f{i}.json"));
        ops.push(lab.apply_ok(&p));
    }
    for nef in &nefs {
        let t = lab.xmp_tags(&nef.with_extension("xmp"));
        assert_eq!(t["XMP-dc:Creator"], "森 Morii", "{t:?}");
        assert_eq!(t["XMP-dc:Rights"], "© 2026 Morii", "{t:?}");
        assert!(t.contains_key("XMP-exif:GPSLatitude"), "{t:?}");
    }
    for op in ops.iter().rev() {
        lab.undo(op);
    }
    for (nef, h) in nefs.iter().zip(&before) {
        assert_eq!(
            blake(nef).as_deref(),
            Some(h.as_str()),
            "{} changed",
            nef.display()
        );
        assert!(!nef.with_extension("xmp").exists());
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// APP11 with a JUMBF box labelled `c2pa` (the synthetic construction of research/s3/fields.py;
/// not a real manifest).
fn jumbf_app11() -> Vec<u8> {
    let be32 = |n: usize| (n as u32).to_be_bytes();
    let uuid: [u8; 16] = [
        0x63, 0x32, 0x70, 0x61, 0x00, 0x11, 0x00, 0x10, 0x80, 0x00, 0x00, 0xAA, 0x00, 0x38, 0x9B,
        0x71,
    ];
    let label = b"c2pa\0";
    let mut jumd = be32(8 + 16 + 1 + label.len()).to_vec();
    jumd.extend(b"jumd");
    jumd.extend(uuid);
    jumd.push(3);
    jumd.extend(label);
    let json = br#"{"test":"morimeta synthetic, not a real manifest"}"#;
    let mut jsonbox = be32(8 + json.len()).to_vec();
    jsonbox.extend(b"json");
    jsonbox.extend(json);
    let mut jumb = be32(8 + jumd.len() + jsonbox.len()).to_vec();
    jumb.extend(b"jumb");
    jumb.extend(jumd);
    jumb.extend(jsonbox);
    let mut data = b"JP".to_vec();
    data.extend(1u16.to_be_bytes());
    data.extend(1u32.to_be_bytes());
    data.extend(jumb);
    let mut seg = vec![0xFF, 0xEB];
    seg.extend(((data.len() + 2) as u16).to_be_bytes());
    seg.extend(data);
    seg
}

/// SAFETY_MODEL §8.12: a JPEG with C2PA Content Credentials is excluded by default (writing would
/// invalidate their signature); the other files of the same Plan are written.
#[test]
fn c2pa_files_are_excluded_by_default() {
    let pkg = require!();
    let lab = Lab::new("c2pa", &pkg);
    let f = lab.dir.join("photos").join("credentials.jpg");
    let src = std::fs::read(&lab.photos[0]).unwrap();
    let mut bytes = src[..2].to_vec();
    bytes.extend(jumbf_app11());
    bytes.extend(&src[2..]);
    std::fs::write(&f, &bytes).unwrap();
    let before = blake(&f).unwrap();
    let other = lab.photos[1].clone();
    let (p, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&f, &other], "c.json");
    let st = |i: usize| pj["entries"][i]["status"].clone();
    assert_eq!(st(0)["status"], "blocked", "{pj}");
    assert!(st(0)["reason"].as_str().unwrap().contains("C2PA"), "{pj}");
    assert_eq!(st(1)["status"], "ready", "{pj}");
    let op = lab.apply_ok(&p);
    assert_eq!(blake(&f).as_deref(), Some(before.as_str()));
    assert_ne!(blake(&other).as_deref(), Some(lab.truth[&other].as_str()));
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §6.3: automatic pruning spares recent Operations; a requested prune removes only
/// the backup folder. History keeps the Operation, it can no longer be undone, fsck does not
/// report the missing backups, and later Operations still undo.
#[test]
fn pruned_backups_keep_history_but_not_undo() {
    let pkg = require!();
    let lab = Lab::new("prune", &pkg);
    let op1 = lab.apply_ok(&lab.plan("Morii", "p1.json"));
    let op2 = lab.apply_ok(&lab.plan("Mori", "p2.json"));

    let b = Lab::json(&lab.cli(&["backups"]));
    assert!(b["would_prune"].as_array().unwrap().is_empty(), "{b}");
    assert!(b["total_bytes"].as_u64().unwrap() > 0, "{b}");
    let o = lab.cli(&["prune", &op1]);
    assert!(!o.status.success(), "recent: refused without --requested");
    assert!(lab.data.join("backups").join(&op1).exists());

    let o = lab.cli(&["prune", "--requested", &op1]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert!(!lab.data.join("backups").join(&op1).exists());
    let b = Lab::json(&lab.cli(&["backups"]));
    let row = |id: &str| {
        b["ops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == id)
            .cloned()
            .unwrap()
    };
    assert_eq!(row(&op1)["pruned"], true, "{b}");
    assert_eq!(row(&op1)["bytes"], 0, "{b}");
    assert_eq!(row(&op2)["pruned"], false, "{b}");

    let h = Lab::json(&lab.cli(&["history"]));
    assert!(
        h.as_array()
            .unwrap()
            .iter()
            .any(|o| o["id"] == op1.as_str())
    );
    let u = lab.dir.join("undo-pruned.json");
    let o = lab.cli(&["plan-undo", &op1, "--out", u.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(
        String::from_utf8_lossy(&o.stdout).contains("retention"),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let f = lab.cli(&["fsck", &op1]);
    // op2 rewrote the files after op1, so op1's results differ; but no backup problem is reported
    assert!(
        !String::from_utf8_lossy(&f.stdout).contains("backup missing"),
        "{}",
        String::from_utf8_lossy(&f.stdout)
    );

    // keep: exempt from automatic pruning
    let o = lab.cli(&["keep", &op2]);
    assert!(o.status.success());
    let b = Lab::json(&lab.cli(&["backups"]));
    assert_eq!(
        b["ops"]
            .as_array()
            .unwrap()
            .iter()
            .find(|o| o["id"] == op2.as_str())
            .unwrap()["protection"],
        "kept",
        "{b}"
    );
    lab.undo(&op2);
    let f = lab.cli(&["fsck", &lab.last_op()]);
    assert!(f.status.success(), "{}", String::from_utf8_lossy(&f.stdout));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §7.2: a file changed after the Operation is excluded from its undo by default;
/// forcing it restores the backup after backing up the current content, and undoing the forced
/// restore brings the later change back.
#[test]
fn forced_restore_of_a_file_changed_later() {
    let pkg = require!();
    let lab = Lab::new("forced", &pkg);
    let op1 = lab.apply_ok(&lab.plan("Morii", "p1.json"));
    // another program changes one file afterwards (same content rewritten = new identity/time,
    // then one byte appended so the content differs)
    let target = lab.photos[0].clone();
    let mut bytes = std::fs::read(&target).unwrap();
    bytes.extend_from_slice(b"later");
    std::fs::write(&target, &bytes).unwrap();
    let later = blake(&target).unwrap();

    let u = lab.dir.join("u.json");
    let o = lab.cli(&["plan-undo", &op1, "--out", u.to_str().unwrap()]);
    assert!(o.status.success());
    let pj = Lab::json(&o);
    let e0 = pj["entries"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["path"].as_str().unwrap().ends_with("Writer.jpg"))
        .unwrap()
        .clone();
    assert_eq!(e0["excluded"], true, "{e0}");
    assert_eq!(pj["summary"]["excluded"], 1, "{pj}");
    assert!(
        e0["notes"].to_string().contains("changed outside MoriMeta"),
        "{e0}"
    );
    // default: the other files are restored, the changed one is left alone
    lab.apply_ok(&u);
    assert_eq!(blake(&target).as_deref(), Some(later.as_str()));
    for p in &lab.photos[1..] {
        assert_eq!(blake(p).as_deref(), Some(lab.truth[p].as_str()));
    }

    // forced: a fresh plan (the files the first undo restored are no change now)
    let f = lab.dir.join("f.json");
    let o = lab.cli(&[
        "plan-undo",
        &op1,
        "--out",
        f.to_str().unwrap(),
        "--force-conflicts",
    ]);
    assert!(o.status.success());
    let forced = lab.apply_ok(&f);
    assert_eq!(blake(&target).as_deref(), Some(lab.truth[&target].as_str()));
    // the forced restore is an Operation like any other: undo it
    lab.undo(&forced);
    assert_eq!(blake(&target).as_deref(), Some(later.as_str()));

    // a later Operation changed the files: the undo of the earlier one names it
    let lab2 = Lab::new("forced-by-op", &pkg);
    let a = lab2.apply_ok(&lab2.plan("Morii", "a.json"));
    let b = lab2.apply_ok(&lab2.plan("Mori", "b.json"));
    let o = lab2.cli(&[
        "plan-undo",
        &a,
        "--out",
        lab2.dir.join("u.json").to_str().unwrap(),
    ]);
    let pj = Lab::json(&o);
    for e in pj["entries"].as_array().unwrap() {
        assert!(e["notes"].to_string().contains(&b), "{e}");
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
    let _ = std::fs::remove_dir_all(&lab2.dir);
}

/// SAFETY_MODEL §10 step 3: a file recovery cannot settle keeps its Operation interrupted and
/// blocks every write; the user keeps it as found, writes are allowed again, and the undo Plan
/// can still restore its original from the backup (a forced restore).
#[test]
fn needs_attention_is_resolved_by_the_user() {
    let pkg = require!();
    let lab = Lab::new("attention", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let o = lab.cli_env(
        &[
            "--workers",
            "1",
            "apply",
            plan.to_str().unwrap(),
            "--crash-at",
            "2:7",
        ],
        true,
    );
    assert_eq!(o.status.code(), Some(77));
    let op = lab.last_op();
    // another program writes the file while the operation is interrupted
    let target = lab.photos[2].clone();
    let mut other = std::fs::read(&target).unwrap();
    other.extend_from_slice(b"other program");
    std::fs::write(&target, &other).unwrap();
    let other_hash = blake(&target).unwrap();

    let r = lab.cli(&["recover"]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stdout));
    let state = |op: &str, i: usize| lab.show(op)["files"][i]["state"].clone();
    assert_eq!(state(&op, 2), "attention");
    assert_eq!(blake(&target).as_deref(), Some(other_hash.as_str()));
    // every write waits for the person
    let p2 = lab.plan("Mori", "p2.json");
    let a = lab.cli(&["apply", p2.to_str().unwrap()]);
    assert!(!a.status.success());
    assert!(String::from_utf8_lossy(&a.stdout).contains("RecoveryPending"));

    let r = lab.cli(&["resolve", &op, "--keep", "2"]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stdout));
    assert_eq!(Lab::json(&r)["status"], "recovered");
    assert_eq!(state(&op, 2), "conflict");
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    assert_eq!(
        blake(&target).as_deref(),
        Some(other_hash.as_str()),
        "kept as found"
    );

    // the undo restores the two finished files, and the kept one only when forced
    let u = lab.dir.join("u.json");
    let o = lab.cli(&[
        "plan-undo",
        &op,
        "--out",
        u.to_str().unwrap(),
        "--force-conflicts",
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    lab.apply_ok(&u);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// PRODUCT_SPEC §6.9 (scenario D's template): values rendered per file from its original
/// snapshot; a file without a capture time is blocked (no silent empty value), a default fills a
/// missing creator; literal braces and unknown variables.
#[test]
fn copyright_template_per_file() {
    let pkg = require!();
    let lab = Lab::new("template", &pkg);
    let scan = Lab::json(
        &lab.cli(
            &[
                vec!["scan"],
                lab.photos.iter().map(|p| p.to_str().unwrap()).collect(),
            ]
            .concat(),
        ),
    );
    let (p, pj) = lab.plan_copyright("© {creator|Anonymous} {year}", "t.json");
    let entries = pj["entries"].as_array().unwrap();
    let mut ready = 0;
    for (e, s) in entries.iter().zip(scan.as_array().unwrap()) {
        let time = s["capture_time"].as_str();
        match time {
            None => {
                assert_eq!(e["status"]["status"], "blocked", "{e}");
                assert!(
                    e["status"]["reason"].as_str().unwrap().contains("{year}"),
                    "{e}"
                );
            }
            Some(t) => {
                let who = s["creator"]
                    .as_array()
                    .map(|v| {
                        v.iter()
                            .map(|x| x.as_str().unwrap())
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_else(|| "Anonymous".into());
                let want = format!("© {who} {}", &t[..4]);
                if e["status"]["status"] == "ready" {
                    ready += 1;
                    assert_eq!(e["changes"][0]["after"][0], want.as_str(), "{e}");
                }
            }
        }
    }
    assert!(ready >= 3, "{pj}");
    let op = lab.apply_ok(&p);
    lab.undo(&op);
    lab.assert_all_original();

    let (_, pj) = lab.plan_copyright("{{Studio}} ©", "l.json");
    let after: Vec<&Value> = pj["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["status"]["status"] == "ready")
        .map(|e| &e["changes"][0]["after"][0])
        .collect();
    assert!(
        !after.is_empty() && after.iter().all(|a| *a == "{Studio} ©"),
        "{pj}"
    );
    let o = lab.cli(&[
        "plan-copyright",
        "--set",
        "{index}",
        "--out",
        "x.json",
        lab.photos[0].to_str().unwrap(),
    ]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("unknown variable"));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// PRODUCT_SPEC §6.10–6.11: a Preset from JSON with conditions on the original snapshot and two
/// fields in one entry; the built-in "Remove GPS"; undo restores everything.
#[test]
fn preset_rules_plan_apply_and_undo() {
    let pkg = require!();
    let lab = Lab::new("preset", &pkg);
    let preset = lab.dir.join("preset.json");
    std::fs::write(
        &preset,
        r#"{"schema_version": 1, "name": "Studio", "rules": [
            {"name": "copyright where none",
             "when": [{"if": "empty", "field": "copyright"}],
             "then": [{"do": "set_copyright", "value": "© {creator|Studio} {year|2026}"}]},
            {"name": "no position in JPEGs",
             "when": [{"if": "not_empty", "field": "gps"}, {"if": "extension", "any": ["jpg"]}],
             "then": [{"do": "remove_gps"}]},
            {"name": "clock was an hour behind",
             "when": [{"if": "not_empty", "field": "capture_time"}],
             "then": [{"do": "shift_time", "by": "+01:00:00"}]}
        ]}"#,
    )
    .unwrap();
    let out = lab.dir.join("p.json");
    let mut args = vec![
        "plan-preset",
        "--preset",
        preset.to_str().unwrap(),
        "--out",
        out.to_str().unwrap(),
    ];
    let ps: Vec<String> = lab
        .photos
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    args.extend(ps.iter().map(String::as_str));
    let o = lab.cli(&args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pj = Lab::json(&o);
    let gps = lab
        .photos
        .iter()
        .position(|p| p.ends_with("GPS.jpg"))
        .unwrap();
    let fields = |i: usize| -> Vec<String> {
        pj["entries"][i]["changes"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["field"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(fields(gps).contains(&"gps".to_string()), "{pj}");
    let timed = (0..lab.photos.len())
        .filter(|&i| fields(i).contains(&"capture_time".to_string()))
        .count();
    assert!(timed >= 3, "{pj}");
    let with_copyright = lab.copyrights();
    for (i, c) in with_copyright.iter().enumerate() {
        let had = c["value"].as_str().is_some();
        assert_eq!(
            fields(i).contains(&"copyright".to_string()),
            !had && pj["entries"][i]["status"]["status"] == "ready",
            "{i}: {c} {pj}"
        );
    }
    let op = lab.apply_ok(&out);
    lab.undo(&op);
    lab.assert_all_original();

    // the built-in Remove GPS
    let o = lab.cli(&[
        "plan-preset",
        "--id",
        "builtin:Remove GPS",
        "--out",
        lab.dir.join("builtin.json").to_str().unwrap(),
        lab.photos[gps].to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert_eq!(Lab::json(&o)["entries"][0]["status"]["status"], "ready");
    let list = Lab::json(&lab.cli(&["presets"]));
    assert!(
        list.as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Copyright Template")
    );

    // saved presets: import, apply by id (last used), export, delete
    let id = Lab::json(&lab.cli(&["preset-import", preset.to_str().unwrap()]))["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let o = lab.cli(&[
        "plan-preset",
        "--id",
        &id,
        "--out",
        lab.dir.join("by-id.json").to_str().unwrap(),
        lab.photos[0].to_str().unwrap(),
    ]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    // SECURITY_MODEL §9: the first use of an imported Preset says so in the Preview, later ones not
    let untrusted = |o: &Output| {
        Lab::json(o)["entries"]
            .to_string()
            .contains("imported Preset")
    };
    assert!(untrusted(&o), "{}", String::from_utf8_lossy(&o.stdout));
    let again = lab.cli(&[
        "plan-preset",
        "--id",
        &id,
        "--out",
        lab.dir.join("by-id-2.json").to_str().unwrap(),
        lab.photos[0].to_str().unwrap(),
    ]);
    assert!(again.status.success());
    assert!(!untrusted(&again));
    let list = Lab::json(&lab.cli(&["presets"]));
    let saved = list
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == id.as_str())
        .unwrap()
        .clone();
    assert!(saved["last_used_ms"].is_i64(), "{saved}");
    assert_eq!(
        saved["fields"],
        serde_json::json!(["copyright", "gps", "capture_time"])
    );
    let exported = lab.cli(&["preset-export", &id]);
    let back: Value = serde_json::from_slice(&exported.stdout).unwrap();
    assert_eq!(back["name"], "Studio");
    assert!(lab.cli(&["preset-delete", &id]).status.success());
    assert!(
        !lab.cli(&["preset-delete", "builtin:Remove GPS"])
            .status
            .success()
    );
    // settings drive the retention policy and are checked
    assert!(
        lab.cli(&["setting", "backup.max_age_days", "7"])
            .status
            .success()
    );
    assert!(
        !lab.cli(&["setting", "backup.max_share_of_volume", "5"])
            .status
            .success()
    );
    let kept = Lab::json(&lab.cli(&["setting", "backup.max_share_of_volume"]));
    assert!(
        kept["value"].is_null(),
        "an invalid value is not kept: {kept}"
    );
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// PRODUCT_SPEC §6.14: History rows carry file and change counts, results, backup state and the
/// undo link; Export Log writes the field-level before/after to a new file.
#[test]
fn history_summary_and_export_log() {
    let pkg = require!();
    let lab = Lab::new("history", &pkg);
    let op = lab.apply_ok(&lab.plan("Morii", "p.json"));
    lab.undo(&op);
    let h = Lab::json(&lab.cli(&["history"]));
    let rows = h.as_array().unwrap();
    assert_eq!(rows.len(), 2);
    let (a, u) = (&rows[0], &rows[1]);
    assert_eq!(a["id"], op.as_str());
    assert_eq!(
        a["files"].as_u64().unwrap(),
        a["states"]["done"].as_u64().unwrap()
    );
    assert!(
        a["changes"].as_u64().unwrap() >= a["files"].as_u64().unwrap(),
        "{a}"
    );
    assert_eq!(a["undone_by"], serde_json::json!([u["id"]]));
    assert_eq!(u["undo_of"], op.as_str());
    assert_eq!(
        (a["undoable"].as_bool(), a["backups_pruned"].as_bool()),
        (Some(true), Some(false))
    );

    // by default no paths, no values, no user name (SECURITY_MODEL §8, T-14)
    let out = lab.dir.join("log.json");
    let o = lab.cli(&["export-log", &op, "--out", out.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let text = std::fs::read_to_string(&out).unwrap();
    let log: Value = serde_json::from_str(&text).unwrap();
    let first = &log["files_detail"][0];
    assert_eq!(first["path"], "asset#0.jpg", "{first}");
    assert_eq!(first["changes"][0]["field"], "creator", "{first}");
    assert!(first["changes"][0]["after"].is_null(), "{first}");
    assert!(!text.contains("Morii"), "a value was exported");
    let folder = lab.photos[0]
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    assert!(
        !text.to_lowercase().contains(&folder),
        "a path was exported"
    );
    if let Some(user) = std::env::var_os("USERNAME") {
        let u = user.to_string_lossy().to_lowercase();
        assert!(
            !text.to_lowercase().contains(&format!("users\\\\{u}")), // JSON-escaped separator
            "the user name was exported"
        );
    }
    assert_eq!(log["redacted"]["paths"], true);
    // the user may include them
    let full = lab.dir.join("full.json");
    let o = lab.cli(&[
        "export-log",
        &op,
        "--out",
        full.to_str().unwrap(),
        "--include-paths",
        "--include-values",
    ]);
    assert!(o.status.success());
    let log: Value = serde_json::from_slice(&std::fs::read(&full).unwrap()).unwrap();
    let first = &log["files_detail"][0];
    assert_eq!(
        first["changes"][0]["after"],
        serde_json::json!(["Morii"]),
        "{first}"
    );
    assert!(first["path"].as_str().unwrap().ends_with("Writer.jpg"));
    let before = std::fs::read(&out).unwrap();
    assert!(
        !lab.cli(&["export-log", &op, "--out", out.to_str().unwrap()])
            .status
            .success()
    );
    assert_eq!(
        std::fs::read(&out).unwrap(),
        before,
        "an existing file is never replaced"
    );
    // the report is written under a name of its own and renamed when complete
    let partial = std::fs::read_dir(out.parent().unwrap())
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".partial")
        })
        .count();
    assert_eq!(partial, 0);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// PRODUCT_SPEC §6.14 Retry Failed: a file that failed (injected IO error before its commit, the
/// original unchanged) is written by a retry Plan made from the persisted entries; both
/// Operations undo back to the originals. Nothing to retry is an error.
#[test]
fn retry_failed_files() {
    let pkg = require!();
    let lab = Lab::new("retry", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let o = lab.cli_env(&["apply", plan.to_str().unwrap(), "--fail-at", "2:4"], true);
    let op = Lab::json(&o)["op_id"].as_str().unwrap().to_owned();
    assert_eq!(lab.show(&op)["files"][2]["state"], "failed");
    let r = lab.dir.join("retry.json");
    let o = lab.cli(&["plan-retry", &op, "--out", r.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pj = Lab::json(&o);
    assert_eq!(pj["entries"].as_array().unwrap().len(), 1, "{pj}");
    assert_eq!(pj["entries"][0]["seq"], 2);
    let retry = lab.apply_ok(&r);
    assert_ne!(
        blake(&lab.photos[2]).as_deref(),
        Some(lab.truth[&lab.photos[2]].as_str())
    );
    let again = lab.cli(&[
        "plan-retry",
        &retry,
        "--out",
        lab.dir.join("r2.json").to_str().unwrap(),
    ]);
    assert!(!again.status.success());
    lab.undo(&retry);
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// Inspector and aggregate through the CLI, for a NEF whose creator lives in its sidecar: the
/// field view reads the sidecar first and lists the sidecar's own tags.
#[test]
fn inspect_and_aggregate_a_nef_with_its_sidecar() {
    let pkg = require!();
    let lab = Lab::new("inspect-nef", &pkg);
    let nef = lab.add_nef("a.NEF");
    let (p, _) = lab.plan_on(&["plan-creator", "--set", "Mori"], &[&nef], "c.json");
    let op = lab.apply_ok(&p);
    let d = Lab::json(&lab.cli(&["inspect", nef.to_str().unwrap()]));
    let creator = &d["fields"][0];
    assert_eq!(creator["field"], "creator");
    assert_eq!(creator["value"], "Mori", "{d}");
    assert!(
        d["sidecar"]
            .as_str()
            .unwrap()
            .to_lowercase()
            .ends_with("a.xmp")
    );
    assert_eq!(d["sidecar_tags"]["XMP-dc:Creator"], "Mori", "{d}");
    assert!(d["tags"].as_object().unwrap().len() > 20);

    let mut args = vec!["aggregate", nef.to_str().unwrap()];
    let ps: Vec<String> = lab
        .photos
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    args.extend(ps.iter().map(String::as_str));
    let a = Lab::json(&lab.cli(&args));
    let c = &a[0];
    assert_eq!(c["files"], 9, "{a}");
    assert!(
        c["values"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v[0] == "Mori" && v[1] == 1),
        "{a}"
    );
    lab.undo(&op);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §8.9 (D-6 option): by default a write gives the file a new modification time;
/// with `metadata.preserve_mtime` apply and undo keep it.
#[test]
fn preserve_mtime_option() {
    let pkg = require!();
    let lab = Lab::new("mtime", &pkg);
    let mtimes = || -> Vec<std::time::SystemTime> {
        lab.photos
            .iter()
            .map(|p| std::fs::metadata(p).unwrap().modified().unwrap())
            .collect()
    };
    let written = |before: &[String]| -> Vec<usize> {
        (0..lab.photos.len())
            .filter(|&i| blake(&lab.photos[i]).as_deref() != Some(before[i].as_str()))
            .collect()
    };
    let hashes = || -> Vec<String> { lab.photos.iter().map(|p| blake(p).unwrap()).collect() };

    let (t0, h0) = (mtimes(), hashes());
    std::thread::sleep(Duration::from_millis(50));
    let op = lab.apply_ok(&lab.plan("Morii", "a.json"));
    let w = written(&h0);
    assert!(!w.is_empty());
    let t1 = mtimes();
    assert!(
        w.iter().all(|&i| t1[i] != t0[i]),
        "default: new modification times"
    );
    lab.undo(&op);

    assert!(
        lab.cli(&["setting", "metadata.preserve_mtime", "true"])
            .status
            .success()
    );
    let (t2, h2) = (mtimes(), hashes());
    std::thread::sleep(Duration::from_millis(50));
    let op = lab.apply_ok(&lab.plan("Morii", "b.json"));
    let w = written(&h2);
    assert!(!w.is_empty());
    assert_eq!(mtimes(), t2, "kept through apply");
    lab.undo(&op);
    assert_eq!(mtimes(), t2, "kept through undo");
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §8.3 / §6.2: files in a synced folder get a note in the Plan (writing is still
/// allowed); backups in a synced folder get a warning. The OneDrive client's environment variable
/// is simulated for the child process.
/// SAFETY_MODEL §6.1: MoriMeta's own backups are never planned or written, wherever the backup
/// location is; a Plan edited to point at one is refused before anything is written.
#[test]
fn backups_are_never_planned_or_written() {
    let pkg = require!();
    let lab = Lab::new("ownbackup", &pkg);
    let op = lab.apply_ok(&lab.plan("Morii", "p.json"));
    let backup = std::fs::read_dir(lab.data.join("backups").join(&op))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("jpg")))
        .unwrap();
    let before = blake(&backup);
    let (_, pj) = lab.plan_on(&["plan-creator", "--set", "Mori"], &[&backup], "b.json");
    assert_eq!(pj["entries"][0]["status"]["status"], "blocked", "{pj}");
    assert!(pj.to_string().contains("MoriMeta's backups"), "{pj}");

    let (fp, _) = lab.plan_on(
        &["plan-creator", "--set", "Mori"],
        &[&lab.photos[1]],
        "c.json",
    );
    let mut plan: Value = serde_json::from_slice(&std::fs::read(&fp).unwrap()).unwrap();
    plan["entries"][0]["path"] = backup.to_string_lossy().into_owned().into();
    std::fs::write(&fp, plan.to_string()).unwrap();
    let a = lab.cli(&["apply", fp.to_str().unwrap()]);
    assert!(!a.status.success());
    let said = format!(
        "{}{}",
        String::from_utf8_lossy(&a.stdout),
        String::from_utf8_lossy(&a.stderr)
    );
    assert!(said.contains("MoriMeta's backups"), "{said}");
    assert_eq!(blake(&backup), before);
    assert_eq!(
        Lab::json(&lab.cli(&["history"])).as_array().unwrap().len(),
        1
    );
    lab.undo(&op);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

#[test]
fn synced_folders_are_noted() {
    let pkg = require!();
    let lab = Lab::new("sync", &pkg);
    let run = |args: &[&str], onedrive: &Path| {
        Command::new(env!("CARGO_BIN_EXE_mm-cli"))
            .env("OneDrive", onedrive)
            .arg("--data")
            .arg(&lab.data)
            .arg("--exiftool")
            .arg(&lab.pkg)
            .args(args)
            .output()
            .unwrap()
    };
    let out = lab.dir.join("p.json");
    let photo = lab.photos[0].to_str().unwrap();
    let o = run(
        &[
            "plan-creator",
            "--set",
            "Morii",
            "--out",
            out.to_str().unwrap(),
            photo,
        ],
        &lab.dir,
    );
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pj = Lab::json(&o);
    assert_eq!(pj["entries"][0]["status"]["status"], "ready");
    assert!(
        pj["entries"][0]["notes"]
            .to_string()
            .contains("OneDrive folder"),
        "{pj}"
    );
    let b = Lab::json(&run(&["backups"], &lab.dir));
    assert!(b["warning"].as_str().unwrap().contains("OneDrive"), "{b}");
    // the pre-flight the app shows before Apply carries it too, without blocking
    let pf = Lab::json(&run(&["preflight", out.to_str().unwrap()], &lab.dir));
    assert!(
        pf["backup_warning"].as_str().unwrap().contains("OneDrive"),
        "{pf}"
    );
    assert_eq!(pf["ok"], true, "{pf}");
    // a RAW's new sidecar in the synced folder (an ordinary new file) is noted the same way
    let nef = lab.add_nef("DSC_0001.NEF");
    let o = run(
        &[
            "plan-creator",
            "--set",
            "Morii",
            "--out",
            lab.dir.join("n.json").to_str().unwrap(),
            nef.to_str().unwrap(),
        ],
        &lab.dir,
    );
    let pj = Lab::json(&o);
    assert_eq!(pj["entries"][0]["status"]["status"], "ready", "{pj}");
    assert!(
        pj["entries"][0]["notes"]
            .to_string()
            .contains("OneDrive folder"),
        "{pj}"
    );
    // elsewhere: no note, no warning
    let elsewhere = lab.dir.join("not-synced");
    let o = run(
        &[
            "plan-creator",
            "--set",
            "Morii",
            "--out",
            lab.dir.join("q.json").to_str().unwrap(),
            photo,
        ],
        &elsewhere,
    );
    assert!(
        !Lab::json(&o)["entries"][0]["notes"]
            .to_string()
            .contains("OneDrive")
    );
    assert!(Lab::json(&run(&["backups"], &elsewhere))["warning"].is_null());
    let pf = Lab::json(&run(&["preflight", out.to_str().unwrap()], &elsewhere));
    assert!(pf["backup_warning"].is_null(), "{pf}");
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// PRODUCT_SPEC §6.15 "re-preview": a file changed after the Preview is a Conflict; the Plan
/// records what it was made from, so the same edit is planned again for that file from a fresh
/// read, applied, and both Operations undo.
#[test]
fn plan_again_for_a_conflict() {
    let pkg = require!();
    let lab = Lab::new("again", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let saved: Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
    assert_eq!(saved["source"]["from"], "creator", "{}", saved["source"]);
    assert_eq!(saved["source"]["set"], serde_json::json!(["Morii"]));
    // rewritten with the same bytes after the Preview: new identity and time
    std::thread::sleep(Duration::from_millis(20));
    let bytes = std::fs::read(&lab.photos[0]).unwrap();
    std::fs::write(&lab.photos[0], &bytes).unwrap();
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    assert_eq!(lab.show(&op)["files"][0]["state"], "conflict");

    let again = lab.dir.join("again.json");
    let o = lab.cli(&["plan-again", &op, "--out", again.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pj = Lab::json(&o);
    assert_eq!(pj["entries"].as_array().unwrap().len(), 1, "{pj}");
    assert_eq!(pj["entries"][0]["status"]["status"], "ready", "{pj}");
    let op2 = lab.apply_ok(&again);
    lab.undo(&op2);
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// PRODUCT_SPEC §6.15: after a crash the recovery screen shows done / remaining / attention;
/// "keep as it is" closes it, and the remaining files can still be continued later.
#[test]
fn recovery_summary_and_dismiss() {
    let pkg = require!();
    let lab = Lab::new("recovery-summary", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let o = lab.cli_env(
        &[
            "--workers",
            "1",
            "apply",
            plan.to_str().unwrap(),
            "--crash-at",
            "2:7",
        ],
        true,
    );
    assert_eq!(o.status.code(), Some(77));
    let op = lab.last_op();
    assert!(lab.cli(&["recover"]).status.success());
    let s = Lab::json(&lab.cli(&["recovery-status"]));
    let row = &s[0];
    assert_eq!(row["op_id"], op.as_str(), "{s}");
    assert_eq!(
        (row["done"].as_u64(), row["attention"].as_u64()),
        (Some(2), Some(0)),
        "{s}"
    );
    assert_eq!(
        row["remaining"].as_u64(),
        Some(lab.photos.len() as u64 - 2),
        "{s}"
    );

    assert!(lab.cli(&["dismiss", &op]).status.success());
    assert!(
        Lab::json(&lab.cli(&["recovery-status"]))
            .as_array()
            .unwrap()
            .is_empty()
    );
    let r = lab.cli(&["resume", &op]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stdout));
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// manifest.jsonl (the append-only record that rebuilds a lost journal) cannot be appended to:
/// while the Operation is registered, and at several points while files are written, once or
/// for good. Recovery (and resume where the Operation stopped) and undo restore every file.
#[test]
fn manifest_log_append_failures_recover() {
    let pkg = require!();
    for (skip, persist) in [(0, false), (3, true), (12, false), (20, true), (33, false)] {
        let case = format!("log append #{skip} fails (persistent {persist})");
        let lab = Lab::new(&format!("log-{skip}-{persist}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let fault = format!("log:{skip}");
        let mut args = vec!["apply", plan.to_str().unwrap(), "--journal-fail-at", &fault];
        if persist {
            args.push("--journal-fail-persist");
        }
        let o = lab.cli_env(&args, true);
        let out = String::from_utf8_lossy(&o.stdout).into_owned();
        // the failure is reported (as an error, or in the file it hit; after a commit that the
        // disk confirms, the file is done and keeps the error as its note)
        assert!(out.contains("manifest.jsonl"), "{case}: {out}");
        let op = lab.last_op();
        // a single failure mid-transaction is settled for that file from the journal and the
        // disk and the Operation goes on; a lasting one stops it for recovery
        let status = op_status(&lab, &op);
        assert!(
            status == "running" || status.starts_with("completed"),
            "{case}: {status}"
        );
        lab.assert_preimages(&op);
        let r = lab.cli(&["recover"]);
        assert!(
            r.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&r.stdout)
        );
        lab.assert_recovered(&op);
        if status == "running" {
            // files interrupted by the failure were settled as failed (original unchanged), so
            // resume may end with files not done (exit 3); it must not stop again
            let res = lab.cli(&["resume", &op]);
            let rj = Lab::json(&res);
            assert!(
                matches!(res.status.code(), Some(0 | 3)) && rj["status"] != "running",
                "{case}: {rj}"
            );
        }
        lab.undo(&op);
        lab.assert_all_original();
        assert!(lab.leftovers().is_empty(), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// SECURITY_MODEL §8 / T-14: the log file records Operations, files as asset#n.ext and their
/// states, recovery decisions, and nothing private: no written value, no photo path, no user
/// name.
#[test]
fn log_file_holds_no_private_data() {
    let pkg = require!();
    let lab = Lab::new("log-privacy", &pkg);
    let value = "LogPrivacyCheck Studio";
    let plan = lab.plan(value, "p.json");
    let o = lab.cli_env(
        &[
            "--workers",
            "1",
            "apply",
            plan.to_str().unwrap(),
            "--crash-at",
            "2:7",
        ],
        true,
    );
    assert_eq!(o.status.code(), Some(77));
    let op = lab.last_op();
    assert!(lab.cli(&["recover"]).status.success());
    assert!(lab.cli(&["resume", &op]).status.success());
    lab.undo(&op);
    lab.assert_all_original();

    let logs = lab.data.join("logs");
    let mut text = String::new();
    for e in std::fs::read_dir(&logs).unwrap() {
        text.push_str(&std::fs::read_to_string(e.unwrap().path()).unwrap());
    }
    for want in [
        "operation started",
        "operation finished",
        " file ",
        " recovered ",
        "asset#2.jpg",
    ] {
        assert!(text.contains(want), "no {want:?} in the log:\n{text}");
    }
    let lower = text.to_lowercase();
    assert!(!lower.contains(&value.to_lowercase()), "a value was logged");
    let folder = lab.photos[0]
        .parent()
        .unwrap()
        .to_string_lossy()
        .to_lowercase();
    assert!(!lower.contains(&folder), "a path was logged");
    assert!(
        !lower.contains("writer.jpg"),
        "a file name was logged:\n{text}"
    );
    if let Some(user) = std::env::var_os("USERNAME") {
        let u = user.to_string_lossy().to_lowercase();
        assert!(
            !lower.contains(&format!("users\\{u}")),
            "the user name was logged"
        );
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// ARCHITECTURE §12 debug log: off by default; once turned on it records ExifTool commands with
/// values cut to 12 characters and their timings; turned off, nothing more.
#[test]
fn debug_log_records_cut_commands_while_on() {
    let pkg = require!();
    let lab = Lab::new("debug-log", &pkg);
    let read_logs = || {
        let mut t = String::new();
        for e in std::fs::read_dir(lab.data.join("logs")).unwrap() {
            t.push_str(&std::fs::read_to_string(e.unwrap().path()).unwrap());
        }
        t
    };
    lab.apply_ok(&lab.plan("DebugValueThatIsLong", "a.json"));
    assert!(!read_logs().contains("exiftool"), "debug lines while off");

    assert!(lab.cli(&["debug-log", "on"]).status.success());
    let op = lab.apply_ok(&lab.plan("DebugValueThatIsLonger", "b.json"));
    let t = read_logs();
    assert!(t.contains(" debug exiftool "), "{t}");
    assert!(t.contains("=DebugValueTh…"), "{t}");
    assert!(
        !t.contains("DebugValueThatIsLonger"),
        "a full value was logged"
    );

    assert!(lab.cli(&["debug-log", "off"]).status.success());
    let before = read_logs().matches(" debug exiftool ").count();
    lab.undo(&op);
    assert_eq!(read_logs().matches(" debug exiftool ").count(), before);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// History "Now vs. after operation": as written, changed by another program, missing, and after
/// the undo the original again.
#[test]
fn now_vs_after_operation() {
    let pkg = require!();
    let lab = Lab::new("now", &pkg);
    let op = lab.apply_ok(&lab.plan("Morii", "p.json"));
    let now = |seqs: &[&str]| -> Vec<String> {
        let mut args = vec!["now", op.as_str()];
        args.extend(seqs);
        Lab::json(&lab.cli(&args))
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["now"].as_str().unwrap().to_owned())
            .collect()
    };
    assert!(now(&[]).iter().all(|s| s == "as_written"));
    let mut b = std::fs::read(&lab.photos[1]).unwrap();
    b.extend_from_slice(b"edited elsewhere");
    std::fs::write(&lab.photos[1], &b).unwrap();
    let moved = lab.dir.join("moved.jpg");
    std::fs::rename(&lab.photos[2], &moved).unwrap();
    assert_eq!(now(&["0", "1", "2"]), ["as_written", "changed", "missing"]);
    std::fs::rename(&moved, &lab.photos[2]).unwrap();
    let u = lab.dir.join("u.json");
    lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()]);
    lab.apply_ok(&u);
    let after = now(&["0", "1", "2"]);
    assert_eq!(
        after,
        ["original", "changed", "original"],
        "file 1 was not restored (conflict)"
    );
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §6 and INTERACTION_SPEC: the backup location is a setting that applies to later
/// Operations (earlier ones keep theirs); when it is unavailable nothing is written, and once it
/// is back the work continues; retention removes a backup folder wherever it is.
#[test]
fn backup_location_setting_and_unavailable_location() {
    let pkg = require!();
    let lab = Lab::new("backup-root", &pkg);
    let first = lab.apply_ok(&lab.plan("Morii", "a.json"));
    let default_dir = lab.data.join("backups").join(&first);
    assert!(default_dir.exists());

    let elsewhere = lab.dir.join("elsewhere-backups");
    assert!(
        lab.cli(&["setting", "backup.root", elsewhere.to_str().unwrap()])
            .status
            .success()
    );
    let second = lab.apply_ok(&lab.plan("Mori", "b.json"));
    assert!(elsewhere.join(&second).join("manifest.jsonl").exists());
    let b = Lab::json(&lab.cli(&["backups"]));
    assert!(b["total_bytes"].as_u64().unwrap() > 0);

    // unavailable: the location's parent is a file
    let blocker = lab.dir.join("not-a-folder");
    std::fs::write(&blocker, b"x").unwrap();
    let gone = blocker.join("backups");
    assert!(
        lab.cli(&["setting", "backup.root", gone.to_str().unwrap()])
            .status
            .success()
    );
    let before: Vec<String> = lab.photos.iter().map(|p| blake(p).unwrap()).collect();
    let p3 = lab.plan("Someone Else", "c.json");
    let o = lab.cli(&["apply", p3.to_str().unwrap()]);
    assert!(!o.status.success());
    assert!(String::from_utf8_lossy(&o.stdout).contains("BackupUnavailable"));
    let after: Vec<String> = lab.photos.iter().map(|p| blake(p).unwrap()).collect();
    assert_eq!(before, after, "nothing may be written without backups");

    // back: undo both (each from its own recorded folder), prune the second
    assert!(
        lab.cli(&["setting", "backup.root", elsewhere.to_str().unwrap()])
            .status
            .success()
    );
    lab.undo(&second);
    lab.undo(&first);
    lab.assert_all_original();
    assert!(lab.cli(&["prune", "--requested", &second]).status.success());
    assert!(!elsewhere.join(&second).exists());
    assert!(default_dir.exists());
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SCREEN_SPEC History "Restore backup to folder…": copies of the files as they were before the
/// Operation, as new files (a taken name gets a number), checked against their hashes; the
/// written files stay as they are.
#[test]
fn restore_backups_to_a_folder() {
    let pkg = require!();
    let lab = Lab::new("restore-to", &pkg);
    let op = lab.apply_ok(&lab.plan("Morii", "p.json"));
    let after: Vec<String> = lab.photos.iter().map(|p| blake(p).unwrap()).collect();
    let out = lab.dir.join("restored");
    std::fs::create_dir_all(&out).unwrap();
    std::fs::write(out.join("Writer.jpg"), b"already here").unwrap();
    // what an interrupted earlier restore leaves: an incomplete copy under its own name
    std::fs::write(out.join("Nikon.jpg.mmrestore-0"), b"partial").unwrap();
    let r = Lab::json(&lab.cli(&["restore-to", &op, "--dir", out.to_str().unwrap()]));
    let rows = r.as_array().unwrap();
    assert_eq!(rows.len(), lab.photos.len(), "{r}");
    for (row, p) in rows.iter().zip(&lab.photos) {
        let to = PathBuf::from(row["to"].as_str().unwrap());
        assert_eq!(blake(&to).as_deref(), Some(lab.truth[p].as_str()), "{row}");
    }
    assert!(
        rows[0]["to"].as_str().unwrap().ends_with("Writer (2).jpg"),
        "{r}"
    );
    assert_eq!(
        std::fs::read(out.join("Writer.jpg")).unwrap(),
        b"already here"
    );
    assert!(
        rows[1]["to"].as_str().unwrap().ends_with("Nikon.jpg"),
        "{r}"
    );
    // a copy only ever takes a photo's name when it is complete: no other incomplete copies
    let incomplete: Vec<String> = std::fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".mmrestore-"))
        .collect();
    assert_eq!(
        incomplete,
        ["Nikon.jpg.mmrestore-0"],
        "the earlier one is left alone"
    );
    let now: Vec<String> = lab.photos.iter().map(|p| blake(p).unwrap()).collect();
    assert_eq!(now, after, "the written files are not touched");
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §3–4 pre-flight: a clean Plan passes; a file changed after the Preview is
/// reported for rescan (and Apply would block it as a Conflict); an unusable backup location is
/// reported; nothing is written by the check.
#[test]
fn preflight_reports_rescans_and_backup_problems() {
    let pkg = require!();
    let lab = Lab::new("preflight", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let check = || lab.cli(&["preflight", plan.to_str().unwrap()]);
    let o = check();
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    assert_eq!(Lab::json(&o)["ok"], true);

    std::thread::sleep(Duration::from_millis(20));
    let bytes = std::fs::read(&lab.photos[3]).unwrap();
    std::fs::write(&lab.photos[3], &bytes).unwrap();
    let o = check();
    assert_eq!(o.status.code(), Some(3));
    assert_eq!(Lab::json(&o)["rescan"], serde_json::json!([3]));

    let blocker = lab.dir.join("a-file");
    std::fs::write(&blocker, b"x").unwrap();
    let bad = blocker.join("backups");
    assert!(
        lab.cli(&["setting", "backup.root", bad.to_str().unwrap()])
            .status
            .success()
    );
    let r = Lab::json(&check());
    assert!(
        r["backup"].as_str().unwrap().contains("BackupUnavailable"),
        "{r}"
    );
    assert!(
        lab.cli(&["setting", "backup.root", "--clear"])
            .status
            .success()
    );
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §17: in a Sequence a RAW and its JPG (same folder, same name) share one
/// timestamp; the next file takes the next step.
#[test]
fn sequence_gives_a_raw_and_its_jpg_one_time() {
    let pkg = require!();
    let lab = Lab::new("seq-pair", &pkg);
    let nef = lab.add_nef("DSC_0001.NEF");
    let dir = nef.parent().unwrap().to_path_buf();
    let jpg = dir.join("DSC_0001.JPG");
    std::fs::copy(&lab.photos[1], &jpg).unwrap(); // Nikon.jpg has a capture time
    let other = dir.join("DSC_0002.JPG");
    std::fs::copy(&lab.photos[2], &other).unwrap();
    let (_, pj) = lab.plan_on(
        &[
            "plan-time",
            "--sequence",
            "2024:05:01 09:00:00",
            "--step",
            "00:01:00",
            "--order",
            "name",
        ],
        &[&jpg, &nef, &other],
        "s.json",
    );
    let after = |i: usize| -> String {
        pj["entries"][i]["changes"][0]["after"][0].as_str().unwrap()[..19].to_owned()
    };
    assert_eq!(after(0), "2024:05:01 09:00:00", "{pj}");
    assert_eq!(after(1), "2024:05:01 09:00:00", "{pj}");
    assert_eq!(after(2), "2024:05:01 09:01:00", "{pj}");
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §4–5: a Plan that removes a field lists the acknowledgement it needs; the
/// ones given are recorded with the Operation, exported with its log, and survive a rebuilt
/// journal.
#[test]
fn acknowledgements_are_recorded_with_the_operation() {
    let pkg = require!();
    let lab = Lab::new("acks", &pkg);
    let gps = lab
        .photos
        .iter()
        .find(|p| p.ends_with("GPS.jpg"))
        .unwrap()
        .clone();
    let (p, pj) = lab.plan_gps(&["--remove"], &[&gps], "r.json");
    assert_eq!(
        pj["required_acks"],
        serde_json::json!(["remove:gps"]),
        "{pj}"
    );
    let a = lab.cli(&["apply", p.to_str().unwrap(), "--ack", "remove:gps"]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    let log = lab.dir.join("log.json");
    assert!(
        lab.cli(&["export-log", &op, "--out", log.to_str().unwrap()])
            .status
            .success()
    );
    let v: Value = serde_json::from_slice(&std::fs::read(&log).unwrap()).unwrap();
    assert_eq!(v["acks"], serde_json::json!(["remove:gps"]));

    lab.lose_database();
    lab.rebuild();
    let log2 = lab.dir.join("log2.json");
    assert!(
        lab.cli(&["export-log", &op, "--out", log2.to_str().unwrap()])
            .status
            .success()
    );
    let v: Value = serde_json::from_slice(&std::fs::read(&log2).unwrap()).unwrap();
    assert_eq!(
        v["acks"],
        serde_json::json!(["remove:gps"]),
        "after rebuild"
    );
    assert!(lab.cli(&["recover"]).status.success());
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §7: a read-only file is blocked until the user clears the attribute through
/// the explicit action; then it is planned like any other.
#[test]
fn read_only_attribute_is_cleared_only_on_request() {
    let pkg = require!();
    let lab = Lab::new("clear-ro", &pkg);
    let f = lab.photos[0].clone();
    let mut perm = std::fs::metadata(&f).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&f, perm).unwrap();
    let (_, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&f], "a.json");
    assert_eq!(pj["entries"][0]["status"]["status"], "blocked", "{pj}");
    let o = Lab::json(&lab.cli(&["clear-readonly", f.to_str().unwrap()]));
    assert_eq!(o["cleared"], true);
    assert!(!std::fs::metadata(&f).unwrap().permissions().readonly());
    let (_, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&f], "b.json");
    assert_eq!(pj["entries"][0]["status"]["status"], "ready", "{pj}");
    let o = Lab::json(&lab.cli(&["clear-readonly", f.to_str().unwrap()]));
    assert_eq!(o["cleared"], false);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SCREEN_SPEC Library "Needs attention": read-only files, links, a RAW with a darktable sidecar
/// and a file with C2PA Content Credentials are found by one call; nothing is written.
#[test]
fn needs_attention_summary() {
    let pkg = require!();
    let lab = Lab::new("attention-summary", &pkg);
    let dir = lab.photos[0].parent().unwrap().to_path_buf();
    let mut perm = std::fs::metadata(&lab.photos[0]).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&lab.photos[0], perm).unwrap();
    std::fs::hard_link(&lab.photos[2], lab.dir.join("second-link.jpg")).unwrap();
    let nef = lab.add_nef("dt.NEF");
    std::fs::write(
        dir.join("dt.NEF.xmp"),
        b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>",
    )
    .unwrap();
    let cred = dir.join("credentials.jpg");
    let src = std::fs::read(&lab.photos[1]).unwrap();
    let mut bytes = src[..2].to_vec();
    bytes.extend(jumbf_app11());
    bytes.extend(&src[2..]);
    std::fs::write(&cred, &bytes).unwrap();
    let files = [&lab.photos[0], &lab.photos[1], &lab.photos[2], &nef, &cred];
    let mut args = vec!["attention"];
    args.extend(files.iter().map(|p| p.to_str().unwrap()));
    let a = Lab::json(&lab.cli(&args));
    assert_eq!(a["read_only"], serde_json::json!([0]), "{a}");
    assert_eq!(a["links"], serde_json::json!([2]), "{a}");
    assert_eq!(a["darktable_sidecars"], serde_json::json!([3]), "{a}");
    assert_eq!(a["c2pa"], serde_json::json!([4]), "{a}");
    let mut perm = std::fs::metadata(&lab.photos[0]).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    perm.set_readonly(false);
    std::fs::set_permissions(&lab.photos[0], perm).unwrap();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §12 Retry failed: a file skipped because it became read-only is in the retry
/// Plan but excluded until the user clears the attribute.
#[test]
fn retry_waits_for_the_read_only_attribute() {
    let pkg = require!();
    let lab = Lab::new("retry-ro", &pkg);
    let plan = lab.plan("Morii", "p.json");
    let f = lab.photos[1].clone();
    let mut perm = std::fs::metadata(&f).unwrap().permissions();
    perm.set_readonly(true);
    std::fs::set_permissions(&f, perm).unwrap();
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    assert_eq!(lab.show(&op)["files"][1]["state"], "skipped");

    let r1 = lab.dir.join("r1.json");
    let pj = Lab::json(&lab.cli(&["plan-retry", &op, "--out", r1.to_str().unwrap()]));
    assert_eq!(pj["entries"][0]["excluded"], true, "{pj}");
    assert!(
        lab.cli(&["clear-readonly", f.to_str().unwrap()])
            .status
            .success()
    );
    let r2 = lab.dir.join("r2.json");
    let pj = Lab::json(&lab.cli(&["plan-retry", &op, "--out", r2.to_str().unwrap()]));
    assert_eq!(pj["entries"][0]["excluded"], false, "{pj}");
    let retry = lab.apply_ok(&r2);
    lab.undo(&retry);
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SCREEN_SPEC Settings: every known setting is listed with its default; values are checked,
/// unknown keys refused; exec.workers becomes the default worker count.
#[test]
fn settings_are_listed_checked_and_used() {
    let pkg = require!();
    let lab = Lab::new("settings", &pkg);
    let list = Lab::json(&lab.cli(&["settings"]));
    let keys: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k["key"].as_str().unwrap())
        .collect();
    for k in [
        "backup.root",
        "backup.max_age_days",
        "metadata.copyright_template",
        "exec.workers",
    ] {
        assert!(keys.contains(&k), "{k} missing: {list}");
    }
    assert!(!lab.cli(&["setting", "no.such", "1"]).status.success());
    assert!(!lab.cli(&["setting", "exec.workers", "0"]).status.success());
    let o = Lab::json(&lab.cli(&["setting", "exec.workers", "2"]));
    assert_eq!(o["effective"], "2");
    let a = lab.cli(&["apply", lab.plan("Morii", "p.json").to_str().unwrap()]);
    assert!(a.status.success());
    let log = std::fs::read_dir(lab.data.join("logs"))
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap())
        .collect::<String>();
    assert!(log.contains("workers=\"2\""), "{log}");
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// The IO-error and simulated disk-full injections at the first and the last file (the fault
/// matrix had them at file 2 only): same per-file outcomes, and every case ends byte-identical.
#[test]
fn io_errors_and_disk_full_at_the_first_and_last_file() {
    let pkg = require!();
    let last = SAMPLES.len() as u64 - 1;
    for seq in [0, last] {
        for step in [1u8, 4, 7, 8, 10] {
            for kind in ["fail", "disk-full"] {
                let case = format!("{kind} at {seq}:{step}");
                let lab = Lab::new(&format!("edge-{kind}-{seq}-{step}"), &pkg);
                let plan = lab.plan("Morii", "p.json");
                let flag = format!("--{kind}-at");
                let at = format!("{seq}:{step}");
                let o = lab.cli_env(
                    &[
                        "apply",
                        plan.to_str().unwrap(),
                        &flag,
                        &at,
                        "--workers",
                        "1",
                    ],
                    true,
                );
                let r = Lab::json(&o);
                let op = r["op_id"]
                    .as_str()
                    .unwrap_or_else(|| panic!("{case}: {r}"))
                    .to_owned();
                let committed = step >= 8;
                for s in 0..=last {
                    let want = match (s.cmp(&seq), kind) {
                        (std::cmp::Ordering::Less, _) => "done",
                        (std::cmp::Ordering::Equal, _) if committed => "done",
                        (std::cmp::Ordering::Equal, "fail") => "failed",
                        (_, "fail") => "done",
                        _ => "cancelled",
                    };
                    assert_eq!(state_of(&r, s), want, "{case} file {s}: {r}");
                }
                if !committed {
                    let p = &lab.photos[seq as usize];
                    assert_eq!(blake(p).as_deref(), Some(lab.truth[p].as_str()), "{case}");
                }
                // a disk-full stop with no file left to start is not reported as paused
                let status = match kind {
                    "fail" if committed => "completed",
                    "fail" => "completed_with_errors",
                    _ if committed && seq == last => "completed",
                    _ => "cancelled",
                };
                assert_eq!(r["status"], status, "{case}: {r}");
                assert_eq!(r["note"].is_string(), status == "cancelled", "{case}: {r}");
                assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
                assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
                if kind == "disk-full" {
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
                }
                lab.undo(&op);
                lab.assert_all_original();
                assert!(lab.leftovers().is_empty(), "{case}");
                let _ = std::fs::remove_dir_all(&lab.dir);
            }
        }
    }
}

/// SAFETY_MODEL §8.13 on the Undo path: a simulated ERROR_DISK_FULL at every fault point pauses
/// the Undo Operation; each file is its applied or its original content; resume finishes the Undo.
#[test]
fn disk_full_during_undo_pauses_and_resume_completes_it() {
    let pkg = require!();
    for step in 1u8..=10 {
        let case = format!("undo disk full at 2:{step}");
        let lab = Lab::new(&format!("undo-full-{step}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let op = lab.apply_ok(&plan);
        let applied = lab.snapshot();
        let u = lab.dir.join("undo.json");
        assert!(
            lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()])
                .status
                .success()
        );
        let o = lab.cli_env(
            &[
                "apply",
                u.to_str().unwrap(),
                "--disk-full-at",
                &format!("2:{step}"),
                "--workers",
                "1",
            ],
            true,
        );
        assert_eq!(o.status.code(), Some(3), "{case}");
        let r = Lab::json(&o);
        let undo_op = r["op_id"].as_str().unwrap().to_owned();
        assert_ne!(undo_op, op, "{case}");
        assert_eq!(r["status"], "cancelled", "{case}: {r}");
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
                assert_eq!(blake(p).as_deref(), Some(applied[p].as_str()), "{case}");
            }
        }
        lab.assert_recovered_of(&undo_op, &applied);
        let res = lab.cli(&["resume", &undo_op]);
        assert!(
            res.status.success(),
            "{case}: {}",
            String::from_utf8_lossy(&res.stdout)
        );
        lab.assert_all_original();
        assert!(lab.leftovers().is_empty(), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// Updating an existing sidecar (a NEF's second Operation): crash and IO error at every fault
/// point. The sidecar only ever holds the first or the second Operation's complete content;
/// recovery, resume and both undos return to no sidecar, and the NEF is never touched.
#[test]
fn nef_sidecar_update_crashes_and_io_errors_recover() {
    let pkg = require!();
    for (kind, step) in (1u8..=10)
        .map(|s| ("crash", s))
        .chain((1u8..=10).map(|s| ("fail", s)))
    {
        let case = format!("sidecar update {kind} at 0:{step}");
        let lab = Lab::new(&format!("nef-upd-{kind}-{step}"), &pkg);
        let nef = lab.add_nef("DSC_0001.NEF");
        let xmp = nef.with_file_name("DSC_0001.xmp");
        let (p1, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&nef], "c1.json");
        let op1 = lab.apply_ok(&p1);
        let first = blake(&xmp).expect("sidecar created");
        let (p2, _) = lab.plan_on(&["plan-creator", "--set", "Someone"], &[&nef], "c2.json");
        let flag = if kind == "crash" {
            "--crash-at"
        } else {
            "--fail-at"
        };
        let o = lab.cli_env(
            &["apply", p2.to_str().unwrap(), flag, &format!("0:{step}")],
            true,
        );
        let op2 = lab.last_op();
        assert_ne!(op2, op1, "{case}: not registered");
        // true: the complete second sidecar; false: still the first one; anything else fails
        let second = |when: &str| -> bool {
            if blake(&xmp).as_deref() == Some(first.as_str()) {
                return false;
            }
            assert_eq!(
                lab.xmp_tags(&xmp).get("XMP-dc:Creator"),
                Some(&serde_json::json!("Someone")),
                "{case} {when}: neither the first nor the complete second sidecar"
            );
            true
        };
        if kind == "crash" {
            assert_eq!(o.status.code(), Some(77), "{case}");
            if xmp.exists() {
                second("before recovery");
            }
            assert!(lab.cli(&["recover"]).status.success(), "{case}");
            second("after recovery");
            assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
            assert!(lab.cli(&["fsck", &op2]).status.success(), "{case}");
            let res = lab.cli(&["resume", &op2]);
            assert!(
                res.status.success(),
                "{case}: {}",
                String::from_utf8_lossy(&res.stdout)
            );
            assert!(second("after resume"), "{case}");
        } else {
            let r = Lab::json(&o);
            assert_ne!(r["status"], "running", "{case}: {r}");
            let want = if step <= 7 { "failed" } else { "done" };
            assert_eq!(state_of(&r, 0), want, "{case}: {r}");
            assert_eq!(second("after the error"), step >= 8, "{case}");
            assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
            assert!(lab.cli(&["fsck", &op2]).status.success(), "{case}");
        }
        if blake(&xmp).as_deref() != Some(first.as_str()) {
            lab.undo(&op2);
        }
        assert_eq!(blake(&xmp).as_deref(), Some(first.as_str()), "{case}");
        lab.undo(&op1);
        assert!(!xmp.exists(), "{case}");
        assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// SAFETY_MODEL §12 "verification effectiveness": file 2's temporary output is spoiled after
/// ExifTool wrote it and before it is verified. Every defect is refused by the check named (the
/// file fails, its original is unchanged, nothing is left behind); the other files are written,
/// and undo restores every file byte for byte.
#[test]
fn verification_refuses_every_spoiled_output() {
    let pkg = require!();
    for (kind, caught) in [
        ("warning", &["verification V1"][..]),
        ("truncate", &["ImageData(", "Unreadable(", "V5"][..]),
        ("image-byte", &["ImageData("][..]),
        ("drop:IFD0:Make", &["unexpected changes"][..]),
        ("extra-tag", &["unexpected changes"][..]),
        ("wrong-value", &["Value("][..]),
        ("list-append", &["Value("][..]),
    ] {
        let case = format!("tamper {kind}");
        let lab = Lab::new(&format!("tamper-{}", kind.replace(':', "-")), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--tamper-at",
                &format!("2:{kind}"),
            ],
            true,
        );
        let r = Lab::json(&o);
        let op = r["op_id"]
            .as_str()
            .unwrap_or_else(|| panic!("{case}: {r}"))
            .to_owned();
        assert_eq!(r["status"], "completed_with_errors", "{case}: {r}");
        for f in r["files"].as_array().unwrap() {
            if f["seq"] == 2 {
                assert_eq!(f["state"], "failed", "{case}: {f}");
                let why = f["reason"].as_str().unwrap_or_default();
                assert!(
                    caught.iter().any(|c| why.contains(c)),
                    "{case}: caught by the wrong check: {why}"
                );
            } else {
                assert_eq!(f["state"], "done", "{case}: {f}");
            }
        }
        let p2 = &lab.photos[2];
        assert_eq!(blake(p2).as_deref(), Some(lab.truth[p2].as_str()), "{case}");
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// The same for a new XMP sidecar (verified against an empty source, V1–V3 and V5): a spoiled
/// sidecar is never created and the NEF is never touched.
#[test]
fn verification_refuses_a_spoiled_new_sidecar() {
    let pkg = require!();
    for (kind, caught) in [
        ("warning", "verification V1"),
        ("extra-tag", "unexpected changes"),
        ("wrong-value", "Value("),
        ("list-append", "Value("),
    ] {
        let case = format!("new sidecar tamper {kind}");
        let lab = Lab::new(&format!("tamper-xmp-{kind}"), &pkg);
        let nef = lab.add_nef("DSC_0001.NEF");
        let xmp = nef.with_file_name("DSC_0001.xmp");
        let (p, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&nef], "c.json");
        let o = lab.cli_env(
            &[
                "apply",
                p.to_str().unwrap(),
                "--tamper-at",
                &format!("0:{kind}"),
            ],
            true,
        );
        let r = Lab::json(&o);
        assert_eq!(state_of(&r, 0), "failed", "{case}: {r}");
        let why = r["files"][0]["reason"].as_str().unwrap_or_default();
        assert!(
            why.contains(caught),
            "{case}: caught by the wrong check: {why}"
        );
        assert!(!xmp.exists(), "{case}");
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3), "{case}");
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// SAFETY_MODEL §8.6: a file that is a symbolic link is blocked when planned, and one that became a
/// link after the Preview is skipped when applied; neither the link nor its target is written. A
/// sidecar that is a link blocks its NEF. Needs the right to create symbolic links (Developer Mode
/// or an elevated test run, as on CI); skipped otherwise.
#[test]
fn symbolic_links_are_never_written() {
    let pkg = require!();
    let lab = Lab::new("symlinks", &pkg);
    let dir = lab.photos[0].parent().unwrap().to_path_buf();
    let outside = lab.dir.join("elsewhere");
    std::fs::create_dir_all(&outside).unwrap();
    let target = outside.join("target.jpg");
    std::fs::copy(&lab.photos[4], &target).unwrap();
    let link = dir.join("link.jpg");
    if let Err(e) = std::os::windows::fs::symlink_file(&target, &link) {
        // the CI runner is elevated: there, a skip would hide a broken test
        assert!(
            std::env::var_os("CI").is_none(),
            "cannot create a link on CI: {e}"
        );
        eprintln!("SKIP: cannot create symbolic links here ({e})");
        let _ = std::fs::remove_dir_all(&lab.dir);
        return;
    }
    let target_hash = blake(&target).unwrap();

    // planned as a link: blocked
    let (p, pj) = lab.plan_on(
        &["plan-creator", "--set", "Morii"],
        &[&link, &lab.photos[0]],
        "p.json",
    );
    let st = |i: usize| {
        pj["entries"][i]["status"]["status"]
            .as_str()
            .unwrap()
            .to_owned()
    };
    assert_eq!(st(0), "blocked", "{pj}");
    assert_eq!(st(1), "ready", "{pj}");
    let op = lab.apply_ok(&p);
    assert!(
        std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(blake(&target).as_deref(), Some(target_hash.as_str()));

    // replaced by a link after the Preview: skipped at apply
    let later = &lab.photos[5];
    let (p2, pj2) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[later], "p2.json");
    assert_eq!(pj2["entries"][0]["status"]["status"], "ready", "{pj2}");
    let moved = outside.join("moved.jpg");
    std::fs::rename(later, &moved).unwrap();
    std::os::windows::fs::symlink_file(&moved, later).unwrap();
    let moved_hash = blake(&moved).unwrap();
    let a = lab.cli(&["apply", p2.to_str().unwrap()]);
    let r = Lab::json(&a);
    let s0 = state_of(&r, 0);
    assert!(s0 == "skipped" || s0 == "conflict", "{r}");
    assert!(
        std::fs::symlink_metadata(later)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(blake(&moved).as_deref(), Some(moved_hash.as_str()));
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    std::fs::remove_file(later).unwrap();
    std::fs::rename(&moved, later).unwrap();

    // a sidecar that is a link: its NEF is blocked, nothing written through the link
    let nef = lab.add_nef("DSC_0001.NEF");
    let real_xmp = outside.join("real.xmp");
    std::fs::write(&real_xmp, b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>").unwrap();
    std::os::windows::fs::symlink_file(&real_xmp, nef.with_file_name("DSC_0001.xmp")).unwrap();
    let (_, pj3) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&nef], "p3.json");
    assert_eq!(pj3["entries"][0]["status"]["status"], "blocked", "{pj3}");
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));

    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §12 corpus regression, the procedure for every ExifTool upgrade: every one of
/// ExifTool's own test files goes through the four MVP writes (formats this build does not write
/// must stay Unsupported and untouched); each write is verified before its commit and then undone
/// byte for byte. What happened to each file in each write is
/// compared with `tests/regression/exiftool-<version>.json`. After an upgrade, run with
/// `MM_UPDATE_REGRESSION=1` to rewrite that file and review its diff: every change needs a
/// known reason (SAFETY_MODEL §12 "0 verification failures, or each with a known cause").
#[test]
fn fixture_regression_through_every_write() {
    let pkg = require!();
    let lab = Lab::new("regression", &pkg);
    let dir = lab.dir.join("all");
    std::fs::create_dir_all(&dir).unwrap();
    let mut files: Vec<PathBuf> = std::fs::read_dir(timages())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.is_file())
        .map(|p| {
            let to = dir.join(p.file_name().unwrap());
            std::fs::copy(&p, &to).unwrap();
            to
        })
        .collect();
    files.sort();
    let refs: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let anchor = dir.join("Canon.jpg");
    let anchor = anchor.to_str().unwrap();
    let name = |p: &str| {
        Path::new(p)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned()
    };
    // reasons can name the file; keep the record free of this machine's paths
    let scrub = |s: &str| {
        s.replace(dir.to_str().unwrap(), "<dir>")
            .replace(lab.dir.to_str().unwrap(), "<lab>")
    };
    let original: BTreeMap<String, String> = files
        .iter()
        .map(|p| (name(p.to_str().unwrap()), blake(p).unwrap()))
        .collect();
    let mut got: BTreeMap<String, BTreeMap<String, String>> = original
        .iter()
        .map(|(n, h)| {
            (
                n.clone(),
                BTreeMap::from([("blake3".to_owned(), h.clone())]),
            )
        })
        .collect();
    for (write, cmd) in [
        ("1 creator", &["plan-creator", "--set", "Morii"][..]),
        (
            "2 copyright",
            &["plan-copyright", "--set", "© 2026 Morii"][..],
        ),
        ("3 gps", &["plan-gps", "--set", "35.6586,139.7454,40"][..]),
        ("4 time", &["plan-time", "--shift", "+01:00:00"][..]),
        // the built-in Presets: rules and per-file templates over every odd file
        (
            "5 preset copyright template",
            &["plan-preset", "--id", "builtin:Copyright Template"][..],
        ),
        (
            "6 preset remove gps",
            &["plan-preset", "--id", "builtin:Remove GPS"][..],
        ),
        // the template again, with defaults: {creator} and {year} rendered per file
        (
            "7 preset copyright template with defaults",
            &["plan-preset", "--id", "builtin:Copyright Template"][..],
        ),
        // the other time tools over every odd timestamp (sub-seconds, offsets, invalid dates)
        (
            "8 time absolute",
            &["plan-time", "--absolute", "2026:09:28 10:00:00"][..],
        ),
        (
            "9 time sequence",
            &[
                "plan-time",
                "--sequence",
                "2026:09:28 10:00:00",
                "--step",
                "00:00:10",
                "--order",
                "name",
            ][..],
        ),
        (
            "10 time preserve relative timing",
            &[
                "plan-time",
                "--preserve",
                anchor,
                "--to",
                "2026:09:28 10:00:00",
            ][..],
        ),
    ] {
        if write.starts_with('7') {
            let set = lab.cli(&[
                "setting",
                "metadata.copyright_template",
                "© {creator|Morii} {year|2026}",
            ]);
            assert!(
                set.status.success(),
                "{}",
                String::from_utf8_lossy(&set.stdout)
            );
        }
        let n = write.split(' ').next().unwrap();
        let (p, pj) = lab.plan_on(cmd, &refs, &format!("w{n}.json"));
        for e in pj["entries"].as_array().unwrap() {
            let st = &e["status"];
            let s = match (st["status"].as_str().unwrap(), st["reason"].as_str()) {
                ("ready", _) => "ready".to_owned(),
                (k, Some(r)) => format!("{k}: {}", scrub(r)),
                (k, None) => k.to_owned(),
            };
            // a new sidecar for a RAW is an entry of its own
            got.entry(name(e["path"].as_str().unwrap()))
                .or_default()
                .insert(write.to_owned(), s);
        }
        // one worker: files finish in plan order, so the circuit breaker (counted in completion
        // order) stops at the same place on every machine
        let a = lab.cli(&["apply", p.to_str().unwrap(), "--workers", "1"]);
        let r = Lab::json(&a);
        if let Some(op) = r["op_id"].as_str() {
            let mut how = r["status"].as_str().unwrap().to_owned();
            if r["status"] == "cancelled" {
                // paused (the breaker): the files it held back are tried by resume
                how = format!("{how} ({}); resumed", r["note"].as_str().unwrap_or("?"));
                let res = lab.cli(&["resume", op, "--workers", "1"]);
                assert!(
                    res.status.code().is_some_and(|c| c == 0 || c == 3),
                    "{write}"
                );
            }
            got.entry("_operations".into())
                .or_default()
                .insert(write.to_owned(), how);
            for f in lab.show(op)["files"].as_array().unwrap() {
                let n = name(f["path"].as_str().unwrap());
                let s = match (f["state"].as_str().unwrap(), f["error"].as_str()) {
                    ("done", _) => "done".to_owned(),
                    (k, Some(why)) => format!("{k}: {}", scrub(why)),
                    (k, None) => k.to_owned(),
                };
                got.entry(n).or_default().insert(write.to_owned(), s);
            }
            assert!(lab.cli(&["fsck", op]).status.success(), "{write}");
            lab.undo(op);
        }
        for p in &files {
            let n = name(p.to_str().unwrap());
            assert_eq!(
                blake(p).as_deref(),
                Some(original[&n].as_str()),
                "{write}: {n} not restored"
            );
        }
        assert!(
            std::fs::read_dir(&dir).unwrap().count() == files.len(),
            "{write}: a file was left behind"
        );
    }
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("research/exiftool.lock.json")).unwrap(),
    )
    .unwrap();
    let record = repo().join(format!(
        "crates/mm-cli/tests/regression/exiftool-{}.json",
        lock["version"].as_str().unwrap()
    ));
    let text = serde_json::to_string_pretty(&got).unwrap() + "\n";
    if std::env::var("MM_UPDATE_REGRESSION").as_deref() == Ok("1") {
        std::fs::create_dir_all(record.parent().unwrap()).unwrap();
        std::fs::write(&record, &text).unwrap();
    } else {
        let want = std::fs::read_to_string(&record).unwrap_or_else(|e| {
            panic!(
                "{}: {e}; run with MM_UPDATE_REGRESSION=1 and review",
                record.display()
            )
        });
        let want: BTreeMap<String, BTreeMap<String, String>> = serde_json::from_str(&want).unwrap();
        for (n, w) in &want {
            assert_eq!(got.get(n), Some(w), "{n} differs from the record");
        }
        assert_eq!(got.len(), want.len(), "fixture set differs from the record");
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// RESEARCH_NOTES F-104: a Google HDR+ JPEG (unknown protobuf fields that ExifTool decodes
/// differently on a second read in one process) is written and undone; the same-state re-read
/// that makes this possible does not hide a real collateral change.
#[test]
fn google_hdr_plus_jpeg_is_written_and_a_real_change_still_refused() {
    let pkg = require!();
    let lab = Lab::new("google", &pkg);
    let g = lab.photos[0].parent().unwrap().join("Google.jpg");
    std::fs::copy(timages().join("Google.jpg"), &g).unwrap();
    let before = blake(&g).unwrap();
    let (p, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&g], "g.json");
    let o = lab.cli_env(
        &["apply", p.to_str().unwrap(), "--tamper-at", "0:extra-tag"],
        true,
    );
    let r = Lab::json(&o);
    assert_eq!(state_of(&r, 0), "failed", "{r}");
    let why = r["files"][0]["reason"].as_str().unwrap();
    assert!(why.ends_with("unexpected changes: XMP-dc:Title"), "{why}");
    assert_eq!(blake(&g).as_deref(), Some(before.as_str()));
    let op = lab.apply_ok(&p);
    assert_ne!(blake(&g).as_deref(), Some(before.as_str()));
    lab.undo(&op);
    assert_eq!(blake(&g).as_deref(), Some(before.as_str()));
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SECURITY_MODEL §5: the pinned package matches the manifest made from the official download
/// (`research/exiftool-<version>.manifest.json`, every file), and a package whose key file was
/// changed refuses every write before ExifTool runs.
#[test]
fn exiftool_package_integrity() {
    let pkg = require!();
    let lab = Lab::new("integrity", &pkg);
    let lock: Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("research/exiftool.lock.json")).unwrap(),
    )
    .unwrap();
    let v = lock["version"].as_str().unwrap();
    let manifest = repo().join(format!("research/exiftool-{v}.manifest.json"));
    let m: Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    assert_eq!(m["version"], v);
    let run = |pkg: &Path, args: &[&str]| {
        let o = Command::new(env!("CARGO_BIN_EXE_mm-cli"))
            .arg("--data")
            .arg(&lab.data)
            .arg("--exiftool")
            .arg(pkg)
            .args(args)
            .output()
            .unwrap();
        (o.status.code(), Lab::json(&o))
    };
    let (code, r) = run(
        &pkg,
        &[
            "exiftool-check",
            "--full",
            "--manifest",
            manifest.to_str().unwrap(),
        ],
    );
    assert_eq!((code, &r["intact"]), (Some(0), &Value::Bool(true)), "{r}");

    // a small package with its manifest inside: writes go ahead only while the key files match
    let fake = lab.dir.join("fake-package");
    for rel in mm_core::integrity::KEY_FILES {
        let f = fake.join(rel);
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, rel.as_bytes()).unwrap();
    }
    let made = lab.dir.join("made.manifest");
    let (code, _) = run(
        &fake,
        &[
            "exiftool-manifest",
            fake.to_str().unwrap(),
            "--version",
            "test",
            "--out",
            made.to_str().unwrap(),
        ],
    );
    assert_eq!(code, Some(0));
    std::fs::copy(&made, fake.join(mm_core::integrity::MANIFEST_NAME)).unwrap();
    let missing_plan = lab.dir.join("no-such-plan.json");
    let (_, r) = run(&fake, &["apply", missing_plan.to_str().unwrap()]);
    assert!(
        !r["error"].as_str().unwrap().contains("not as shipped"),
        "intact package refused: {r}"
    );
    std::fs::write(fake.join("exiftool_files/perl532.dll"), b"patched").unwrap();
    let (code, r) = run(&fake, &["exiftool-check"]);
    assert_eq!(code, Some(3), "{r}");
    assert!(
        r["problem"]
            .as_str()
            .unwrap()
            .contains("changed: exiftool_files/perl532.dll"),
        "{r}"
    );
    for cmd in [
        &["apply", missing_plan.to_str().unwrap()][..],
        &["recover"][..],
    ] {
        let (code, r) = run(&fake, cmd);
        assert_eq!(code, Some(1), "{cmd:?}: {r}");
        assert!(
            r["error"].as_str().unwrap().contains("writing is disabled"),
            "{cmd:?}: {r}"
        );
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// Mark a lab file as a cloud placeholder that is not on this computer (FILE_ATTRIBUTE_OFFLINE,
/// which MoriMeta treats like RECALL_ON_DATA_ACCESS; no sync client is involved).
fn mark_offline(p: &Path, on: bool) {
    let ok = Command::new("attrib")
        .arg(if on { "+O" } else { "-O" })
        .arg(p)
        .status()
        .unwrap()
        .success();
    assert!(ok, "attrib failed for {}", p.display());
}

/// SAFETY_MODEL §8.3: a placeholder is never read — reading would make the sync client download
/// it. Inspector, selection aggregate, "Needs attention", planning and the pre-write check all
/// leave it alone (the debug log, which records every ExifTool command, never names it), and a
/// RAW whose sidecar is a placeholder is not read either.
#[test]
fn cloud_placeholders_are_never_read() {
    let pkg = require!();
    let lab = Lab::new("placeholder", &pkg);
    let ph = lab.photos[0].clone();
    let local = lab.photos[1].clone();
    // planned while downloaded; becomes a placeholder before Apply (the client freed space)
    let later = lab.photos[2].clone();
    let (early, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&later], "early.json");
    // from here on every ExifTool command is in the log
    assert!(lab.cli(&["debug-log", "on"]).status.success());
    let nef = lab.add_nef("DSC_0001.NEF");
    let xmp = nef.with_file_name("DSC_0001.xmp");
    std::fs::write(&xmp, b"<x:xmpmeta xmlns:x='adobe:ns:meta/'/>").unwrap();
    for f in [&ph, &later, &xmp] {
        mark_offline(f, true);
    }

    let i = Lab::json(&lab.cli(&["inspect", ph.to_str().unwrap()]));
    assert!(
        i["error"].as_str().unwrap().contains("NotDownloaded"),
        "{i}"
    );
    let i = Lab::json(&lab.cli(&["inspect", nef.to_str().unwrap()]));
    assert!(
        i["error"].as_str().unwrap().contains("NotDownloaded"),
        "sidecar: {i}"
    );

    let args = |cmd: &'static str| -> Vec<String> {
        [cmd.to_owned()]
            .into_iter()
            .chain(
                [&ph, &local, &nef]
                    .iter()
                    .map(|p| p.to_string_lossy().into_owned()),
            )
            .collect()
    };
    let run = |a: Vec<String>| {
        let r: Vec<&str> = a.iter().map(String::as_str).collect();
        Lab::json(&lab.cli(&r))
    };
    let agg = run(args("aggregate"));
    let creator = &agg.as_array().unwrap()[0];
    assert_eq!(
        (
            creator["files"].as_u64(),
            creator["not_downloaded"].as_u64()
        ),
        (Some(3), Some(2)),
        "{agg}"
    );
    let att = run(args("attention"));
    assert_eq!(att["cloud_placeholders"], serde_json::json!([0]), "{att}");

    let (_, pj) = lab.plan_on(
        &["plan-creator", "--set", "Morii"],
        &[&ph, &local, &nef],
        "p.json",
    );
    let st = |i: usize| pj["entries"][i]["status"].to_string();
    assert!(
        st(0).contains("cloud placeholder that is not downloaded"),
        "{pj}"
    );
    assert!(st(1).contains("ready"), "{pj}");
    assert!(st(2).contains("sidecar: cloud placeholder"), "{pj}");

    let r = Lab::json(&lab.cli(&["apply", early.to_str().unwrap()]));
    assert_eq!(state_of(&r, 0), "skipped", "{r}");
    assert!(
        r["files"][0]["reason"]
            .as_str()
            .unwrap()
            .contains("cloud placeholder"),
        "{r}"
    );
    assert_eq!(blake(&later).as_deref(), Some(lab.truth[&later].as_str()));

    // written while downloaded, then the client frees the space: undo planning and "now vs.
    // after" report it as not downloaded instead of hashing (reading) it
    let freed = lab.photos[3].clone();
    let (fp, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&freed], "freed.json");
    let written = lab.apply_ok(&fp);
    mark_offline(&freed, true);
    let u = lab.dir.join("undo-freed.json");
    let uo = lab.cli(&["plan-undo", &written, "--out", u.to_str().unwrap()]);
    assert!(
        String::from_utf8_lossy(&uo.stdout).contains("download it to undo"),
        "{}",
        String::from_utf8_lossy(&uo.stdout)
    );
    let now = Lab::json(&lab.cli(&["now", &written]));
    assert_eq!(now[0]["now"], "not_downloaded", "{now}");
    mark_offline(&freed, false);
    lab.undo(&written);

    let logs = lab.data.join("logs");
    let log: String = std::fs::read_dir(&logs)
        .unwrap()
        .map(|e| std::fs::read_to_string(e.unwrap().path()).unwrap_or_default())
        .collect();
    let name = |p: &Path| p.file_name().unwrap().to_string_lossy().into_owned();
    assert!(log.contains(&name(&local)), "the debug log records reads");
    for f in [&ph, &later, &xmp] {
        assert!(
            !log.contains(&name(f)),
            "{} was handed to ExifTool",
            name(f)
        );
    }
    for f in [&ph, &later, &xmp] {
        mark_offline(f, false);
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SECURITY_MODEL §4: every tag a Plan writes or deletes (all MVP writes, JPEG and NEF sidecar)
/// is writable in that group according to the pinned ExifTool's own tag database (`-listx`), so
/// an ExifTool upgrade that drops or moves one of them is caught here, not by a failed write.
#[test]
fn every_planned_tag_is_writable_in_the_pinned_exiftool() {
    let pkg = require!();
    let lab = Lab::new("listx", &pkg);
    let nef = lab.add_nef("DSC_0001.NEF");
    let mut files: Vec<&Path> = lab.photos.iter().map(PathBuf::as_path).collect();
    files.push(&nef);
    let mut planned: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for (i, cmd) in [
        &["plan-creator", "--set", "Morii"][..],
        &["plan-copyright", "--set", "(c) Morii"][..],
        &["plan-gps", "--set", "35.6586,139.7454,40"][..],
        &["plan-gps", "--remove"][..],
        &["plan-time", "--absolute", "2026:09:28 10:00:00"][..],
        &["plan-time", "--shift", "+01:00:00"][..],
        &["plan-creator", "--clear"][..],
        &["plan-copyright", "--clear"][..],
    ]
    .iter()
    .enumerate()
    {
        let (plan, _) = lab.plan_on(cmd, &files, &format!("w{i}.json"));
        let pj: Value = serde_json::from_slice(&std::fs::read(&plan).unwrap()).unwrap();
        for e in pj["entries"].as_array().unwrap() {
            for op in e["action"]["ops"].as_array().into_iter().flatten() {
                if let Some(t) = op["tag"].as_str() {
                    planned.insert(t.to_owned());
                }
            }
        }
    }
    assert!(planned.len() >= 20, "too few tags collected: {planned:?}");

    // `-listx` takes one group per run
    let mut xml = String::new();
    for group in ["-EXIF:All", "-XMP:All", "-IPTC:All", "-GPS:All"] {
        let o = Command::new(pkg.join("exiftool.exe"))
            .args(["-config", "", "-listx", group])
            .output()
            .unwrap();
        assert!(o.status.success(), "{group}");
        xml.push_str(&String::from_utf8_lossy(&o.stdout));
    }

    let attr = |line: &str, name: &str| -> Option<String> {
        let at = line.find(&format!(" {name}='"))? + name.len() + 3;
        Some(line[at..at + line[at..].find('\'')?].to_owned())
    };
    let mut writable: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut table_g1 = String::new();
    for line in xml.lines() {
        let line = line.trim();
        if line.starts_with("<table ") {
            table_g1 = attr(line, "g1").unwrap_or_default();
        } else if line.starts_with("<tag ") && attr(line, "writable").as_deref() == Some("true") {
            let g1 = attr(line, "g1").unwrap_or_else(|| table_g1.clone());
            writable.insert(format!("{g1}:{}", attr(line, "name").unwrap()));
        }
    }
    // the one whole-group operation engine::check_writable allows: deleting all GPS tags
    let groups: Vec<&String> = planned.iter().filter(|t| t.ends_with(":all")).collect();
    assert_eq!(groups, ["GPS:all"], "whole-group operations");
    let missing: Vec<&String> = planned
        .iter()
        .filter(|t| !t.ends_with(":all"))
        .filter(|t| {
            // a language alternative (`-x-default`) is written through its base tag
            let base = t.strip_suffix("-x-default").unwrap_or(t);
            !writable.contains(base)
        })
        .collect();
    assert!(
        missing.is_empty(),
        "not writable in ExifTool {}: {missing:?}",
        pkg.display()
    );
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// G-7 for the other file roles: after the Journal database is lost, Operations that created a
/// NEF sidecar, updated it, and a time Sequence over JPEG and NEF are rebuilt from their backup
/// folders with the same History; each can then be undone, newest first, back to no sidecar and
/// the original JPEGs.
#[test]
fn journal_rebuilt_for_sidecar_creation_update_and_sequence() {
    let pkg = require!();
    let lab = Lab::new("rebuild-roles", &pkg);
    let nef = lab.add_nef("DSC_0001.NEF");
    let xmp = nef.with_file_name("DSC_0001.xmp");
    let (p1, _) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&nef], "c1.json");
    let create = lab.apply_ok(&p1);
    let (p2, _) = lab.plan_on(&["plan-creator", "--set", "Someone"], &[&nef], "c2.json");
    let update = lab.apply_ok(&p2);
    let (p3, _) = lab.plan_on(
        &[
            "plan-time",
            "--sequence",
            "2026:09:28 10:00:00",
            "--step",
            "00:00:10",
            "--order",
            "name",
        ],
        &[&lab.photos[1], &lab.photos[2], &nef],
        "s.json",
    );
    let sequence = lab.apply_ok(&p3);
    let history = Lab::json(&lab.cli(&["history"]));
    let shown: Vec<Value> = [&create, &update, &sequence]
        .iter()
        .map(|op| lab.show(op))
        .collect();
    // older Operations report their files as changed later: the same before and after the rebuild
    let fsck = |op: &str| Lab::json(&lab.cli(&["fsck", op]));
    let checked: Vec<Value> = [&create, &update, &sequence]
        .iter()
        .map(|op| fsck(op))
        .collect();

    lab.lose_database();
    let r = lab.rebuild();
    let mut imported: Vec<String> = r["imported"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    imported.sort();
    let mut want = vec![create.clone(), update.clone(), sequence.clone()];
    want.sort();
    assert_eq!(imported, want, "{r}");
    assert_eq!(Lab::json(&lab.cli(&["history"])), history);
    for ((op, before), check) in [&create, &update, &sequence]
        .iter()
        .zip(&shown)
        .zip(&checked)
    {
        assert_eq!(&lab.show(op)["files"], &before["files"], "{op}");
        assert_eq!(&fsck(op), check, "{op}");
    }
    assert!(
        checked[2]["problems"].as_array().unwrap().is_empty(),
        "{}",
        checked[2]
    );
    lab.undo(&sequence);
    lab.undo(&update);
    assert_eq!(
        lab.xmp_tags(&xmp).get("XMP-dc:Creator"),
        Some(&serde_json::json!("Morii"))
    );
    lab.undo(&create);
    assert!(!xmp.exists(), "the created sidecar is undone");
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    lab.assert_all_original();
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL §8.10 through the product: files deeper than 300 characters in folders named in
/// Chinese and with an emoji, a JPEG and a NEF (new sidecar). "Needs attention" marks the long
/// paths; Creator and a time shift are written, checked and undone byte for byte, and a crash
/// right after a commit recovers there too (bak names and backups are longer still).
#[test]
fn long_unicode_paths_through_write_crash_and_undo() {
    let pkg = require!();
    let lab = Lab::new("longpath", &pkg);
    let mut dir = lab.photos[0].parent().unwrap().to_path_buf();
    for i in 0..3 {
        dir = dir.join(format!("长路径测试📷-{i}-{}", "照片".repeat(30)));
    }
    std::fs::create_dir_all(&dir).unwrap();
    let jpg = dir.join("风景 🌄 Canon.jpg");
    std::fs::copy(&lab.photos[2], &jpg).unwrap();
    let nef = dir.join("DSC_0001.NEF");
    std::fs::copy(timages().join("Nikon.nef"), &nef).unwrap();
    assert!(jpg.as_os_str().len() > 300, "{}", jpg.as_os_str().len());
    let before = blake(&jpg).unwrap();
    let xmp = nef.with_file_name("DSC_0001.xmp");

    let att = Lab::json(&lab.cli(&["attention", jpg.to_str().unwrap(), nef.to_str().unwrap()]));
    assert_eq!(att["long_paths"], serde_json::json!([0, 1]), "{att}");

    let (p1, pj) = lab.plan_on(
        &["plan-creator", "--set", "森 Morii"],
        &[&jpg, &nef],
        "c.json",
    );
    assert!(
        pj["entries"].to_string().matches("\"ready\"").count() == 2,
        "{pj}"
    );
    let creator = lab.apply_ok(&p1);
    assert!(lab.cli(&["fsck", &creator]).status.success());
    assert!(xmp.exists());

    // a crash right after the commit of the first file, then recovery and resume
    let (p2, _) = lab.plan_on(
        &["plan-time", "--shift", "+01:00:00"],
        &[&jpg, &nef],
        "t.json",
    );
    let o = lab.cli_env(
        &[
            "apply",
            p2.to_str().unwrap(),
            "--crash-at",
            "0:8",
            "--workers",
            "1",
        ],
        true,
    );
    assert_eq!(
        o.status.code(),
        Some(77),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let shift = lab.last_op();
    assert!(lab.cli(&["recover"]).status.success());
    let res = lab.cli(&["resume", &shift]);
    assert!(
        res.status.code().is_some_and(|c| c == 0 || c == 3),
        "{}",
        String::from_utf8_lossy(&res.stdout)
    );
    assert!(lab.cli(&["fsck", &shift]).status.success());
    let names: Vec<String> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert!(
        names
            .iter()
            .all(|n| !n.contains(".mmtmp-") && !n.contains(".mmbak-")),
        "{names:?}"
    );

    lab.undo(&shift);
    lab.undo(&creator);
    assert_eq!(blake(&jpg).as_deref(), Some(before.as_str()));
    assert!(!xmp.exists());
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// SAFETY_MODEL A-1 / §8.4: a USB drive Windows reports as a fixed disk may be exFAT, which keeps
/// no journal and is not verified for writing in place. JPEG and NEF (new sidecar) there are
/// Blocked with the file system named, and "Needs attention" lists them. Needs an exFAT test
/// volume (`MM_E2E_EXFAT_VOLUME`; CI mounts one).
#[test]
fn files_on_an_exfat_drive_are_not_written() {
    let pkg = require!();
    let Some(vol) = std::env::var_os("MM_E2E_EXFAT_VOLUME").map(PathBuf::from) else {
        eprintln!("SKIP: MM_E2E_EXFAT_VOLUME not set (needs an exFAT test volume)");
        return;
    };
    let lab = Lab::new("exfat", &pkg);
    let dir = vol.join(format!("mm-e2e-exfat-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let jpg = dir.join("Canon.jpg");
    std::fs::copy(&lab.photos[2], &jpg).unwrap();
    let nef = dir.join("DSC_0001.NEF");
    std::fs::copy(timages().join("Nikon.nef"), &nef).unwrap();
    let before = blake(&jpg);

    let (_, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&jpg, &nef], "x.json");
    for e in pj["entries"].as_array().unwrap() {
        assert_eq!(e["status"]["status"], "blocked", "{pj}");
        assert!(e.to_string().contains("on a drive formatted exFAT"), "{pj}");
    }
    let att = Lab::json(&lab.cli(&["attention", jpg.to_str().unwrap(), nef.to_str().unwrap()]));
    assert_eq!(att["other_file_system"], serde_json::json!([0, 1]), "{att}");
    assert_eq!(blake(&jpg), before);
    assert!(!nef.with_extension("xmp").exists());

    // RESEARCH_NOTES F-105 on a drive without 8.3 names (exFAT keeps none): an ExifTool package
    // in a folder outside the code page is refused with the reason, not a Perl error
    let far = dir
        .join("\u{AE40}\u{BAA8}\u{B9AC} \u{1F4F7}")
        .join("exiftool");
    copy_tree(&pkg, &far);
    let o = Command::new(env!("CARGO_BIN_EXE_mm-cli"))
        .arg("--data")
        .arg(&lab.data)
        .arg("--exiftool")
        .arg(&far)
        .args(["plan-creator", "--set", "Morii", "--out"])
        .arg(lab.dir.join("far.json"))
        .arg(&lab.photos[1])
        .output()
        .unwrap();
    let said = String::from_utf8_lossy(&o.stdout);
    assert!(
        o.status.success() || said.contains("keeps no short names"),
        "{said}"
    );
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §9: in Preview the user leaves out one change of one file and a whole edit.
/// With a three-rule Preset, GPS.jpg keeps its position (only its GPS removal left out) and no
/// file gets a copyright (the whole edit left out). What is written, verified and counted is the
/// rest; taking an edit in again restores the previewed changes; undo is byte for byte.
#[test]
fn a_change_or_a_whole_edit_is_left_out_in_preview() {
    let pkg = require!();
    let lab = Lab::new("exclude-field", &pkg);
    let preset = lab.dir.join("preset.json");
    std::fs::write(
        &preset,
        r#"{"schema_version": 1, "name": "Studio", "rules": [
            {"name": "copyright where none",
             "when": [{"if": "empty", "field": "copyright"}],
             "then": [{"do": "set_copyright", "value": "© {creator|Studio} {year|2026}"}]},
            {"name": "no position in JPEGs",
             "when": [{"if": "not_empty", "field": "gps"}, {"if": "extension", "any": ["jpg"]}],
             "then": [{"do": "remove_gps"}]},
            {"name": "clock was an hour behind",
             "when": [{"if": "not_empty", "field": "capture_time"}],
             "then": [{"do": "shift_time", "by": "+01:00:00"}]}
        ]}"#,
    )
    .unwrap();
    let files: Vec<&Path> = lab.photos.iter().map(PathBuf::as_path).collect();
    let (p1, pj1) = lab.plan_on(
        &["plan-preset", "--preset", preset.to_str().unwrap()],
        &files,
        "p1.json",
    );
    let gps = lab
        .photos
        .iter()
        .position(|p| p.ends_with("GPS.jpg"))
        .unwrap();
    let fields = |pj: &Value, i: usize, key: &str| -> Vec<String> {
        pj["entries"][i][key]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| c["field"].as_str().unwrap().to_owned())
            .collect()
    };
    assert_eq!(
        fields(&pj1, gps, "changes"),
        ["gps", "capture_time"],
        "{pj1}"
    );
    let with_copyright = (0..lab.photos.len())
        .filter(|&i| fields(&pj1, i, "changes").contains(&"copyright".to_string()))
        .count();
    assert!(with_copyright >= 2, "{pj1}");
    let seq = pj1["entries"][gps]["seq"].as_u64().unwrap().to_string();

    let exclude = |from: &Path, to: &str, extra: &[&str]| -> (PathBuf, Value) {
        let out = lab.dir.join(to);
        let mut args = vec![
            "plan-exclude",
            from.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ];
        args.extend(extra);
        let o = lab.cli(&args);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        (out, Lab::json(&o))
    };
    let (p2, _) = exclude(&p1, "p2.json", &["--field", "gps", "--seq", &seq]);
    let (p3, pj3) = exclude(&p2, "p3.json", &["--field", "copyright"]);
    assert_eq!(fields(&pj3, gps, "changes"), ["capture_time"], "{pj3}");
    assert_eq!(fields(&pj3, gps, "excluded_changes"), ["gps"], "{pj3}");
    for i in 0..lab.photos.len() {
        assert!(
            !fields(&pj3, i, "changes").contains(&"copyright".to_string()),
            "{pj3}"
        );
    }
    let changes = |pj: &Value| pj["summary"]["changes"].as_u64().unwrap();
    assert_eq!(
        changes(&pj3),
        changes(&pj1) - 1 - with_copyright as u64,
        "{pj3}"
    );
    assert!(
        !pj3["required_acks"].to_string().contains("remove:gps"),
        "{pj3}"
    );
    // taking the whole edit in again gives back what the Preview first showed
    let (_, pj4) = exclude(&p3, "p4.json", &["--field", "copyright", "--include"]);
    for i in 0..lab.photos.len() {
        if i != gps {
            assert_eq!(
                pj4["entries"][i]["changes"], pj1["entries"][i]["changes"],
                "{i}"
            );
        }
    }

    let copyrights = lab.copyrights();
    // GPS.jpg is read-only at Apply: skipped; planned again once cleared, it still has only
    // what the user kept (§12 "the failed files' original changes")
    let set_ro = |on: bool| {
        let mut perm = std::fs::metadata(&lab.photos[gps]).unwrap().permissions();
        perm.set_readonly(on);
        std::fs::set_permissions(&lab.photos[gps], perm).unwrap();
    };
    set_ro(true);
    let rep = Lab::json(&lab.cli(&["apply", p3.to_str().unwrap()]));
    set_ro(false);
    let op = rep["op_id"].as_str().unwrap().to_owned();
    let skipped: Vec<&Value> = rep["files"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["state"] == "skipped")
        .collect();
    assert_eq!(skipped.len(), 1, "{rep}");
    assert!(
        skipped[0]["path"].as_str().unwrap().ends_with("GPS.jpg"),
        "{rep}"
    );
    let again = lab.dir.join("again.json");
    let o = lab.cli(&["plan-again", &op, "--out", again.to_str().unwrap()]);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pa = Lab::json(&o);
    assert_eq!(fields(&pa, 0, "changes"), ["capture_time"], "{pa}");
    assert_eq!(fields(&pa, 0, "excluded_changes"), ["gps"], "{pa}");
    let op2 = lab.apply_ok(&again);
    assert!(lab.cli(&["fsck", &op2]).status.success());
    let insp = Lab::json(&lab.cli(&["inspect", lab.photos[gps].to_str().unwrap()]));
    let value = |f: &str| {
        insp["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["field"] == f)
            .unwrap()["value"]
            .clone()
    };
    assert_eq!(value("gps"), "54.9896667, -1.9141667", "{insp}");
    assert_eq!(value("capture_time"), "2002:07:13 16:58:28", "{insp}");
    assert_eq!(lab.copyrights(), copyrights);
    lab.undo(&op2);
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// ARCHITECTURE §6.1 / SCREEN_SPEC Library: the rows of the table come from the core in
/// streamed batches: each file's fields as the Inspector shows them, camera and lens, where an
/// edit goes (in the file, the existing or a new sidecar, read-only format); a cloud placeholder
/// is listed without being read; a file that cannot be read gets its reason.
#[test]
fn library_rows_are_read_in_streamed_batches() {
    let pkg = require!();
    let lab = Lab::new("rows", &pkg);
    let gps = lab
        .photos
        .iter()
        .find(|p| p.ends_with("GPS.jpg"))
        .unwrap()
        .clone();
    let bare = lab.add_nef("DSC_0001.NEF");
    let paired = lab.add_nef("DSC_0002.NEF");
    let (pp, _) = lab.plan_on(&["plan-creator", "--set", "Mori"], &[&paired], "c.json");
    lab.apply_ok(&pp);
    let png = lab.dir.join("photos").join("PNG.png");
    std::fs::copy(timages().join("PNG.png"), &png).unwrap();
    let cloud = lab.dir.join("photos").join("cloud.jpg");
    std::fs::copy(&lab.photos[1], &cloud).unwrap();
    let broken = lab.dir.join("photos").join("broken.jpg");
    std::fs::write(&broken, b"not a JPEG").unwrap();
    mark_offline(&cloud, true);
    let files = [&gps, &bare, &paired, &png, &cloud, &broken];
    let mut args = vec!["rows".to_string()];
    args.extend(files.iter().map(|p| p.to_string_lossy().into_owned()));
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    let out = Lab::json(&lab.cli(&a));
    let rows = out["rows"].as_array().unwrap();
    assert_eq!(rows.len(), files.len(), "{out}");
    let field = |r: &Value, f: &str| {
        r["fields"]
            .as_array()
            .unwrap()
            .iter()
            .find(|x| x["field"] == f)
            .map(|x| x["value"].clone())
            .unwrap_or(Value::Null)
    };
    let writes: Vec<&str> = rows
        .iter()
        .map(|r| r["writes_to"].as_str().unwrap())
        .collect();
    assert_eq!(
        writes,
        [
            "in_file",
            "new_sidecar",
            "sidecar",
            "read_only",
            "in_file",
            "in_file"
        ],
        "{out}"
    );
    assert_eq!(field(&rows[0], "gps"), "54.9896667, -1.9141667", "{out}");
    assert!(rows[0]["make"].is_string(), "{out}");
    assert!(
        rows[1]["model"].as_str().unwrap().contains("NIKON"),
        "{out}"
    );
    assert_eq!(field(&rows[2], "creator"), "Mori", "{out}");
    assert!(rows[3]["error"].is_null() && rows[3]["fields"].as_array().unwrap().len() == 4);
    assert_eq!(rows[4]["not_downloaded"], true, "{out}");
    assert!(rows[4]["fields"].as_array().unwrap().is_empty());
    assert!(crate_is_offline(&cloud), "still not downloaded");
    assert!(
        rows[5]["error"]
            .as_str()
            .unwrap()
            .contains("content is TXT"),
        "{out}"
    );
    mark_offline(&cloud, false);
    // the same file is not planned: its write would only fail at verification
    let (_, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &[&broken], "b.json");
    assert_eq!(pj["entries"][0]["status"]["status"], "blocked", "{pj}");
    assert!(pj.to_string().contains("content is TXT"), "{pj}");

    // more than one batch: rows arrive in order, each once
    let many: Vec<String> = std::iter::repeat_n(gps.to_string_lossy().into_owned(), 401).collect();
    let mut args = vec!["rows"];
    args.extend(many.iter().map(String::as_str));
    let out = Lab::json(&lab.cli(&args));
    assert_eq!(out["batches"], 2, "{}", out["batches"]);
    let idx: Vec<u64> = out["rows"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["index"].as_u64().unwrap())
        .collect();
    assert_eq!(idx, (0..401).collect::<Vec<u64>>());
    let _ = std::fs::remove_dir_all(&lab.dir);
}

fn crate_is_offline(p: &Path) -> bool {
    use std::os::windows::fs::MetadataExt;
    std::fs::symlink_metadata(p).unwrap().file_attributes() & 0x1000 != 0
}

/// SAFETY_MODEL §8.5 / A-1: photos on an SMB share are written through the whole product chain
/// (the transaction itself was verified on SMB in S2): planned with the network note, written in
/// place and through a new NEF sidecar, crashed after a commit, recovered and resumed, and undone
/// byte for byte. Needs a writable UNC folder (`MM_E2E_SMB_DIR`; CI shares a folder over
/// `\\localhost`).
#[test]
fn photos_on_a_network_share_through_write_crash_and_undo() {
    let pkg = require!();
    let Some(share) = std::env::var_os("MM_E2E_SMB_DIR").map(PathBuf::from) else {
        eprintln!("SKIP: MM_E2E_SMB_DIR not set (needs a writable folder on an SMB share)");
        return;
    };
    let base = tmp().join(format!("mm-e2e-smb-{}", std::process::id()));
    let on_share = share.join(format!("mm-e2e-smb-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let _ = std::fs::remove_dir_all(&on_share);
    let lab = Lab::at(&pkg, base.clone(), on_share.clone(), base.join("data"), 1);
    let nef = on_share.join("DSC_0001.NEF");
    std::fs::copy(timages().join("Nikon.nef"), &nef).unwrap();
    let mut files: Vec<&Path> = lab.photos.iter().map(PathBuf::as_path).collect();
    files.push(&nef);

    let (p1, pj) = lab.plan_on(&["plan-creator", "--set", "Morii"], &files, "c.json");
    let ready: Vec<&Value> = pj["entries"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["status"]["status"] == "ready")
        .collect();
    assert!(ready.len() >= 6, "{pj}");
    assert!(
        ready
            .iter()
            .all(|e| e.to_string().contains("on a network drive")),
        "{pj}"
    );
    let creator = lab.apply_ok(&p1);
    assert!(lab.cli(&["fsck", &creator]).status.success());
    assert!(nef.with_extension("xmp").exists());

    let (p2, _) = lab.plan_on(&["plan-time", "--shift", "+01:00:00"], &files, "t.json");
    let o = lab.cli_env(
        &[
            "apply",
            p2.to_str().unwrap(),
            "--crash-at",
            "1:8",
            "--workers",
            "2",
        ],
        true,
    );
    assert_eq!(
        o.status.code(),
        Some(77),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let shift = lab.last_op();
    assert!(lab.cli(&["recover"]).status.success());
    let res = lab.cli(&["resume", &shift]);
    assert!(
        res.status.code().is_some_and(|c| c == 0 || c == 3),
        "{}",
        String::from_utf8_lossy(&res.stdout)
    );
    assert!(lab.cli(&["fsck", &shift]).status.success());
    let left: Vec<String> = std::fs::read_dir(&on_share)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.contains(".mmtmp-") || n.contains(".mmbak-"))
        .collect();
    assert!(left.is_empty(), "{left:?}");

    lab.undo(&shift);
    lab.undo(&creator);
    lab.assert_all_original();
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    assert!(!nef.with_extension("xmp").exists());
    let _ = std::fs::remove_dir_all(&on_share);
    let _ = std::fs::remove_dir_all(&base);
}

/// SECURITY_MODEL T-15 / T-17: one file whose metadata output is abnormally large (a huge XMP
/// packet) failed the whole read of its chunk, so the Plan (and the Library scan) of every file
/// selected with it failed. Now that file alone is reported as not read and the others are
/// planned, listed and written. The output limit (256 MB) is lowered for the test.
#[test]
fn one_file_with_abnormally_large_metadata_does_not_stop_the_others() {
    let pkg = require!();
    let lab = Lab::new("toolarge", &pkg);
    let big = lab.dir.join("photos").join("big.jpg");
    std::fs::copy(&lab.photos[2], &big).unwrap();
    let text = lab.dir.join("desc.txt");
    std::fs::write(&text, "A".repeat(3_000_000)).unwrap();
    // the pinned ExifTool itself, on this lab copy only
    let made = Command::new(pkg.join("exiftool.exe"))
        .args(["-config", "", "-overwrite_original"])
        .arg(format!("-XMP-dc:Description<={}", text.display()))
        .arg(&big)
        .output()
        .unwrap();
    assert!(made.status.success(), "{made:?}");
    let before = blake(&big);
    let run = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_mm-cli"))
            .env("MM_FAULT_INJECTION", "1")
            .env("MM_MAX_OUTPUT", "1000000")
            .arg("--data")
            .arg(&lab.data)
            .arg("--exiftool")
            .arg(&lab.pkg)
            .args(args)
            .output()
            .unwrap()
    };
    let mut files: Vec<String> = vec![big.to_string_lossy().into_owned()];
    files.extend(lab.photos.iter().map(|p| p.to_string_lossy().into_owned()));
    let plan = lab.dir.join("p.json");
    let mut args = vec![
        "plan-creator",
        "--set",
        "Morii",
        "--out",
        plan.to_str().unwrap(),
    ];
    args.extend(files.iter().map(String::as_str));
    let o = run(&args);
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
    let pj = Lab::json(&o);
    let entries = pj["entries"].as_array().unwrap();
    assert_eq!(entries[0]["status"]["status"], "blocked", "{pj}");
    assert!(entries[0].to_string().contains("abnormally large"), "{pj}");
    let ready = entries
        .iter()
        .filter(|e| e["status"]["status"] == "ready")
        .count();
    assert!(ready >= 6, "{pj}");

    let mut args = vec!["rows"];
    args.extend(files.iter().map(String::as_str));
    let rows = Lab::json(&run(&args));
    let rows = rows["rows"].as_array().unwrap();
    assert!(
        rows[0]["error"]
            .as_str()
            .unwrap()
            .contains("abnormally large")
    );
    assert!(rows[1..].iter().all(|r| r["error"].is_null()), "{rows:?}");

    let a = run(&["apply", plan.to_str().unwrap()]);
    assert!(a.status.success(), "{}", String::from_utf8_lossy(&a.stdout));
    assert_eq!(blake(&big), before);
    let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// INTERACTION_SPEC §15 / SAFETY_MODEL §10: an Operation was interrupted, and at the next start
/// its backup location (an external drive) is not connected. Recovery used to fail as a whole
/// (so the app could not start); now that Operation waits, writes stay refused, and once the drive
/// is back it is recovered, resumed and undone as usual.
#[test]
fn recovery_waits_for_a_backup_location_that_is_not_connected() {
    let pkg = require!();
    let lab = Lab::new("unplugged", &pkg);
    let ext = lab.dir.join("external drive");
    assert!(
        lab.cli(&["setting", "backup.root", ext.to_str().unwrap()])
            .status
            .success()
    );
    let plan = lab.plan("Morii", "p.json");
    let o = lab.cli_env(
        &[
            "apply",
            plan.to_str().unwrap(),
            "--crash-at",
            "1:8",
            "--workers",
            "1",
        ],
        true,
    );
    assert_eq!(
        o.status.code(),
        Some(77),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let op = lab.last_op();
    let away = lab.dir.join("drive not connected");
    std::fs::rename(&ext, &away).unwrap();

    let r = lab.cli(&["recover"]);
    assert!(r.status.success(), "{}", String::from_utf8_lossy(&r.stdout));
    assert_eq!(Lab::json(&r), serde_json::json!([]));
    let again = lab.plan("Mori", "q.json");
    let refused = lab.cli(&["apply", again.to_str().unwrap()]);
    assert!(
        !refused.status.success(),
        "a write while recovery is pending"
    );

    // the drive is back
    let _ = std::fs::remove_dir_all(&ext); // what a later start may have created in its place
    std::fs::rename(&away, &ext).unwrap();
    let r = Lab::json(&lab.cli(&["recover"]));
    assert_eq!(r[0]["op_id"], op.as_str(), "{r}");
    let res = lab.cli(&["resume", &op]);
    assert!(
        res.status.code().is_some_and(|c| c == 0 || c == 3),
        "{}",
        String::from_utf8_lossy(&res.stdout)
    );
    assert!(lab.cli(&["fsck", &op]).status.success());
    lab.undo(&op);
    lab.assert_all_original();

    // a finished Operation while the drive is away: undo and restore-to say so instead of
    // reporting every backup as missing or damaged
    let done = lab.apply_ok(&lab.plan("Mori", "r.json"));
    std::fs::rename(&ext, &away).unwrap();
    let h = Lab::json(&lab.cli(&["history"]));
    let row = h
        .as_array()
        .unwrap()
        .iter()
        .find(|o| o["id"] == done.as_str())
        .unwrap()
        .clone();
    assert_eq!(
        (row["backups_unavailable"].clone(), row["undoable"].clone()),
        (serde_json::json!(true), serde_json::json!(false)),
        "{row}"
    );
    for args in [
        vec!["plan-undo", done.as_str(), "--out", "u.json"],
        vec!["restore-to", done.as_str(), "--dir", "restored"],
    ] {
        let mut args: Vec<String> = args.into_iter().map(str::to_owned).collect();
        args[3] = lab.dir.join(&args[3]).to_string_lossy().into_owned();
        let a: Vec<&str> = args.iter().map(String::as_str).collect();
        let o = lab.cli(&a);
        let said = String::from_utf8_lossy(&o.stdout).into_owned();
        assert!(
            !o.status.success() && said.contains("connect the drive"),
            "{said}"
        );
    }
    std::fs::rename(&away, &ext).unwrap();
    lab.undo(&done);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// A folder that does not let MoriMeta create files (its permissions, or Windows Controlled
/// folder access, which guards Pictures): every write there fails before anything is changed,
/// with that reason rather than a "verification" failure; nothing is left behind, and once the
/// folder allows it a retry writes the files.
#[test]
fn a_folder_that_refuses_new_files_fails_with_the_reason() {
    let pkg = require!();
    let lab = Lab::new("no-create", &pkg);
    let folder = lab.photos[0].parent().unwrap().to_path_buf();
    let user = std::env::var("USERNAME").unwrap();
    let icacls = |args: &[&str]| {
        let o = Command::new("icacls")
            .arg(&folder)
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "{o:?}");
    };
    icacls(&["/deny", &format!("{user}:(AD,WD)")]);
    // two files: fewer than the circuit breaker needs to pause
    let (plan, _) = lab.plan_on(
        &["plan-creator", "--set", "Morii"],
        &[&lab.photos[1], &lab.photos[2]],
        "p.json",
    );
    let a = lab.cli(&["apply", plan.to_str().unwrap()]);
    let rep = Lab::json(&a);
    icacls(&["/remove:d", &user]);
    let files = rep["files"].as_array().unwrap();
    assert_eq!(files.len(), 2, "{rep}");
    for f in files {
        assert_eq!(f["state"], "failed", "{rep}");
        assert!(
            f["reason"]
                .as_str()
                .unwrap()
                .contains("cannot create a file in this folder"),
            "{rep}"
        );
    }
    lab.assert_all_original();
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    let op = rep["op_id"].as_str().unwrap().to_owned();
    let (retry, _) = {
        let out = lab.dir.join("retry.json");
        let o = lab.cli(&["plan-retry", &op, "--out", out.to_str().unwrap()]);
        assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stdout));
        (out, ())
    };
    let done = lab.apply_ok(&retry);
    lab.undo(&done);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// A file whose metadata already makes ExifTool warn when read (GE.jpg: a suspicious maker-note
/// offset) is refused by verification (V1) when written. The Preview says so beforehand, as a
/// warning; the write is still refused and the file stays unchanged (whether such warnings may be
/// accepted is an open decision, research/PROGRESS.md).
#[test]
fn a_file_that_warns_when_read_is_flagged_in_the_preview() {
    let pkg = require!();
    let lab = Lab::new("read-warning", &pkg);
    let ge = lab.dir.join("photos").join("GE.jpg");
    std::fs::copy(timages().join("GE.jpg"), &ge).unwrap();
    let before = blake(&ge);
    let (p, pj) = lab.plan_on(
        &["plan-creator", "--set", "Morii"],
        &[&ge, &lab.photos[1]],
        "p.json",
    );
    let notes = pj["entries"][0]["notes"].to_string();
    assert!(notes.contains("ExifTool reports a problem"), "{pj}");
    assert!(
        !pj["entries"][1]["notes"]
            .to_string()
            .contains("ExifTool reports")
    );
    assert_eq!(pj["summary"]["warnings"], 1, "{pj}");
    let a = Lab::json(&lab.cli(&["apply", p.to_str().unwrap()]));
    assert_eq!(a["files"][0]["state"], "failed", "{a}");
    assert!(
        a["files"][0]["reason"].as_str().unwrap().contains("V1"),
        "{a}"
    );
    assert_eq!(blake(&ge), before);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// A JPEG with an XMP file of the same name next to it (some programs keep a JPEG's metadata in
/// such a sidecar): MoriMeta writes into the JPEG and leaves the sidecar alone, and the Preview
/// says that programs reading the sidecar may still show its values. With a RAW of that name the
/// XMP belongs to the RAW (SAFETY_MODEL §3.1) and the JPEG gets no such note.
#[test]
fn a_jpeg_with_its_own_xmp_sidecar_is_noted() {
    let pkg = require!();
    let lab = Lab::new("jpeg-xmp", &pkg);
    let dir = lab.photos[0].parent().unwrap().to_path_buf();
    let alone = dir.join("IMG_0001.JPG");
    std::fs::copy(&lab.photos[1], &alone).unwrap();
    let sidecar = dir.join("IMG_0001.xmp");
    std::fs::copy(timages().join("XMP.xmp"), &sidecar).unwrap();
    let paired = dir.join("DSC_0002.JPG");
    std::fs::copy(&lab.photos[2], &paired).unwrap();
    lab.add_nef("DSC_0002.NEF");
    std::fs::copy(timages().join("XMP.xmp"), dir.join("DSC_0002.xmp")).unwrap();
    let side_before = blake(&sidecar);

    let (p, pj) = lab.plan_on(
        &["plan-creator", "--set", "Morii"],
        &[&alone, &paired],
        "p.json",
    );
    assert!(
        pj["entries"][0]["notes"]
            .to_string()
            .contains("XMP sidecar of the same name"),
        "{pj}"
    );
    assert!(
        !pj["entries"][1]["notes"]
            .to_string()
            .contains("XMP sidecar of the same name"),
        "{pj}"
    );
    let op = lab.apply_ok(&p);
    assert_eq!(blake(&sidecar), side_before, "the sidecar is left alone");
    lab.undo(&op);
    let _ = std::fs::remove_dir_all(&lab.dir);
}

fn copy_tree(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for e in std::fs::read_dir(from).unwrap() {
        let e = e.unwrap();
        if e.file_type().unwrap().is_dir() {
            copy_tree(&e.path(), &to.join(e.file_name()));
        } else {
            std::fs::copy(e.path(), to.join(e.file_name())).unwrap();
        }
    }
}

/// RESEARCH_NOTES F-105: the ExifTool launcher and Perl find their own files through the ANSI
/// API, so a package in a folder whose name has characters outside the system code page (a
/// per-user install under an account named in Korean, Polish, … on another code page) did not
/// start in either mode. The engine now starts it through the folder's 8.3 short name; a write
/// and its undo go through in both modes. Where the drive keeps no short names, the start is
/// refused with the reason.
#[test]
fn exiftool_package_in_a_folder_outside_the_code_page() {
    let pkg = require!();
    let lab = Lab::new("pkgpath", &pkg);
    let far = lab
        .dir
        .join("\u{AE40}\u{BAA8}\u{B9AC} \u{1F4F7} \u{141}ukasz")
        .join("exiftool");
    copy_tree(&pkg, &far);
    let photo = &lab.photos[1];
    // and, since ExifTool runs in its own working folder, a package given by a relative path
    for (mode, relative) in [("launcher", false), ("perl", false), ("perl", true)] {
        let pkg_arg = if relative {
            far.strip_prefix(&lab.dir).unwrap().to_path_buf()
        } else {
            far.clone()
        };
        let mode_case = format!("{mode}-{relative}");
        let run = |args: &[&str]| {
            Command::new(env!("CARGO_BIN_EXE_mm-cli"))
                .current_dir(&lab.dir)
                .arg("--data")
                .arg(&lab.data)
                .arg("--exiftool")
                .arg(&pkg_arg)
                .args(["--engine", mode])
                .args(args)
                .output()
                .unwrap()
        };
        let plan = lab.dir.join(format!("{mode_case}.json"));
        let o = run(&[
            "plan-creator",
            "--set",
            "Morii",
            "--out",
            plan.to_str().unwrap(),
            photo.to_str().unwrap(),
        ]);
        let said = String::from_utf8_lossy(&o.stdout).into_owned();
        if !o.status.success() && said.contains("keeps no short names") {
            assert!(std::env::var_os("CI").is_some(), "{said}");
            eprintln!("SKIP: this drive keeps no 8.3 names: {said}");
            return;
        }
        assert!(o.status.success(), "{mode_case}: {said}");
        let a = run(&["apply", plan.to_str().unwrap()]);
        assert!(
            a.status.success(),
            "{mode_case}: {}",
            String::from_utf8_lossy(&a.stdout)
        );
        assert_ne!(blake(photo).as_deref(), Some(lab.truth[photo].as_str()));
        let op = Lab::json(&a)["op_id"].as_str().unwrap().to_owned();
        let u = lab.dir.join(format!("{mode_case}-undo.json"));
        assert!(
            run(&["plan-undo", &op, "--out", u.to_str().unwrap()])
                .status
                .success()
        );
        assert!(run(&["apply", u.to_str().unwrap()]).status.success());
        assert_eq!(
            blake(photo).as_deref(),
            Some(lab.truth[photo].as_str()),
            "{mode_case}"
        );
    }
    let _ = std::fs::remove_dir_all(&lab.dir);
}

/// A Windows account named in Chinese puts MoriMeta's data (Journal, backups, logs, lock) under a
/// Chinese path, and ExifTool reads every backup as its write source (§4.1 step 4). The whole
/// chain with the data folder and a second backup location under Chinese / emoji names: write
/// JPEG + new NEF sidecar, crash and resume, lose and rebuild the Journal, export the log, restore
/// to a folder, undo byte for byte.
#[test]
fn unicode_data_folder_and_backup_location() {
    let pkg = require!();
    let dir = tmp().join(format!("mm-e2e-unidata-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let data = dir.join("用户 森 Morii 🙂").join("AppData 数据");
    let lab = Lab::at(&pkg, dir.clone(), dir.join("photos"), data.clone(), 1);
    let nef = lab.add_nef("DSC_0001.NEF");
    let xmp = nef.with_file_name("DSC_0001.xmp");
    let mut files: Vec<&Path> = lab.photos.iter().map(PathBuf::as_path).collect();
    files.push(&nef);

    let (p1, _) = lab.plan_on(&["plan-creator", "--set", "森 Morii"], &files, "c.json");
    let creator = lab.apply_ok(&p1);
    assert!(
        data.join("backups")
            .join(&creator)
            .join("plan.json")
            .exists()
    );
    assert!(xmp.exists());

    // a second backup location, also in Chinese with an emoji; a crash after a commit
    let second = dir.join("备份 📦 位置");
    assert!(
        lab.cli(&["setting", "backup.root", second.to_str().unwrap()])
            .status
            .success()
    );
    let (p2, _) = lab.plan_on(&["plan-time", "--shift", "+01:00:00"], &files, "t.json");
    let o = lab.cli_env(
        &[
            "apply",
            p2.to_str().unwrap(),
            "--crash-at",
            "1:8",
            "--workers",
            "1",
        ],
        true,
    );
    assert_eq!(
        o.status.code(),
        Some(77),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    let shift = lab.last_op();
    assert!(second.join(&shift).join("plan.json").exists());
    assert!(lab.cli(&["recover"]).status.success());
    let res = lab.cli(&["resume", &shift]);
    assert!(
        res.status.code().is_some_and(|c| c == 0 || c == 3),
        "{}",
        String::from_utf8_lossy(&res.stdout)
    );
    // (the first one's files were changed later by the second, which fsck reports for it)
    assert!(lab.cli(&["fsck", &shift]).status.success());

    // the Journal is lost, with the backup location setting in it: both Operations come back
    // from their Chinese-named folders (the second from the list of locations used)
    lab.lose_database();
    let r = lab.rebuild();
    assert_eq!(r["imported"].as_array().unwrap().len(), 2, "{r}");
    assert!(lab.cli(&["recover"]).status.success());
    // that list lost as well, and the backup folder moved elsewhere since: the user names the
    // folder where it is now, and the undo below restores from there
    lab.lose_database();
    std::fs::remove_file(data.join("backup-locations.txt")).unwrap();
    assert_eq!(lab.rebuild()["imported"].as_array().unwrap().len(), 1);
    let moved = dir.join("移动后 🚚");
    std::fs::rename(&second, &moved).unwrap();
    let o = lab.cli(&["rebuild-journal", "--from", moved.to_str().unwrap()]);
    assert_eq!(
        Lab::json(&o)["imported"],
        serde_json::json!([shift]),
        "{}",
        String::from_utf8_lossy(&o.stdout)
    );
    assert!(lab.cli(&["recover"]).status.success());

    let log = dir.join("日志 导出.json");
    assert!(
        lab.cli(&["export-log", &shift, "--out", log.to_str().unwrap()])
            .status
            .success()
    );
    assert!(log.exists());
    let restored = dir.join("恢复 🗂");
    std::fs::create_dir_all(&restored).unwrap();
    let rows = Lab::json(&lab.cli(&["restore-to", &creator, "--dir", restored.to_str().unwrap()]));
    // every file written in place comes back as its original under a Chinese folder name
    let mut restored_n = 0;
    for row in rows.as_array().unwrap() {
        let Some(to) = row["to"].as_str() else {
            continue; // the new sidecar had nothing before it
        };
        let orig = lab
            .photos
            .iter()
            .find(|p| p.file_name() == Path::new(to).file_name())
            .unwrap();
        assert_eq!(
            blake(Path::new(to)).as_deref(),
            Some(lab.truth[orig].as_str()),
            "{row}"
        );
        restored_n += 1;
    }
    assert!(restored_n >= 6, "{rows}");

    lab.undo(&shift);
    lab.undo(&creator);
    for p in &lab.photos {
        assert_eq!(
            blake(p).as_deref(),
            Some(lab.truth[p].as_str()),
            "{}",
            p.display()
        );
    }
    assert_eq!(blake(&nef).as_deref(), Some(NEF_BLAKE3));
    assert!(!xmp.exists());
    let logs: Vec<_> = std::fs::read_dir(data.join("logs")).unwrap().collect();
    assert!(!logs.is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// SAFETY_MODEL §8.2 at the commit: another program (a virus scanner) opens the file without
/// delete sharing just before ReplaceFileW. A short hold is waited out by the retries and the file
/// is written; a hold longer than every retry leaves the file skipped as in use, unchanged, with
/// nothing left behind, and it is written by a retry later.
#[test]
fn a_file_held_open_at_the_commit_is_retried_or_skipped() {
    let pkg = require!();
    for (ms, want) in [(500u64, "done"), (5000, "skipped")] {
        let case = format!("held {ms} ms");
        let lab = Lab::new(&format!("hold-{ms}"), &pkg);
        let plan = lab.plan("Morii", "p.json");
        let o = lab.cli_env(
            &[
                "apply",
                plan.to_str().unwrap(),
                "--hold-before-commit",
                &format!("2:{ms}"),
            ],
            true,
        );
        let r = Lab::json(&o);
        let op = r["op_id"].as_str().unwrap().to_owned();
        assert_eq!(state_of(&r, 2), want, "{case}: {r}");
        for f in r["files"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|f| f["seq"] != 2)
        {
            assert_eq!(f["state"], "done", "{case}: {f}");
        }
        let p2 = &lab.photos[2];
        if want == "skipped" {
            assert!(
                r["files"][2]["reason"].as_str().unwrap().contains("in use"),
                "{case}: {r}"
            );
            assert_eq!(blake(p2).as_deref(), Some(lab.truth[p2].as_str()), "{case}");
            // once the other program has let go, the file is written by a retry
            std::thread::sleep(Duration::from_millis(ms));
            let retry = lab.dir.join("retry.json");
            assert!(
                lab.cli(&["plan-retry", &op, "--out", retry.to_str().unwrap()])
                    .status
                    .success()
            );
            let again = lab.apply_ok(&retry);
            lab.undo(&again);
        }
        assert!(lab.leftovers().is_empty(), "{case}: {:?}", lab.leftovers());
        assert!(lab.cli(&["fsck", &op]).status.success(), "{case}");
        lab.undo(&op);
        lab.assert_all_original();
        let _ = std::fs::remove_dir_all(&lab.dir);
    }
}

/// The same on the Undo path: a file held open by another program at the moment its original
/// would be put back is skipped as in use and keeps the applied content; undoing the Operation
/// again once it is released restores it, and every file ends byte-identical.
#[test]
fn a_file_held_open_at_an_undo_commit_is_restored_by_a_later_undo() {
    let pkg = require!();
    let lab = Lab::new("hold-undo", &pkg);
    let op = lab.apply_ok(&lab.plan("Morii", "p.json"));
    let applied = lab.snapshot();
    let u = lab.dir.join("undo.json");
    assert!(
        lab.cli(&["plan-undo", &op, "--out", u.to_str().unwrap()])
            .status
            .success()
    );
    let o = lab.cli_env(
        &[
            "apply",
            u.to_str().unwrap(),
            "--hold-before-commit",
            "2:5000",
        ],
        true,
    );
    let r = Lab::json(&o);
    assert_eq!(state_of(&r, 2), "skipped", "{r}");
    let p2 = &lab.photos[2];
    assert_eq!(blake(p2).as_deref(), Some(applied[p2].as_str()));
    assert!(lab.leftovers().is_empty(), "{:?}", lab.leftovers());
    std::thread::sleep(Duration::from_millis(5000));
    lab.undo(&op);
    lab.assert_all_original();
    let _ = std::fs::remove_dir_all(&lab.dir);
}
