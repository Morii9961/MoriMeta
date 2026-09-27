//! One `exiftool -stay_open True -@ -` process (ARCHITECTURE §7.1–7.2, ADR-11).
//!
//! * Each command ends with `-echo4 {mm-end:<ID>:${status}}` and `-execute<ID>`, where ID is a
//!   fresh random u64. Only terminators carrying the current ID are accepted; anything else is
//!   data (file content may contain `{ready1}`). Terminators are not required to start a line:
//!   `-b` output has no trailing newline.
//! * stdin is written by a dedicated thread; stdout and stderr are drained by two threads. A hung
//!   ExifTool can never block the caller past the deadline; a full stderr pipe never stalls it.
//! * The child runs in a Job Object with KILL_ON_JOB_CLOSE.

use std::io::{self, Read, Write};
use std::os::windows::io::AsRawHandle;
use std::os::windows::process::CommandExt;
use std::path::PathBuf;
use std::process::{Child, ChildStdin, Stdio};
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

use crate::encode::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
/// Output above this size is treated as abnormal (SECURITY_MODEL §4.2).
const MAX_OUTPUT: usize = 256 << 20;

/// How to start ExifTool (ARCHITECTURE ADR-03; the choice between A and B is D-17).
#[derive(Debug, Clone)]
pub struct EngineConfig {
    /// A: the renamed launcher `exiftool.exe`; B: `exiftool_files\perl.exe`.
    pub program: PathBuf,
    /// B only: `exiftool_files\exiftool.pl`, passed as the first argument.
    pub script: Option<PathBuf>,
    /// Private, empty working directory (no `.ExifTool_config` can be picked up).
    pub cwd: PathBuf,
    /// Private TEMP/TMP.
    pub temp: PathBuf,
}

#[derive(Debug)]
pub struct Output {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// `${status}`: 0 ok, 1 error, 2 condition failed; -1 if absent.
    pub status: i32,
    pub elapsed: Duration,
}

impl Output {
    pub fn stdout_text(&self) -> String {
        String::from_utf8_lossy(&self.stdout).into_owned()
    }
    pub fn stderr_text(&self) -> String {
        String::from_utf8_lossy(&self.stderr).into_owned()
    }
    /// Parse `-json` output (use [`Command::read_json`], which quotes every value).
    pub fn json(&self) -> Result<serde_json::Value, serde_json::Error> {
        serde_json::from_slice(&self.stdout)
    }
}

#[derive(Debug)]
pub enum EngineError {
    Spawn(io::Error),
    Random(String),
    /// The session was killed earlier (timeout, crash); spawn a new one.
    Dead,
    Timeout {
        after: Duration,
    },
    Crashed {
        stdout: Vec<u8>,
        stderr: Vec<u8>,
    },
    OutputTooLarge,
}

impl std::fmt::Display for EngineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EngineError::Crashed { stderr, .. } => write!(
                f,
                "ExifTool exited unexpectedly: {}",
                String::from_utf8_lossy(stderr).trim()
            ),
            other => write!(f, "{other:?}"),
        }
    }
}

impl std::error::Error for EngineError {}

enum Frame {
    Marker { data: Vec<u8>, status: Option<i32> },
    Eof { data: Vec<u8> },
    TooLarge,
}

struct Job(HANDLE);

// SAFETY: a job object handle may be used and closed from any thread; it is closed once, in Drop.
unsafe impl Send for Job {}

