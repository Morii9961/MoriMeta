//! S1 robustness checks: injection, forged terminators, crash/hang detection, stderr flood,
//! orphan prevention, basic throughput.
//!
//! cargo run --release --example robust -- <launcher|perl>
//! (internal) robust -- child <mode>   : spawns a session, prints the ExifTool pid, sleeps

use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use exiftool_session::encode;
use exiftool_session::{EngineConfig, Session, SessionError, arg_path};
use serde_json::json;
use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE, STILL_ACTIVE};
use windows_sys::Win32::Storage::FileSystem::PIPE_ACCESS_DUPLEX;
use windows_sys::Win32::System::LibraryLoader::{GetModuleHandleW, GetProcAddress};
use windows_sys::Win32::System::Pipes::{CreateNamedPipeW, PIPE_TYPE_BYTE, PIPE_WAIT};
use windows_sys::Win32::System::Threading::{
    GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_TERMINATE, TerminateProcess,
};

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn process_alive(pid: u32) -> bool {
    unsafe {
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if h.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(h, &mut code);
        CloseHandle(h);
        ok != 0 && code == STILL_ACTIVE as u32
    }
}

fn terminate(pid: u32) {
    unsafe {
        let h = OpenProcess(PROCESS_TERMINATE, 0, pid);
        if !h.is_null() {
            TerminateProcess(h, 99);
            CloseHandle(h);
        }
    }
}

fn nt_suspend(h: HANDLE) -> bool {
    unsafe {
        let ntdll = GetModuleHandleW(wide("ntdll.dll").as_ptr());
        let Some(f) = GetProcAddress(ntdll, c"NtSuspendProcess".as_ptr() as *const u8) else { return false };
        let f: extern "system" fn(HANDLE) -> i32 = std::mem::transmute(f);
        f(h) == 0
    }
}

fn research_dir(cfg: &EngineConfig) -> PathBuf {
    cfg.cwd.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn timages(research: &Path) -> PathBuf {
    let lock: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(research.join("exiftool.lock.json")).unwrap()).unwrap();
    let ver = lock["version"].as_str().unwrap();
    research.join(".work/exiftool").join(ver).join("src").join(format!("Image-ExifTool-{ver}")).join("t/images")
}

