// SPDX-License-Identifier: GPL-3.0-or-later
// The time tools' preview rows (SCREEN_SPEC §4): each file's time now and after, the change,
// and what happens to its place in the order by time. The Plan the backend makes is what is
// written; this only shows the effect of the parameters while they are typed.

import { formatShift, naturalCompare, parseExif } from './timeMath'

export interface TimeRow {
  id: number
  name: string
  folder: string
  writesTo: string | null
  now: string | null
  next: string | null
  /** Shares its position with another file of the same name (a RAW and its JPG, §17). */
  pair: boolean
  /** Order status: `kept`, `moved`, `tied` (same time as a file that was not at the same time), `none` (no time). */
  order: 'kept' | 'moved' | 'tied' | 'none'
  /** `+01:00:00`; null when there is no time on one side. */
  change: string | null
}

/** The UTC offset a time carries (`+09:00`, `Z`), which every time tool keeps (DECISIONS D-18). */
export function offsetOf(v: string | null): string {
  return (v && /([+-]\d{2}:\d{2}|Z)$/.exec(v)?.[1]) || ''
}

export function stemKey(folder: string, name: string): string {
  return `${folder}|${name.replace(/\.[^.]+$/, '').toLowerCase()}`
}

/** Fill in pairs, changes and the order status of rows that have `now` and `next`. */
export function finishRows(rows: Omit<TimeRow, 'pair' | 'order' | 'change'>[]): TimeRow[] {
  const stems = new Map<string, number>()
  for (const r of rows) stems.set(stemKey(r.folder, r.name), (stems.get(stemKey(r.folder, r.name)) ?? 0) + 1)
  // a position is a file, or a RAW+JPG pair counted once
  const pos = (r: (typeof rows)[number]) => stemKey(r.folder, r.name)
  const byNow = new Map<string, number>()
  const byNext = new Map<string, number>()
  // only positions with a time on both sides are compared
  const both = new Set(rows.filter((r) => parseExif(r.now) !== null && parseExif(r.next) !== null).map(pos))
  const rank = (key: 'now' | 'next', into: Map<string, number>) => {
    const seen = new Map<string, number>()
    for (const r of rows) {
      const s = parseExif(r[key])
      const p = pos(r)
      if (s === null || !both.has(p)) continue
      seen.set(p, Math.min(seen.get(p) ?? Infinity, s))
    }
    const ordered = [...seen.entries()].sort((a, b) => a[1] - b[1] || naturalCompare(a[0], b[0]))
    ordered.forEach(([p], i) => into.set(p, i))
    return seen
  }
  const nowTimes = rank('now', byNow)
  const nextTimes = rank('next', byNext)
  const nextCount = new Map<number, Set<string>>()
  const nowCount = new Map<number, Set<string>>()
  for (const [p, s] of nextTimes) nextCount.set(s, (nextCount.get(s) ?? new Set()).add(p))
  for (const [p, s] of nowTimes) nowCount.set(s, (nowCount.get(s) ?? new Set()).add(p))
  return rows.map((r) => {
    if (r.next && r.now && !offsetOf(r.next)) r = { ...r, next: r.next + offsetOf(r.now) }
    const n = parseExif(r.now)
    const x = parseExif(r.next)
    const p = pos(r)
    let order: TimeRow['order'] = 'none'
    if (n !== null && x !== null) {
      const sharedNext = nextCount.get(nextTimes.get(p)!)!
      const sharedNow = nowCount.get(nowTimes.get(p)!)!
      if (sharedNext.size > 1 && [...sharedNext].some((o) => !sharedNow.has(o))) order = 'tied'
      else order = byNow.get(p) === byNext.get(p) ? 'kept' : 'moved'
    }
    return {
      ...r,
      pair: (stems.get(p) ?? 0) > 1,
      order,
      change: n !== null && x !== null ? formatShift(x - n) : null,
    }
  })
}

export interface TimeSummary {
  files: number
  changing: number
  moved: number
  tied: number
  noTime: number
}

export function summarize(rows: TimeRow[]): TimeSummary {
  return {
    files: rows.length,
    changing: rows.filter((r) => r.next !== null && r.next !== r.now).length,
    moved: rows.filter((r) => r.order === 'moved').length,
    tied: rows.filter((r) => r.order === 'tied').length,
    noTime: rows.filter((r) => r.order === 'none').length,
  }
}
