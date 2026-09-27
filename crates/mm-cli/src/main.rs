//! mm-cli — development and test driver for the MoriMeta core (not a public product; ARCHITECTURE §4.1).
//!
//! Global options (before the command):
//!   --data DIR          application data (db, backups, run); default %LOCALAPPDATA%\MoriMeta-dev
//!   --exiftool DIR      pinned ExifTool package (folder containing exiftool_files); or MM_EXIFTOOL_PKG
//!   --engine MODE       launcher | perl   (ARCHITECTURE ADR-03 A/B)
//!
//! Commands:
//!   scan FILE...
//!   plan-creator (--set NAME)... [--set-from UTF8_FILE] | --clear  --out PLAN.json [--title T] FILE...
//!   apply PLAN.json [--crash-at SEQ:STEP] [--fail-at SEQ:STEP]
//!   recover
//!   resume OP_ID [--crash-at SEQ:STEP] [--fail-at SEQ:STEP]
//!   plan-undo OP_ID --out PLAN.json
//!   history | show OP_ID | fsck OP_ID
//!
//! `--crash-at` terminates the process at a fault point, `--fail-at` injects an IO error there;
//! both require MM_FAULT_INJECTION=1.
//! Output is JSON on stdout. Exit codes: 0 ok, 1 error, 3 operation finished with files not done,
//! 4 fsck found problems.

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mm_core::engine::Engine;
use mm_core::executor::{self, ExecOptions, FaultPoint, OpReport};
use mm_core::{fsck, planner, recovery, undo};
use mm_domain::creator::{self, CreatorEdit};
use mm_domain::plan::Plan;
use mm_exiftool::EngineConfig;
use mm_store::Store;
use serde_json::{Value, json};

struct Global {
    data: PathBuf,
    exiftool: Option<PathBuf>,
    engine: String,
}

fn usage() -> ExitCode {
    eprintln!(
        "{}",
        include_str!("main.rs")
            .lines()
            .take_while(|l| l.starts_with("//!"))
            .map(|l| l.trim_start_matches("//!").trim_start_matches(' '))
            .collect::<Vec<_>>()
            .join("\n")
    );
    ExitCode::from(1)
}

fn fail(msg: impl std::fmt::Display) -> ExitCode {
    println!("{}", json!({"error": msg.to_string()}));
    ExitCode::from(1)
}

fn engine_config(g: &Global) -> Result<EngineConfig, String> {
    let pkg = g
        .exiftool
        .clone()
        .or_else(|| std::env::var_os("MM_EXIFTOOL_PKG").map(PathBuf::from))
        .ok_or("no ExifTool package: pass --exiftool DIR or set MM_EXIFTOOL_PKG")?;
    let run = g.data.join("run");
    let cwd = run.join("exiftool-cwd");
    let temp = run.join("tmp");
    std::fs::create_dir_all(&cwd).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(&temp).map_err(|e| e.to_string())?;
    match g.engine.as_str() {
        "perl" => Ok(EngineConfig {
            program: pkg.join("exiftool_files").join("perl.exe"),
            script: Some(pkg.join("exiftool_files").join("exiftool.pl")),
            cwd,
            temp,
        }),
        "launcher" => Ok(EngineConfig {
            program: pkg.join("exiftool.exe"),
            script: None,
            cwd,
            temp,
        }),
        other => Err(format!("unknown engine mode {other}")),
    }
}

/// Single-instance lock (SAFETY_MODEL §8.17): an exclusive handle held for the process lifetime.
fn instance_lock(data: &Path) -> Result<std::fs::File, String> {
    std::fs::create_dir_all(data.join("run")).map_err(|e| e.to_string())?;
    OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .share_mode(0)
        .open(data.join("run").join("instance.lock"))
        .map_err(|_| "another MoriMeta instance is using this data directory".to_string())
}

