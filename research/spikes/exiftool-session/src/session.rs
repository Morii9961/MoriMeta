//! One `exiftool -stay_open True -@ -` process speaking a framed request/response protocol.
//!
//! Framing per command:  <lines…> / -echo4 / {mm-end:<ID>:${status}} / -execute<ID>
//! * stdout ends with `{ready<ID>}` on its own line, stderr with `{mm-end:<ID>:<status>}`.
//! * ID is a fresh random u64 per command, so file content cannot forge a terminator.
//! * stdin is written by a dedicated thread, stdout/stderr are drained by two threads:
//!   a hung ExifTool can never block the caller past the deadline, and a full stderr pipe
//!   can never stall ExifTool.
//! * The child runs in a Job Object with KILL_ON_JOB_CLOSE: if MoriMeta dies, ExifTool dies.

use std::io::{self, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_OUTPUT: usize = 256 << 20;

#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// `exiftool.exe` (renamed launcher) or `perl.exe` when `script` is set.
    pub program: PathBuf,
    /// `exiftool.pl` for the launcher-bypass candidate.
    pub script: Option<PathBuf>,
    /// Private empty working directory (no `.ExifTool_config` can be picked up).
    pub cwd: PathBuf,
    /// Private TEMP/TMP.
    pub temp: PathBuf,
}

impl EngineConfig {
    /// Locate the pinned research copy under research/.work (see research/scripts/fetch_exiftool.py).
    pub fn research(mode: &str) -> EngineConfig {
        let research = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("..").join("..");
        let research = research.canonicalize().unwrap_or(research);
        let lock: serde_json::Value = serde_json::from_str(
            &std::fs::read_to_string(research.join("exiftool.lock.json")).expect("lock file"),
        )
        .expect("lock json");
        let ver = lock["version"].as_str().expect("version").to_owned();
        let pkg = research
            .join(".work/exiftool")
            .join(&ver)
            .join("win64")
            .join(format!("exiftool-{ver}_64"));
        let run = research.join(".work/run");
        let cwd = run.join("exiftool-cwd");
        let temp = run.join("tmp");
        std::fs::create_dir_all(&cwd).ok();
        std::fs::create_dir_all(&temp).ok();
        match mode {
            "perl" => EngineConfig {
                program: pkg.join("exiftool_files").join("perl.exe"),
                script: Some(pkg.join("exiftool_files").join("exiftool.pl")),
                cwd,
                temp,
            },
            _ => {
                let exe = pkg.join("exiftool.exe");
                if !exe.exists() {
                    std::fs::copy(pkg.join("exiftool(-k).exe"), &exe).expect("copy launcher");
                }
                EngineConfig { program: exe, script: None, cwd, temp }
            }
        }
    }
}

#[derive(Debug)]
pub struct Response {
    pub id: u64,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `${status}` of the command: 0 ok, 1 error, 2 condition failed (per exiftool_pod).
    pub status: i32,
    pub elapsed: Duration,
}

impl Response {
    pub fn out(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn err(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
}

#[derive(Debug)]
pub enum SessionError {
    Spawn(io::Error),
    Dead,
    Timeout { after: Duration },
    Crashed { stdout: Vec<u8>, stderr: Vec<u8> },
    Desync(String),
    OutputTooLarge,
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::Crashed { stderr, .. } => {
                write!(f, "Crashed(stderr={:?})", String::from_utf8_lossy(stderr))
            }
            other => write!(f, "{other:?}"),
        }
    }
}

impl std::error::Error for SessionError {}

enum Frame {
    Marker { id: u64, data: Vec<u8>, status: Option<i32> },
    Eof { data: Vec<u8> },
    TooLarge,
}

struct Job(HANDLE);

// SAFETY: a job handle is a kernel object handle; it is only closed once, in Drop.
unsafe impl Send for Job {}

impl Job {
    fn kill_on_close_for(child: &Child) -> io::Result<Job> {
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            if SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) == 0
            {
                let e = io::Error::last_os_error();
                CloseHandle(job);
                return Err(e);
            }
            if AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) == 0 {
                let e = io::Error::last_os_error();
                CloseHandle(job);
                return Err(e);
            }
            Ok(Job(job))
        }
    }
}

impl Drop for Job {
    fn drop(&mut self) {
        unsafe {
            CloseHandle(self.0);
        }
    }
}

pub struct Session {
    child: Child,
    _job: Option<Job>,
    to_stdin: Option<Sender<Vec<u8>>>,
    out_rx: Receiver<Frame>,
    err_rx: Receiver<Frame>,
    dead: bool,
    expected: Arc<AtomicU64>,
    pub spawned_at: Instant,
}

