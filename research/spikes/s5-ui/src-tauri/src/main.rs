//! S5 UI technology spike (throwaway; not product code): IPC volume, Channel progress rate,
//! Rust→TS type generation (ts-rs), capabilities limited to the spike's own commands.
#![windows_subsystem = "windows"]

use serde::Serialize;
use tauri::ipc::Channel;
use ts_rs::TS;

/// One table row: 30 text fields, like the Library row summary (ARCHITECTURE §8.3, ~1 KB/row).
#[derive(Serialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct Row {
    pub id: u32,
    pub fields: Vec<String>,
}

#[derive(Clone, Serialize, TS)]
#[ts(export, export_to = "../../src/bindings/")]
pub struct Progress {
    pub done: u32,
    pub total: u32,
    pub current: String,
}

const CAMERAS: [&str; 5] = ["NIKON Z 8", "NIKON Z 6_3", "NIKON D850", "Z f", "Z 9"];
const LENSES: [&str; 4] = ["NIKKOR Z 14-24mm f/2.8 S", "NIKKOR Z 50mm f/1.8 S", "Viltrox 35mm F1.2 LAB", "NIKKOR Z 24-120mm f/4 S"];

#[tauri::command]
fn rows(count: u32) -> Vec<Row> {
    (0..count)
        .map(|i| {
            let mut f = vec![
                format!("DSC_{:05}.NEF", 10000 + i),
                if i % 3 == 0 { "JPEG".into() } else { "NEF".into() },
                format!("2026-09-{:02} {:02}:{:02}:{:02}", 1 + i % 28, i % 24, (i * 7) % 60, (i * 13) % 60),
                "+09:00".into(),
                CAMERAS[(i % 5) as usize].into(),
                LENSES[(i % 4) as usize].into(),
                format!("{}", 64 << (i % 6)),
                format!("f/{}.{}", 1 + i % 8, i % 10),
                format!("1/{}", 30 << (i % 6)),
                format!("{} mm", 14 + i % 110),
                if i % 4 == 0 { "—".into() } else { "43.06 N 141.35 E".into() },
                if i % 7 == 0 { "(mixed)".into() } else { "森 Morii".into() },
                "© Morii 2026".into(),
                format!("{}", i % 6),
                if i % 3 == 0 { "Embedded".into() } else { "Sidecar".into() },
            ];
            while f.len() < 30 {
                f.push(format!("value {} / {}", i, f.len()));
            }
            Row { id: i, fields: f }
        })
        .collect()
}

#[tauri::command]
fn progress(total: u32, on_event: Channel<Progress>) -> Result<u32, String> {
    let mut sent = 0;
    for done in 1..=total {
        on_event.send(Progress { done, total, current: format!("DSC_{:05}.NEF", done) }).map_err(|e| e.to_string())?;
        sent += 1;
    }
    Ok(sent)
}

#[tauri::command]
fn autorun() -> bool {
    std::env::var("MM_S5_AUTORUN").as_deref() == Ok("1")
}

#[tauri::command]
fn report(app: tauri::AppHandle, json: String) -> Result<(), String> {
    if let Ok(out) = std::env::var("MM_S5_OUT") {
        std::fs::write(out, json).map_err(|e| e.to_string())?;
    }
    if std::env::var("MM_S5_AUTORUN").as_deref() == Ok("1") {
        app.exit(0);
    }
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![rows, progress, autorun, report])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    #[test]
    fn export_bindings() {
        // `cargo test` writes src/bindings/*.ts via #[ts(export)]
    }
}
