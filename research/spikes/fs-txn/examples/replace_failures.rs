//! S2-D: induced ReplaceFileW failures (what is on disk afterwards?) and synthetic recovery states
//! from SAFETY_MODEL §10 that a crash cannot reliably produce.
//!
//! cargo run --release --example replace_failures -- <root> [other-volume-dir]

use std::path::{Path, PathBuf};
use std::time::Duration;

use exiftool_session::{EngineConfig, Session, arg_path};
use fs_txn::txn::{self, Journal, PlanItem};
use fs_txn::*;
use serde_json::{Value, json};
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Storage::FileSystem::{
    CreateHardLinkW, FILE_ATTRIBUTE_READONLY, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileAttributesW, SetFileAttributesW,
};

fn research() -> PathBuf {
    EngineConfig::research("launcher").cwd.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf()
}

fn state(orig: &Path, temp: &Path, bak: &Path, h0: &str, h1: &str) -> Value {
    let label = |p: &Path| match hash_path(p) {
        Ok(h) if hex(&h) == h0 => "H0".to_string(),
        Ok(h) if hex(&h) == h1 => "H1".to_string(),
        Ok(_) => "other".to_string(),
        Err(_) => "missing".to_string(),
    };
    json!({"original": label(orig), "temp": label(temp), "bak": label(bak)})
}

