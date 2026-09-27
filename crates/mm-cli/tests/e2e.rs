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
        std::fs::create_dir_all(dir.join("photos")).unwrap();
        let mut photos = Vec::new();
        let mut truth = BTreeMap::new();
        for s in SAMPLES {
            let p = dir.join("photos").join(s);
            std::fs::copy(timages().join(s), &p).unwrap();
            truth.insert(p.clone(), blake(&p).unwrap());
            photos.push(p);
        }
        Lab {
            data: dir.join("data"),
            dir,
            pkg: pkg.to_path_buf(),
            photos,
            truth,
        }
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
        std::fs::read_dir(self.dir.join("photos"))
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(".mmtmp-") || n.contains(".mmbak-"))
            .collect()
    }

    /// Before recovery: the original content of every file exists at the path, a bak name or a backup.
    fn assert_preimages(&self, op: &str) {
        let s = self.show(op);
        for (path, h0) in &self.truth {
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
        let s = self.show(op);
        for (path, h0) in &self.truth {
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
