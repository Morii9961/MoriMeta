// SPDX-License-Identifier: GPL-3.0-or-later
//! Rust-only updater: explicit check/download/install, weekly checks only after opt-in.
//! Artifact signatures also bind the advertised version (legacy signatures are refused).
//! The public key is build configuration; an unconfigured build makes no updater requests.

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::updater_artifact::validate_artifact;
use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_updater::{Update, UpdaterExt};
use tokio::sync::Notify;

use crate::core::{Core, lock};
use crate::dto::AppEvent;
use crate::errors::ue;

const PUBLIC_KEY: &str = match option_env!("MORIMETA_UPDATER_PUBLIC_KEY") {
    Some(key) => key,
    None => "",
};
const ENDPOINT: &str = "https://github.com/Morii9961/MoriMeta/releases/latest/download/latest.json";
const WEEK_MS: i64 = 7 * 86_400_000;
const LAST_ATTEMPT: &str = "updates.last_attempt_ms";
const MAX_DOWNLOAD: u64 = crate::updater_artifact::MAX_INSTALLER_BYTES;

#[derive(Clone, Serialize)]
pub struct Offer {
    pub id: String,
    pub version: String,
    pub notes: Option<String>,
    pub date: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Info {
    pub configured: bool,
    pub busy: bool,
    pub phase: String,
    pub offer: Option<Offer>,
    pub downloaded: bool,
    pub bytes: u64,
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Default)]
struct Book {
    update: Option<Update>,
    offer: Option<Offer>,
    bytes: Option<Vec<u8>>,
    progress: u64,
    total: Option<u64>,
    phase: String,
    error: Option<String>,
    cancel: Option<Arc<Notify>>,
}

#[derive(Default)]
pub struct Updates(Mutex<Book>);

struct Job<'a>(&'a Updates, Arc<Notify>);
impl Drop for Job<'_> {
    fn drop(&mut self) {
        lock(&self.0.0).cancel = None;
    }
}

impl Updates {
    pub fn info(&self) -> Info {
        let b = lock(&self.0);
        Info {
            configured: !PUBLIC_KEY.trim().is_empty(),
            busy: b.cancel.is_some(),
            phase: if b.phase.is_empty() {
                "idle".into()
            } else {
                b.phase.clone()
            },
            offer: b.offer.clone(),
            downloaded: b.bytes.is_some(),
            bytes: b.progress,
            total: b.total,
            error: b.error.clone(),
        }
    }

    fn begin(&self, phase: &str) -> Result<Job<'_>, String> {
        let mut b = lock(&self.0);
        if b.cancel.is_some() {
            return Err("an update request is already running".into());
        }
        let cancel = Arc::new(Notify::new());
        b.cancel = Some(cancel.clone());
        b.phase = phase.into();
        b.error = None;
        Ok(Job(self, cancel))
    }

    fn selected(&self, id: &str) -> Result<Update, String> {
        let b = lock(&self.0);
        if !b.offer.as_ref().is_some_and(|o| o.id == id) {
            return Err("the update offer changed; check for updates again".into());
        }
        b.update
            .clone()
            .ok_or_else(|| "no update is available".into())
    }
}

fn finish<T>(core: &Core, job: Job<'_>, result: Result<T, String>) -> Result<T, String> {
    if let Err(error) = &result {
        let mut b = lock(&core.updates.0);
        b.error = Some(error.clone());
        b.phase = "failed".into();
    }
    drop(job);
    core.emit(AppEvent::Status);
    result
}

fn due(preference: &str, last: Option<i64>, now: i64) -> bool {
    preference == "weekly" && last.is_none_or(|last| now.saturating_sub(last) >= WEEK_MS)
}

#[tauri::command]
pub fn update_status(core: tauri::State<'_, Arc<Core>>) -> Info {
    core.updates.info()
}

#[tauri::command]
pub async fn update_check(app: AppHandle, manual: bool) -> Result<Info, String> {
    let core = app.state::<Arc<Core>>().inner().clone();
    if PUBLIC_KEY.trim().is_empty() {
        return Ok(core.updates.info());
    }
    if !manual {
        let store = lock(&core.store);
        let preference = mm_core::settings::get(&store, "updates.check").map_err(ue)?;
        let last = store
            .setting(LAST_ATTEMPT)
            .map_err(ue)?
            .and_then(|s| s.parse().ok());
        if !due(&preference, last, mm_store::now_ms()) || core.updates.info().busy {
            return Ok(core.updates.info());
        }
    }
    let job = core.updates.begin("checking")?;
    core.emit(AppEvent::Status);
    let result = async {
        {
            let mut store = lock(&core.store);
            let now = mm_store::now_ms();
            let preference = mm_core::settings::get(&store, "updates.check").map_err(ue)?;
            let last = store
                .setting(LAST_ATTEMPT)
                .map_err(ue)?
                .and_then(|s| s.parse().ok());
            if !manual && !due(&preference, last, now) {
                return Ok(());
            }
            // Persist attempts (including network failures) to avoid weekly retry storms.
            store
                .set_setting(LAST_ATTEMPT, &now.to_string())
                .map_err(ue)?;
        }
        let exit_core = core.clone();
        let updater = app
            .updater_builder()
            .pubkey(PUBLIC_KEY.trim())
            .endpoints(vec![ENDPOINT.parse().map_err(|e| format!("{e}"))?])
            .map_err(ue)?
            .timeout(Duration::from_secs(300))
            .on_before_exit(move || exit_core.shutdown())
            .build()
            .map_err(ue)?;
        let update = tokio::select! {
            _ = job.1.notified() => return Err("update check cancelled".into()),
            result = updater.check() => result.map_err(ue)?,
        };
        if let Some(update) = &update {
            let url = &update.download_url;
            if url.scheme() != "https"
                || url.host_str() != Some("github.com")
                || !url
                    .path()
                    .starts_with("/Morii9961/MoriMeta/releases/download/")
                || !url.path().to_ascii_lowercase().ends_with(".exe")
            {
                return Err("the update artifact is not a MoriMeta HTTPS installer".into());
            }
        }
        let offer = update
            .as_ref()
            .map(|u| {
                Ok::<Offer, String>(Offer {
                    id: mm_core::new_id("update").map_err(ue)?,
                    version: u.version.clone(),
                    notes: u.body.clone(),
                    date: u.date.map(|d| d.to_string()),
                })
            })
            .transpose()?;
        let mut b = lock(&core.updates.0);
        b.phase = if update.is_some() {
            "available"
        } else {
            "current"
        }
        .into();
        b.update = update;
        b.offer = offer;
        b.bytes = None;
        b.progress = 0;
        b.total = None;
        Ok(())
    }
    .await;
    finish(&core, job, result)?;
    Ok(core.updates.info())
}