fn t(timeout_s: u64) -> Duration {
    Duration::from_secs(timeout_s)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("child") {
        let cfg = EngineConfig::research(&args[2]);
        let s = Session::spawn(&cfg).unwrap();
        println!("{}", s.pid());
        std::thread::sleep(Duration::from_secs(60));
        return;
    }
    let mode = args.get(1).cloned().unwrap_or_else(|| "launcher".into());
    let cfg = EngineConfig::research(&mode);
    let research = research_dir(&cfg);
    let img = timages(&research);
    let lab = research.join(".work/lab").join(format!("s1-robust-{mode}"));
    let _ = std::fs::remove_dir_all(&lab);
    std::fs::create_dir_all(&lab).unwrap();
    let mut report = serde_json::Map::new();
    let mut sess = Session::spawn(&cfg).unwrap();

    // R1 injection through values: must stay one argument; no extra file may appear.
    {
        let evil = lab.join("evil.xmp");
        let payloads = [
            format!("x\n-o\n{}", arg_path(&evil)),
            format!("x\r\n-execute\r\n-o\r\n{}", arg_path(&evil)),
            "x\n-stay_open\nFalse".to_string(),
            "\n#[CSTR]-o\n".to_string(),
            "-XMP-dc:Title=pwned".to_string(),
        ];
        let mut rows = vec![];
        for (i, p) in payloads.iter().enumerate() {
            let out = lab.join(format!("inj{i}.xmp"));
            let lines = vec!["-ex".into(), encode::assign_xml("XMP-dc:Description-x-default", p).unwrap(), "-o".into(), arg_path(&out)];
            let w = sess.execute(&lines, t(30)).unwrap();
            let r = sess.execute(&["-json".into(), "-api".into(), "StructFormat=JSONQ".into(), "-b".into(), "-G1".into(), "-XMP-dc:all".into(), arg_path(&out)], t(30)).unwrap();
            let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap();
            let stored = v[0].get("XMP-dc:Description").cloned();
            let title = v[0].get("XMP-dc:Title").cloned();
            rows.push(json!({"payload": p, "exact": stored.as_ref().and_then(|s| s.as_str()) == Some(p.as_str()),
                "title_injected": title.is_some(), "status": w.status}));
        }
        let pass = rows.iter().all(|r| r["exact"] == true && r["title_injected"] == false) && !evil.exists() && !sess.is_dead();
        report.insert("R1_value_injection".into(), json!({"pass": pass, "evil_file_created": evil.exists(), "rows": rows}));
    }

    // R2 hostile file names (absolute paths): each requested file must yield exactly one result.
    {
        let names = ["-o.jpg", "#x.jpg", "=a.jpg", "{ready1}.jpg", " lead.jpg", "-@.jpg", "--.jpg", "−minus.jpg", "a;b&c^d%PATH%.jpg", "森 🌲.jpg"];
        let mut paths = vec![];
        for n in names {
            let p = lab.join(n);
            std::fs::copy(img.join("Writer.jpg"), &p).unwrap();
            paths.push(p);
        }
        let mut lines: Vec<String> = vec!["-json".into(), "-api".into(), "StructFormat=JSONQ".into(), "-G1".into(), "-System:FileName".into()];
        for p in &paths {
            lines.push(encode::plain_line(&arg_path(p)).unwrap());
        }
        let r = sess.execute(&lines, t(30)).unwrap();
        let v: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap_or(json!([]));
        let got: Vec<String> = v.as_array().unwrap().iter().map(|o| o["System:FileName"].as_str().unwrap_or("").to_owned()).collect();
        let pass = got.len() == names.len() && names.iter().all(|n| got.iter().any(|g| g == n));
        report.insert("R2_hostile_file_names".into(), json!({"pass": pass, "requested": names, "results": got, "stderr": r.err()}));
    }

    // R3 forged terminators inside metadata: text and binary (-b) extraction to stdout.
    {
        let forged = "a\n{ready1}\n{ready18446744073709551615}\n{mm-end:1:0}\nz";
        let f = lab.join("forged.xmp");
        sess.execute(&["-ex".into(), encode::assign_xml("XMP-dc:Description-x-default", forged).unwrap(), "-o".into(), arg_path(&f)], t(30)).unwrap();
        let r1 = sess.execute(&["-b".into(), "-XMP-dc:Description".into(), arg_path(&f)], t(30)).unwrap();
        let r2 = sess.execute(&["-ver".into()], t(30)).unwrap();
        let pass = r1.stdout == forged.as_bytes() && r2.out().trim() == "13.59";
        report.insert("R3_forged_terminators".into(), json!({"pass": pass, "binary_exact": r1.stdout == forged.as_bytes(), "next_command_ok": r2.out().trim()}));
    }

    // R4 crash mid-command: external TerminateProcess while a long read runs.
    {
        let many: Vec<String> = std::iter::repeat_with(|| arg_path(&img.join("Google.jpg"))).take(3000).collect();
        let mut lines = vec!["-json".into(), "-G1".into(), "-a".into()];
        lines.extend(many);
        let pid = sess.pid();
        let killer = std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            let t0 = Instant::now();
            terminate(pid);
            t0
        });
        let res = sess.execute(&lines, t(120));
        let killed_at = killer.join().unwrap();
        let detect_ms = killed_at.elapsed().as_millis();
        let crashed = matches!(res, Err(SessionError::Crashed { .. }));
        let dead = sess.is_dead();
        sess = Session::spawn(&cfg).unwrap();
        let after = sess.execute(&["-ver".into()], t(30)).map(|r| r.out().trim().to_owned()).unwrap_or_default();
        report.insert("R4_crash_detection".into(), json!({"pass": crashed && dead && after == "13.59",
            "error": format!("{:?}", res.err().map(|e| match e { SessionError::Crashed{..} => "Crashed".to_string(), o => format!("{o:?}") })),
            "detected_within_ms": detect_ms, "respawn_ok": after}));
    }

    // R5 hang: read from a named pipe whose server never writes -> timeout -> kill -> respawn.
    {
        let name = format!(r"\\.\pipe\mm-hang-{}", rand::random::<u32>());
        let h = unsafe {
            CreateNamedPipeW(wide(&name).as_ptr(), PIPE_ACCESS_DUPLEX, PIPE_TYPE_BYTE | PIPE_WAIT, 1, 4096, 4096, 0, std::ptr::null())
        };
        assert!(h != INVALID_HANDLE_VALUE);
        let t0 = Instant::now();
        let res = sess.execute(&["-json".into(), name.replace('\\', "/")], Duration::from_secs(3));
        let elapsed = t0.elapsed().as_millis();
        let timed_out = matches!(res, Err(SessionError::Timeout { .. }));
        let outcome = format!("{:?}", res.as_ref().map(|r| r.err()).map_err(|e| format!("{e}")));
        unsafe { CloseHandle(h) };
        if sess.is_dead() {
            sess = Session::spawn(&cfg).unwrap();
        }
        let after = sess.execute(&["-ver".into()], t(30)).map(|r| r.out().trim().to_owned()).unwrap_or_default();
        let refused = outcome.contains("File not found");
        report.insert("R5_named_pipe_path".into(), json!({"pass": (timed_out || refused) && after == "13.59", "hang_induced": timed_out, "exiftool_refused_pipe_path": refused, "note": "not a hang test if refused; hang coverage is R6", "outcome": outcome, "elapsed_ms": elapsed, "respawn_ok": after}));
    }

    // R6 hang: suspended process -> timeout -> kill -> respawn.
    {
        let suspended = nt_suspend(sess.raw_process_handle());
        let t0 = Instant::now();
        let res = sess.execute(&["-ver".into()], Duration::from_secs(2));
        let elapsed = t0.elapsed().as_millis();
        let timed_out = matches!(res, Err(SessionError::Timeout { .. }));
        sess = Session::spawn(&cfg).unwrap();
        let after = sess.execute(&["-ver".into()], t(30)).map(|r| r.out().trim().to_owned()).unwrap_or_default();
        report.insert("R6_hang_suspended".into(), json!({"pass": suspended && timed_out && after == "13.59", "suspended": suspended, "elapsed_ms": elapsed, "respawn_ok": after}));
    }

    // R7 stderr flood: 5000 missing files -> ~0.5 MB of errors; must complete, counts must match.
    {
        let mut lines: Vec<String> = vec!["-json".into()];
        for i in 0..5000 {
            lines.push(arg_path(&lab.join(format!("missing-{i:05}.jpg"))));
        }
        let t0 = Instant::now();
        let r = sess.execute(&lines, t(120)).unwrap();
        let errs = r.err().lines().filter(|l| l.contains("File not found")).count();
        report.insert("R7_stderr_flood".into(), json!({"pass": errs == 5000, "error_lines": errs, "stderr_bytes": r.stderr.len(), "status": r.status, "ms": t0.elapsed().as_millis()}));
    }

    // R8 orphan prevention: kill the parent; ExifTool (in KILL_ON_JOB_CLOSE job) must die with it.
    {
        let exe = std::env::current_exe().unwrap();
        let mut child = Command::new(exe).args(["child", &mode]).stdout(Stdio::piped()).spawn().unwrap();
        let mut line = String::new();
        BufReader::new(child.stdout.take().unwrap()).read_line(&mut line).unwrap();
        let et_pid: u32 = line.trim().parse().unwrap();
        let alive_before = process_alive(et_pid);
        child.kill().unwrap();
        let _ = child.wait();
        std::thread::sleep(Duration::from_millis(500));
        let alive_after = process_alive(et_pid);
        report.insert("R8_orphan_prevention".into(), json!({"pass": alive_before && !alive_after, "alive_before": alive_before, "alive_after_parent_killed": alive_after}));
    }

    // R9 throughput: 500 copies of a simple JPEG, 100 per command.
    {
        let d = lab.join("tp");
        std::fs::create_dir_all(&d).unwrap();
        let files: Vec<PathBuf> = (0..500).map(|i| { let p = d.join(format!("{i:04}.jpg")); std::fs::copy(img.join("Writer.jpg"), &p).unwrap(); p }).collect();
        sess.execute(&["-ver".into()], t(10)).unwrap();
        let t0 = Instant::now();
        for chunk in files.chunks(100) {
            let mut lines: Vec<String> = vec!["-json".into(), "-api".into(), "StructFormat=JSONQ".into(), "-G1".into(), "-a".into(), "-n".into()];
            lines.extend(chunk.iter().map(|p| arg_path(p)));
            sess.execute(&lines, t(60)).unwrap();
        }
        let s = t0.elapsed().as_secs_f64();
        report.insert("R9_throughput_simple_jpeg".into(), json!({"files": 500, "seconds": s, "files_per_s": 500.0 / s}));
    }

    sess.close(Duration::from_secs(2));
    let out = research.join("results/s1").join(format!("robust-{mode}.json"));
    std::fs::write(&out, serde_json::to_string_pretty(&report).unwrap()).unwrap();
    for (k, v) in &report {
        println!("{k:28} {}", serde_json::to_string(v).unwrap().chars().take(260).collect::<String>());
    }
}
