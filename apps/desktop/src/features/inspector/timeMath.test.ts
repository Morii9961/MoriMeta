// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { compareExifOrder, exifOrder, formatExif, localFromParts, naturalCompare, parseExif, parseShift, subsecOf } from './timeMath'
// defined by the backend (mm_domain::time) and checked there by crates/mm-domain/tests/time_text.rs
import shared from '../../../../../crates/mm-domain/tests/time_text_cases.json'

describe('the time preview follows the backend', () => {
  it('orders file names as a Sequence by name does', () => {
    const want = shared.natural_order
    expect(want.length).toBeGreaterThan(15)
    for (const start of [[...want].reverse(), [...want.slice(9), ...want.slice(0, 9)]]) {
      expect([...start].sort(naturalCompare)).toEqual(want)
    }
  })
  it('reads Shift amounts as the backend does', () => {
    for (const [text, want] of shared.shift as [string, number | null][]) {
      expect(parseShift(text), JSON.stringify(text)).toBe(want)
    }
  })
  it('orders by capture time with the sub-seconds, as the backend does', () => {
    const burst = ['2026:01:01 10:00:00.50', '2026:01:01 10:00:00.5', '2026:01:01 10:00:00.07', '2026:01:01 10:00:00']
    const keys = burst.map(exifOrder)
    expect(compareExifOrder(keys[0], keys[1])).toBe(0)
    expect(compareExifOrder(keys[3], keys[2])).toBe(-1)
    expect(compareExifOrder(keys[2], keys[1])).toBe(-1)
    expect(compareExifOrder(exifOrder('2026:01:01 10:00:00.999999999+09:00'), exifOrder('2026:01:01 10:00:01'))).toBe(-1)
    expect(compareExifOrder(null, keys[0])).toBe(1)
  })
  it('keeps the sub-seconds Shift keeps', () => {
    expect(subsecOf('2023:06:02 18:53:25.67+02:00')).toBe('.67')
    expect(subsecOf('2023:06:02 18:53:25+02:00')).toBe('')
    expect(subsecOf(null)).toBe('')
  })
  it('handles the whole EXIF range, years 0001 to 0099 included', () => {
    for (const v of ['0001:01:01 00:00:00', '0050:02:28 12:00:00', '0099:12:31 23:59:59', '9999:12:31 23:59:59', '2024:02:29 00:00:00']) {
      expect(formatExif(parseExif(v)!)).toBe(v)
    }
    expect(parseExif('0000:01:01 00:00:00')).toBeNull()
    expect(localFromParts('0050-06-01', '8:00')).toBe('0050:06:01 08:00:00')
    expect(formatExif(parseExif('0001:01:01 00:00:30')! - 60)).not.toMatch(/^0001/)
  })
})
