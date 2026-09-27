//! mm-cli — development and test driver for the MoriMeta core (not a public product; ARCHITECTURE §4.1).
//!
//! Global options (before the command):
//!   --data DIR          application data (db, backups, run); default %LOCALAPPDATA%\MoriMeta-dev
//!   --exiftool DIR      pinned ExifTool package (folder containing exiftool_files); or MM_EXIFTOOL_PKG
//!   --engine MODE       launcher | perl   (ARCHITECTURE ADR-03 A/B)
//!   --workers N         parallel file transactions (default clamp(cores/2, 1, 4)); each has its
//!                       own ExifTool session, and each volume limits its own concurrency
//!
//! Commands:
//!   scan [--files-from UTF8_FILE] FILE...
//!   inspect FILE              Inspector data: fields with sources, every tag (and the sidecar's)
//!   aggregate [--files-from UTF8_FILE] FILE...   per field, which values how many files hold
//!   plan-creator (--set NAME)... [--set-from UTF8_FILE] | --clear  --out PLAN.json [--title T] [--files-from UTF8_FILE] FILE...
//!   plan-time (--absolute "YYYY:MM:DD HH:MM:SS" | --shift [+|-][Nd]HH:MM:SS
//!              | --sequence "START" --step HH:MM:SS [--order time|name] | --preserve ANCHOR_FILE --to "TIME")
//!             [--no-digitized] --out PLAN.json [--title T] [--files-from UTF8_FILE] FILE...
//!   plan-gps --set "lat,lon[,alt]" | --remove  --out PLAN.json [--title T] [--files-from UTF8_FILE] FILE...
//!   plan-copyright --set TEXT | --set-from UTF8_FILE | --clear  --out PLAN.json [--title T] [--files-from UTF8_FILE] FILE...
//!   plan-preset (--id PRESET_ID | --preset PRESET.json) --out PLAN.json [--files-from UTF8_FILE] FILE...
//!   presets                   built-in (id builtin:NAME) and saved Presets (PRODUCT_SPEC §6.11)
//!   preset-import FILE.json | preset-export ID | preset-duplicate ID | preset-delete ID
//!   setting KEY [VALUE | --clear]   e.g. backup.max_age_days, backup.max_share_of_volume,
//!                             backup.keep_latest (the retention policy, SAFETY_MODEL §6.3)
//!                             metadata.preserve_mtime = true keeps each written file's
//!                             modification time (off by default, SAFETY_MODEL §8.9, D-6)
//!   apply PLAN.json [FAULTS]
//!   recover [--journal-fail-at ...]
//!   resolve OP_ID --keep SEQ...   files recovery left as "needs attention": keep what is on
//!                             disk (the backup stays; plan-undo --force-conflicts restores it)
//!   rebuild-journal           re-import operations missing from the database from their
//!                             backup folders (manifest.jsonl, plan.json); then run recover
//!   resume OP_ID [FAULTS]
//!   plan-undo OP_ID --out PLAN.json [--force-conflicts]
//!                             files changed after the Operation are excluded unless forced (their
//!                             current content is backed up first, so the forced restore can be undone)
//!   history | show OP_ID | fsck OP_ID
//!   plan-retry OP_ID --out PLAN.json   the failed and skipped files of an Operation again
//!   export-log OP_ID --out FILE.json   the Operation with field-level before/after (a new file)
//!   backups [--now-ms MS]     backup usage, protection and what the retention policy would prune
//!   prune [--requested] OP_ID...   remove backups (the policy's choice, or the user's with
//!                             --requested); unfinished Operations are always refused
//!   keep OP_ID [--off]        exempt an Operation from automatic pruning
//!
//! FAULTS (tests only; all require MM_FAULT_INJECTION=1):
//!   --crash-at SEQ:STEP       terminate the process at a fault point
//!   --fail-at SEQ:STEP        return an IO error there
//!   --disk-full-at SEQ:STEP   return a simulated disk-full error (Win32 112) there
//!   --fill-at SEQ:STEP --fill-dir DIR   really fill the (small test) volume of DIR there
//!   --journal-fail-at begin | finish | manifest[:N] | SEQ:STATE [--journal-fail-persist]
//!                             make SQLite fail that journal write (another connection holds the
//!                             write lock), or the file system refuse a manifest.json write
//!                             (after N successful ones);
//!                             with --journal-fail-persist every later write fails too
//!   --space-reserve BYTES     replace the 1 GiB backup-volume reserve of the space pre-check
//! Output is JSON on stdout. Exit codes: 0 ok, 1 error, 3 operation finished with files not done,
//! 4 fsck found problems.

