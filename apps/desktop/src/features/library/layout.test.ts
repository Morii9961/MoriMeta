// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import type { Asset, Row } from '../../ipc/types'
import type { Item } from './data'
import {
  DEFAULT_LAYOUT,
  itemsInOrder,
  matches,
  moveColumn,
  normalizeConditions,
  normalizeLayout,
  valuesOf,
  visibleColumns,
  withGroups,
} from './layout'

function item(id: number, name: string, row: Partial<Row> | undefined): Item {
  const asset = { id, name, ext: 'JPG', size: 1000, folder: 'D:\\p', path: `D:\\p\\${name}`, writable: true } as Asset
  return { asset, row: row ? ({ conflicts: [], invalid: [], ...row } as Row) : undefined }
}

const a = item(1, 'a.jpg', { make: 'NIKON CORPORATION', model: 'NIKON Z 8', creator: 'Morii', copyright: null, gps: '1, 2' })
const b = item(2, 'b.jpg', { make: 'NIKON CORPORATION', model: 'NIKON D850', creator: null, copyright: '© Morii', gps: null })
const c = item(3, 'c.jpg', { make: 'NIKON CORPORATION', model: 'NIKON Z 8', creator: 'M. Mori', copyright: null, gps: null })
const unread = item(4, 'd.jpg', undefined)

describe('layout', () => {
  it('keeps the file name first and shown, drops unknown keys, adds new columns', () => {
    const l = normalizeLayout({ order: ['camera', 'name', 'bogus'], hidden: ['name', 'lens', 'bogus'], widths: { lens: 9999, bogus: 3 } })
    expect(l.order[0]).toBe('name')
    expect(l.order[1]).toBe('camera')
    expect(l.order).toHaveLength(DEFAULT_LAYOUT.order.length)
    expect(l.hidden).toEqual(['lens'])
    expect(l.widths).toEqual({ lens: 640 })
    expect(visibleColumns(l).map((c) => c.key)).not.toContain('lens')
  })
  it('repairs anything storage gives back', () => {
    expect(normalizeLayout(null)).toEqual(DEFAULT_LAYOUT)
    expect(normalizeLayout({ order: 'x' as unknown as string[] }).order).toEqual(DEFAULT_LAYOUT.order)
  })
  it('moves columns but never the file name', () => {
    const l = normalizeLayout(DEFAULT_LAYOUT)
    expect(moveColumn(l, 'name', 1)).toBe(l)
    expect(moveColumn(l, l.order[1], -1)).toBe(l)
    const moved = moveColumn(l, l.order[1], 1)
    expect(moved.order[2]).toBe(l.order[1])
  })
})

describe('conditions', () => {
  it('tests a column value; unread files never match', () => {
    expect(matches(a, { field: 'camera', op: 'is', value: 'nikon z 8' })).toBe(true)
    expect(matches(b, { field: 'camera', op: 'is', value: 'NIKON Z 8' })).toBe(false)
    expect(matches(b, { field: 'camera', op: 'is_not', value: 'NIKON Z 8' })).toBe(true)
    expect(matches(c, { field: 'creator', op: 'contains', value: 'mori' })).toBe(true)
    expect(matches(b, { field: 'creator', op: 'empty', value: '' })).toBe(true)
    expect(matches(a, { field: 'gps', op: 'present', value: '' })).toBe(true)
    expect(matches(unread, { field: 'creator', op: 'empty', value: '' })).toBe(false)
  })
  it('keeps only well-formed stored conditions', () => {
    expect(normalizeConditions([{ field: 'gps', op: 'present', value: '' }, { field: 'evil', op: 'is', value: 'x' }, null, 3])).toEqual([
      { field: 'gps', op: 'present', value: '' },
    ])
    expect(normalizeConditions('nope')).toEqual([])
  })
  it('lists the values of a field by frequency', () => {
    expect(valuesOf([a, b, c, unread], 'camera')).toEqual([
      ['NIKON Z 8', 2],
      ['NIKON D850', 1],
    ])
  })
})

describe('grouping', () => {
  it('puts a header before each group and gathers its files', () => {
    const d = withGroups([a, b, c, unread], 'camera')
    expect(d.map((x) => (x.kind === 'group' ? `[${x.label}:${x.count}]` : x.it.asset.name))).toEqual([
      '[NIKON Z 8:2]',
      'a.jpg',
      'c.jpg',
      '[NIKON D850:1]',
      'b.jpg',
      '[null:1]',
      'd.jpg',
    ])
    expect(itemsInOrder(d).map((x) => x.asset.id)).toEqual([1, 3, 2, 4])
  })
  it('is the plain list without a grouping column', () => {
    expect(withGroups([a, b], null).every((x) => x.kind === 'item')).toBe(true)
  })
})
