//! jpegseg inspect <file>...   -> JSON segment map per file
//! jpegseg check <file>...     -> JSON {file: {ok, reasons}} using the Clean Export whitelist
fn main() {
    let args: Vec<String> = std::env::args().collect();
    let mode = args.get(1).map(String::as_str).unwrap_or("inspect");
    let mut out = serde_json::Map::new();
    for f in &args[2..] {
        let b = std::fs::read(f).expect("read");
        let v = match jpeg_segments::parse(&b) {
            Err(e) => serde_json::json!({"ok": false, "parse_error": format!("{e:?}")}),
            Ok(j) if mode == "check" => {
                let why = jpeg_segments::check_clean(&j);
                serde_json::json!({"ok": why.is_empty(), "reasons": why})
            }
            Ok(j) => jpeg_segments::to_json(&j),
        };
        out.insert(f.clone(), v);
    }
    println!("{}", serde_json::to_string(&out).unwrap());
}
