// SPDX-License-Identifier: GPL-3.0-or-later
/// The app's own commands, each granted explicitly in `capabilities/default.json` (least
/// privilege, SECURITY_MODEL §6); no plugin permissions at all.
const COMMANDS: &[&str] = &[
    "subscribe",
    "app_info",
    "about",
    "ui_zoom",
    "third_party_notices",
    "import_dialog",
    "scan_cancel",
    "rescan",
    "session_clear",
    "asset_detail",
    "asset_preview",
    "selection_aggregate",
    "attention",
    "clear_read_only",
    "plan_batch",
    "plan_cancel",
    "plan_view",
    "plan_page",
    "plan_exclude",
    "plan_exclude_field",
    "plan_preflight",
    "plan_confirm",
    "op_execute",
    "op_cancel",
    "history_list",
    "op_detail",
    "undo_plan",
    "retry_plan",
    "plan_again",
    "recovery_keep",
    "export_log",
    "restore_to",
    "history_import",
    "recovery_status",
    "recovery_dismiss",
    "recovery_resume",
    "settings_list",
    "setting_set",
    "settings_reset",
    "settings_migrations",
    "backup_usage",
    "backup_keep",
    "prune_preview",
    "prune_execute",
    "app_close",
    "now_vs_after",
    "choose_backup_folder",
    "clean_plan",
    "clean_entry",
    "clean_export",
    "presets_list",
    "preset_save",
    "preset_duplicate",
    "preset_delete",
    "preset_import",
    "preset_export",
    "plan_preset",
    "preset_dry_run",
    "update_status",
    "update_check",
    "update_download",
    "update_cancel",
    "update_install",
];

/// The app icon is drawn here rather than committed (the repository keeps no binaries,
/// REPOSITORY_CHECKLIST): an accent "M" on the panel colour of the design system, as a
/// multi-size ICO of 32-bit bitmaps.
fn ensure_icon() {
    let path = std::path::Path::new("icons/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if path.exists() {
        return;
    }
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];
    let images: Vec<Vec<u8>> = sizes.iter().map(|&s| bitmap(s)).collect();
    let mut ico = Vec::new();
    ico.extend_from_slice(&[0, 0, 1, 0]);
    ico.extend_from_slice(&(sizes.len() as u16).to_le_bytes());
    let mut offset = 6 + 16 * sizes.len() as u32;
    for (s, img) in sizes.iter().zip(&images) {
        let b = if *s >= 256 { 0 } else { *s as u8 };
        ico.extend_from_slice(&[b, b, 0, 0]);
        ico.extend_from_slice(&1u16.to_le_bytes());
        ico.extend_from_slice(&32u16.to_le_bytes());
        ico.extend_from_slice(&(img.len() as u32).to_le_bytes());
        ico.extend_from_slice(&offset.to_le_bytes());
        offset += img.len() as u32;
    }
    for img in images {
        ico.extend_from_slice(&img);
    }
    std::fs::create_dir_all("icons").expect("icons folder");
    std::fs::write(path, ico).expect("write icon");
}

/// One BGRA bitmap (bottom-up, with an empty AND mask) of size `s`, 4×4 supersampled.
fn bitmap(s: u32) -> Vec<u8> {
    let bg = [0x1a, 0x17, 0x15]; // #15171a (BGR)
    let edge = [0x2d, 0x29, 0x26]; // #26292d
    let accent = [0xd0, 0xb4, 0x5a]; // #5ab4d0
    let m = [
        (0.27, 0.73),
        (0.27, 0.29),
        (0.50, 0.57),
        (0.73, 0.29),
        (0.73, 0.73),
    ];
    let stroke = 0.095;
    let seg = |px: f64, py: f64| -> f64 {
        m.windows(2)
            .map(|w| {
                let ((ax, ay), (bx, by)) = (w[0], w[1]);
                let (dx, dy) = (bx - ax, by - ay);
                let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
                ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
            })
            .fold(f64::MAX, f64::min)
    };
    let radius = 0.16;
    let mut px = Vec::with_capacity((s * s * 4) as usize);
    for row in (0..s).rev() {
        for col in 0..s {
            let (mut cover, mut ink, mut rim) = (0.0, 0.0, 0.0);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = (col as f64 + (sx as f64 + 0.5) / 4.0) / s as f64;
                    let y = (row as f64 + (sy as f64 + 0.5) / 4.0) / s as f64;
                    // rounded square
                    let qx = (x - 0.5).abs() - (0.5 - radius);
                    let qy = (y - 0.5).abs() - (0.5 - radius);
                    let d = qx.max(0.0).hypot(qy.max(0.0)) + qx.max(qy).min(0.0) - radius;
                    if d <= 0.0 {
                        cover += 1.0;
                        if d > -1.2 / s as f64 {
                            rim += 1.0;
                        } else if seg(x, y) <= stroke / 2.0 {
                            ink += 1.0;
                        }
                    }
                }
            }
            let n = 16.0;
            let mix = |i: usize| -> u8 {
                let base = bg[i] as f64 * (cover - ink - rim)
                    + accent[i] as f64 * ink
                    + edge[i] as f64 * rim;
                if cover > 0.0 {
                    (base / cover).round() as u8
                } else {
                    0
                }
            };
            px.extend_from_slice(&[mix(0), mix(1), mix(2), (cover / n * 255.0).round() as u8]);
        }
    }
    let mask_row = (s.div_ceil(32) * 4) as usize;
    let mut out = Vec::new();
    for v in [40u32, s, s * 2] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    for v in [0u32, (px.len() + mask_row * s as usize) as u32, 0, 0, 0, 0] {
        out.extend_from_slice(&v.to_le_bytes());
    }
    out.extend_from_slice(&px);
    out.extend(std::iter::repeat_n(0u8, mask_row * s as usize));
    out
}

fn main() {
    println!("cargo:rerun-if-env-changed=MORIMETA_UPDATER_PUBLIC_KEY");
    ensure_icon();
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(COMMANDS)),
    )
    .expect("tauri build script");
}