fn main() {
    let root = PathBuf::from(std::env::args().nth(1).expect("root").replace('/', "\\"));
    let other_vol = std::env::args().nth(2).map(|s| PathBuf::from(s.replace('/', "\\")));
    let lab = root.join("s2-failures");
    let _ = std::fs::remove_dir_all(&lab);
    std::fs::create_dir_all(&lab).unwrap();
    let src = research().join(".work/exiftool/13.59/src/Image-ExifTool-13.59/t/images/Writer.jpg");
    let mut sess = Session::spawn(&EngineConfig::research("launcher")).unwrap();
    let mut out = serde_json::Map::new();

    let mk = |sess: &mut Session, name: &str, temp_dir: Option<&Path>| {
        let d = lab.join(name);
        std::fs::create_dir_all(&d).unwrap();
        let orig = d.join("orig.jpg");
        std::fs::copy(&src, &orig).unwrap();
        let temp = temp_dir.map(|t| { std::fs::create_dir_all(t).unwrap(); t.join(format!("{name}.mmtmp.jpg")) }).unwrap_or(d.join("orig.mmtmp.jpg"));
        let _ = std::fs::remove_file(&temp);
        sess.execute(&["-XMP-dc:Creator=x".into(), "-o".into(), arg_path(&temp), arg_path(&orig)], Duration::from_secs(30)).unwrap();
        let bak = d.join("orig.mmbak.jpg");
        let h0 = hex(&hash_path(&orig).unwrap());
        let h1 = hex(&hash_path(&temp).unwrap());
        (orig, temp, bak, h0, h1)
    };

    // F1/F2: original open elsewhere without FILE_SHARE_DELETE (typical viewer/editor handles)
    for (name, share) in [("F1 original open, share READ", FILE_SHARE_READ), ("F2 original open, share READ|WRITE", FILE_SHARE_READ | FILE_SHARE_WRITE)] {
        let (orig, temp, bak, h0, h1) = mk(&mut sess, &name[..2], None);
        let h = open_with(&orig, GENERIC_READ, share).unwrap();
        let r = replace_file(&orig, &temp, &bak);
        let st = state(&orig, &temp, &bak, &h0, &h1);
        drop(h);
        out.insert(name.into(), json!({"result": format!("{r:?}"), "disk": st}));
    }
    // F3: replacement (temp) open elsewhere without FILE_SHARE_DELETE
    {
        let (orig, temp, bak, h0, h1) = mk(&mut sess, "F3", None);
        let h = open_with(&temp, GENERIC_READ, FILE_SHARE_READ).unwrap();
        let r = replace_file(&orig, &temp, &bak);
        let st = state(&orig, &temp, &bak, &h0, &h1);
        drop(h);
        out.insert("F3 temp open, share READ".into(), json!({"result": format!("{r:?}"), "disk": st}));
    }
    // F4: read-only original
    {
        let (orig, temp, bak, h0, h1) = mk(&mut sess, "F4", None);
        let w = wide(&orig);
        unsafe { SetFileAttributesW(w.as_ptr(), GetFileAttributesW(w.as_ptr()) | FILE_ATTRIBUTE_READONLY) };
        let r = replace_file(&orig, &temp, &bak);
        let st = state(&orig, &temp, &bak, &h0, &h1);
        let ro_after = unsafe { GetFileAttributesW(wide(&orig).as_ptr()) } & FILE_ATTRIBUTE_READONLY != 0;
        for p in [&orig, &bak] {
            let w = wide(p);
            unsafe { SetFileAttributesW(w.as_ptr(), GetFileAttributesW(w.as_ptr()) & !FILE_ATTRIBUTE_READONLY) };
        }
        out.insert("F4 read-only original".into(), json!({"result": format!("{r:?}"), "disk": st, "readonly_attr_after": ro_after}));
    }
    // F5: backup name already exists
    {
        let (orig, temp, bak, h0, h1) = mk(&mut sess, "F5", None);
        std::fs::write(&bak, b"pre-existing").unwrap();
        let r = replace_file(&orig, &temp, &bak);
        let st = state(&orig, &temp, &bak, &h0, &h1);
        out.insert("F5 backup name exists".into(), json!({"result": format!("{r:?}"), "disk": st, "pre_existing_bak_overwritten": std::fs::read(&bak).map(|b| b != b"pre-existing").unwrap_or(true)}));
    }
    // F6: replacement on another volume
    if let Some(ov) = &other_vol {
        let (orig, temp, bak, h0, h1) = mk(&mut sess, "F6", Some(ov));
        let r = replace_file(&orig, &temp, &bak);
        let st = state(&orig, &temp, &bak, &h0, &h1);
        let _ = std::fs::remove_file(&temp);
        out.insert("F6 temp on other volume".into(), json!({"result": format!("{r:?}"), "disk": st}));
    }
    // F7: hard-linked original (link count 2)
    {
        let (orig, temp, bak, h0, h1) = mk(&mut sess, "F7", None);
        let link = orig.with_file_name("link.jpg");
        let ok = unsafe { CreateHardLinkW(wide(&link).as_ptr(), wide(&orig).as_ptr(), std::ptr::null()) } != 0;
        let links = link_count(&open_attr(&orig).unwrap()).unwrap();
        let r = replace_file(&orig, &temp, &bak);
        let st = state(&orig, &temp, &bak, &h0, &h1);
        let link_state = state(&link, &temp, &bak, &h0, &h1)["original"].clone();
        out.insert("F7 hard-linked original".into(), json!({"link_created": ok, "link_count": links, "result": format!("{r:?}"), "disk": st, "other_link_content": link_state}));
    }

    // Synthetic recovery states (SAFETY_MODEL §10)
    let mut syn = serde_json::Map::new();
    let cases: Vec<(&str, &str, &str, &str, &str)> = vec![
        // name, journal last record, original, temp, bak
        ("S1 ready, orig=H0, temp present", "ready", "H0", "H1", "-"),
        ("S2 ready, orig=H1, bak=H0", "ready", "H1", "-", "H0"),
        ("S3 ready, orig missing, bak=H0, temp=H1", "ready", "-", "H1", "H0"),
        ("S4 committed, orig=H0 (rolled back)", "committed", "H0", "-", "-"),
        ("S5 ready, orig externally modified", "ready", "X", "H1", "-"),
        ("S6 ready, orig missing, no bak", "ready", "-", "H1", "-"),
        ("S7 backed_up, orig=H0, temp present", "backed_up", "H0", "H1", "-"),
    ];
    for (i, (name, last, o, t, b)) in cases.iter().enumerate() {
        let d = lab.join(format!("syn{i}"));
        std::fs::create_dir_all(&d).unwrap();
        let (orig0, temp0, _, h0, h1) = mk(&mut sess, &format!("synsrc{i}"), None);
        let orig = d.join("IMG.jpg");
        let temp = d.join("IMG.mmtmp-1.jpg");
        let bak = d.join("IMG.mmbak-1.jpg");
        let put = |spec: &str, dst: &Path| match spec {
            "H0" => { std::fs::copy(&orig0, dst).unwrap(); }
            "H1" => { std::fs::copy(&temp0, dst).unwrap(); }
            "X" => { std::fs::write(dst, b"someone else's edit").unwrap(); }
            _ => {}
        };
        put(o, &orig);
        put(t, &temp);
        put(b, &bak);
        let j = Journal::new(d.join("j.jsonl"));
        let it = PlanItem { seq: 0, path: orig.clone(), temp: temp.clone(), bak: bak.clone(), backup: d.join("backup.jpg"), size: 0, file_id: FileId { volume: 0, id: [0; 16] }, value: "x".into() };
        txn::write_plan(&j, &[it.clone()]).unwrap();
        j.append(&json!({"t":"backed_up","seq":0,"h0":h0})).unwrap();
        if *last == "ready" || *last == "committed" {
            j.append(&json!({"t":"ready","seq":0,"h0":h0,"h1":h1})).unwrap();
        }
        if *last == "committed" {
            j.append(&json!({"t":"committed","seq":0})).unwrap();
        }
        let verdict = txn::recover_item(&it, &j.read());
        let after = state(&orig, &temp, &bak, &h0, &h1);
        let x_untouched = if *o == "X" { std::fs::read(&orig).map(|b| b == b"someone else's edit").unwrap_or(false) } else { true };
        syn.insert(name.to_string(), json!({"verdict": verdict, "after": after, "external_edit_untouched": x_untouched}));
    }
    out.insert("synthetic_recovery".into(), Value::Object(syn));

    sess.close(Duration::from_secs(2));
    let s = serde_json::to_string_pretty(&out).unwrap();
    println!("{s}");
    let tag = if root.to_string_lossy().starts_with(r"\\") { "smb" } else { "ntfs" };
    std::fs::write(research().join("results/s2").join(format!("replace-failures-{tag}.json")), s).unwrap();
}
