// SPDX-License-Identifier: GPL-3.0-or-later
//! Rules and Presets, MVP (PRODUCT_SPEC §6.10, §6.11): a Rule is optional conditions (all must
//! hold) and actions; a Preset is an ordered list of Rules stored as JSON with `schema_version`.
//! Every condition is evaluated on the file's original snapshot; actions are combined in order and
//! a later action on the same field replaces an earlier one, with a note. No rule sees another
//! rule's result. Of the capture-time tools, Absolute and Shift are rule actions (each file on its
//! own); Sequence and Preserve Relative Timing need the whole selection and are not.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::capture;
use crate::copyright::{self, CopyrightEdit};
use crate::creator::{self, CreatorEdit};
use crate::gps::{self, GeoPoint, GpsEdit};
use crate::plan::{FieldPlan, Target};
use crate::template::{Template, TemplateCtx};
use crate::time::{self, TimeItem, TimeOp};

pub const PRESET_SCHEMA_VERSION: u32 = 1;

/// Limits of an imported Preset (SECURITY_MODEL §9).
pub const MAX_PRESET_BYTES: usize = 1 << 20;
pub const MAX_RULES: usize = 100;
pub const MAX_CONDITIONS: usize = 20;
pub const MAX_ACTIONS: usize = 20;
pub const MAX_ITEMS: usize = 50;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Field {
    Creator,
    Copyright,
    CaptureTime,
    Gps,
}

