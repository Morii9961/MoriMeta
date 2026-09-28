// SPDX-License-Identifier: GPL-3.0-or-later
//! ExifTool engine wrapper used by the application services: respawns a dead session, applies
//! size-based timeouts (ARCHITECTURE §7.2) and turns typed `TagOp`s into validated argfile lines.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mm_domain::plan::TagOp;
use mm_domain::snapshot::Snapshot;
use mm_exiftool::{Command, EngineConfig, EngineError, Line, Output, Session, TagName, Terminator};

use crate::CoreError;

pub struct Engine {
    cfg: EngineConfig,
    session: Option<Session>,
    version: String,
    restarts: u32,
    kill: KillSwitch,
}

/// Ends whatever ExifTool process an [`Engine`] currently runs, from another thread (a user's
/// Cancel, SAFETY_MODEL §11). The engine starts a new process on its next use.
#[derive(Clone, Default)]
pub struct KillSwitch(std::sync::Arc<std::sync::Mutex<Option<Terminator>>>);

impl KillSwitch {
    pub fn kill(&self) {
        if let Some(t) = self.0.lock().unwrap_or_else(|p| p.into_inner()).as_ref() {
            t.terminate();
        }
    }
}

/// Files per ExifTool read command (ARCHITECTURE §6.1: 100–200).
const READ_CHUNK: usize = 100;

type ChunkResult = Result<Vec<Result<Snapshot, String>>, CoreError>;

/// Read timeout: 10 s + 0.2 s/MB; write: 30 s + 0.5 s/MB (initial values, S4 calibrates).
pub fn read_timeout(bytes: u64) -> Duration {
    Duration::from_millis(10_000 + bytes / 5_000)
}
pub fn write_timeout(bytes: u64) -> Duration {
    Duration::from_millis(30_000 + bytes / 2_000)
}

fn tag(name: &str) -> Result<TagName, CoreError> {
    TagName::new(name).map_err(|e| CoreError::Internal(format!("tag {name}: {e}")))
}

fn path_line(p: &Path) -> Result<Line, CoreError> {
    Line::path(p).map_err(|e| CoreError::Input(format!("{}: {e}", p.display())))
}

/// GPS values as numbers (compared numerically, SAFETY_MODEL §5 V2), every other tag printed as
/// before. ExifTool 13.59 honours a `TAG#` request only when it comes before `-all`.
fn numeric_gps_then_all(c: &mut Command) {
    for o in mm_domain::gps::NUMERIC_READ {
        c.push(Line::option(o));
    }
    c.push(Line::option("-all"));
}

/// The string ExifTool reports as `SourceFile` for a path we passed.
pub fn source_key(p: &Path) -> Option<String> {
    Line::path(p).ok().map(|l| l.as_str().to_owned())
}

