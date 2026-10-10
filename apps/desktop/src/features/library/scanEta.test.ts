// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { etaParts, scanSecondsLeft } from './scanEta'

describe('scan time left', () => {
  const start = { ms: 0, done: 0 }
  it('waits for enough of a rate to show', () => {
    expect(scanSecondsLeft(start, 2000, 500, 5000)).toBeNull()
    expect(scanSecondsLeft(start, 10_000, 10, 5000)).toBeNull()
    expect(scanSecondsLeft(start, 10_000, 5000, 5000)).toBeNull()
  })
  it('extrapolates the rate since the scan started', () => {
    expect(scanSecondsLeft(start, 10_000, 1000, 5000)).toBe(40)
    expect(scanSecondsLeft({ ms: 5000, done: 1000 }, 15_000, 2000, 5000)).toBe(30)
  })
  it('rounds as a person reads it', () => {
    expect(etaParts(1)).toEqual({ unit: 's', n: 5 })
    expect(etaParts(41)).toEqual({ unit: 's', n: 45 })
    expect(etaParts(61)).toEqual({ unit: 'min', n: 2 })
    expect(etaParts(600)).toEqual({ unit: 'min', n: 10 })
  })
})