/// `--crash-at` / `--fail-at SEQ:STEP` (fault-injection tests only).
fn parse_point(args: &mut Vec<String>, flag: &str) -> Result<Option<FaultPoint>, String> {
    let Some(i) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    if std::env::var("MM_FAULT_INJECTION").as_deref() != Ok("1") {
        return Err(format!("{flag} requires MM_FAULT_INJECTION=1"));
    }
    let v = args.get(i + 1).cloned().ok_or(format!("{flag} SEQ:STEP"))?;
    args.drain(i..=i + 1);
    let (s, t) = v.split_once(':').ok_or(format!("{flag} SEQ:STEP"))?;
    Ok(Some(FaultPoint {
        seq: s.parse().map_err(|_| "bad SEQ")?,
        step: t.parse().map_err(|_| "bad STEP")?,
    }))
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    let v = args.get(i + 1).cloned();
    args.drain(i..(i + 2).min(args.len()));
    v
}

fn report_json(r: &OpReport) -> Value {
    json!({
        "op_id": r.op_id,
        "status": r.status.as_str(),
        "note": r.note,
        "files": r.files.iter().map(|f| json!({"seq": f.seq, "path": f.path, "state": f.state.as_str(), "reason": f.reason})).collect::<Vec<_>>(),
    })
}

fn report_exit(r: &OpReport) -> ExitCode {
    println!("{}", report_json(r));
    if r.files.iter().all(|f| f.state == mm_store::FileState::Done) {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(3)
    }
}

fn plan_json(p: &Plan) -> Value {
    json!({
        "plan_id": p.id,
        "kind": serde_json::to_value(&p.kind).unwrap_or_default(),
        "summary": serde_json::to_value(p.summary()).unwrap_or_default(),
        "entries": p.entries.iter().map(|e| json!({
            "seq": e.seq, "path": e.path, "status": serde_json::to_value(&e.status).unwrap_or_default(),
            "changes": serde_json::to_value(&e.changes).unwrap_or_default(), "notes": e.notes})).collect::<Vec<_>>(),
    })
}