use std::fs::OpenOptions;
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use mm_core::engine::Engine;
use mm_core::executor::{self, ExecOptions, FaultPoint, OpReport};
use mm_core::planner::TimeTool;
use mm_core::{fsck, history, inspect, planner, presets, recovery, retention, undo};
use mm_domain::capture;
use mm_domain::copyright::{self, CopyrightEdit};
use mm_domain::creator::{self, CreatorEdit};
use mm_domain::gps::{self, GeoPoint, GpsEdit};
use mm_domain::plan::Plan;
use mm_domain::rules;
use mm_domain::time::{self, SequenceOrder};
use mm_exiftool::EngineConfig;
use mm_store::{FileState, Store, WriteFault, WriteTarget};
use serde_json::{Value, json};

struct Global {
    data: PathBuf,
    exiftool: Option<PathBuf>,
    engine: String,
    workers: usize,
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

/// ARCHITECTURE §7.5: clamp(physical cores / 2, 1, 4); logical cores / 2 approximates it.
fn default_workers() -> usize {
    std::thread::available_parallelism()
        .map(|n| n.get() / 2)
        .unwrap_or(1)
        .clamp(1, 4)
}

/// `n` ExifTool sessions for the executor's workers, started side by side.
fn start_engines(g: &Global, n: usize) -> Result<Vec<Engine>, String> {
    let cfg = engine_config(g)?;
    std::thread::scope(|s| {
        let handles: Vec<_> = (0..n)
            .map(|_| s.spawn(|| Engine::start(cfg.clone())))
            .collect();
        handles
            .into_iter()
            .map(|h| {
                h.join()
                    .map_err(|_| "ExifTool start panicked".to_string())?
                    .map_err(|e| e.to_string())
            })
            .collect()
    })
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

fn require_fault_injection(flag: &str) -> Result<(), String> {
    if std::env::var("MM_FAULT_INJECTION").as_deref() != Ok("1") {
        return Err(format!("{flag} requires MM_FAULT_INJECTION=1"));
    }
    Ok(())
}

/// `--crash-at` / `--fail-at SEQ:STEP` (fault-injection tests only).
fn parse_point(args: &mut Vec<String>, flag: &str) -> Result<Option<FaultPoint>, String> {
    let Some(i) = args.iter().position(|a| a == flag) else {
        return Ok(None);
    };
    require_fault_injection(flag)?;
    let v = args.get(i + 1).cloned().ok_or(format!("{flag} SEQ:STEP"))?;
    args.drain(i..=i + 1);
    let (s, t) = v.split_once(':').ok_or(format!("{flag} SEQ:STEP"))?;
    Ok(Some(FaultPoint {
        seq: s.parse().map_err(|_| "bad SEQ")?,
        step: t.parse().map_err(|_| "bad STEP")?,
    }))
}

/// Remove a flag without a value; whether it was there.
fn take_flag(args: &mut Vec<String>, name: &str) -> bool {
    let had = args.iter().any(|a| a == name);
    args.retain(|a| a != name);
    had
}

fn take_opt(args: &mut Vec<String>, name: &str) -> Option<String> {
    let i = args.iter().position(|a| a == name)?;
    let v = args.get(i + 1).cloned();
    args.drain(i..(i + 2).min(args.len()));
    v
}

/// `(--set VALUE)... [--set-from UTF8_FILE]` → Some(values), or `--clear` → None.
/// The file holds one value per line and avoids shell code-page conversion of non-ASCII text.
fn set_values(args: &mut Vec<String>) -> Result<Option<Vec<String>>, String> {
    let clear = args.iter().any(|a| a == "--clear");
    args.retain(|a| a != "--clear");
    let mut values = Vec::new();
    while let Some(v) = take_opt(args, "--set") {
        values.push(v);
    }
    if let Some(f) = take_opt(args, "--set-from") {
        let text = std::fs::read_to_string(&f).map_err(|e| format!("{f}: {e}"))?;
        values.extend(
            text.lines()
                .map(|l| l.trim_start_matches('\u{feff}').to_owned())
                .filter(|l| !l.is_empty()),
        );
    }
    match (clear, values.is_empty()) {
        (true, true) => Ok(None),
        (false, false) => Ok(Some(values)),
        _ => Err("use either --set VALUE / --set-from FILE, or --clear".into()),
    }
}

/// Fault-injection options of `apply` and `resume` (tests only).
fn exec_options(args: &mut Vec<String>, store: &mut Store) -> Result<ExecOptions, String> {
    let fault = parse_point(args, "--crash-at")?;
    let fail = parse_point(args, "--fail-at")?;
    let disk_full = parse_point(args, "--disk-full-at")?;
    let fill = match parse_point(args, "--fill-at")? {
        Some(p) => Some((
            p,
            PathBuf::from(take_opt(args, "--fill-dir").ok_or("--fill-at needs --fill-dir DIR")?),
        )),
        None => None,
    };
    let space_reserve = match take_opt(args, "--space-reserve") {
        Some(v) => {
            require_fault_injection("--space-reserve")?;
            Some(v.parse().map_err(|_| "--space-reserve BYTES")?)
        }
        None => None,
    };
    let persistent = args.iter().any(|a| a == "--journal-fail-persist");
    args.retain(|a| a != "--journal-fail-persist");
    if let Some(t) = take_opt(args, "--journal-fail-at") {
        require_fault_injection("--journal-fail-at")?;
        let target = match t.as_str() {
            "begin" => WriteTarget::Begin,
            "finish" => WriteTarget::Finish,
            "manifest" => WriteTarget::Manifest(0),
            m if m.starts_with("manifest:") => WriteTarget::Manifest(
                m["manifest:".len()..]
                    .parse()
                    .map_err(|_| "--journal-fail-at manifest:N")?,
            ),
            s => {
                let bad =
                    || "--journal-fail-at begin | finish | manifest[:N] | SEQ:STATE".to_string();
                let (seq, state) = s.split_once(':').ok_or_else(bad)?;
                WriteTarget::File {
                    seq: seq.parse().map_err(|_| bad())?,
                    state: FileState::parse(state).ok_or_else(bad)?,
                }
            }
        };
        store.arm_write_fault(WriteFault { target, persistent });
    } else if persistent {
        return Err("--journal-fail-persist needs --journal-fail-at".into());
    }
    Ok(ExecOptions {
        fault,
        fail,
        disk_full,
        fill,
        space_reserve,
        preserve_mtime: store
            .setting("metadata.preserve_mtime")
            .map_err(|e| e.to_string())?
            .is_some_and(|v| v == "true"),
        ..Default::default()
    })
}

/// Expand a UTF-8 path list so large batches do not exceed the Windows command-line limit.
/// Each nonempty line is one path; spaces are preserved and CRLF is accepted.
fn file_paths(args: &mut Vec<String>) -> Result<Vec<PathBuf>, String> {
    let list_file = take_opt(args, "--files-from");
    let mut paths: Vec<PathBuf> = args.iter().map(PathBuf::from).collect();
    if let Some(file) = list_file {
        let contents = std::fs::read_to_string(&file).map_err(|e| format!("{file}: {e}"))?;
        paths.extend(
            contents
                .trim_start_matches('\u{feff}')
                .lines()
                .map(|line| line.trim_end_matches('\r'))
                .filter(|line| !line.is_empty())
                .map(PathBuf::from),
        );
    }
    if paths.is_empty() {
        return Err("no input files".into());
    }
    Ok(paths)
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
            "changes": serde_json::to_value(&e.changes).unwrap_or_default(), "notes": e.notes,
            "excluded": e.excluded})).collect::<Vec<_>>(),
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
        workers: match take_opt(&mut args, "--workers").map(|v| v.parse::<usize>()) {
            None => default_workers(),
            Some(Ok(n)) if (1..=16).contains(&n) => n,
            Some(_) => return fail("--workers takes 1..16"),
        },
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
                let paths = file_paths(&mut args)?;
                let snaps = eng.read_snapshots(&paths).map_err(|e| e.to_string())?;
                let out: Vec<Value> = paths
                    .iter()
                    .zip(snaps)
                    .map(|(p, s)| match s {
                        Ok(s) => {
                            let c = creator::read(&s);
                            let r = copyright::read(&s);
                            let t = match capture::read(&s) {
                                Ok(t) => json!(t.map(|t| capture::display(&t))),
                                Err(e) => json!({"invalid": e}),
                            };
                            json!({"path": p, "creator": c.effective, "sources": c.sources, "conflicting": c.conflicting,
                                   "capture_time": t,
                                   "gps": gps::read(&s).map(|p| p.display()),
                                   "copyright": {"value": r.effective, "sources": r.sources, "conflicting": r.conflicting,
                                                 "other_languages": r.other_languages}})
                        }
                        Err(e) => json!({"path": p, "error": e}),
                    })
                    .collect();
                println!("{}", Value::Array(out));
                Ok(ExitCode::SUCCESS)
            }
            "inspect" => {
                let f = args.first().ok_or("inspect FILE")?;
                let mut eng = with_engine(&g)?;
                let d = inspect::asset_detail(&mut eng, Path::new(f)).map_err(|e| e.to_string())?;
                println!("{}", serde_json::to_value(&d).unwrap_or_default());
                Ok(ExitCode::SUCCESS)
            }
            "aggregate" => {
                let mut eng = with_engine(&g)?;
                let paths = file_paths(&mut args)?;
                let a = inspect::selection_aggregate(&mut eng, &paths, &Default::default())
                    .map_err(|e| e.to_string())?;
                println!("{}", serde_json::to_value(&a).unwrap_or_default());
                Ok(ExitCode::SUCCESS)
            }
            "plan-creator" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let title = take_opt(&mut args, "--title").unwrap_or_else(|| "Set creator".into());
                let edit = match set_values(&mut args)? {
                    None => CreatorEdit::Clear,
                    Some(names) => CreatorEdit::Set(names),
                };
                let mut eng = with_engine(&g)?;
                let paths = file_paths(&mut args)?;
                let plan =
                    planner::plan_creator(&mut eng, &paths, &edit, &title, &Default::default())
                        .map_err(|e| e.to_string())?;
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "plan-time" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let title =
                    take_opt(&mut args, "--title").unwrap_or_else(|| "Set capture time".into());
                let digitized = !args.iter().any(|a| a == "--no-digitized");
                args.retain(|a| a != "--no-digitized");
                let local = |v: String| {
                    time::parse_local(&v).ok_or(format!("{v:?}: use YYYY:MM:DD HH:MM:SS"))
                };
                let delta = |v: String| {
                    time::parse_shift(&v).ok_or(format!("{v:?}: use [+|-][Nd]HH:MM:SS"))
                };
                let tools = [
                    take_opt(&mut args, "--absolute").map(|v| Ok(TimeTool::Absolute(local(v)?))),
                    take_opt(&mut args, "--shift").map(|v| Ok(TimeTool::Shift(delta(v)?))),
                    take_opt(&mut args, "--sequence").map(|v| {
                        let step =
                            take_opt(&mut args, "--step").ok_or("--sequence needs --step")?;
                        let order = match take_opt(&mut args, "--order").as_deref() {
                            None | Some("time") => SequenceOrder::CaptureTimeThenName,
                            Some("name") => SequenceOrder::NaturalFileName,
                            Some(o) => return Err(format!("--order time | name, not {o}")),
                        };
                        Ok(TimeTool::Sequence {
                            start: local(v)?,
                            step: delta(step)?,
                            order,
                        })
                    }),
                    take_opt(&mut args, "--preserve").map(|anchor| {
                        let to = take_opt(&mut args, "--to").ok_or("--preserve needs --to")?;
                        Ok(TimeTool::PreserveRelative {
                            anchor: PathBuf::from(anchor),
                            new_local: local(to)?,
                        })
                    }),
                ];
                let mut given = tools.into_iter().flatten();
                let tool = match (given.next(), given.next()) {
                    (Some(t), None) => t?,
                    _ => {
                        return Err(
                            "use exactly one of --absolute, --shift, --sequence, --preserve".into(),
                        );
                    }
                };
                let mut eng = with_engine(&g)?;
                let paths = file_paths(&mut args)?;
                let plan = planner::plan_capture_time(
                    &mut eng,
                    &paths,
                    &tool,
                    digitized,
                    &title,
                    &Default::default(),
                )
                .map_err(|e| e.to_string())?;
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "plan-gps" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let remove = args.iter().any(|a| a == "--remove");
                args.retain(|a| a != "--remove");
                let edit = match (take_opt(&mut args, "--set"), remove) {
                    (Some(v), false) => GpsEdit::Set(GeoPoint::parse(&v)?),
                    (None, true) => GpsEdit::Remove,
                    _ => return Err("use either --set lat,lon[,alt] or --remove".into()),
                };
                let title = take_opt(&mut args, "--title").unwrap_or_else(|| match edit {
                    GpsEdit::Set(_) => "Set GPS".into(),
                    GpsEdit::Remove => "Remove GPS".into(),
                });
                let mut eng = with_engine(&g)?;
                let paths = file_paths(&mut args)?;
                let plan = planner::plan_gps(&mut eng, &paths, &edit, &title, &Default::default())
                    .map_err(|e| e.to_string())?;
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "presets" => {
                let list = presets::list(&store).map_err(|e| e.to_string())?;
                println!(
                    "{}",
                    Value::Array(list.iter().map(|p| json!({"id": p.id, "name": p.name, "builtin": p.builtin,
                        "fields": p.fields.iter().map(|f| f.name()).collect::<Vec<_>>(), "last_used_ms": p.last_used_ms,
                        "preset": serde_json::to_value(&p.preset).unwrap_or_default()})).collect())
                );
                Ok(ExitCode::SUCCESS)
            }
            "preset-import" => {
                let file = args.first().ok_or("preset-import PRESET.json")?;
                let text = std::fs::read_to_string(file).map_err(|e| format!("{file}: {e}"))?;
                let id = presets::import(&mut store, &text).map_err(|e| e.to_string())?;
                println!("{}", json!({"id": id}));
                Ok(ExitCode::SUCCESS)
            }
            "preset-export" => {
                let id = args.first().ok_or("preset-export ID")?;
                let p = presets::get(&store, id).map_err(|e| e.to_string())?;
                println!("{}", p.preset.to_json());
                Ok(ExitCode::SUCCESS)
            }
            "preset-duplicate" => {
                let id = args.first().ok_or("preset-duplicate ID")?;
                let new = presets::duplicate(&mut store, id).map_err(|e| e.to_string())?;
                println!("{}", json!({"id": new}));
                Ok(ExitCode::SUCCESS)
            }
            "preset-delete" => {
                let id = args.first().ok_or("preset-delete ID")?;
                presets::delete(&mut store, id).map_err(|e| e.to_string())?;
                println!("{}", json!({"deleted": id}));
                Ok(ExitCode::SUCCESS)
            }
            "setting" => {
                let clear = take_flag(&mut args, "--clear");
                let key = args
                    .first()
                    .cloned()
                    .ok_or("setting KEY [VALUE | --clear]")?;
                let before = store.setting(&key).map_err(|e| e.to_string())?;
                if clear {
                    store.clear_setting(&key).map_err(|e| e.to_string())?;
                } else if let Some(v) = args.get(1) {
                    store.set_setting(&key, v).map_err(|e| e.to_string())?;
                }
                // a policy setting is checked before it is kept: it decides what is deleted
                if key.starts_with("backup.") {
                    if let Err(e) = retention::Policy::from_settings(&store) {
                        match &before {
                            Some(b) => store.set_setting(&key, b),
                            None => store.clear_setting(&key),
                        }
                        .map_err(|e| e.to_string())?;
                        return Err(e.to_string());
                    }
                }
                let v = store.setting(&key).map_err(|e| e.to_string())?;
                println!("{}", json!({"key": key, "value": v}));
                Ok(ExitCode::SUCCESS)
            }
            "plan-preset" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let (preset, used) =
                    match (take_opt(&mut args, "--id"), take_opt(&mut args, "--preset")) {
                        (Some(id), None) => (
                            presets::get(&store, &id).map_err(|e| e.to_string())?.preset,
                            Some(id),
                        ),
                        (None, Some(file)) => {
                            let text = std::fs::read_to_string(&file)
                                .map_err(|e| format!("{file}: {e}"))?;
                            (
                                rules::Preset::from_json(text.trim_start_matches('\u{feff}'))?,
                                None,
                            )
                        }
                        _ => {
                            return Err(
                                "use either --id PRESET_ID (see `presets`) or --preset FILE.json"
                                    .into(),
                            );
                        }
                    };
                let mut eng = with_engine(&g)?;
                let paths = file_paths(&mut args)?;
                let plan = planner::plan_preset(&mut eng, &paths, &preset, &Default::default())
                    .map_err(|e| e.to_string())?;
                if let Some(id) = used {
                    presets::used(&mut store, &id).map_err(|e| e.to_string())?;
                }
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "plan-copyright" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let title =
                    take_opt(&mut args, "--title").unwrap_or_else(|| "Set copyright".into());
                let edit = match set_values(&mut args)?.as_deref() {
                    None => CopyrightEdit::Clear,
                    Some([one]) => CopyrightEdit::Set(one.clone()),
                    Some(_) => return Err("copyright takes one value".into()),
                };
                let mut eng = with_engine(&g)?;
                let paths = file_paths(&mut args)?;
                let plan =
                    planner::plan_copyright(&mut eng, &paths, &edit, &title, &Default::default())
                        .map_err(|e| e.to_string())?;
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "apply" => {
                let opts = exec_options(&mut args, &mut store)?;
                let file = args.first().ok_or("apply PLAN.json")?;
                let plan: Plan =
                    serde_json::from_slice(&std::fs::read(file).map_err(|e| e.to_string())?)
                        .map_err(|e| format!("plan file: {e}"))?;
                let n = g.workers.min(plan.executable().count().max(1));
                let mut engines = start_engines(&g, n)?;
                let r = executor::start(&mut store, &mut engines, &plan, &opts)
                    .map_err(|e| e.to_string())?;
                engines.into_iter().for_each(Engine::close);
                Ok(report_exit(&r))
            }
            "resume" => {
                let opts = exec_options(&mut args, &mut store)?;
                let op = args.first().ok_or("resume OP_ID")?;
                let mut engines = start_engines(&g, g.workers)?;
                let r = executor::resume(&mut store, &mut engines, op, &opts)
                    .map_err(|e| e.to_string())?;
                engines.into_iter().for_each(Engine::close);
                Ok(report_exit(&r))
            }
            "rebuild-journal" => {
                let r = store.import_from_backups().map_err(|e| e.to_string())?;
                println!(
                    "{}",
                    json!({"imported": r.imported,
                           "skipped": r.skipped.iter().map(|(id, why)| json!({"id": id, "reason": why})).collect::<Vec<_>>(),
                           "next": "run `recover` before any new write"})
                );
                Ok(ExitCode::SUCCESS)
            }
            "recover" => {
                exec_options(&mut args, &mut store)?; // only the journal faults apply here
                let reps = recovery::recover(&mut store).map_err(|e| e.to_string())?;
                // a prune interrupted between its database mark and the deletion
                retention::finish_interrupted(&store).map_err(|e| e.to_string())?;
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
            "resolve" => {
                let keep = take_flag(&mut args, "--keep");
                let (Some(op), true) = (args.first().cloned(), keep) else {
                    return Err("resolve OP_ID --keep SEQ...".into());
                };
                let seqs = args[1..]
                    .iter()
                    .map(|s| s.parse::<u32>().map_err(|_| format!("bad SEQ {s}")))
                    .collect::<Result<Vec<_>, _>>()?;
                recovery::resolve_keep(&mut store, &op, &seqs).map_err(|e| e.to_string())?;
                let o = store
                    .operation(&op)
                    .map_err(|e| e.to_string())?
                    .ok_or("no such operation")?;
                println!("{}", json!({"id": op, "status": o.status}));
                Ok(ExitCode::SUCCESS)
            }
            "plan-undo" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let force = take_flag(&mut args, "--force-conflicts");
                let op = args
                    .first()
                    .ok_or("plan-undo OP_ID --out PLAN.json [--force-conflicts]")?;
                let eng = with_engine(&g)?;
                let mut plan =
                    undo::plan_undo(&store, op, eng.version()).map_err(|e| e.to_string())?;
                if force {
                    for e in plan.entries.iter_mut().filter(|e| undo::is_forced(e)) {
                        e.excluded = false;
                    }
                }
                eng.close();
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "history" => {
                // oldest first here; history::list pages newest first for the UI
                let mut ops = history::list(&store, 0, usize::MAX).map_err(|e| e.to_string())?;
                ops.reverse();
                println!("{}", serde_json::to_value(&ops).unwrap_or_default());
                Ok(ExitCode::SUCCESS)
            }
            "plan-retry" => {
                let out = take_opt(&mut args, "--out").ok_or("--out PLAN.json is required")?;
                let op = args.first().ok_or("plan-retry OP_ID --out PLAN.json")?;
                let eng = with_engine(&g)?;
                let plan =
                    history::retry_plan(&store, op, eng.version()).map_err(|e| e.to_string())?;
                eng.close();
                write_plan(&plan, &out)?;
                println!("{}", plan_json(&plan));
                Ok(ExitCode::SUCCESS)
            }
            "export-log" => {
                let out = take_opt(&mut args, "--out").ok_or("export-log OP_ID --out FILE.json")?;
                let op = args.first().ok_or("export-log OP_ID --out FILE.json")?;
                history::export_log(&store, op, Path::new(&out)).map_err(|e| e.to_string())?;
                println!("{}", json!({"written": out}));
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
            "backups" => {
                let policy = retention::Policy::from_settings(&store).map_err(|e| e.to_string())?;
                let now = match take_opt(&mut args, "--now-ms") {
                    Some(v) => v.parse().map_err(|_| "--now-ms MS")?,
                    None => mm_store::now_ms(),
                };
                let u = retention::usage(&store, &policy).map_err(|e| e.to_string())?;
                let plan = retention::prune_plan(&u, &policy, now);
                println!(
                    "{}",
                    json!({"total_bytes": u.total_bytes, "volume_bytes": u.volume_bytes,
                           "ops": u.ops.iter().map(|o| json!({"id": o.op_id, "title": o.title, "bytes": o.bytes,
                               "pruned": o.pruned, "protection": o.protection.map(|p| format!("{p:?}").to_lowercase())})).collect::<Vec<_>>(),
                           "would_prune": plan.iter().map(|(id, why)| json!({"id": id, "reason": format!("{why:?}").to_lowercase()})).collect::<Vec<_>>()})
                );
                Ok(ExitCode::SUCCESS)
            }
            "prune" => {
                let requested = take_flag(&mut args, "--requested");
                if args.is_empty() {
                    return Err("prune [--requested] OP_ID...".into());
                }
                let policy = retention::Policy::from_settings(&store).map_err(|e| e.to_string())?;
                let done = retention::prune(&mut store, &policy, &args, requested)
                    .map_err(|e| e.to_string())?;
                println!("{}", json!({"pruned": done}));
                Ok(ExitCode::SUCCESS)
            }
            "keep" => {
                let off = take_flag(&mut args, "--off");
                let op = args.first().ok_or("keep OP_ID [--off]")?;
                store
                    .operation(op)
                    .map_err(|e| e.to_string())?
                    .ok_or("no such operation")?;
                store.set_keep(op, !off).map_err(|e| e.to_string())?;
                println!("{}", json!({"id": op, "keep": !off}));
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

#[cfg(test)]
mod tests {
    use super::file_paths;
    use std::path::PathBuf;

    #[test]
    fn file_list_accepts_utf8_bom_crlf_and_spaces() {
        let list = std::env::temp_dir().join(format!(
            "mm-cli-files-{}.txt",
            mm_fs::random_token().unwrap()
        ));
        std::fs::write(&list, "\u{feff}first photo.jpg\r\n\r\n二枚目.jpg\r\n").unwrap();
        let mut args = vec![
            "direct.jpg".to_owned(),
            "--files-from".to_owned(),
            list.to_string_lossy().into_owned(),
        ];
        let paths = file_paths(&mut args).unwrap();
        std::fs::remove_file(&list).unwrap();
        assert_eq!(
            paths,
            vec![
                PathBuf::from("direct.jpg"),
                PathBuf::from("first photo.jpg"),
                PathBuf::from("二枚目.jpg")
            ]
        );
        assert_eq!(args, ["direct.jpg"]);
    }
}
