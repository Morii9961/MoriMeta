// SPDX-License-Identifier: GPL-3.0-or-later
//! Integration tests against the pinned ExifTool (research/exiftool.lock.json).
//! Skipped (with a message) when `research/scripts/fetch_exiftool.py` has not been run,
//! or when MM_EXIFTOOL_PKG points elsewhere.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mm_exiftool::{Command, EngineConfig, EngineError, Line, Session, TagName};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn package() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("MM_EXIFTOOL_PKG") {
        return Some(PathBuf::from(p));
    }
    let lock: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(repo().join("research/exiftool.lock.json")).ok()?,
    )
    .ok()?;
    let v = lock["version"].as_str()?;
    let p = repo().join(format!("research/.work/exiftool/{v}/win64/exiftool-{v}_64"));
    p.join("exiftool_files/perl.exe").exists().then_some(p)
}

fn scratch(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("mm-exiftool-it-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn config(pkg: &Path, mode: &str, dir: &Path) -> EngineConfig {
    let cwd = dir.join("cwd");
    let temp = dir.join("tmp");
    std::fs::create_dir_all(&cwd).unwrap();
    std::fs::create_dir_all(&temp).unwrap();
    match mode {
        "perl" => EngineConfig {
            program: pkg.join("exiftool_files/perl.exe"),
            script: Some(pkg.join("exiftool_files/exiftool.pl")),
            cwd,
            temp,
        },
        _ => {
            let exe = pkg.join("exiftool.exe");
            if !exe.exists() {
                std::fs::copy(pkg.join("exiftool(-k).exe"), &exe).unwrap();
            }
            EngineConfig {
                program: exe,
                script: None,
                cwd,
                temp,
            }
        }
    }
}

macro_rules! require_pkg {
    () => {
        match package() {
            Some(p) => p,
            None => {
                eprintln!(
                    "SKIP: pinned ExifTool not found (run research/scripts/fetch_exiftool.py)"
                );
                return;
            }
        }
    };
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn random_value(r: &mut Rng) -> String {
    const PIECES: &[&str] = &[
        " ",
        "  ",
        "\t",
        "\n",
        "\r",
        "\r\n",
        "&",
        "<",
        ">",
        "\"",
        "'",
        "&amp;",
        "#",
        "-",
        "=",
        "$",
        "@",
        "${status}",
        "\\",
        "#[CSTR]",
        "{ready1}",
        "\n{mm-end:7:0}\n",
        "\n-o\nC:/x.jpg",
        "-execute",
        "森",
        "林",
        "é",
        "🌲",
        "\u{0301}",
        "\u{05D0}",
        "\u{200B}",
        "\u{FEFF}",
        "\u{2028}",
        "\u{85}",
        "\u{7F}",
        "a",
        "Z",
        "0",
        "9",
        "1e5",
        "true",
    ];
    let n = 1 + (r.next() % 10) as usize;
    (0..n)
        .map(|_| PIECES[(r.next() % PIECES.len() as u64) as usize])
        .collect()
}

fn json_strings(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::Array(a) => a.iter().flat_map(json_strings).collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        other => vec![other.to_string()],
    }
}

#[test]
fn values_round_trip_exactly_in_both_modes() {
    let pkg = require_pkg!();
    for mode in ["launcher", "perl"] {
        let dir = scratch(&format!("rt-{mode}"));
        let mut s = Session::spawn(&config(&pkg, mode, &dir)).unwrap();
        let subject = TagName::new("XMP-dc:Subject").unwrap();
        let mut rng = Rng(20260926);
        for batch in 0..5 {
            let mut want = Vec::new();
            let mut cmd = Command::write();
            while want.len() < 1000 {
                let v = random_value(&mut rng);
                if want.contains(&v) {
                    continue;
                }
                cmd.push(Line::assign(&subject, &v).unwrap());
                want.push(v);
            }
            let out = dir.join(format!("b{batch}.xmp"));
            cmd.push(Line::option("-o"));
            cmd.push(Line::path(&out).unwrap());
            let w = s.execute(&cmd, Duration::from_secs(60)).unwrap();
            assert_eq!(w.status, 0, "{}", w.stderr_text());
            let mut r = Command::read_json();
            r.push(Line::option("-b"))
                .push(Line::option("-G1"))
                .push(Line::request(&subject))
                .push(Line::path(&out).unwrap());
            let got = json_strings(
                &s.execute(&r, Duration::from_secs(60))
                    .unwrap()
                    .json()
                    .unwrap()[0]["XMP-dc:Subject"],
            );
            assert_eq!(got, want, "mode {mode} batch {batch}");
        }
        s.close(Duration::from_secs(2));
    }
}

#[test]
fn injected_lines_stay_values_and_forged_terminators_are_data() {
    let pkg = require_pkg!();
    let dir = scratch("inject");
    let mut s = Session::spawn(&config(&pkg, "launcher", &dir)).unwrap();
    let desc = TagName::new("XMP-dc:Description-x-default").unwrap();
    let evil = dir.join("evil.xmp");
    let payload = format!("x\n-o\n{}\n{{ready1}}\n{{mm-end:1:0}}\nz", evil.display());
    let out = dir.join("v.xmp");
    let mut w = Command::write();
    w.push(Line::assign(&desc, &payload).unwrap())
        .push(Line::option("-o"))
        .push(Line::path(&out).unwrap());
    s.execute(&w, Duration::from_secs(30)).unwrap();
    assert!(!evil.exists());
    // binary extraction: output has no trailing newline and contains fake terminators
    let mut r = Command::empty();
    r.push(Line::option("-b"))
        .push(Line::request(&TagName::new("XMP-dc:Description").unwrap()))
        .push(Line::path(&out).unwrap());
    let got = s.execute(&r, Duration::from_secs(30)).unwrap();
    assert_eq!(got.stdout, payload.as_bytes());
    let mut v = Command::empty();
    v.push(Line::option("-ver"));
    assert!(
        s.execute(&v, Duration::from_secs(30))
            .unwrap()
            .stdout_text()
            .trim()
            .starts_with("13.")
    );
}

#[test]
fn crash_timeout_and_drop_are_handled() {
    let pkg = require_pkg!();
    let dir = scratch("crash");
    let cfg = config(&pkg, "launcher", &dir);
    let mut ver = Command::empty();
    ver.push(Line::option("-ver"));

    // external kill while idle -> next command reports a crash, session becomes dead
    let mut s = Session::spawn(&cfg).unwrap();
    s.execute(&ver, Duration::from_secs(30)).unwrap();
    std::process::Command::new("taskkill")
        .args(["/F", "/PID", &s.pid().to_string()])
        .output()
        .unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert!(matches!(
        s.execute(&ver, Duration::from_secs(10)),
        Err(EngineError::Crashed { .. })
    ));
    assert!(s.is_dead());
    assert!(matches!(
        s.execute(&ver, Duration::from_secs(10)),
        Err(EngineError::Dead)
    ));

    // timeout: a large read with a 1 ms deadline
    let mut s = Session::spawn(&cfg).unwrap();
    let mut big = Command::read_json();
    let exe = cfg.program.clone();
    for _ in 0..200 {
        big.push(Line::path(&exe).unwrap());
    }
    assert!(matches!(
        s.execute(&big, Duration::from_millis(1)),
        Err(EngineError::Timeout { .. })
    ));
    assert!(s.is_dead());

    // drop kills the process (job object / explicit kill)
    let s = Session::spawn(&cfg).unwrap();
    let pid = s.pid();
    drop(s);
    std::thread::sleep(Duration::from_millis(300));
    let listed = std::process::Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&listed.stdout).contains(&pid.to_string()));
}
