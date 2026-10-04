// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { finishRows, summarize } from './timeRows'

const row = (id: number, name: string, now: string | null, next: string | null) => ({ id, name, folder: 'D:\p', writesTo: 'in_file', now, next })

describe('time preview rows', () => {
  it('a shift keeps the order and shows the change', () => {
    const r = finishRows([
      row(1, 'a.jpg', '2026:09:11 10:00:00', '2026:09:11 11:00:00'),
      row(2, 'b.jpg', '2026:09:11 10:05:00', '2026:09:11 11:05:00'),
    ])
    expect(r.map((x) => x.order)).toEqual(['kept', 'kept'])
    expect(r[0].change).toBe('+01:00:00')
    expect(summarize(r)).toEqual({ files: 2, changing: 2, moved: 0, tied: 0, noTime: 0 })
  })
  it('one time for every file loses the order', () => {
    const r = finishRows([
      row(1, 'a.jpg', '2026:09:11 10:00:00', '2026:01:01 00:00:00'),
      row(2, 'b.jpg', '2026:09:11 10:05:00', '2026:01:01 00:00:00'),
    ])
    expect(r.map((x) => x.order)).toEqual(['tied', 'tied'])
  })
  it('a RAW and its JPG are one position', () => {
    const r = finishRows([
      row(1, 'DSC_1.NEF', '2026:09:11 10:00:00', '2026:01:01 00:00:00'),
      row(2, 'DSC_1.JPG', '2026:09:11 10:00:00', '2026:01:01 00:00:00'),
      row(3, 'DSC_2.JPG', '2026:09:11 10:01:00', '2026:01:01 00:00:01'),
    ])
    expect(r.map((x) => [x.pair, x.order])).toEqual([
      [true, 'kept'],
      [true, 'kept'],
      [false, 'kept'],
    ])
  })
  it('a sequence by name can move a file', () => {
    const r = finishRows([
      row(1, 'a.jpg', '2026:09:11 10:05:00', '2026:01:01 00:00:00'),
      row(2, 'b.jpg', '2026:09:11 10:00:00', '2026:01:01 00:00:10'),
    ])
    expect(r.map((x) => x.order)).toEqual(['moved', 'moved'])
  })
  it('the new time keeps the UTC offset of the file', () => {
    const r = finishRows([row(1, 'a.jpg', '2026:09:11 10:00:00+09:00', '2026:09:11 12:00:00')])
    expect(r[0].next).toBe('2026:09:11 12:00:00+09:00')
    expect(r[0].change).toBe('+02:00:00')
  })
  it('files without a time are not compared and do not disturb the others', () => {
    const r = finishRows([
      row(1, 'a.jpg', null, '2026:01:01 00:00:00'),
      row(2, 'b.jpg', '2026:09:11 10:00:00', '2026:01:01 00:00:10'),
      row(3, 'c.jpg', '2026:09:11 10:01:00', '2026:01:01 00:00:20'),
    ])
    expect(r.map((x) => x.order)).toEqual(['none', 'kept', 'kept'])
    expect(r[0].change).toBeNull()
  })
})
