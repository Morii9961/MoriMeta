fn main() {
    // App manifest: custom commands must be granted explicitly in capabilities (least privilege).
    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(tauri_build::AppManifest::new().commands(&["rows", "progress", "report", "autorun"])),
    )
    .expect("tauri build script");
}
