// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import type { Asset, Row } from '../../ipc/types'
import { filterItems, middleTruncate, sortItems, type Item } from './data'
import { formatShift, localFromParts, parseExif, parseShift } from '../inspector/timeMath'

const asset = (id: number, name: string, writable = true): Asset => ({
  id,
  name,
  path: `D:\\p\\${name}`,
  folder: 'D:\\p',
  ext: name.split('.').pop()!.toUpperCase(),
  writable,
  size: 1000 * id,
})
const row = (id: number, p: Partial<Row> = {}): Row => ({
  id,
  writes_to: 'in_file',
  creator: null,
  copyright: null,
  capture_time: null,
  gps: null,
  conflicts: [],
  invalid: [],
  make: null,
  model: null,
  lens: null,
  not_downloaded: false,
  error: null,
  ...p,
})

describe('library table', () => {
  const items: Item[] = [
    { asset: asset(1, 'IMG_10.jpg'), row: row(1, { creator: 'Morii', gps: '1, 2' }) },
    { asset: asset(2, 'IMG_2.jpg'), row: row(2) },
    { asset: asset(3, 'IMG_1.nef'), row: undefined },
  ]

  it('sorts names naturally and puts empty values last', () => {
    expect(sortItems(items, [{ key: 'name', dir: 1 }]).map((i) => i.asset.name)).toEqual(['IMG_1.nef', 'IMG_2.jpg', 'IMG_10.jpg'])
    expect(sortItems(items, [{ key: 'creator', dir: -1 }])[0].asset.name).toBe('IMG_10.jpg')
    expect(sortItems(items, [{ key: 'creator', dir: 1 }])[0].asset.name).toBe('IMG_10.jpg')
  })

  it('facets are OR within a group and AND across groups; unread files do not match a value', () => {
    expect(filterItems(items, '', { type: new Set(['JPG', 'NEF']) })).toHaveLength(3)
    expect(filterItems(items, '', { type: new Set(['JPG']), gps: new Set(['present']) })).toHaveLength(1)
    expect(filterItems(items, '', { creator: new Set(['empty']) }).map((i) => i.asset.id)).toEqual([2])
    expect(filterItems(items, 'morii', {})).toHaveLength(1)
  })

  it('middle truncation keeps the end and counts CJK as two', () => {
    expect(middleTruncate('short.jpg', 20)).toBe('short.jpg')
    const t = middleTruncate('a_very_long_file_name_from_the_camera_0001.NEF', 20)
    expect(t.endsWith('0001.NEF')).toBe(true)
    expect(t).toContain('…')
    const cjk = middleTruncate('北海道の夕焼けと雪の写真_0001.jpg', 20)
    expect(cjk.endsWith('.jpg')).toBe(true)
  })
})

describe('time arithmetic for the tools preview', () => {
  it('parses and formats EXIF times and shifts', () => {
    const t = parseExif('2026:09:30 23:59:50+09:00')!
    expect(parseShift('+00:00:15')).toBe(15)
    expect(parseShift('-1d01:00:00')).toBe(-90000)
    expect(parseShift('+00:61:00')).toBeNull()
    expect(formatShift(-90000)).toBe('-1d01:00:00')
    expect(localFromParts('2026-02-30', '10:00')).toBeNull()
    expect(localFromParts('2024-02-29', '9:05')).toBe('2024:02:29 09:05:00')
    expect(t).not.toBeNull()
  })
})
