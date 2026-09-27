//! Pure domain model (no IO, no processes).
//!
//! Phase 1a: MVP time tools, value rules. Phase 1b (in progress): Plan model, tag snapshot,
//! and the `creator` field (provisional registry v0 until the S3 third-party checks).

pub mod capture;
pub mod copyright;
pub mod cp1252;
pub mod creator;
pub mod gps;
pub mod iptc;
pub mod plan;
pub mod risk;
pub mod snapshot;
pub mod time;
pub mod value;
