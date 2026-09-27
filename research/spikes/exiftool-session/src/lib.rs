//! S1 spike: ExifTool stay_open protocol, argfile encoding, process supervision (Windows).

pub mod encode;
pub mod valuegen;
pub mod session;

pub use session::{EngineConfig, Response, Session, SessionError};

/// Absolute forward-slash path for argfiles.
pub fn arg_path(p: &std::path::Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    s.strip_prefix("//?/").map(str::to_owned).unwrap_or(s)
}
