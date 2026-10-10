// SPDX-License-Identifier: GPL-3.0-or-later
//! Property tests for the pure time tools, the text forms they read back and GPS input
//! (DEVELOPMENT_PLAN §5.1): random capture times over the whole EXIF range, weighted towards
//! day, month, year and leap-day boundaries, with and without sub-seconds and offsets.

use chrono::{DateTime, Datelike, FixedOffset, NaiveDate, NaiveDateTime, TimeDelta};
use mm_domain::gps::GeoPoint;
use mm_domain::time::{
    CaptureTime, SequenceOrder, TimeItem, TimeOp, TimeOpError, apply, format_local, format_shift,
    natural_cmp, parse_local, parse_offset, parse_shift,
};
use proptest::prelude::*;
use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

/// Cases per property: the default here, or `MM_PROPTEST_CASES` (the nightly run uses many more).
fn cases(default: u32) -> u32 {
    std::env::var("MM_PROPTEST_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// 0001-01-01 00:00:00 and 9999-12-31 23:59:59 as Unix seconds: what EXIF can store.
const MIN: i64 = -62_135_596_800;
const MAX: i64 = 253_402_300_799;

fn naive(secs: i64) -> NaiveDateTime {
    DateTime::from_timestamp(secs, 0).unwrap().naive_utc()
}

fn in_exif_range(t: NaiveDateTime) -> bool {
    (1..=9999).contains(&t.year())
}

/// Anywhere in the range, or a few seconds around midnight at the end of a month, a year or
/// February (leap years included), or at the ends of the range.
fn local() -> impl Strategy<Value = NaiveDateTime> {
    let edge = (1i32..=9999, 1u32..=12, -90i64..=90).prop_map(|(y, m, s)| {
        let first_of_next = if m == 12 {
            NaiveDate::from_ymd_opt(y + 1, 1, 1)
        } else {
            NaiveDate::from_ymd_opt(y, m + 1, 1)
        };
        let midnight = first_of_next.map_or(MAX, |d| {
            d.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp()
        });
        naive((midnight + s).clamp(MIN, MAX))
    });
    let leap = (0i32..=2499, -90i64..=90).prop_map(|(q, s)| {
        let y = 4 * q.max(1);
        let feb29 =
            NaiveDate::from_ymd_opt(y, 2, 29).unwrap_or(NaiveDate::from_ymd_opt(y, 2, 28).unwrap());
        naive((feb29.and_hms_opt(0, 0, 0).unwrap().and_utc().timestamp() + s).clamp(MIN, MAX))
    });
    prop_oneof![
        4 => (MIN..=MAX).prop_map(naive),
        3 => edge,
        2 => leap,
        1 => prop_oneof![MIN..=MIN + 200, MAX - 200..=MAX].prop_map(naive),
    ]
}

fn subsec() -> impl Strategy<Value = Option<String>> {
    proptest::option::of("[0-9]{1,9}")
}

fn offset() -> impl Strategy<Value = Option<FixedOffset>> {
    proptest::option::of((-720i32..=840).prop_map(|m| FixedOffset::east_opt(m * 60).unwrap()))
}

fn capture_time() -> impl Strategy<Value = CaptureTime> {
    (local(), subsec(), offset()).prop_map(|(local, ss, off)| {
        let off = off.map(|o| {
            let s = o.local_minus_utc();
            format!(
                "{}{:02}:{:02}",
                if s < 0 { '-' } else { '+' },
                s.abs() / 3600,
                s.abs() % 3600 / 60
            )
        });
        CaptureTime::from_exif(&format_local(local), ss.as_deref(), off.as_deref()).unwrap()
    })
}

/// Seconds to a few days, or up to 400 years either way.
fn delta() -> impl Strategy<Value = TimeDelta> {
    prop_oneof![
        3 => -300_000i64..=300_000,
        1 => -12_622_780_800i64..=12_622_780_800,
    ]
    .prop_map(TimeDelta::seconds)
}

fn items(times: Vec<CaptureTime>) -> Vec<TimeItem> {
    times
        .into_iter()
        .enumerate()
        .map(|(i, t)| TimeItem {
            id: i as u64,
            file_name: format!("IMG_{i}.JPG"),
            time: Some(t),
            pair: None,
        })
        .collect()
}

fn expected(t: NaiveDateTime, d: TimeDelta) -> Result<NaiveDateTime, TimeOpError> {
    t.checked_add_signed(d)
        .filter(|t| in_exif_range(*t))
        .ok_or(TimeOpError::OutOfRange)
}

proptest! {
    // a failure prints the shrunk case; nothing is written next to the sources
    #![proptest_config(ProptestConfig { cases: cases(1024), failure_persistence: None, ..ProptestConfig::default() })]

    #[test]
    fn shift_moves_wall_clock_only_and_is_reversible(t in capture_time(), d in delta()) {
        let r = apply(&TimeOp::Shift(d), &items(vec![t.clone()])).unwrap();
        let after = &r[0].after;
        prop_assert_eq!(after.as_ref().map(|a| a.local).map_err(Clone::clone), expected(t.local, d));
        if let Ok(a) = after {
            prop_assert_eq!(&a.subsec, &t.subsec);
            prop_assert_eq!(a.offset, t.offset);
            let back = apply(&TimeOp::Shift(-d), &items(vec![a.clone()])).unwrap();
            prop_assert_eq!(back[0].after.as_ref().unwrap(), &t);
        }
    }

    #[test]
    fn preserve_relative_keeps_every_interval(
        times in proptest::collection::vec(capture_time(), 1..12),
        pick in any::<prop::sample::Index>(),
        new_local in local(),
    ) {
        let anchor = pick.index(times.len());
        let list = items(times.clone());
        let r = apply(&TimeOp::PreserveRelative { anchor: anchor as u64, new_local }, &list).unwrap();
        prop_assert_eq!(r[anchor].after.as_ref().unwrap().local, new_local);
        let d = new_local - times[anchor].local;
        for (i, x) in r.iter().enumerate() {
            prop_assert_eq!(x.id, i as u64);
            prop_assert_eq!(x.after.as_ref().map(|a| a.local).map_err(Clone::clone), expected(times[i].local, d));
        }
        // between any two files that could be moved, the interval is unchanged
        let ok: Vec<(usize, NaiveDateTime)> =
            r.iter().enumerate().filter_map(|(i, x)| x.after.as_ref().ok().map(|a| (i, a.local))).collect();
        for &(i, a) in &ok {
            for &(j, b) in &ok {
                prop_assert_eq!(a - b, times[i].local - times[j].local);
            }
        }
    }

    #[test]
    fn sequence_by_capture_time_keeps_the_order_and_spaces_evenly(
        times in proptest::collection::vec(capture_time(), 1..16),
        start in local(),
        step in 0i64..=86_400 * 40,
    ) {
        let step = TimeDelta::seconds(step);
        let list = items(times.clone());
        let r = apply(&TimeOp::Sequence { start, step, order: SequenceOrder::CaptureTimeThenName }, &list).unwrap();
        prop_assert_eq!(r.len(), list.len());
        prop_assert_eq!(r.iter().map(|x| x.id).collect::<HashSet<_>>().len(), list.len());
        let mut prev: Option<(NaiveDateTime, u64)> = None;
        for (pos, x) in r.iter().enumerate() {
            prop_assert_eq!(x.index, pos);
            let t = &times[x.id as usize];
            // sub-seconds count in the order: compare as nanoseconds
            let key = (t.local, t.subsec.as_ref().map_or(0, |s| format!("{:0<9}", s.as_str()).parse().unwrap()));
            if let Some(p) = prev {
                prop_assert!(p <= key, "sorted by capture time");
            }
            prev = Some(key);
            let want = step.checked_mul(pos as i32).ok_or(TimeOpError::OutOfRange).and_then(|d| expected(start, d));
            prop_assert_eq!(x.after.as_ref().map(|a| a.local).map_err(Clone::clone), want);
            if let Ok(a) = &x.after {
                prop_assert!(a.subsec.is_none());
                prop_assert_eq!(a.offset, t.offset);
            }
        }
    }

    #[test]
    fn paired_files_share_one_sequence_position(
        pairs in proptest::collection::vec(proptest::option::of(0u8..4), 1..16),
        start in local(),
        order in prop_oneof![Just(SequenceOrder::NaturalFileName), Just(SequenceOrder::CaptureTimeThenName)],
    ) {
        let t = CaptureTime::from_exif("2024:02:29 23:59:59", None, None).unwrap();
        let mut list = items(vec![t; pairs.len()]);
        for (it, p) in list.iter_mut().zip(&pairs) {
            it.pair = p.map(|k| format!("dir/dsc_{k}"));
        }
        let step = TimeDelta::seconds(1);
        let r = apply(&TimeOp::Sequence { start, step, order }, &list).unwrap();
        // each position belongs to one pair, or to one file without a pair, and back
        let mut owner_of: HashMap<usize, String> = HashMap::new();
        let mut position_of: HashMap<String, usize> = HashMap::new();
        for x in &r {
            let owner = list[x.id as usize].pair.clone().unwrap_or_else(|| format!("#{}", x.id));
            prop_assert_eq!(owner_of.entry(x.index).or_insert(owner.clone()), &owner);
            prop_assert_eq!(*position_of.entry(owner).or_insert(x.index), x.index);
        }
        // positions are 0..n with no gap
        prop_assert_eq!(owner_of.keys().copied().collect::<HashSet<_>>(), (0..position_of.len()).collect());
    }

    #[test]
    fn absolute_sets_every_file_and_keeps_its_offset(
        times in proptest::collection::vec(proptest::option::of(capture_time()), 1..8),
        l in local(),
    ) {
        let list: Vec<TimeItem> = times
            .iter()
            .enumerate()
            .map(|(i, t)| TimeItem { id: i as u64, file_name: format!("{i}"), time: t.clone(), pair: None })
            .collect();
        let r = apply(&TimeOp::Absolute(l), &list).unwrap();
        for (x, t) in r.iter().zip(&times) {
            let a = x.after.as_ref().unwrap();
            prop_assert_eq!(a.local, l);
            prop_assert!(a.subsec.is_none());
            prop_assert_eq!(a.offset, t.as_ref().and_then(|t| t.offset));
        }
    }

    #[test]
    fn text_forms_read_back(t in capture_time(), d in delta()) {
        prop_assert_eq!(parse_local(&format_local(t.local)), Some(t.local));
        prop_assert_eq!(parse_shift(&format_shift(d)), Some(d));
        let again = CaptureTime::from_exif(
            &t.exif_datetime(),
            t.subsec.as_ref().map(|s| s.as_str()),
            t.exif_offset().as_deref(),
        )
        .unwrap();
        prop_assert_eq!(&again, &t);
        if let Some(o) = t.exif_offset() {
            prop_assert_eq!(parse_offset(&o), t.offset);
            prop_assert!(t.xmp().ends_with(&o));
        }
    }

    #[test]
    fn offsets_outside_minus_12_plus_14_are_refused(m in -1439i32..=1439) {
        let s = format!("{}{:02}:{:02}", if m < 0 { '-' } else { '+' }, m.abs() / 60, m.abs() % 60);
        let ok = (-720..=840).contains(&m);
        prop_assert_eq!(parse_offset(&s).is_some(), ok, "{}", s);
    }

    #[test]
    fn readers_never_panic_on_any_text(s in any::<String>(), t in "[0-9:+\\-d .]{0,24}") {
        for x in [&s, &t] {
            let _ = parse_shift(x);
            let _ = parse_offset(x);
            if let Some(l) = parse_local(x) {
                prop_assert!(in_exif_range(l));
            }
            let _ = CaptureTime::from_exif(x, Some(x), None);
            if let Ok(p) = GeoPoint::parse(x) {
                prop_assert!(p.validate().is_ok());
            }
        }
    }

    #[test]
    fn natural_order_is_a_total_order(
        mut names in proptest::collection::vec("([0-9]{1,3}|[aAbB_. ]|é|É|ẞ|ß|İ|Ⅻ|٣|１){0,6}", 2..8),
    ) {
        for a in &names {
            for b in &names {
                let ab = natural_cmp(a, b);
                prop_assert_eq!(ab, natural_cmp(b, a).reverse());
                prop_assert_eq!(ab == Ordering::Equal, a == b);
            }
        }
        // sorting panics or misorders when the comparison is not transitive
        names.sort_by(|a, b| natural_cmp(a, b));
        for i in 0..names.len() {
            for j in i + 1..names.len() {
                prop_assert_ne!(natural_cmp(&names[i], &names[j]), Ordering::Greater);
            }
        }
    }

    #[test]
    fn camera_numbers_sort_by_value_whatever_the_case(
        mut files in proptest::collection::vec((0u32..100_000, any::<bool>()), 1..20),
    ) {
        let name = |(n, upper): (u32, bool)| if upper { format!("DSC_{n}.JPG") } else { format!("dsc_{n}.jpg") };
        let mut names: Vec<String> = files.iter().copied().map(name).collect();
        names.sort_by(|a, b| natural_cmp(a, b));
        files.sort_by_key(|&(n, _)| n);
        let numbers = |v: &[String]| v.iter().map(|s| s[4..s.len() - 4].parse::<u32>().unwrap()).collect::<Vec<_>>();
        prop_assert_eq!(numbers(&names), files.iter().map(|&(n, _)| n).collect::<Vec<_>>());
    }

    #[test]
    fn gps_decimal_display_reads_back(lat in -90.0f64..=90.0, lon in -180.0f64..=180.0) {
        let p = GeoPoint { lat, lon, alt: None };
        let q = GeoPoint::parse(&p.display()).unwrap();
        prop_assert!((q.lat - lat).abs() <= 5e-8 && (q.lon - lon).abs() <= 5e-8, "{:?}", q);
    }

    #[test]
    fn gps_degrees_minutes_seconds_in_any_order(
        lat_ms in 0u64..=90 * 3_600_000,
        lon_ms in 0u64..=180 * 3_600_000,
        south: bool,
        west: bool,
        prefix: bool,
        lon_first: bool,
    ) {
        // milliseconds of arc: written with three decimals on the seconds
        let dms = |ms: u64| (ms / 3_600_000, ms / 60_000 % 60, (ms % 60_000) as f64 / 1000.0);
        let coord = |ms: u64, h: char| {
            let (d, m, s) = dms(ms);
            if prefix { format!("{h} {d}° {m}′ {s:.3}″") } else { format!("{d}°{m}′{s:.3}″{h}") }
        };
        let la = coord(lat_ms, if south { 'S' } else { 'N' });
        let lo = coord(lon_ms, if west { 'W' } else { 'E' });
        let text = if lon_first { format!("{lo}, {la}") } else { format!("{la} {lo}") };
        let p = GeoPoint::parse(&text).unwrap();
        let sign = |neg: bool| if neg { -1.0 } else { 1.0 };
        let (wl, wo) = (sign(south) * lat_ms as f64 / 3.6e6, sign(west) * lon_ms as f64 / 3.6e6);
        prop_assert!((p.lat - wl).abs() < 1e-9 && (p.lon - wo).abs() < 1e-9, "{} -> {:?}", text, p);
    }
}