#[tauri::command]
pub async fn update_download(app: AppHandle, id: String) -> Result<Info, String> {
    let core = app.state::<Arc<Core>>().inner().clone();
    let job = core.updates.begin("downloading")?;
    core.emit(AppEvent::Status);
    let result = async {
        let update = core.updates.selected(&id)?;
        { let mut b = lock(&core.updates.0); b.bytes = None; b.progress = 0; b.total = None; }
        let mut last_emit = Instant::now();
        let bytes = tokio::select! {
            _ = job.1.notified() => return Err("update download cancelled or too large".into()),
            result = update.download(|chunk, total| {
                let mut b = lock(&core.updates.0);
                b.progress = b.progress.saturating_add(chunk as u64);
                b.total = total;
                if b.progress > MAX_DOWNLOAD || total.is_some_and(|n| n > MAX_DOWNLOAD) { job.1.notify_one(); }
                drop(b);
                if last_emit.elapsed() >= Duration::from_millis(200) {
                    core.emit(AppEvent::Status); last_emit = Instant::now();
                }
            }, || {}) => result.map_err(ue)?,
        };
        if bytes.len() as u64 > MAX_DOWNLOAD { return Err("the update installer is too large".into()); }
        validate_artifact(&bytes, PUBLIC_KEY.trim(), &update.signature, &update.version, env!("CARGO_PKG_VERSION"))?;
        let mut b = lock(&core.updates.0);
        b.progress = bytes.len() as u64;
        b.bytes = Some(bytes);
        b.phase = "ready".into();
        Ok(())
    }.await;
    finish(&core, job, result)?;
    Ok(core.updates.info())
}

#[tauri::command]
pub fn update_cancel(core: tauri::State<'_, Arc<Core>>) {
    if let Some(cancel) = &lock(&core.updates.0).cancel {
        cancel.notify_one();
    }
}

#[tauri::command]
pub async fn update_install(app: AppHandle, id: String) -> Result<(), String> {
    let core = app.state::<Arc<Core>>().inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        core.wait_launched();
        let job = core.updates.begin("installing")?;
        let result = (|| {
            let update = core.updates.selected(&id)?;
            // Reserve the exclusive slot before taking the downloaded bytes: a Busy failure can
            // be retried without downloading again. The installer remains behind the Rust gate.
            let store = lock(&core.store);
            let _exclusive = core.gate.exclusive(&store).map_err(ue)?;
            store.checkpoint().map_err(ue)?;
            drop(store);
            let _running = core.mark_running();
            let bytes = lock(&core.updates.0)
                .bytes
                .take()
                .ok_or("download and verify the update first")?;
            validate_artifact(
                &bytes,
                PUBLIC_KEY.trim(),
                &update.signature,
                &update.version,
                env!("CARGO_PKG_VERSION"),
            )?;
            // Windows launches the passive installer and exits. The plugin's before-exit hook
            // closes ExifTool; installer launch failure leaves the app and its sessions alive.
            update.install(bytes).map_err(ue)
        })();
        finish(&core, job, result)
    })
    .await
    .map_err(ue)?
}

pub fn auto_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        loop {
            tokio::time::sleep(Duration::from_secs(60)).await;
            let _ = update_check(app.clone(), false).await;
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_checks_require_consent_and_wait_a_week() {
        assert!(!due("ask", None, 100));
        assert!(!due("never", None, 100));
        assert!(due("weekly", None, 100));
        assert!(!due("weekly", Some(100), 100 + WEEK_MS - 1));
        assert!(due("weekly", Some(100), 100 + WEEK_MS));
        assert!(!due("weekly", Some(100), 99));
    }

    #[test]
    fn update_jobs_are_exclusive_and_clear_cancel_state_on_every_exit() {
        let updates = Updates::default();
        let job = updates.begin("checking").unwrap();
        assert!(updates.info().busy);
        assert!(updates.begin("downloading").is_err());
        assert!(updates.selected("unknown").is_err());
        drop(job);
        assert!(!updates.info().busy);
        drop(updates.begin("checking").unwrap());
    }
}