impl Job {
    fn kill_on_close_for(child: &Child) -> io::Result<Job> {
        // SAFETY: plain Win32 calls on handles we own; `info` outlives the call that reads it.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const _,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            ) != 0
                && AssignProcessToJobObject(job, child.as_raw_handle() as HANDLE) != 0;
            if !ok {
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
        // SAFETY: the handle was created by CreateJobObjectW and is closed exactly once.
        unsafe {
            CloseHandle(self.0);
        }
    }
}

/// Locate `{<prefix><expected>…}` in `buf[from..]`. Returns (start, end) of the terminator
/// including its line break, and the status for stderr terminators.
pub(crate) fn find_terminator(
    buf: &[u8],
    from: usize,
    prefix: &[u8],
    with_status: bool,
    expected: u64,
) -> Option<(usize, usize, Option<i32>)> {
    let mut i = from;
    while i + prefix.len() <= buf.len() {
        let off = buf[i..].windows(prefix.len()).position(|w| w == prefix)?;
        let p = i + off;
        i = p + 1;
        let mut q = p + prefix.len();
        let d0 = q;
        while q < buf.len() && buf[q].is_ascii_digit() && q - d0 < 20 {
            q += 1;
        }
        if q == d0
            || std::str::from_utf8(&buf[d0..q])
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                != Some(expected)
        {
            continue;
        }
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
            status = std::str::from_utf8(&buf[s0..q])
                .ok()
                .and_then(|s| s.parse().ok());
        }
        if q >= buf.len() {
            return None; // terminator incomplete; wait for more bytes
        }
        if buf[q] != b'}' {
            continue;
        }
        q += 1;
        if q < buf.len() && buf[q] == b'\r' {
            q += 1;
        }
        if q >= buf.len() {
            return None;
        }
        if buf[q] != b'\n' {
            continue;
        }
        return Some((p, q + 1, status));
    }
    None
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
                    let _ = tx.send(Frame::Eof {
                        data: std::mem::take(&mut buf),
                    });
                    return;
                }
                Ok(n) => n,
            };
            buf.extend_from_slice(&chunk[..n]);
            while let Some((start, end, status)) = find_terminator(
                &buf,
                scan_from,
                prefix,
                with_status,
                expected.load(Ordering::SeqCst),
            ) {
                let data = buf[..start].to_vec();
                buf.drain(..end);
                scan_from = 0;
                if tx.send(Frame::Marker { data, status }).is_err() {
                    return;
                }
            }
            // a terminator is < 64 bytes; rescan only the tail next time
            scan_from = buf.len().saturating_sub(64);
            if buf.len() > MAX_OUTPUT {
                let _ = tx.send(Frame::TooLarge);
                buf.clear();
                scan_from = 0;
            }
        }
    });
}

/// See [`Session::terminator`].
#[derive(Debug)]
pub struct Terminator(std::os::windows::io::OwnedHandle);

impl Terminator {
    /// End the process. ExifTool only ever writes to temporary files (I-1), so this is safe at
    /// any point; the session notices and is replaced on its next use.
    pub fn terminate(&self) {
        use windows_sys::Win32::System::Threading::TerminateProcess;
        // SAFETY: a valid process handle owned by self; failure (already exited) is harmless.
        unsafe {
            TerminateProcess(self.0.as_raw_handle() as HANDLE, 1);
        }
    }
}

pub struct Session {
    child: Child,
    _job: Option<Job>,
    to_stdin: Option<Sender<Vec<u8>>>,
    out_rx: Receiver<Frame>,
    err_rx: Receiver<Frame>,
    expected: Arc<AtomicU64>,
    dead: bool,
}

