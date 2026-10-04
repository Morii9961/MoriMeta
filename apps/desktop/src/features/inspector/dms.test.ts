// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { dms } from './FileInspector'

describe('dms', () => {
  it('turns the GPS display into degrees, minutes and seconds', () => {
    expect(dms('35.6894875, 139.6917064')).toBe('35°41′22.2″N 139°41′30.1″E')
    expect(dms('-33.8567844, -70.6482610, 520.00 m')).toBe('33°51′24.4″S 70°38′53.7″W')
    expect(dms('0.0000000, 0.0000000')).toBe('0°00′00.0″N 0°00′00.0″E')
  })
  it('carries rounding into the next minute and degree', () => {
    expect(dms('10.9999999, 20.0000001')).toBe('11°00′00.0″N 20°00′00.0″E')
  })
  it('gives nothing for a value it cannot read', () => {
    expect(dms(null)).toBeNull()
    expect(dms('')).toBeNull()
    expect(dms('north')).toBeNull()
  })
})