/// Find `{<prefix><digits>…}` terminators at line starts; returns (start, end_after_newline, id, status).
fn find_marker(buf: &[u8], from: usize, prefix: &[u8], with_status: bool, expected: u64) -> Option<(usize, usize, u64, Option<i32>)> {
    let mut i = from;
    while let Some(off) = memchr_slice(&buf[i..], prefix) {
        let p = i + off;
        i = p + 1;
        // No line-start requirement: `-b` output has no trailing newline, so ExifTool's own
        // terminator can follow data directly. Unforgeability comes from the random id.
        let mut q = p + prefix.len();
        let d0 = q;
        while q < buf.len() && buf[q].is_ascii_digit() {
            q += 1;
        }
        if q == d0 || q - d0 > 20 {
            continue;
        }
        let Ok(id) = std::str::from_utf8(&buf[d0..q]).unwrap().parse::<u64>() else { continue };
        let mut status = None;
        if with_status {
            if q >= buf.len() || buf[q] != b':' {
                continue;
            }
            q += 1;
            let s0 = q;
            while q < buf.len() && (buf[q].is_ascii_digit() || buf[q] == b'-') {
                q += 1;
            }
            status = std::str::from_utf8(&buf[s0..q]).unwrap().parse::<i32>().ok();
        }
        if q >= buf.len() || buf[q] != b'}' {
            continue;
        }
        q += 1;
        if q < buf.len() && buf[q] == b'\r' {
            q += 1;
        }
        if q >= buf.len() {
            return None; // incomplete: wait for more bytes
        }
        if buf[q] != b'\n' {
            continue;
        }
        if id != expected {
            // a terminator carrying another id can only originate from file content: treat as data
            continue;
        }
        return Some((p, q + 1, id, status));
    }
    None
}

