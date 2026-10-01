// SPDX-License-Identifier: GPL-3.0-or-later
//! The user's settings (PRODUCT_SPEC §6.16, SCREEN_SPEC Settings): every key the backend knows,
//! with its default and its check. A value is checked before it is stored; unknown keys are
//! refused, so a typo cannot silently do nothing.

use std::path::Path;

use mm_store::Store;

use crate::CoreError;

pub struct Key {
    pub name: &'static str,
    pub default: &'static str,
    pub about: &'static str,
    check: fn(&str) -> Result<(), String>,
}

fn number<T: std::str::FromStr>(v: &str) -> Result<T, String> {
    v.parse().map_err(|_| format!("{v:?} is not a number"))
}

fn check_share(v: &str) -> Result<(), String> {
    let x: f64 = number(v)?;
    (0.0..=1.0)
        .contains(&x)
        .then_some(())
        .ok_or_else(|| "a share between 0 and 1".into())
}

fn check_days(v: &str) -> Result<(), String> {
    let d: u32 = number(v)?;
    (1..=3650)
        .contains(&d)
        .then_some(())
        .ok_or_else(|| "between 1 and 3650 days".into())
}

fn check_count(v: &str) -> Result<(), String> {
    number::<usize>(v).map(|_| ())
}

fn check_root(v: &str) -> Result<(), String> {
    let p = Path::new(v);
    if !p.is_absolute() {
        return Err("an absolute folder path".into());
    }
    Ok(())
}

fn check_bool(v: &str) -> Result<(), String> {
    matches!(v, "true" | "false")
        .then_some(())
        .ok_or_else(|| "true or false".into())
}

fn check_workers(v: &str) -> Result<(), String> {
    let n: usize = number(v)?;
    (1..=16)
        .contains(&n)
        .then_some(())
        .ok_or_else(|| "between 1 and 16".into())
}

fn check_ms(v: &str) -> Result<(), String> {
    number::<i64>(v).map(|_| ())
}

fn check_creator(v: &str) -> Result<(), String> {
    let names: Vec<String> = v.split("; ").map(str::to_owned).collect();
    for n in &names {
        mm_domain::template::Template::parse(n)?;
    }
    if names.iter().any(|n| n.contains('{')) {
        return Ok(()); // a template: checked per file when rendered
    }
    mm_domain::creator::validate(&names)
}

fn check_update_check(v: &str) -> Result<(), String> {
    matches!(v, "ask" | "weekly" | "never")
        .then_some(())
        .ok_or_else(|| "ask, weekly or never".into())
}

fn check_copyright(v: &str) -> Result<(), String> {
    let t = mm_domain::template::Template::parse(v)?;
    if t.is_literal() {
        mm_domain::copyright::validate(v)?;
    }
    Ok(())
}

pub const KEYS: &[Key] = &[
    Key {
        name: "backup.root",
        default: "",
        about: "where later Operations keep their backups (empty: <data>/backups)",
        check: check_root,
    },
    Key {
        name: crate::retention::SETTING_MAX_AGE_DAYS,
        default: "30",
        about: "backups older than this are pruned (D-7)",
        check: check_days,
    },
    Key {
        name: crate::retention::SETTING_MAX_SHARE,
        default: "0.1",
        about: "all backups together stay below this share of their volume (D-7)",
        check: check_share,
    },
    Key {
        name: crate::retention::SETTING_KEEP_LATEST,
        default: "10",
        about: "the most recent Operations are never pruned automatically",
        check: check_count,
    },
    Key {
        name: "metadata.preserve_mtime",
        default: "false",
        about: "keep each written file's modification time (D-6)",
        check: check_bool,
    },
    Key {
        name: "metadata.default_creator",
        default: "",
        about: "proposed Creator (names separated by \"; \"; templates allowed)",
        check: check_creator,
    },
    Key {
        name: "metadata.copyright_template",
        default: "© {creator} {year}",
        about: "proposed Copyright (templates allowed)",
        check: check_copyright,
    },
    Key {
        name: "exec.workers",
        default: "",
        about: "files written side by side (empty: half the cores, at most 4)",
        check: check_workers,
    },
    Key {
        name: "ui.setup_done",
        default: "false",
        about: "the first-launch setup was completed or skipped (SCREEN_SPEC §13)",
        check: check_bool,
    },
    Key {
        name: "updates.check",
        default: "ask",
        about: "whether to look for updates: asked at first launch, nothing chosen in advance (D-4)",
        check: check_update_check,
    },
    Key {
        name: crate::log::SETTING_DEBUG_SINCE,
        default: "",
        about: "debug log on since (ms); it ends by itself after 24 hours",
        check: check_ms,
    },
];

