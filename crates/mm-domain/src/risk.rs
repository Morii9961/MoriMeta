// SPDX-License-Identifier: GPL-3.0-or-later
//! Content that changes what writing means (SAFETY_MODEL §8).

use crate::snapshot::{Snapshot, value_text};

/// C2PA Content Credentials (JUMBF with a `c2pa` label, S3/F-98): any metadata change leaves the
/// manifest in place but invalidates its signature, so such files are excluded by default
/// (SAFETY_MODEL §8.12).
pub fn has_c2pa(snap: &Snapshot) -> bool {
    snap.iter().any(|(k, v)| {
        (k.starts_with("JUMBF") || k.starts_with("C2PA"))
            && value_text(v).to_ascii_lowercase().contains("c2pa")
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn detects_the_c2pa_label_only() {
        assert!(has_c2pa(&Snapshot::from_json(
            &json!({"JUMBF:JUMDLabel": "c2pa"})
        )));
        assert!(!has_c2pa(&Snapshot::from_json(
            &json!({"JUMBF:JUMDLabel": "other"})
        )));
        assert!(!has_c2pa(&Snapshot::from_json(
            &json!({"XMP-dc:Title": "about c2pa"})
        )));
    }
}