fn memchr_slice(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

fn spawn_reader(
    mut src: impl Read + Send + 'static,
    tx: Sender<Frame>,
    prefix: &'static [u8],
    with_status: bool,
    expected: Arc<AtomicU64>,
) {
    thread::spawn(move || {
        let mut buf: Vec<u8> = Vec::with_capacity(1 << 16);
        let mut chunk = vec![0u8; 1 << 16];
        let mut scan_from = 0usize;
        loop {
            let n = match src.read(&mut chunk) {
                Ok(0) | Err(_) => {
                    let _ = tx.send(Frame::Eof { data: std::mem::take(&mut buf) });
                    return;
                }
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            loop {
                match find_marker(&buf, scan_from, prefix, with_status, expected.load(Ordering::SeqCst)) {
                    Some((start, end, id, status)) => {
                        let data = buf[..start].to_vec();
                        buf.drain(..end);
                        scan_from = 0;
                        if tx.send(Frame::Marker { id, data, status }).is_err() {
                            return;
                        }
                    }
                    None => {
                        // rescan the tail next time in case a marker straddles reads
                        scan_from = buf.len().saturating_sub(64);
                        break;
                    }
                }
            }
            if buf.len() > MAX_OUTPUT {
                let _ = tx.send(Frame::TooLarge);
                buf.clear();
                scan_from = 0;
            }
        }
    });
}

impl Session {
    pub fn spawn(cfg: &EngineConfig) -> Result<Session, SessionError> {
        let mut cmd = Command::new(&cfg.program);
        if let Some(script) = &cfg.script {
            cmd.arg(script);
        }
        // -config "" must be first; -charset filename=utf8 must precede -@
        cmd.args(["-config", "", "-charset", "filename=utf8", "-stay_open", "True", "-@", "-"])
            .env_clear()
            .env("SystemRoot", std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()))
            .env("TEMP", &cfg.temp)
            .env("TMP", &cfg.temp)
            .current_dir(&cfg.cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd.spawn().map_err(SessionError::Spawn)?;
        let job = Job::kill_on_close_for(&child).ok();
        let stdin: ChildStdin = child.stdin.take().unwrap();
        let (to_stdin, stdin_rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut stdin = stdin;
            for payload in stdin_rx {
                if stdin.write_all(&payload).and_then(|_| stdin.flush()).is_err() {
                    return;
                }
            }
        });
        let (out_tx, out_rx) = mpsc::channel();
        let (err_tx, err_rx) = mpsc::channel();
        let expected = Arc::new(AtomicU64::new(0));
        spawn_reader(child.stdout.take().unwrap(), out_tx, b"{ready", false, expected.clone());
        spawn_reader(child.stderr.take().unwrap(), err_tx, b"{mm-end:", true, expected.clone());
        Ok(Session {
            child,
            _job: job,
            to_stdin: Some(to_stdin),
            out_rx,
            err_rx,
            dead: false,
            expected,
            spawned_at: Instant::now(),
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn raw_process_handle(&self) -> HANDLE {
        self.child.as_raw_handle() as HANDLE
    }

    pub fn is_dead(&self) -> bool {
        self.dead
    }

    /// Execute one command. `lines` must already be encoded (see `encode`).
    pub fn execute(&mut self, lines: &[String], timeout: Duration) -> Result<Response, SessionError> {
        if self.dead {
            return Err(SessionError::Dead);
        }
        let id: u64 = loop {
            let v = rand::random::<u64>();
            if v != 0 {
                break v;
            }
        };
        let mut payload = String::new();
        for l in lines {
            debug_assert!(!l.contains(['\n', '\r']) || l.starts_with("#[CSTR]") && !l.contains('\n'));
            payload.push_str(l);
            payload.push('\n');
        }
        payload.push_str(&format!("-echo4\n{{mm-end:{id}:${{status}}}}\n-execute{id}\n"));
        self.expected.store(id, Ordering::SeqCst);
        let t0 = Instant::now();
        let deadline = t0 + timeout;
        if self.to_stdin.as_ref().unwrap().send(payload.into_bytes()).is_err() {
            self.kill();
            return Err(SessionError::Crashed { stdout: vec![], stderr: vec![] });
        }
        let stdout = match self.wait_frame(true, id, deadline) {
            Ok((d, _)) => d,
            Err(e) => {
                self.kill();
                return Err(e);
            }
        };
        let (stderr, status) = match self.wait_frame(false, id, deadline) {
            Ok(v) => v,
            Err(e) => {
                self.kill();
                return Err(e);
            }
        };
        Ok(Response { id, stdout, stderr, status: status.unwrap_or(-1), elapsed: t0.elapsed() })
    }

    fn wait_frame(&self, stdout: bool, id: u64, deadline: Instant) -> Result<(Vec<u8>, Option<i32>), SessionError> {
        let rx = if stdout { &self.out_rx } else { &self.err_rx };
        let now = Instant::now();
        let left = deadline.saturating_duration_since(now);
        match rx.recv_timeout(left) {
            Ok(Frame::Marker { id: got, data, status }) if got == id => Ok((data, status)),
            Ok(Frame::Marker { id: got, .. }) => Err(SessionError::Desync(format!("expected {id}, got {got}"))),
            Ok(Frame::Eof { data }) => {
                let other = if stdout { &self.err_rx } else { &self.out_rx };
                let other_data = match other.recv_timeout(Duration::from_millis(500)) {
                    Ok(Frame::Eof { data }) | Ok(Frame::Marker { data, .. }) => data,
                    _ => vec![],
                };
                let (so, se) = if stdout { (data, other_data) } else { (other_data, data) };
                Err(SessionError::Crashed { stdout: so, stderr: se })
            }
            Ok(Frame::TooLarge) => Err(SessionError::OutputTooLarge),
            Err(RecvTimeoutError::Timeout) => Err(SessionError::Timeout { after: deadline.duration_since(deadline - left) }),
            Err(RecvTimeoutError::Disconnected) => Err(SessionError::Crashed { stdout: vec![], stderr: vec![] }),
        }
    }

    pub fn kill(&mut self) {
        self.dead = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.to_stdin = None;
    }

    /// Ask ExifTool to exit; kill if it does not within `grace`.
    pub fn close(mut self, grace: Duration) {
        if !self.dead {
            if let Some(tx) = &self.to_stdin {
                let _ = tx.send(b"-stay_open\nFalse\n".to_vec());
            }
            let t0 = Instant::now();
            while t0.elapsed() < grace {
                if let Ok(Some(_)) = self.child.try_wait() {
                    self.dead = true;
                    return;
                }
                thread::sleep(Duration::from_millis(20));
            }
            self.kill();
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if !self.dead {
            self.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::find_marker;

    #[test]
    fn marker_requires_line_start_and_newline() {
        let b = b"x{ready5}\n{ready6}\nz{ready7}\r\nrest";
        let (s, e, id, _) = find_marker(b, 0, b"{ready", false, 7).unwrap();
        assert_eq!((s, id), (20, 7));
        assert_eq!(&b[e..], b"rest");
        assert!(find_marker(b"{ready12}", 0, b"{ready", false, 12).is_none());
        assert!(find_marker(b"{ready12}\n", 0, b"{ready", false, 13).is_none());
        let (_, _, id, st) = find_marker(b"w\n{mm-end:9:1}\n", 0, b"{mm-end:", true, 9).unwrap();
        assert_eq!((id, st), (9, Some(1)));
    }
}
