//! Pure domain model (no IO, no processes).
//!
//! Phase 1a: the four MVP time tools and input-value rules. FieldRegistry, rules, templates and
//! the Plan/Diff model follow once the registry is frozen (S3 third-party checks).

pub mod time;
pub mod value;