impl Field {
    pub fn name(self) -> &'static str {
        match self {
            Field::Creator => "creator",
            Field::Copyright => "copyright",
            Field::CaptureTime => "capture_time",
            Field::Gps => "gps",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FileKind {
    Jpeg,
    Raw,
    Xmp,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "if", rename_all = "snake_case", deny_unknown_fields)]
pub enum Condition {
    Empty {
        field: Field,
    },
    NotEmpty {
        field: Field,
    },
    /// Exact text of the field's effective value (a list joined with "; ").
    Equals {
        field: Field,
        value: String,
    },
    /// Case-insensitive substring of the field's effective value.
    Contains {
        field: Field,
        value: String,
    },
    /// File extension, case-insensitive, without the dot.
    Extension {
        any: Vec<String>,
    },
    Kind {
        kind: FileKind,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "do", rename_all = "snake_case", deny_unknown_fields)]
pub enum Action {
    /// Names may use template variables.
    SetCreator {
        names: Vec<String>,
    },
    ClearCreator,
    /// May use template variables.
    SetCopyright {
        value: String,
    },
    ClearCopyright,
    /// `lat,lon[,alt]` in decimal degrees and metres.
    SetGps {
        position: String,
    },
    RemoveGps,
    /// Absolute: `YYYY:MM:DD HH:MM:SS` (local time; each location keeps its offset).
    SetTime {
        to: String,
        #[serde(default = "yes")]
        digitized: bool,
    },
    /// Shift: `[+|-][Nd]HH:MM:SS`; sub-seconds and offsets are kept.
    ShiftTime {
        by: String,
        #[serde(default = "yes")]
        digitized: bool,
    },
}

impl Action {
    pub fn field(&self) -> Field {
        match self {
            Action::SetCreator { .. } | Action::ClearCreator => Field::Creator,
            Action::SetCopyright { .. } | Action::ClearCopyright => Field::Copyright,
            Action::SetGps { .. } | Action::RemoveGps => Field::Gps,
            Action::SetTime { .. } | Action::ShiftTime { .. } => Field::CaptureTime,
        }
    }
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Rule {
    #[serde(default)]
    pub name: String,
    #[serde(default = "yes")]
    pub enabled: bool,
    #[serde(default)]
    pub when: Vec<Condition>,
    pub then: Vec<Action>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preset {
    pub schema_version: u32,
    pub name: String,
    pub rules: Vec<Rule>,
}

impl Preset {
    /// Parse and check a Preset (import must pass this, PRODUCT_SPEC §6.11).
    pub fn from_json(s: &str) -> Result<Preset, String> {
        if s.len() > MAX_PRESET_BYTES {
            return Err(format!(
                "a preset file is at most {} KB (this one is {} KB)",
                MAX_PRESET_BYTES >> 10,
                s.len() >> 10
            ));
        }
        let v: serde_json::Value = serde_json::from_str(s).map_err(|e| format!("not JSON: {e}"))?;
        match v.get("schema_version").and_then(serde_json::Value::as_u64) {
            Some(n) if n == u64::from(PRESET_SCHEMA_VERSION) => {}
            Some(n) => return Err(format!("schema_version {n} is not supported (expected 1)")),
            None => return Err("schema_version is missing".into()),
        }
        let p: Preset = serde_json::from_value(v).map_err(|e| format!("invalid preset: {e}"))?;
        p.validate()?;
        Ok(p)
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("a preset serializes")
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.rules.is_empty() {
            return Err("a preset needs at least one rule".into());
        }
        if self.rules.len() > MAX_RULES {
            return Err(format!("a preset has at most {MAX_RULES} rules"));
        }
        for (i, r) in self.rules.iter().enumerate() {
            let at = |e: String| format!("rule {} ({}): {e}", i + 1, r.name);
            if r.then.is_empty() {
                return Err(at("no action".into()));
            }
            if r.when.len() > MAX_CONDITIONS || r.then.len() > MAX_ACTIONS {
                return Err(at(format!(
                    "at most {MAX_CONDITIONS} conditions and {MAX_ACTIONS} actions"
                )));
            }
            for c in &r.when {
                if let Condition::Extension { any } = c
                    && any.len() > MAX_ITEMS
                {
                    return Err(at(format!("at most {MAX_ITEMS} extensions")));
                }
                if let Condition::Extension { any } = c
                    && any.is_empty()
                {
                    return Err(at("extension condition without extensions".into()));
                }
            }
            for a in &r.then {
                match a {
                    Action::SetCreator { names } => {
                        if names.len() > MAX_ITEMS {
                            return Err(at(format!("at most {MAX_ITEMS} names")));
                        }
                        if names.is_empty() {
                            return Err(at("set_creator without names".into()));
                        }
                        for n in names {
                            Template::parse(n).map_err(at)?;
                        }
                    }
                    Action::SetCopyright { value } => {
                        Template::parse(value).map_err(at)?;
                    }
                    Action::SetGps { position } => {
                        GeoPoint::parse(position).map_err(at)?;
                    }
                    Action::SetTime { to, .. } => {
                        time::parse_local(to)
                            .ok_or_else(|| at(format!("{to:?}: use YYYY:MM:DD HH:MM:SS")))?;
                    }
                    Action::ShiftTime { by, .. } => {
                        time::parse_shift(by)
                            .ok_or_else(|| at(format!("{by:?}: use [+|-][Nd]HH:MM:SS")))?;
                    }
                    Action::ClearCreator | Action::ClearCopyright | Action::RemoveGps => {}
                }
            }
        }
        Ok(())
    }

    /// Fields this Preset may change (for its summary).
    pub fn fields(&self) -> Vec<Field> {
        let mut out = Vec::new();
        for a in self
            .rules
            .iter()
            .filter(|r| r.enabled)
            .flat_map(|r| &r.then)
        {
            if !out.contains(&a.field()) {
                out.push(a.field());
            }
        }
        out
    }
}

/// Built-in general Presets (PRODUCT_SPEC §6.11).
pub fn builtin() -> Vec<Preset> {
    vec![
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            name: "Copyright Template".into(),
            rules: vec![Rule {
                name: "Copyright from creator and year where there is none".into(),
                enabled: true,
                when: vec![Condition::Empty {
                    field: Field::Copyright,
                }],
                then: vec![Action::SetCopyright {
                    value: "© {creator} {year}".into(),
                }],
            }],
        },
        Preset {
            schema_version: PRESET_SCHEMA_VERSION,
            name: "Remove GPS".into(),
            rules: vec![Rule {
                name: "Remove the position".into(),
                enabled: true,
                when: vec![],
                then: vec![Action::RemoveGps],
            }],
        },
    ]
}

/// The effective value of a field, as text; None when empty or unreadable.
fn field_text(t: &Target, f: Field) -> Option<String> {
    let v = match f {
        Field::Creator => creator::read_target(t).effective.map(|v| v.join("; ")),
        Field::Copyright => copyright::read_target(t).effective,
        Field::CaptureTime => capture::read_target(t)
            .ok()
            .flatten()
            .map(|c| capture::display(&c)),
        Field::Gps => gps::read_target(t).map(|p| p.display()),
    };
    v.filter(|s| !s.trim().is_empty())
}

fn kind_of(shown_path: &str, t: &Target) -> FileKind {
    match t {
        Target::Sidecar { .. } => FileKind::Raw,
        Target::Embedded(_) => {
            let ext = Path::new(shown_path)
                .extension()
                .map(|e| e.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            if ext == "xmp" {
                FileKind::Xmp
            } else {
                FileKind::Jpeg
            }
        }
    }
}

impl Condition {
    pub fn holds(&self, t: &Target, shown_path: &str) -> bool {
        match self {
            Condition::Empty { field } => field_text(t, *field).is_none(),
            Condition::NotEmpty { field } => field_text(t, *field).is_some(),
            Condition::Equals { field, value } => field_text(t, *field).as_deref() == Some(value),
            Condition::Contains { field, value } => field_text(t, *field)
                .is_some_and(|v| v.to_lowercase().contains(&value.to_lowercase())),
            Condition::Extension { any } => {
                let ext = Path::new(shown_path)
                    .extension()
                    .map(|e| e.to_string_lossy().to_lowercase())
                    .unwrap_or_default();
                any.iter()
                    .any(|a| a.trim_start_matches('.').to_lowercase() == ext)
            }
            Condition::Kind { kind } => kind_of(shown_path, t) == *kind,
        }
    }
}

/// The field plans of one file under a Preset: the actions of every enabled rule whose conditions
/// hold on the original snapshot, the last one per field winning. An empty result means no rule
/// applies (the file has no change).
pub fn plan_target(
    preset: &Preset,
    t: &Target,
    shown_path: &str,
) -> Vec<(&'static str, FieldPlan)> {
    let mut chosen: Vec<(Field, &Action, usize)> = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    let rule_name = |i: usize| {
        let r: &Rule = &preset.rules[i];
        if r.name.is_empty() {
            format!("rule {}", i + 1)
        } else {
            format!("rule {} \"{}\"", i + 1, r.name)
        }
    };
    for (i, r) in preset.rules.iter().enumerate() {
        if !r.enabled || !r.when.iter().all(|c| c.holds(t, shown_path)) {
            continue;
        }
        for a in &r.then {
            let f = a.field();
            if let Some(slot) = chosen.iter_mut().find(|(g, _, _)| *g == f) {
                notes.push(format!(
                    "{} overrides {} for {}",
                    rule_name(i),
                    rule_name(slot.2),
                    f.name()
                ));
                *slot = (f, a, i);
            } else {
                chosen.push((f, a, i));
            }
        }
    }
    let ctx = TemplateCtx::from_target(t, shown_path);
    let mut out: Vec<(&'static str, FieldPlan)> = chosen
        .into_iter()
        .map(|(f, a, _)| (f.name(), plan_action(a, t, &ctx)))
        .collect();
    if let Some((_, first)) = out.first_mut() {
        first.notes.splice(0..0, notes);
    }
    out
}

fn plan_action(a: &Action, t: &Target, ctx: &TemplateCtx) -> FieldPlan {
    let render = |s: &str| Template::parse(s).and_then(|tm| tm.render(ctx));
    match a {
        Action::SetCreator { names } => {
            match names
                .iter()
                .map(|n| render(n))
                .collect::<Result<Vec<_>, _>>()
            {
                Ok(v) => creator::plan_target(t, &CreatorEdit::Set(v)),
                Err(why) => FieldPlan::blocked(why),
            }
        }
        Action::ClearCreator => creator::plan_target(t, &CreatorEdit::Clear),
        Action::SetCopyright { value } => match render(value) {
            Ok(v) => copyright::plan_target(t, &CopyrightEdit::Set(v)),
            Err(why) => FieldPlan::blocked(why),
        },
        Action::ClearCopyright => copyright::plan_target(t, &CopyrightEdit::Clear),
        Action::SetGps { position } => match GeoPoint::parse(position) {
            Ok(p) => gps::plan_target(t, &GpsEdit::Set(p)),
            Err(why) => FieldPlan::blocked(why),
        },
        Action::RemoveGps => gps::plan_target(t, &GpsEdit::Remove),
        Action::SetTime { to, digitized } => match time::parse_local(to) {
            Some(l) => plan_time(t, &TimeOp::Absolute(l), false, *digitized),
            None => FieldPlan::blocked(format!("{to:?} is not a time")),
        },
        Action::ShiftTime { by, digitized } => match time::parse_shift(by) {
            Some(d) => plan_time(t, &TimeOp::Shift(d), true, *digitized),
            None => FieldPlan::blocked(format!("{by:?} is not a shift")),
        },
    }
}

/// A time operation that needs no other file (Absolute, Shift), for one file.
fn plan_time(t: &Target, op: &TimeOp, keep_subsec: bool, digitized: bool) -> FieldPlan {
    let item = TimeItem {
        id: 0,
        file_name: String::new(),
        time: capture::read_target(t).ok().flatten(),
    };
    let after = time::apply(op, &[item])
        .map_err(|e| e.to_string())
        .and_then(|mut r| {
            r.pop()
                .map(|r| r.after)
                .ok_or_else(|| "no result".to_string())
        });
    match after {
        Ok(after) => capture::plan_target(t, &after, keep_subsec, digitized),
        Err(why) => FieldPlan::blocked(why),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{EntryStatus, merge};
    use crate::snapshot::Snapshot;
    use serde_json::json;

    fn preset(json: &str) -> Preset {
        Preset::from_json(json).unwrap()
    }

    #[test]
    fn json_round_trip_and_schema_checks() {
        for p in builtin() {
            assert_eq!(Preset::from_json(&p.to_json()).unwrap(), p);
        }
        assert!(
            Preset::from_json(r#"{"name":"x","rules":[]}"#)
                .unwrap_err()
                .contains("schema_version")
        );
        assert!(Preset::from_json(r#"{"schema_version":2,"name":"x","rules":[]}"#).is_err());
        let bad = r#"{"schema_version":1,"name":"x","rules":[{"then":[{"do":"set_copyright","value":"{index}"}]}]}"#;
        assert!(
            Preset::from_json(bad)
                .unwrap_err()
                .contains("unknown variable")
        );
        let unknown = r#"{"schema_version":1,"name":"x","rules":[{"then":[{"do":"rename"}]}]}"#;
        assert!(Preset::from_json(unknown).is_err());
        let extra =
            r#"{"schema_version":1,"name":"x","rules":[{"then":[{"do":"remove_gps"}],"also":1}]}"#;
        assert!(Preset::from_json(extra).is_err());
    }

    #[test]
    fn import_limits() {
        let big = format!(
            r#"{{"schema_version":1,"name":"{}","rules":[{{"then":[{{"do":"remove_gps"}}]}}]}}"#,
            "x".repeat(MAX_PRESET_BYTES)
        );
        assert!(Preset::from_json(&big).unwrap_err().contains("at most"));
        let rule = r#"{"then":[{"do":"remove_gps"}]}"#;
        let many = format!(
            r#"{{"schema_version":1,"name":"x","rules":[{}]}}"#,
            vec![rule; MAX_RULES + 1].join(",")
        );
        assert!(Preset::from_json(&many).unwrap_err().contains("rules"));
        let cond = r#"{"if":"empty","field":"gps"}"#;
        let conds = format!(
            r#"{{"schema_version":1,"name":"x","rules":[{{"when":[{}],"then":[{{"do":"remove_gps"}}]}}]}}"#,
            vec![cond; MAX_CONDITIONS + 1].join(",")
        );
        assert!(
            Preset::from_json(&conds)
                .unwrap_err()
                .contains("conditions")
        );
    }

    #[test]
    fn conditions_on_the_original_snapshot() {
        let snap = Snapshot::from_json(&json!({
            "IFD0:Artist": "Morii",
            "IFD0:Model": "NIKON Z 8",
        }));
        let t = Target::Embedded(&snap);
        let p = r"C:\p\a.JPG";
        assert!(
            Condition::Empty {
                field: Field::Copyright
            }
            .holds(&t, p)
        );
        assert!(
            Condition::NotEmpty {
                field: Field::Creator
            }
            .holds(&t, p)
        );
        assert!(
            Condition::Equals {
                field: Field::Creator,
                value: "Morii".into()
            }
            .holds(&t, p)
        );
        assert!(
            Condition::Contains {
                field: Field::Creator,
                value: "MOR".into()
            }
            .holds(&t, p)
        );
        assert!(
            Condition::Extension {
                any: vec![".jpg".into()]
            }
            .holds(&t, p)
        );
        assert!(
            Condition::Kind {
                kind: FileKind::Jpeg
            }
            .holds(&t, p)
        );
        assert!(
            !Condition::Kind {
                kind: FileKind::Raw
            }
            .holds(&t, p)
        );
        assert!(
            !Condition::Empty {
                field: Field::Creator
            }
            .holds(&t, p)
        );
    }

    #[test]
    fn later_rules_override_and_conditions_ignore_earlier_rules() {
        let p = preset(
            r#"{"schema_version":1,"name":"t","rules":[
                {"name":"a","then":[{"do":"set_copyright","value":"© A"}]},
                {"name":"b","when":[{"if":"empty","field":"copyright"}],"then":[{"do":"set_copyright","value":"© B {creator}"}]},
                {"name":"off","enabled":false,"then":[{"do":"remove_gps"}]}
            ]}"#,
        );
        let snap = Snapshot::from_json(&json!({"IFD0:Artist": "Morii"}));
        let fields = plan_target(&p, &Target::Embedded(&snap), r"C:\p\a.jpg");
        // rule b still sees the original (empty) copyright although rule a sets one
        assert_eq!(fields.len(), 1);
        let m = merge(fields);
        assert_eq!(m.status, EntryStatus::Ready);
        assert_eq!(m.changes[0].after, Some(vec!["© B Morii".to_string()]));
        assert!(
            m.notes.iter().any(|n| n.contains("overrides")),
            "{:?}",
            m.notes
        );
    }

    #[test]
    fn time_actions_per_file() {
        let p = preset(
            r#"{"schema_version":1,"name":"t","rules":[
                {"when":[{"if":"not_empty","field":"capture_time"}],"then":[{"do":"shift_time","by":"+01:00:00"}]},
                {"when":[{"if":"empty","field":"capture_time"}],"then":[{"do":"set_time","to":"2024:01:02 03:04:05"}]}
            ]}"#,
        );
        let with = Snapshot::from_json(&json!({"ExifIFD:DateTimeOriginal": "2020:05:06 07:08:09"}));
        let m = merge(plan_target(&p, &Target::Embedded(&with), r"C:\p\a.jpg"));
        assert_eq!(m.status, EntryStatus::Ready);
        assert_eq!(m.changes[0].field, "capture_time");
        assert!(
            format!("{:?}", m.changes[0].after).contains("2020:05:06 08:08:09"),
            "{m:?}"
        );
        let without = Snapshot::from_json(&json!({}));
        let m = merge(plan_target(&p, &Target::Embedded(&without), r"C:\p\b.jpg"));
        assert!(
            format!("{:?}", m.changes[0].after).contains("2024:01:02 03:04:05"),
            "{m:?}"
        );
        assert!(Preset::from_json(
            r#"{"schema_version":1,"name":"x","rules":[{"then":[{"do":"shift_time","by":"1 hour"}]}]}"#
        )
        .is_err());
    }

    #[test]
    fn a_blocked_field_does_not_stop_the_others() {
        let p = preset(
            r#"{"schema_version":1,"name":"t","rules":[
                {"then":[{"do":"set_copyright","value":"© {year}"},{"do":"set_creator","names":["Morii"]}]}
            ]}"#,
        );
        let snap = Snapshot::from_json(&json!({}));
        let m = merge(plan_target(&p, &Target::Embedded(&snap), r"C:\p\a.jpg"));
        assert_eq!(m.status, EntryStatus::Ready);
        assert_eq!(m.changes.len(), 1);
        assert_eq!(m.changes[0].field, "creator");
        assert!(
            m.notes
                .iter()
                .any(|n| n.starts_with("copyright not changed")),
            "{:?}",
            m.notes
        );
        // no rule applies: nothing to plan
        let none = preset(
            r#"{"schema_version":1,"name":"t","rules":[
                {"when":[{"if":"not_empty","field":"gps"}],"then":[{"do":"remove_gps"}]}
            ]}"#,
        );
        assert!(plan_target(&none, &Target::Embedded(&snap), r"C:\p\a.jpg").is_empty());
        assert_eq!(merge(vec![]).status, EntryStatus::NoChange);
    }
}