pub fn key(name: &str) -> Result<&'static Key, CoreError> {
    KEYS.iter().find(|k| k.name == name).ok_or_else(|| {
        CoreError::Input(format!(
            "unknown setting {name:?}; known: {}",
            KEYS.iter().map(|k| k.name).collect::<Vec<_>>().join(", ")
        ))
    })
}

/// Store a setting after checking it; an empty value clears it (back to the default).
pub fn set(store: &mut Store, name: &str, value: &str) -> Result<(), CoreError> {
    let k = key(name)?;
    if value.is_empty() {
        store.clear_setting(name)?;
        return Ok(());
    }
    (k.check)(value).map_err(|e| CoreError::Input(format!("{name}: {e}")))?;
    store.set_setting(name, value)?;
    Ok(())
}

/// Settings › Advanced "Reset": every setting back to its default. Later Operations keep their
/// backups in the default location again; earlier ones keep the folders they recorded.
pub fn reset_all(store: &mut Store) -> Result<(), CoreError> {
    for k in KEYS {
        store.clear_setting(k.name)?;
    }
    Ok(())
}

/// Files written side by side: the `exec.workers` setting, else ARCHITECTURE §8's
/// clamp(physical cores / 2, 1, 4) (logical cores / 2 approximates it).
pub fn workers(store: &Store) -> usize {
    store
        .setting("exec.workers")
        .ok()
        .flatten()
        .and_then(|v| v.parse().ok())
        .filter(|n| (1..=16).contains(n))
        .unwrap_or_else(|| {
            std::thread::available_parallelism()
                .map(|n| n.get() / 2)
                .unwrap_or(1)
                .clamp(1, 4)
        })
}

/// The stored value or the default.
pub fn get(store: &Store, name: &str) -> Result<String, CoreError> {
    let k = key(name)?;
    Ok(store.setting(name)?.unwrap_or_else(|| k.default.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn values_are_checked_before_they_are_kept() {
        let d = std::env::temp_dir().join(format!("mm-settings-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        let mut s = Store::open(&d).unwrap();
        assert_eq!(get(&s, "backup.max_age_days").unwrap(), "30");
        set(&mut s, "backup.max_age_days", "7").unwrap();
        assert_eq!(get(&s, "backup.max_age_days").unwrap(), "7");
        for (k, bad) in [
            ("backup.max_age_days", "0"),
            ("backup.max_share_of_volume", "2"),
            ("backup.root", "relative\\folder"),
            ("metadata.preserve_mtime", "yes"),
            ("exec.workers", "64"),
            ("metadata.copyright_template", "© {index}"),
            ("metadata.default_creator", "line\nbreak"),
            ("no.such.key", "1"),
        ] {
            assert!(set(&mut s, k, bad).is_err(), "{k} = {bad:?} was accepted");
        }
        assert_eq!(
            get(&s, "backup.max_age_days").unwrap(),
            "7",
            "unchanged by a refusal"
        );
        set(&mut s, "metadata.copyright_template", "© {year} Studio").unwrap();
        assert!((1..=4).contains(&workers(&s)), "default");
        set(&mut s, "exec.workers", "7").unwrap();
        assert_eq!(workers(&s), 7);
        set(&mut s, "backup.max_age_days", "").unwrap();
        assert_eq!(get(&s, "backup.max_age_days").unwrap(), "30");
        reset_all(&mut s).unwrap();
        for k in KEYS {
            assert_eq!(get(&s, k.name).unwrap(), k.default, "{}", k.name);
        }
        drop(s);
        let _ = std::fs::remove_dir_all(&d);
    }
}
