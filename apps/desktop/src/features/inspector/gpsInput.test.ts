// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { gpsPosition, parseGpsInput } from './gpsInput'
// the same cases the backend's GeoPoint::parse is tested with (crates/mm-domain/tests/gps_input.rs)
import shared from '../../../../../crates/mm-domain/tests/gps_input_cases.json'

const near = (s: string, lat: number, lon: number) => {
  const p = parseGpsInput(s)
  expect(p, s).not.toBeNull()
  expect(Math.abs(p!.lat - lat)).toBeLessThan(1e-6)
  expect(Math.abs(p!.lon - lon)).toBeLessThan(1e-6)
}

describe('GPS input', () => {
  it('takes decimal degrees with an optional altitude', () => {
    expect(parseGpsInput('35.6812345, 139.7671234, 40.5')).toEqual({ lat: 35.6812345, lon: 139.7671234, alt: 40.5 })
    expect(gpsPosition(parseGpsInput('-33.8688,151.2093')!)).toBe('-33.8688,151.2093')
  })
  it('takes degrees, minutes and seconds as the backend does', () => {
    near('35°41′22.2″N 139°41′30.1″E', 35.6895, 139.6916944)
    near('35°41\'22.2"N, 139°41\'30.1"E', 35.6895, 139.6916944)
    near('N 35 41.37, E 139 41.5', 35.6895, 139.6916667)
    near('33 51 24.4 S 70 38 53.7 W', -33.8567778, -70.64825)
    near('139°41′30.1″E 35°41′22.2″N', 35.6895, 139.6916944)
    near('51.5°N 0.12°w', 51.5, -0.12)
  })
  it('refuses what is not a position', () => {
    for (const bad of ['91,0', '0,181', 'a,b', '1', '1,2,3,4', '0,0,1000000', 'NaN,0', '35°61′N 139°E', '35.5°30′N 139°E', '35°N 139°N', '35°41′N', '35°41′N 139°41′E 40', '91°N 0°E', '35°41′22″X 139°E', '']) {
      expect(parseGpsInput(bad), bad).toBeNull()
    }
  })
  it('agrees with the backend on every shared case', () => {
    const cases = shared.cases as [string, [number, number, number | null] | null][]
    expect(cases.length).toBeGreaterThan(20)
    for (const [input, want] of cases) {
      const p = parseGpsInput(input)
      if (want === null) {
        expect(p, JSON.stringify(input)).toBeNull()
        continue
      }
      expect(p, JSON.stringify(input)).not.toBeNull()
      expect(Math.abs(p!.lat - want[0]), input).toBeLessThan(1e-6)
      expect(Math.abs(p!.lon - want[1]), input).toBeLessThan(1e-6)
      expect(p!.alt === null ? null : Math.round(p!.alt * 1e6) / 1e6, input).toBe(want[2])
    }
  })
})
