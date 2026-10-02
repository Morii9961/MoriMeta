// SPDX-License-Identifier: GPL-3.0-or-later
//! MoriMeta desktop app: a thin Tauri adapter over `mm-core` (ARCHITECTURE ADR-02). The frontend
//! renders state and collects intent; every decision about what is written is made in the core.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod cmd;
mod core;
mod dto;
mod errors;

use std::sync::Arc;

use tauri::{DragDropEvent, Manager, WindowEvent};

fn main() {
    let core = match core::Core::open() {
        Ok(c) => Arc::new(c),
        Err(why) => {
            // most often another MoriMeta already runs with the same data folder
            rfd::MessageDialog::new()
                .set_level(rfd::MessageLevel::Error)
                .set_title("MoriMeta")
                .set_description(format!("MoriMeta cannot start.\n\n{why}"))
                .show();
            std::process::exit(1);
        }
    };
    let for_setup = core.clone();
    tauri::Builder::default()
        .manage(core)
        .setup(move |app| {
            for_setup.launch(app.path().resource_dir().ok());
            Ok(())
        })
        .on_window_event(|window, event| match event {
            WindowEvent::DragDrop(DragDropEvent::Drop { paths, .. }) => {
                let core = window.state::<Arc<core::Core>>().inner().clone();
                cmd::dropped(core, paths.clone());
            }
            // INTERACTION_SPEC §10: closing during an Operation always asks (the frontend
            // offers Keep going or Stop after the current file, then close)
            WindowEvent::CloseRequested { api, .. } => {
                let core = window.state::<Arc<core::Core>>();
                if core.running() {
                    api.prevent_close();
                    core.emit(dto::AppEvent::CloseBlocked);
                }
            }
            WindowEvent::Destroyed => {
                window.state::<Arc<core::Core>>().shutdown();
            }
            _ => {}
        })
        .invoke_handler(tauri::generate_handler![
            cmd::subscribe,
            cmd::app_info,
            cmd::import_dialog,
            cmd::scan_cancel,
            cmd::rescan,
            cmd::session_clear,
            cmd::asset_detail,
            cmd::selection_aggregate,
            cmd::attention,
            cmd::clear_read_only,
            cmd::plan_batch,
            cmd::plan_cancel,
            cmd::plan_view,
            cmd::plan_page,
            cmd::plan_exclude,
            cmd::plan_exclude_field,
            cmd::plan_preflight,
            cmd::plan_confirm,
            cmd::op_execute,
            cmd::op_cancel,
            cmd::history_list,
            cmd::op_detail,
            cmd::undo_plan,
            cmd::retry_plan,
            cmd::export_log,
            cmd::restore_to,
            cmd::recovery_status,
            cmd::recovery_dismiss,
            cmd::recovery_resume,
            cmd::settings_list,
            cmd::setting_set,
            cmd::app_close,
            cmd::now_vs_after,
            cmd::choose_backup_folder,
            cmd::clean_plan,
            cmd::clean_entry,
            cmd::clean_export,
            cmd::presets_list,
            cmd::preset_save,
            cmd::preset_duplicate,
            cmd::preset_delete,
            cmd::preset_import,
            cmd::preset_export,
            cmd::plan_preset,
        ])
        .run(tauri::generate_context!())
        .expect("error while running MoriMeta");
}
