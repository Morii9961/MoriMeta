// SPDX-License-Identifier: GPL-3.0-or-later
//! ExifTool adapter: process session, argfile encoding, supervision (Windows).
//!
//! Scope (Phase 1a): the protocol layer validated in S1 (docs/SPIKE_REPORT.md §2). Typed
//! requests built from the FieldRegistry come later (Phase 1b); until then callers assemble
//! [`Command`]s from validated [`Line`]s.

pub mod encode;
pub mod session;

pub use encode::{ArgError, Command, Line, TagName, ValueError, check_value, xml_value};
pub use session::{EngineConfig, EngineError, Output, Session, Terminator};
