// SPDX-License-Identifier: GPL-3.0-or-later
//! The program's log file (ARCHITECTURE §12, SECURITY_MODEL §8): one file per day in
//! `<data>/logs`, kept for 7 days and 50 MB at most. It names Operations by id and files as
//! `asset#n.ext`; it never records metadata values, GPS, full paths or the user's name. Callers
//! pass only such fields; error texts are scrubbed with the file's path before they are written.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

const KEEP_DAYS: i64 = 7;
const KEEP_BYTES: u64 = 50 << 20;

struct Sink {
    dir: PathBuf,
    day: i64,
    file: std::fs::File,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);

fn now_ms() -> i64 {
    mm_store::now_ms()
}

/// Civil date (UTC) of a day number since 1970-01-01.
fn civil(days: i64) -> (i64, u32, u32) {
    // H. Hinnant, "chrono-Compatible Low-Level Date Algorithms"
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (yoe + era * 400 + i64::from(m <= 2), m, d)
}

fn file_name(day: i64) -> String {
    let (y, m, d) = civil(day);
    format!("morimeta-{y:04}-{m:02}-{d:02}.log")
}

fn open_day(dir: &Path, day: i64) -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join(file_name(day)))
}

/// Start logging into `<data_dir>/logs` and apply the retention limits. Logging is best effort:
/// failing to write it never stops an Operation.
pub fn init(data_dir: &Path) -> std::io::Result<()> {
    let dir = data_dir.join("logs");
    std::fs::create_dir_all(&dir)?;
    prune(&dir, now_ms().div_euclid(86_400_000))?;
    let day = now_ms().div_euclid(86_400_000);
    let file = open_day(&dir, day)?;
    *SINK.lock().unwrap_or_else(|p| p.into_inner()) = Some(Sink { dir, day, file });
    Ok(())
}

/// Delete log files older than 7 days, then the oldest until the rest fit in 50 MB.
fn prune(dir: &Path, today: i64) -> std::io::Result<()> {
    let mut logs: Vec<(String, u64)> = std::fs::read_dir(dir)?
        .filter_map(|e| {
            let e = e.ok()?;
            let name = e.file_name().to_string_lossy().into_owned();
            (name.starts_with("morimeta-") && name.ends_with(".log"))
                .then(|| Some((name, e.metadata().ok()?.len())))?
        })
        .collect();
    logs.sort(); // the date in the name sorts oldest first
    let oldest_kept = file_name(today - (KEEP_DAYS - 1));
    let mut total: u64 = logs.iter().map(|(_, n)| n).sum();
    for (name, len) in &logs {
        if name.as_str() < oldest_kept.as_str() || total > KEEP_BYTES {
            std::fs::remove_file(dir.join(name))?;
            total -= len;
        }
    }
    Ok(())
}

/// One line: time, level, event, then `key=value` fields. Values must already be safe to log.
pub fn event(level: &str, what: &str, fields: &[(&str, &dyn std::fmt::Display)]) {
    let mut guard = SINK.lock().unwrap_or_else(|p| p.into_inner());
    let Some(sink) = guard.as_mut() else {
        return;
    };
    let now = now_ms();
    let day = now.div_euclid(86_400_000);
    if day != sink.day
        && let Ok(f) = open_day(&sink.dir, day)
    {
        sink.file = f;
        sink.day = day;
        let _ = prune(&sink.dir, day);
    }
    let ms = now.rem_euclid(86_400_000);
    let mut line = format!(
        "{:02}:{:02}:{:02}.{:03}Z {level} {what}",
        ms / 3_600_000,
        ms / 60_000 % 60,
        ms / 1000 % 60,
        ms % 1000
    );
    for (k, v) in fields {
        let v = v.to_string().replace(['\r', '\n'], " ");
        line.push_str(&format!(" {k}={v:?}"));
    }
    line.push('\n');
    let _ = sink.file.write_all(line.as_bytes());
}

/// An error text with the file's path (and folder) replaced by its alias and the user's name
/// removed.
pub fn scrub_for(text: &str, n: u32, path: &str) -> String {
    let mut known = vec![(path.to_owned(), crate::privacy::alias(n, path))];
    if let Some(parent) = Path::new(path).parent() {
        known.push((parent.to_string_lossy().into_owned(), "<folder>".into()));
    }
    crate::privacy::scrub(text, &known)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_retention() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(19_782), (2024, 2, 29));
        assert_eq!(file_name(20_724), "morimeta-2026-09-28.log");
        let d = std::env::temp_dir().join(format!("mm-log-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        for day in [20_700, 20_717, 20_718, 20_724] {
            std::fs::write(d.join(file_name(day)), b"x").unwrap();
        }
        std::fs::write(d.join("other.txt"), b"x").unwrap();
        prune(&d, 20_724).unwrap();
        let mut left: Vec<String> = std::fs::read_dir(&d)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "morimeta-2026-09-22.log",
                "morimeta-2026-09-28.log",
                "other.txt"
            ]
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
