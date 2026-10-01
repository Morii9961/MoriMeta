// SPDX-License-Identifier: GPL-3.0-or-later
//! Errors as the user reads them. The core's `Display` is its `Debug` form (the development CLI
//! and its tests rely on the variant names); the app shows sentences instead, which the UI's
//! catalog translates (`src/i18n/backend.zh.json`).

use mm_core::CoreError;
use mm_core::service::ServiceError;

pub trait UserText {
    fn user_text(&self) -> String;
}

impl UserText for CoreError {
    fn user_text(&self) -> String {
        match self {
            CoreError::Input(s)
            | CoreError::Engine(s)
            | CoreError::Internal(s)
            | CoreError::VersionMismatch(s)
            | CoreError::InsufficientSpace(s) => s.clone(),
            CoreError::Store(e) => format!("the journal database: {e:?}"),
            CoreError::Io(e) => e.to_string(),
            CoreError::RecoveryPending(ops) => format!(
                "{} interrupted operation(s) need recovery first; nothing is written until then",
                ops.len()
            ),
            CoreError::Cancelled => "cancelled; nothing was written".into(),
            CoreError::BackupUnavailable(s) => {
                format!(
                    "the backup location is not available ({s}); nothing is written until it is"
                )
            }
            CoreError::NotDownloaded(s) => {
                format!("{s}: a cloud file that is not on this computer; it is not read")
            }
        }
    }
}

impl UserText for ServiceError {
    fn user_text(&self) -> String {
        match self {
            ServiceError::UnknownAsset(_) => "this file is no longer in the session".into(),
            ServiceError::UnknownPlan(_) => "this plan is no longer available".into(),
            ServiceError::StalePlan { current } => {
                format!("the plan changed meanwhile (now version {current}); look at it again")
            }
            ServiceError::NotConfirmed => "the plan was not confirmed; confirm it again".into(),
            ServiceError::NotAcknowledged(a) => format!("acknowledge first: {}", a.join(", ")),
            ServiceError::Busy(s) => s.clone(),
            ServiceError::Elevated => {
                "MoriMeta runs with administrator rights and does not write then; start it normally"
                    .into()
            }
            ServiceError::AnotherInstance => "another MoriMeta is using this data folder".into(),
            ServiceError::WritesDisabled(s) => format!("writing is disabled: {s}"),
            ServiceError::Core(c) => c.user_text(),
        }
    }
}

macro_rules! plain {
    ($($t:ty),*) => {$(
        impl UserText for $t {
            fn user_text(&self) -> String {
                self.to_string()
            }
        }
    )*};
}

plain!(std::io::Error, tauri::Error, serde_json::Error, String);

impl UserText for mm_store::StoreError {
    fn user_text(&self) -> String {
        format!("the journal database: {self:?}")
    }
}

/// For `map_err`: the error as the user reads it.
pub fn ue(x: impl UserText) -> String {
    x.user_text()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences_not_variant_names() {
        let e = ServiceError::Core(CoreError::Input("nothing is staged".into()));
        assert_eq!(e.user_text(), "nothing is staged");
        assert!(
            !CoreError::BackupUnavailable("D:/b: gone".into())
                .user_text()
                .contains("BackupUnavailable")
        );
    }
}