impl Engine {
    pub fn start(cfg: EngineConfig) -> Result<Engine, CoreError> {
        let mut e = Engine {
            cfg,
            session: None,
            version: String::new(),
            restarts: 0,
            kill: KillSwitch::default(),
        };
        let mut c = Command::empty();
        c.push(Line::option("-ver"));
        let out = e.exec(&c, Duration::from_secs(30))?;
        e.version = out.stdout_text().trim().to_owned();
        if e.version.is_empty() {
            return Err(CoreError::Engine(
                "ExifTool did not report a version".into(),
            ));
        }
        Ok(e)
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn restarts(&self) -> u32 {
        self.restarts
    }

    pub fn kill_switch(&self) -> KillSwitch {
        self.kill.clone()
    }

    fn session(&mut self) -> Result<&mut Session, CoreError> {
        if self.session.as_mut().map(|s| s.is_dead()).unwrap_or(true) {
            if self.session.is_some() {
                self.restarts += 1;
            }
            let s = Session::spawn(&self.cfg).map_err(|e| CoreError::Engine(e.to_string()))?;
            *self.kill.0.lock().unwrap_or_else(|p| p.into_inner()) = s.terminator().ok();
            self.session = Some(s);
        }
        Ok(self.session.as_mut().expect("session"))
    }

    pub fn exec(&mut self, cmd: &Command, timeout: Duration) -> Result<Output, CoreError> {
        let t = std::time::Instant::now();
        let r = self.session()?.execute(cmd, timeout).map_err(|e| match e {
            EngineError::Timeout { .. } => CoreError::Engine("ExifTool timed out".into()),
            other => CoreError::Engine(other.to_string()),
        });
        if crate::log::debug_on() {
            let lines: Vec<&str> = cmd.lines().iter().map(|l| l.as_str()).collect();
            crate::log::debug_command(&lines, t.elapsed().as_millis(), r.is_ok());
        }
        r
    }

    /// Read the tags used for planning (`-G1`, all values quoted). Results are matched to the
    /// request by `SourceFile`; a file without a result is reported as an error.
    pub fn read_snapshots(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<Vec<Result<Snapshot, String>>, CoreError> {
        self.read_snapshots_with(paths, &mut |_, _| Ok(()))
    }

    /// [`Engine::read_snapshots`], calling `between(done, total)` before each chunk of 100 files
    /// and at the end; an error from it stops the read.
    pub fn read_snapshots_with(
        &mut self,
        paths: &[PathBuf],
        between: &mut dyn FnMut(usize, usize) -> Result<(), CoreError>,
    ) -> Result<Vec<Result<Snapshot, String>>, CoreError> {
        let mut results = Vec::with_capacity(paths.len());
        for chunk in paths.chunks(READ_CHUNK) {
            between(results.len(), paths.len())?;
            results.extend(self.read_chunk(chunk)?);
        }
        between(results.len(), paths.len())?;
        Ok(results)
    }

    /// [`Engine::read_snapshots_with`] with up to `readers` ExifTool sessions: this one plus
    /// temporary sessions started from the same configuration, each taking the next chunk of
    /// 100 files. Results come back in the order of `paths`. Small selections, where starting a
    /// session costs more than it saves, are read by this session alone.
    pub fn read_snapshots_parallel(
        &mut self,
        paths: &[PathBuf],
        readers: usize,
        between: &mut dyn FnMut(usize, usize) -> Result<(), CoreError>,
    ) -> Result<Vec<Result<Snapshot, String>>, CoreError> {
        let chunks: Vec<&[PathBuf]> = paths.chunks(READ_CHUNK).collect();
        let helpers = readers.min(chunks.len() / 2).saturating_sub(1);
        if helpers == 0 {
            return self.read_snapshots_with(paths, between);
        }
        let mut extra: Vec<Engine> = std::thread::scope(|s| {
            let started: Vec<_> = (0..helpers)
                .map(|_| s.spawn(|| Engine::start(self.cfg.clone())))
                .collect();
            started
                .into_iter()
                .filter_map(|h| h.join().ok().and_then(Result::ok))
                .collect()
        });
        let next = std::sync::atomic::AtomicUsize::new(0);
        let (tx, rx) = std::sync::mpsc::channel::<(usize, ChunkResult)>();
        let mut slots: Vec<Option<Vec<Result<Snapshot, String>>>> = vec![None; chunks.len()];
        let outcome = std::thread::scope(|s| {
            let mut sessions: Vec<&mut Engine> = extra.iter_mut().collect();
            sessions.push(self);
            for engine in sessions {
                let (tx, next, chunks) = (tx.clone(), &next, &chunks);
                s.spawn(move || {
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let Some(chunk) = chunks.get(i) else {
                            return;
                        };
                        let r = engine.read_chunk(chunk);
                        let failed = r.is_err();
                        if tx.send((i, r)).is_err() || failed {
                            return;
                        }
                    }
                });
            }
            drop(tx);
            let mut done = 0;
            between(0, paths.len())?;
            for (i, r) in rx {
                let r = r?;
                done += r.len();
                slots[i] = Some(r);
                if let Err(e) = between(done, paths.len()) {
                    // stop handing out chunks; the sessions finish the one they read
                    next.store(chunks.len(), std::sync::atomic::Ordering::SeqCst);
                    return Err(e);
                }
            }
            Ok(())
        });
        for e in extra {
            e.close();
        }
        outcome?;
        let mut results = Vec::with_capacity(paths.len());
        for s in slots {
            results.extend(s.ok_or_else(|| CoreError::Internal("a chunk was not read".into()))?);
        }
        Ok(results)
    }

    /// One ExifTool command for up to 100 files, results matched by `SourceFile`.
    fn read_chunk(&mut self, chunk: &[PathBuf]) -> ChunkResult {
        let mut c = Command::read_json();
        c.push(Line::option("-G1"));
        numeric_gps_then_all(&mut c);
        let mut bytes = 0u64;
        for p in chunk {
            c.push(path_line(p)?);
            bytes += std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        }
        let out = self.exec(&c, read_timeout(bytes))?;
        let arr = out.json().unwrap_or(serde_json::Value::Array(vec![]));
        let by_source: std::collections::HashMap<String, &serde_json::Value> = arr
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|o| Some((o.get("SourceFile")?.as_str()?.to_owned(), o)))
                    .collect()
            })
            .unwrap_or_default();
        Ok(chunk
            .iter()
            .map(|p| {
                let key = source_key(p).unwrap_or_default();
                match by_source.get(&key) {
                    Some(o) => Ok(Snapshot::from_json(o)),
                    None => Err(format!("no metadata result ({})", out.stderr_text().trim())),
                }
            })
            .collect())
    }

    /// Full read for verification: every tag (`-a -G1 -u`) plus `ImageDataHash` (SHA-256).
    pub fn read_full(&mut self, paths: &[&Path]) -> Result<Vec<Option<Snapshot>>, CoreError> {
        self.read_every_tag(paths, true)
    }

    /// Every tag (`-a -G1 -u`) for display (Inspector), without the image data hash.
    pub fn read_all_tags(&mut self, paths: &[&Path]) -> Result<Vec<Option<Snapshot>>, CoreError> {
        self.read_every_tag(paths, false)
    }

    fn read_every_tag(
        &mut self,
        paths: &[&Path],
        image_hash: bool,
    ) -> Result<Vec<Option<Snapshot>>, CoreError> {
        let mut c = Command::read_json();
        for o in ["-a", "-G1", "-u"] {
            c.push(Line::option(o));
        }
        if image_hash {
            for o in [
                "-api",
                "RequestTags=ImageDataHash",
                "-api",
                "ImageHashType=SHA256",
            ] {
                c.push(Line::option(o));
            }
        }
        numeric_gps_then_all(&mut c);
        let mut bytes = 0;
        for p in paths {
            c.push(path_line(p)?);
            bytes += std::fs::metadata(p).map(|m| m.len()).unwrap_or(0);
        }
        let out = self.exec(&c, read_timeout(bytes))?;
        let arr = out
            .json()
            .map_err(|e| CoreError::Engine(format!("unparsable ExifTool output: {e}")))?;
        let a = arr.as_array().cloned().unwrap_or_default();
        Ok(paths
            .iter()
            .map(|p| {
                let key = source_key(p)?;
                a.iter()
                    .find(|o| o.get("SourceFile").and_then(|s| s.as_str()) == Some(&key))
                    .map(Snapshot::from_json)
            })
            .collect())
    }

    /// `-ex <ops> -o <out> <source>`. `out` must not exist (`-o` never overwrites).
    pub fn write(&mut self, ops: &[TagOp], source: &Path, out: &Path) -> Result<Output, CoreError> {
        let mut c = Command::write();
        for op in ops {
            match op {
                TagOp::Set { tag: t, values } => {
                    let t = tag(t)?;
                    for v in values {
                        c.push(
                            Line::assign(&t, v)
                                .map_err(|e| CoreError::Input(format!("{}: {e}", t.as_str())))?,
                        );
                    }
                }
                TagOp::Delete { tag: t } => {
                    c.push(Line::delete(&tag(t)?));
                }
                TagOp::UpdateIptcDigest => {
                    // must follow the IPTC assignments in the same command
                    c.push(Line::option("-Photoshop:IPTCDigest=new"));
                }
            }
        }
        c.push(Line::option("-o"));
        c.push(path_line(out)?);
        c.push(path_line(source)?);
        let bytes = std::fs::metadata(source).map(|m| m.len()).unwrap_or(0);
        self.exec(&c, write_timeout(bytes))
    }

    /// `-ex <ops> -o <out>` without a source: a new file (e.g. an XMP sidecar) holding only these
    /// tags (SAFETY_MODEL §3.1, §4.3). `out` must not exist.
    pub fn write_new(&mut self, ops: &[TagOp], out: &Path) -> Result<Output, CoreError> {
        let mut c = Command::write();
        for op in ops {
            match op {
                TagOp::Set { tag: t, values } => {
                    let t = tag(t)?;
                    for v in values {
                        c.push(
                            Line::assign(&t, v)
                                .map_err(|e| CoreError::Input(format!("{}: {e}", t.as_str())))?,
                        );
                    }
                }
                // nothing to delete or digest in a file that does not exist yet
                TagOp::Delete { .. } | TagOp::UpdateIptcDigest => {}
            }
        }
        c.push(Line::option("-o"));
        c.push(path_line(out)?);
        self.exec(&c, write_timeout(0))
    }

    pub fn close(mut self) {
        if let Some(s) = self.session.take() {
            s.close(Duration::from_secs(2));
        }
    }
}
