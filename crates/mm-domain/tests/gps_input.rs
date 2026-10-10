// SPDX-License-Identifier: GPL-3.0-or-later
//! GPS input cases shared with the batch editor's own parser
//! (`apps/desktop/src/features/inspector/gpsInput.test.ts` reads the same file): the backend and
//! the UI accept and refuse the same texts and read the same position from them.

use mm_domain::gps::GeoPoint;

#[test]
fn the_shared_gps_input_cases() {
    let file: serde_json::Value =
        serde_json::from_str(include_str!("gps_input_cases.json")).unwrap();
    let cases = file["cases"].as_array().unwrap();
    assert!(cases.len() > 20);
    for case in cases {
        let input = case[0].as_str().unwrap();
        let got = GeoPoint::parse(input);
        match case[1].as_array() {
            None => assert!(got.is_err(), "{input:?} should be refused, got {got:?}"),
            Some(want) => {
                let p = got.unwrap_or_else(|e| panic!("{input:?} refused: {e}"));
                let near = |a: f64, b: &serde_json::Value| (a - b.as_f64().unwrap()).abs() < 1e-6;
                assert!(
                    near(p.lat, &want[0]) && near(p.lon, &want[1]),
                    "{input:?}: {p:?}"
                );
                match (p.alt, want[2].as_f64()) {
                    (None, None) => {}
                    (Some(a), Some(b)) => assert!((a - b).abs() < 1e-6, "{input:?}: {p:?}"),
                    _ => panic!("{input:?}: altitude {:?}, want {}", p.alt, want[2]),
                }
            }
        }
    }
}