fn write_plan(p: &Plan, out: &str) -> Result<(), String> {
    let path = Path::new(out);
    if path.exists() {
        return Err(format!("{out} exists; not overwritten"));
    }
    std::fs::write(
        path,
        serde_json::to_vec_pretty(p).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let data = take_opt(&mut args, "--data")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("LOCALAPPDATA").unwrap_or_default()).join("MoriMeta-dev")
        });
    let g = Global {
        data,
        exiftool: take_opt(&mut args, "--exiftool").map(PathBuf::from),
        engine: take_opt(&mut args, "--engine").unwrap_or_else(|| "launcher".into()),
    };
    if args.is_empty() {
        return usage();
    }
    let cmd = args.remove(0);
    let _lock = match instance_lock(&g.data) {
        Ok(l) => l,
        Err(e) => return fail(e),
    };
    let mut store = match Store::open(&g.data) {
        Ok(s) => s,
        Err(e) => return fail(e),
    };
    let with_engine = |g: &Global| -> Result<Engine, String> {
        Engine::start(engine_config(g)?).map_err(|e| e.to_string())
    };
    let res: Result<ExitCode, String> = (|| {
        match cmd.as_str() {
            "scan" => {
                let mut eng = with_engine(&g)?;
                let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
                let snaps = eng.read_snapshots(&paths).map_err(|e| e.to_string())?;
                let out: Vec<Value> = paths
                    .iter()
                    .zip(snaps)
                    .map(|(p, s)| match s {
                        Ok(s) => {
                            let c = creator::read(&s);
                            json!({"path": p, "creator": c.effective, "sources": c.sources, "conflicting": c.conflicting})
                        }
                        Err(e) => json!({"path": p, "error": e}),
                    })
                    .collect();
                println!("{}", Value::Array(out));
                Ok(ExitCode::SUCCESS)
            }
            "plan-creator" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let title = take_opt(&mut args, "--title").unwrap_or_else(|| "Set creator".into());
                let clear = args.iter().any(|a| a == "--clear");
                args.retain(|a| a != "--clear");
                let mut names = Vec::new();
                while let Some(v) = take_opt(&mut args, "--set") {
                    names.push(v);
                }
                // UTF-8 file, one name per line: avoids shell code-page conversion of non-ASCII text
                if let Some(f) = take_opt(&mut args, "--set-from") {
                    let text = std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?;
                    names.extend(
                        text.lines()
                            .map(|l| l.trim_start_matches('\u{feff}').to_owned())
                            .filter(|l| !l.is_empty()),
                    );
                }
                let edit = match (clear, names.is_empty()) {
                    (true, true) => CreatorEdit::Clear,
                    (false, false) => CreatorEdit::Set(names),
                    _ => return Err("use either --set NAME (repeatable) or --clear".into()),
                };
                let mut eng = with_engine(&g)?;
                let paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
                let plan = planner::plan_creator(&mut eng, &paths, &edit, &title)
                    .map_err(|e| e.to_string())?;
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "apply" => {
                let fault = parse_point(&mut args, "--crash-at")?;
                let fail = parse_point(&mut args, "--fail-at")?;
                let file = args.first().ok_or("apply PLAN.json")?;
                let plan: Plan =
                    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("plan file: {e}"))?;
                let mut eng = with_engine(&g)?;
                let r = executor::start(&mut store, &mut eng, &plan, &ExecOptions { fault, fail })
                    .map_err(|e| e.to_string())?;
                eng.close();
                Ok(report_exit(&r))
            }
            "resume" => {
                let fault = parse_point(&mut args, "--crash-at")?;
                let fail = parse_point(&mut args, "--fail-at")?;
                let op = args.first().ok_or("resume OP_ID")?;
                let mut eng = with_engine(&g)?;
                let r = executor::resume(&mut store, &mut eng, op, &ExecOptions { fault, fail })
                    .map_err(|e| e.to_string())?;
                eng.close();
                Ok(report_exit(&r))
            }
            "recover" => {
                let reps = recovery::recover(&mut store).map_err(|e| e.to_string())?;
                let out: Vec<Value> = reps
                    .iter()
                    .map(|r| {
                        json!({"op_id": r.op_id, "files": r.files.iter().map(|f| json!({
                            "seq": f.seq, "path": f.path, "from": f.from.as_str(), "to": f.to.as_str(), "action": f.action})).collect::<Vec<_>>()})
                    })
                    .collect();
                println!("{}", Value::Array(out));
                Ok(ExitCode::SUCCESS)
            }
            "plan-undo" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let op = args.first().ok_or("plan-undo OP_ID --out PLAN.json")?;
                let eng = with_engine(&g)?;
                let plan = undo::plan_undo(&store, op, eng.version()).map_err(|e| e.to_string())?;
                eng.close();
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "history" => {
                let ops = store.operations().map_err(|e| e.to_string())?;
                println!(
                    "{}",
                    Value::Array(ops.iter().map(|o| json!({"id": o.id, "kind": o.kind, "title": o.title, "status": o.status, "undo_of": o.undo_of})).collect())
                );
                Ok(ExitCode::SUCCESS)
            }
            "show" => {
                let op = args.first().ok_or("show OP_ID")?;
                let o = store
                    .operation(op)
                    .map_err(|e| e.to_string())?
                    .ok_or("no such operation")?;
                let files = store.files(op).map_err(|e| e.to_string())?;
                println!(
                    "{}",
                    json!({"id": o.id, "kind": o.kind, "status": o.status, "files": files.iter().map(|f| json!({
                        "seq": f.seq, "path": f.path, "state": f.state.as_str(), "h0": f.h0, "h1": f.h1, "error": f.error,
                        "backup": f.backup_path, "temp": f.temp_path, "bak": f.bak_path})).collect::<Vec<_>>()})
                );
                Ok(ExitCode::SUCCESS)
            }
            "fsck" => {
                let op = args.first().ok_or("fsck OP_ID")?;
                let r = fsck::fsck(&store, op).map_err(|e| e.to_string())?;
                println!(
                    "{}",
                    json!({"op_id": r.op_id, "files": r.files, "problems": r.problems})
                );
                Ok(if r.problems.is_empty() {
                    ExitCode::SUCCESS
                } else {
                    ExitCode::from(4)
                })
            }
            _ => Ok(usage()),
        }
    })();
    match res {
        Ok(code) => code,
        Err(e) => fail(e),
    }
}
