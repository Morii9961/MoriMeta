// SPDX-License-Identifier: GPL-3.0-or-later
//! Time-tool cases shared with the time tools' preview in the UI
//! (`apps/desktop/src/features/inspector/timeMath.test.ts` reads the same file): the preview
//! must order files and read Shift amounts exactly as the Plan will.

use mm_domain::time::{natural_cmp, parse_shift};

fn cases() -> serde_json::Value {
    serde_json::from_str(include_str!("time_text_cases.json")).unwrap()
}

#[test]
fn the_shared_natural_order() {
    let want: Vec<String> = serde_json::from_value(cases()["natural_order"].clone()).unwrap();
    assert!(want.len() > 15);
    for shuffled in [want.iter().rev().cloned().collect::<Vec<_>>(), {
        let mut v = want.clone();
        v.rotate_left(want.len() / 2);
        v
    }] {
        let mut got = shuffled;
        got.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(got, want);
    }
}

#[test]
fn the_shared_shift_texts() {
    for case in cases()["shift"].as_array().unwrap() {
        let text = case[0].as_str().unwrap();
        let want = case[1].as_i64();
        assert_eq!(parse_shift(text).map(|d| d.num_seconds()), want, "{text:?}");
    }
}
