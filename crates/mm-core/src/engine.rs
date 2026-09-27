//! ExifTool engine wrapper used by the application services: respawns a dead session, applies
//! size-based timeouts (ARCHITECTURE §7.2) and turns typed `TagOp`s into validated argfile lines.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mm_domain::plan::TagOp;
use mm_domain::snapshot::Snapshot;
use mm_exiftool::{Command, EngineConfig, EngineError, Line, Output, Session, TagName};

use crate::CoreError;

pub struct Engine {
    cfg: EngineConfig,
    session: Option<Session>,
    version: String,
    restarts: u32,
}

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

    fn session(&mut self) -> Result<&mut Session, CoreError> {
        if self.session.as_ref().map(|s| s.is_dead()).unwrap_or(true) {
            if self.session.is_some() {
                self.restarts += 1;
            }
            self.session =
                Some(Session::spawn(&self.cfg).map_err(|e| CoreError::Engine(e.to_string()))?);
        }
        Ok(self.session.as_mut().expect("session"))
    }

    pub fn exec(&mut self, cmd: &Command, timeout: Duration) -> Result<Output, CoreError> {
        self.session()?.execute(cmd, timeout).map_err(|e| match e {
            EngineError::Timeout { .. } => CoreError::Engine("ExifTool timed out".into()),
            other => CoreError::Engine(other.to_string()),
        })
    }

    /// Read the tags used for planning (`-G1`, all values quoted). Results are matched to the
    /// request by `SourceFile`; a file without a result is reported as an error.
    pub fn read_snapshots(
        &mut self,
        paths: &[PathBuf],
    ) -> Result<Vec<Result<Snapshot, String>>, CoreError> {
        let mut results = Vec::with_capacity(paths.len());
        for chunk in paths.chunks(100) {
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
            for p in chunk {
                let key = source_key(p).unwrap_or_default();
                results.push(match by_source.get(&key) {
                    Some(o) => Ok(Snapshot::from_json(o)),
                    None => Err(format!("no metadata result ({})", out.stderr_text().trim())),
                });
            }
        }
        Ok(results)
    }

    /// Full read for verification: every tag (`-a -G1 -u`) plus `ImageDataHash` (SHA-256).
    pub fn read_full(&mut self, paths: &[&Path]) -> Result<Vec<Option<Snapshot>>, CoreError> {
        let mut c = Command::read_json();
        for o in [
            "-a",
            "-G1",
            "-u",
            "-api",
            "RequestTags=ImageDataHash",
            "-api",
            "ImageHashType=SHA256",
        ] {
            c.push(Line::option(o));
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

    pub fn close(mut self) {
        if let Some(s) = self.session.take() {
            s.close(Duration::from_secs(2));
        }
    }
}
