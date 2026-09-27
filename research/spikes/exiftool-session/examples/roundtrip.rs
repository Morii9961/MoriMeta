//! S1 exit criterion: N random values round-trip through (encode -> stay_open write -> read) exactly.
//!
//! cargo run --release --example roundtrip -- <n> <xml|cstr> <launcher|perl> <seed>
//! Values go into XMP-dc:Subject items of new .xmp files (1000 per command), read back with -json -b.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use exiftool_session::encode::{self, EncodeError};
use exiftool_session::valuegen::{self, ACCEPTED, Class, REJECTED, SplitMix64};
use exiftool_session::{EngineConfig, Session, arg_path};

fn json_to_strings(v: &serde_json::Value) -> Vec<String> {
    match v {
        serde_json::Value::Array(a) => a.iter().flat_map(json_to_strings).collect(),
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Number(n) => vec![n.to_string()],
        other => vec![other.to_string()],
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let n: usize = args.get(1).map(|s| s.parse().unwrap()).unwrap_or(100_000);
    let strategy = args.get(2).cloned().unwrap_or_else(|| "xml".into());
    let mode = args.get(3).cloned().unwrap_or_else(|| "launcher".into());
    let seed: u64 = args.get(4).map(|s| s.parse().unwrap()).unwrap_or(20260926);
    let batch = 1000usize;

    let cfg = EngineConfig::research(&mode);
    let research = cfg.cwd.parent().unwrap().parent().unwrap().parent().unwrap().to_path_buf();
    let lab: PathBuf = research.join(".work/lab").join(format!("s1-roundtrip-{strategy}-{mode}"));
    let _ = std::fs::remove_dir_all(&lab);
    std::fs::create_dir_all(&lab).unwrap();

    let mut rng = SplitMix64(seed);
    let mut sess = Session::spawn(&cfg).expect("spawn");
    let t0 = Instant::now();

    let mut total = 0usize;
    let mut exact = 0usize;
    let mut encode_rejected: BTreeMap<String, usize> = BTreeMap::new();
    let mut mismatch_by_class: BTreeMap<String, usize> = BTreeMap::new();
    let mut samples: Vec<serde_json::Value> = Vec::new();
    let mut engine_errors = 0usize;

    let mut b = 0usize;
    while total < n {
        let mut values: Vec<(String, Vec<Class>)> = Vec::new();
        let mut seen = std::collections::HashSet::new();
        let mut lines: Vec<String> = Vec::new();
        if strategy == "xml" {
            lines.push("-ex".into());
        }
        while values.len() < batch.min(n - total) {
            let (v, cls) = valuegen::value(&mut rng, ACCEPTED);
            if !seen.insert(v.clone()) {
                continue;
            }
            let enc = if strategy == "xml" {
                encode::assign_xml("XMP-dc:Subject", &v)
            } else {
                encode::assign_cstr("XMP-dc:Subject", &v)
            };
            match enc {
                Ok(line) => {
                    lines.push(line);
                    values.push((v, cls));
                }
                Err(e) => {
                    let k = match e {
                        EncodeError::CstrUnrepresentable(c) => format!("CstrUnrepresentable({c})"),
                        other => format!("{other:?}"),
                    };
                    *encode_rejected.entry(k).or_default() += 1;
                    total += 1;
                }
            }
        }
        let out = lab.join(format!("b{b:05}.xmp"));
        lines.push("-o".into());
        lines.push(arg_path(&out));
        let w = sess.execute(&lines, Duration::from_secs(120)).expect("write");
        if w.status != 0 || !out.exists() {
            engine_errors += 1;
            eprintln!("write batch {b} status={} err={}", w.status, w.err());
        }
        let r = sess
            .execute(&["-json".into(), "-api".into(), "StructFormat=JSONQ".into(), "-b".into(), "-G1".into(), "-XMP-dc:Subject".into(), arg_path(&out)], Duration::from_secs(120))
            .expect("read");
        let parsed: serde_json::Value = serde_json::from_slice(&r.stdout).unwrap_or(serde_json::Value::Null);
        let got = parsed.get(0).and_then(|o| o.get("XMP-dc:Subject")).map(json_to_strings).unwrap_or_default();
        for (i, (v, cls)) in values.iter().enumerate() {
            total += 1;
            let g = got.get(i);
            if g == Some(v) {
                exact += 1;
            } else {
                for c in cls {
                    *mismatch_by_class.entry(format!("{c:?}")).or_default() += 1;
                }
                if samples.len() < 40 {
                    samples.push(serde_json::json!({"want": v, "got": g, "classes": format!("{cls:?}")}));
                }
            }
        }
        if got.len() != values.len() {
            eprintln!("batch {b}: {} values written, {} read", values.len(), got.len());
        }
        b += 1;
    }

    // Rejected domain: the encoder must refuse every value (never emit a line).
    let mut rej_total = 0usize;
    let mut rej_refused = 0usize;
    for _ in 0..20_000 {
        let (v, _) = valuegen::value(&mut rng, REJECTED);
        if !v.chars().any(|c| !encode::is_xml_char(c)) {
            continue; // generator sprinkled only ASCII this time
        }
        rej_total += 1;
        let r = if strategy == "xml" { encode::assign_xml("XMP-dc:Subject", &v) } else { encode::assign_cstr("XMP-dc:Subject", &v) };
        if r.is_err() {
            rej_refused += 1;
        }
    }

    let summary = serde_json::json!({
        "strategy": strategy, "mode": mode, "seed": seed, "requested": n,
        "values_written": total - encode_rejected.values().sum::<usize>(),
        "exact": exact,
        "encode_rejected": encode_rejected,
        "mismatch_by_class": mismatch_by_class,
        "engine_errors": engine_errors,
        "rejected_domain": {"values": rej_total, "refused_by_encoder": rej_refused},
        "seconds": t0.elapsed().as_secs_f64(),
        "mismatch_samples": samples,
    });
    let out_dir = research.join("results/s1");
    std::fs::create_dir_all(&out_dir).unwrap();
    std::fs::write(out_dir.join(format!("roundtrip-{strategy}-{mode}.json")), serde_json::to_string_pretty(&summary).unwrap()).unwrap();
    println!("{}", serde_json::to_string(&summary).unwrap().chars().take(1500).collect::<String>());
    sess.close(Duration::from_secs(2));
}
