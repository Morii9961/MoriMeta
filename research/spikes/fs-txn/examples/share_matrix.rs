//! S2-A: which lock-handle configuration lets ExifTool read and a commit succeed, while blocking
//! other writers?  S2-B: what does each commit method preserve?
//!
//! cargo run --release --example share_matrix -- <root-dir>      (NTFS path or \\localhost\E$\... for SMB)

use std::path::{Path, PathBuf};
use std::time::Duration;

use exiftool_session::{EngineConfig, Session, arg_path};
use fs_txn::*;
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::{GENERIC_READ, GENERIC_WRITE};
use windows_sys::Win32::Storage::FileSystem::{DELETE, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileAttributesW, SetFileAttributesW};

const FILE_ATTRIBUTE_NOT_CONTENT_INDEXED: u32 = 0x2000;

fn timages() -> PathBuf {
    let cfg = EngineConfig::research("launcher");
    let research = cfg.cwd.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
    research.join(".work/exiftool/13.59/src/Image-ExifTool-13.59/t/images")
}

fn code<T>(r: std::io::Result<T>) -> Value {
    match r {
        Ok(_) => json!("ok"),
        Err(e) => json!(e.raw_os_error().unwrap_or(-1)),
    }
}

fn res(r: Result<(), u32>) -> Value {
    match r {
        Ok(()) => json!("ok"),
        Err(c) => json!(c),
    }
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("root dir").replace('/', "\\"));
    let lab = root.join("s2-share");
    let _ = std::fs::remove_dir_all(&lab);
    std::fs::create_dir_all(&lab).unwrap();
    let src = timages().join("Writer.jpg");
    let mut sess = Session::spawn(&EngineConfig::research("launcher")).unwrap();

    let configs: Vec<(&str, Option<(u32, u32)>)> = vec![
        ("C0 no lock", None),
        ("C1 READ, share READ|DELETE (candidate)", Some((GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_DELETE))),
        ("C2 READ, share READ|WRITE|DELETE", Some((GENERIC_READ, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE))),
        ("C3 READ|DELETE, share READ|DELETE", Some((GENERIC_READ | DELETE, FILE_SHARE_READ | FILE_SHARE_DELETE))),
        ("C4 READ, share READ", Some((GENERIC_READ, FILE_SHARE_READ))),
    ];
    let mut out = serde_json::Map::new();
    for (ci, (name, cfg)) in configs.iter().enumerate() {
        let mut row = serde_json::Map::new();
        for (ti, test) in ["exiftool_read", "exiftool_write_temp_from_original", "other_writer_open", "other_rename", "replacefilew", "posix_rename"].iter().enumerate() {
            let d = lab.join(format!("c{ci}t{ti}"));
            std::fs::create_dir_all(&d).unwrap();
            let orig = d.join("orig.jpg");
            std::fs::copy(&src, &orig).unwrap();
            let h0 = hash_path(&orig).unwrap();
            let temp = d.join("orig.mmtmp.jpg");
            let bak = d.join("orig.mmbak.jpg");
            if matches!(*test, "replacefilew" | "posix_rename") {
                sess.execute(&["-XMP-dc:Creator=x".into(), "-o".into(), arg_path(&temp), arg_path(&orig)], Duration::from_secs(30)).unwrap();
            }
            let lock = cfg.map(|(a, s)| open_with(&orig, a, s).unwrap());
            let v: Value = match *test {
                "exiftool_read" => {
                    let r = sess.execute(&["-json".into(), "-FileName".into(), arg_path(&orig)], Duration::from_secs(30)).unwrap();
                    json!({"status": r.status, "stderr": r.err().trim()})
                }
                "exiftool_write_temp_from_original" => {
                    let r = sess.execute(&["-XMP-dc:Creator=x".into(), "-o".into(), arg_path(&temp), arg_path(&orig)], Duration::from_secs(30)).unwrap();
                    json!({"status": r.status, "temp_created": exists(&temp), "stderr": r.err().trim()})
                }
                "other_writer_open" => code(open_with(&orig, GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)),
                "other_rename" => {
                    let r = move_no_replace(&orig, &d.join("renamed.jpg"));
                    if r.is_ok() {
                        move_no_replace(&d.join("renamed.jpg"), &orig).unwrap();
                    }
                    res(r)
                }
                "replacefilew" => {
                    let r = replace_file(&orig, &temp, &bak);
                    let lock_reads_h0 = lock.as_ref().map(|l| {
                        let mut l2 = l.try_clone().unwrap();
                        use std::io::Seek;
                        l2.seek(std::io::SeekFrom::Start(0)).unwrap();
                        hash_reader(&mut l2).unwrap() == h0
                    });
                    json!({"result": res(r), "bak_is_h0": hash_path(&bak).map(|h| h == h0).unwrap_or(false), "lock_still_reads_h0": lock_reads_h0})
                }
                "posix_rename" => {
                    let r = posix_replace(&temp, &orig);
                    json!({"result": res(r), "orig_changed": hash_path(&orig).map(|h| h != h0).unwrap_or(false)})
                }
                _ => unreachable!(),
            };
            drop(lock);
            row.insert(test.to_string(), v);
        }
        out.insert(name.to_string(), Value::Object(row));
    }

    // S2-B preservation (no lock): creation time, ADS, attributes, File ID, for both commit methods.
    let mut pres = serde_json::Map::new();
    for method in ["ReplaceFileW", "PosixRename"] {
        let d = lab.join(format!("pres-{method}"));
        std::fs::create_dir_all(&d).unwrap();
        let orig = d.join("orig.jpg");
        std::fs::copy(&src, &orig).unwrap();
        std::fs::write(format!("{}:mm.test", orig.display()), b"ads-content").unwrap();
        let w = wide(&orig);
        unsafe { SetFileAttributesW(w.as_ptr(), GetFileAttributesW(w.as_ptr()) | FILE_ATTRIBUTE_NOT_CONTENT_INDEXED) };
        let created0 = std::fs::metadata(&orig).unwrap().created().unwrap();
        let fid0 = file_id_of_path(&orig).unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        let temp = d.join("orig.mmtmp.jpg");
        sess.execute(&["-XMP-dc:Creator=x".into(), "-o".into(), arg_path(&temp), arg_path(&orig)], Duration::from_secs(30)).unwrap();
        let fid_temp = file_id_of_path(&temp).unwrap();
        let r = if method == "ReplaceFileW" { replace_file(&orig, &temp, &d.join("orig.mmbak.jpg")) } else { posix_replace(&temp, &orig) };
        let created1 = std::fs::metadata(&orig).unwrap().created().unwrap();
        let ads = std::fs::read(format!("{}:mm.test", orig.display())).ok();
        let attrs = unsafe { GetFileAttributesW(wide(&orig).as_ptr()) };
        let fid1 = file_id_of_path(&orig).unwrap();
        pres.insert(method.into(), json!({
            "result": res(r),
            "creation_time_preserved": created0 == created1,
            "ads_preserved": ads.as_deref() == Some(&b"ads-content"[..]),
            "not_content_indexed_attr_preserved": attrs & FILE_ATTRIBUTE_NOT_CONTENT_INDEXED != 0,
            "file_id_after": if fid1 == fid0 {"original"} else if fid1 == fid_temp {"replacement"} else {"other"},
        }));
    }
    out.insert("preservation".into(), Value::Object(pres));
    sess.close(Duration::from_secs(2));
    let s = serde_json::to_string_pretty(&out).unwrap();
    println!("{s}");
    let research = EngineConfig::research("launcher").cwd.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
    let tag = if root.to_string_lossy().starts_with(r"\\") { "smb" } else { "ntfs" };
    std::fs::create_dir_all(research.join("results/s2")).unwrap();
    std::fs::write(research.join("results/s2").join(format!("share-matrix-{tag}.json")), s).unwrap();
    let _ = Path::new("");
}