impl Session {
    pub fn spawn(cfg: &EngineConfig) -> Result<Session, EngineError> {
        let mut cmd = std::process::Command::new(&cfg.program);
        if let Some(script) = &cfg.script {
            cmd.arg(script);
        }
        // -config "" must be the first ExifTool argument; -charset filename=utf8 must precede -@
        cmd.args([
            "-config",
            "",
            "-charset",
            "filename=utf8",
            "-stay_open",
            "True",
            "-@",
            "-",
        ])
        .env_clear()
        .env(
            "SystemRoot",
            std::env::var_os("SystemRoot").unwrap_or_else(|| "C:\\Windows".into()),
        )
        .env("TEMP", &cfg.temp)
        .env("TMP", &cfg.temp)
        .current_dir(&cfg.cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .creation_flags(CREATE_NO_WINDOW);
        let mut child = cmd.spawn().map_err(EngineError::Spawn)?;
        let job = match Job::kill_on_close_for(&child) {
            Ok(j) => Some(j),
            Err(e) => {
                let _ = child.kill();
                return Err(EngineError::Spawn(e));
            }
        };
        let stdin: ChildStdin = child.stdin.take().expect("piped stdin");
        let (to_stdin, stdin_rx) = mpsc::channel::<Vec<u8>>();
        thread::spawn(move || {
            let mut stdin = stdin;
            for payload in stdin_rx {
                if stdin
                    .write_all(&payload)
                    .and_then(|_| stdin.flush())
                    .is_err()
                {
                    return;
                }
            }
        });
        let expected = Arc::new(AtomicU64::new(0));
        let (out_tx, out_rx) = mpsc::channel();
        let (err_tx, err_rx) = mpsc::channel();
        spawn_reader(
            child.stdout.take().expect("piped stdout"),
            out_tx,
            b"{ready",
            false,
            expected.clone(),
        );
        spawn_reader(
            child.stderr.take().expect("piped stderr"),
            err_tx,
            b"{mm-end:",
            true,
            expected.clone(),
        );
        Ok(Session {
            child,
            _job: job,
            to_stdin: Some(to_stdin),
            out_rx,
            err_rx,
            expected,
            dead: false,
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Dead after a failure here, or because the process has exited (for example ended by a
    /// [`Terminator`] from another thread).
    pub fn is_dead(&mut self) -> bool {
        if !self.dead && !matches!(self.child.try_wait(), Ok(None)) {
            self.dead = true;
        }
        self.dead
    }

    /// Run one command. On timeout, crash or oversize output the process is killed and the
    /// session becomes dead; ExifTool only ever writes new temporary files, so this is safe.
    pub fn execute(&mut self, cmd: &Command, timeout: Duration) -> Result<Output, EngineError> {
        if self.dead {
            return Err(EngineError::Dead);
        }
        let id = loop {
            let v = getrandom::u64().map_err(|e| EngineError::Random(e.to_string()))?;
            if v != 0 {
                break v;
            }
        };
        self.expected.store(id, Ordering::SeqCst);
        let t0 = Instant::now();
        let deadline = t0 + timeout;
        let sent = self
            .to_stdin
            .as_ref()
            .map(|tx| tx.send(cmd.render(id)).is_ok())
            .unwrap_or(false);
        if !sent {
            self.kill();
            return Err(EngineError::Crashed {
                stdout: vec![],
                stderr: vec![],
            });
        }
        let result = self.wait(true, deadline).and_then(|(stdout, _)| {
            self.wait(false, deadline)
                .map(|(stderr, st)| (stdout, stderr, st))
        });
        match result {
            Ok((stdout, stderr, status)) => Ok(Output {
                stdout,
                stderr,
                status: status.unwrap_or(-1),
                elapsed: t0.elapsed(),
            }),
            Err(e) => {
                self.kill();
                Err(e)
            }
        }
    }

    fn wait(&self, stdout: bool, deadline: Instant) -> Result<(Vec<u8>, Option<i32>), EngineError> {
        let rx = if stdout { &self.out_rx } else { &self.err_rx };
        let left = deadline.saturating_duration_since(Instant::now());
        match rx.recv_timeout(left) {
            Ok(Frame::Marker { data, status }) => Ok((data, status)),
            Ok(Frame::TooLarge) => Err(EngineError::OutputTooLarge),
            Ok(Frame::Eof { data }) => {
                let other = if stdout { &self.err_rx } else { &self.out_rx };
                let other_data = match other.recv_timeout(Duration::from_millis(500)) {
                    Ok(Frame::Eof { data }) | Ok(Frame::Marker { data, .. }) => data,
                    _ => vec![],
                };
                let (so, se) = if stdout {
                    (data, other_data)
                } else {
                    (other_data, data)
                };
                Err(EngineError::Crashed {
                    stdout: so,
                    stderr: se,
                })
            }
            Err(RecvTimeoutError::Timeout) => Err(EngineError::Timeout { after: left }),
            Err(RecvTimeoutError::Disconnected) => Err(EngineError::Crashed {
                stdout: vec![],
                stderr: vec![],
            }),
        }
    }

    /// A handle that can end this ExifTool process from another thread (a user's Cancel,
    /// SAFETY_MODEL §11). It holds its own process handle, so a reused process id is never hit.
    pub fn terminator(&self) -> io::Result<Terminator> {
        use std::os::windows::io::AsHandle;
        Ok(Terminator(self.child.as_handle().try_clone_to_owned()?))
    }

    pub fn kill(&mut self) {
        self.dead = true;
        let _ = self.child.kill();
        let _ = self.child.wait();
        self.to_stdin = None;
    }

    /// Ask ExifTool to exit; kill it if it has not exited within `grace`.
    pub fn close(mut self, grace: Duration) {
        if self.dead {
            return;
        }
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

impl Drop for Session {
    fn drop(&mut self) {
        if !self.dead {
            self.kill();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::find_terminator;

    #[test]
    fn only_the_expected_id_terminates() {
        let b = b"x{ready5}\n{ready6}\nz{ready7}\r\nrest";
        let (s, e, _) = find_terminator(b, 0, b"{ready", false, 7).unwrap();
        assert_eq!(s, 20);
        assert_eq!(&b[e..], b"rest");
        assert!(find_terminator(b, 0, b"{ready", false, 8).is_none());
    }

    #[test]
    fn incomplete_terminator_waits() {
        assert!(find_terminator(b"{ready12}", 0, b"{ready", false, 12).is_none());
        assert!(find_terminator(b"{ready12", 0, b"{ready", false, 12).is_none());
    }

    #[test]
    fn stderr_terminator_carries_status() {
        let (_, _, st) =
            find_terminator(b"Warning: x\n{mm-end:9:1}\n", 0, b"{mm-end:", true, 9).unwrap();
        assert_eq!(st, Some(1));
    }
}
