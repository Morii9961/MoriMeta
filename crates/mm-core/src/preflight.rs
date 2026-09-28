// SPDX-License-Identifier: GPL-3.0-or-later
//! Pre-flight of a Plan before Apply (ARCHITECTURE §5.2 `plan_preflight`, INTERACTION_SPEC §3–4):
//! files changed on disk since the Preview (they need a rescan, and Apply stays blocked), the
//! backup location, the space it needs, and the ExifTool the Plan was made with. Read-only.

use serde::Serialize;

use mm_domain::plan::{EntryAction, Plan};
use mm_store::Store;

use crate::executor::{SPACE_RESERVE, check_backup_location, check_space};
use crate::{CoreError, fingerprint};

#[derive(Debug, Clone, Serialize)]
pub struct Preflight {
    /// Entries whose file changed (size, time, identity) or appeared or vanished since the
    /// Preview: plan them again.
    pub rescan: Vec<u32>,
    pub backup: Option<String>,
    pub space: Option<String>,
    pub exiftool: Option<String>,
    /// Nothing above stands in the way.
    pub ok: bool,
}

pub fn preflight(store: &Store, plan: &Plan) -> Result<Preflight, CoreError> {
    let mut rescan = Vec::new();
    for e in plan.executable() {
        let path = std::path::Path::new(&e.path);
        let changed = match &e.action {
            // a file created from nothing (a new sidecar) must still be absent
            Some(EntryAction::CreateFile { .. } | EntryAction::Recreate { .. }) => path.exists(),
            _ => match fingerprint(path) {
                Ok(now) => !crate::planner::same_file(&now, &e.fingerprint),
                Err(_) => true,
            },
        };
        if changed {
            rescan.push(e.seq);
        }
    }
    let backup = check_backup_location(store).err().map(|e| e.to_string());
    let space = if backup.is_none() {
        check_space(store, plan.executable(), SPACE_RESERVE)
            .err()
            .map(|e| e.to_string())
    } else {
        None
    };
    let exiftool = (plan.exiftool_version != crate::engine::EXIFTOOL_VERSION).then(|| {
        format!(
            "planned with ExifTool {}, this build uses {}",
            plan.exiftool_version,
            crate::engine::EXIFTOOL_VERSION
        )
    });
    let ok = rescan.is_empty() && backup.is_none() && space.is_none() && exiftool.is_none();
    Ok(Preflight {
        rescan,
        backup,
        space,
        exiftool,
        ok,
    })
}
