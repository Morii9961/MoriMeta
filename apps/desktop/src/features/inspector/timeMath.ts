// SPDX-License-Identifier: GPL-3.0-or-later
// Wall-clock arithmetic for the time tools' result preview. The Plan made by the backend is what
// is written; this only lets the panel show "now → new" while parameters are typed, so it follows
// the backend's rules (mm-domain::time): the same order, the same accepted inputs.

/** Seconds since 1970 on a naive clock (UTC arithmetic) for any year 0001–9999. */
function utc(y: number, mo: number, d: number, h: number, mi: number, se: number): number {
  const t = new Date(0)
  // Date.UTC would read years 0–99 as 1900–1999
  t.setUTCFullYear(y, mo - 1, d)
  t.setUTCHours(h, mi, se, 0)
  return t.getTime() / 1000
}

/** `YYYY:MM:DD HH:MM:SS[.ss][±HH:MM]` → seconds since 1970 on a naive clock (UTC arithmetic). */
export function parseExif(s: string | null | undefined): number | null {
  if (!s) return null
  const m = /^(\d{4}):(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})/.exec(s)
  if (!m) return null
  const [, y, mo, d, h, mi, se] = m.map(Number)
  if (y < 1) return null
  const t = utc(y, mo, d, h, mi, se)
  return Number.isFinite(t) ? t : null
}

/** The order the backend sorts capture times by: seconds, then the sub-seconds in nanoseconds.
 * A pair, not a float: `.999999999` must stay before the next second. */
export type ExifOrder = [number, number]

export function exifOrder(s: string | null | undefined): ExifOrder | null {
  const t = parseExif(s)
  if (t === null) return null
  const sub = /^.{19}\.(\d{1,9})/.exec(s!)
  return [t, sub ? Number(sub[1].padEnd(9, '0')) : 0]
}

/** Earlier first; a missing time after every time. */
export function compareExifOrder(a: ExifOrder | null, b: ExifOrder | null): number {
  if (a === null || b === null) return a === b ? 0 : a === null ? 1 : -1
  return Math.sign(a[0] - b[0]) || Math.sign(a[1] - b[1])
}

/** The sub-seconds a time carries (`.67`), which Shift and Preserve Relative Timing keep. */
export function subsecOf(s: string | null | undefined): string {
  return (s && /^.{19}(\.\d{1,9})/.exec(s)?.[1]) || ''
}

export function formatExif(sec: number): string {
  const d = new Date(sec * 1000)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${p(d.getUTCFullYear(), 4)}:${p(d.getUTCMonth() + 1)}:${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}`
}

/** `[+|-][Nd]HH:MM:SS` → seconds, as `mm_domain::time::parse_shift` reads it (any number of hours). */
export function parseShift(s: string): number | null {
  const m = /^([+-]?)(?:(\d+)d)?(\d+):(\d{2}):(\d{2})$/.exec(s.trim())
  if (!m) return null
  const sign = m[1] === '-' ? -1 : 1
  const [d, h, mi, se] = [m[2] ? Number(m[2]) : 0, Number(m[3]), Number(m[4]), Number(m[5])]
  if (mi > 59 || se > 59) return null
  return sign * (((d * 24 + h) * 60 + mi) * 60 + se)
}

export function formatShift(sec: number): string {
  const sign = sec < 0 ? '-' : '+'
  let s = Math.abs(sec)
  const d = Math.floor(s / 86400)
  s -= d * 86400
  const p = (n: number) => String(n).padStart(2, '0')
  return `${sign}${d ? `${d}d` : ''}${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`
}

/** From the parts the panel collects: `YYYY-MM-DD` + `HH:MM:SS` → `YYYY:MM:DD HH:MM:SS`. */
export function localFromParts(date: string, time: string): string | null {
  const d = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date.trim())
  const t = /^(\d{1,2}):(\d{2})(?::(\d{2}))?$/.exec(time.trim())
  if (!d || !t) return null
  const hh = t[1].padStart(2, '0')
  const v = `${d[1]}:${d[2]}:${d[3]} ${hh}:${t[2]}:${t[3] ?? '00'}`
  const sec = parseExif(v)
  if (sec === null || formatExif(sec) !== v) return null // 2026-02-30 and the like
  return v
}

/** Compare by Unicode code point, as Rust compares strings. */
function byCodePoint(a: string, b: string): number {
  const x = Array.from(a)
  const y = Array.from(b)
  for (let i = 0; i < Math.min(x.length, y.length); i++) {
    const d = x[i].codePointAt(0)! - y[i].codePointAt(0)!
    if (d) return Math.sign(d)
  }
  return Math.sign(x.length - y.length)
}

function runs(s: string): [boolean, string][] {
  const out: [boolean, string][] = []
  for (const c of s) {
    const digit = c >= '0' && c <= '9'
    const last = out[out.length - 1]
    if (last && last[0] === digit) last[1] += c
    else out.push([digit, c])
  }
  return out
}

/**
 * Natural file-name order exactly as the backend's Sequence uses it (`mm_domain::time::natural_cmp`):
 * digit runs by value ("2" < "10"), other runs case-insensitively by code point; then fewer runs
 * first, then fewer leading zeros ("1" < "01"), then the names themselves.
 */
export function naturalCompare(a: string, b: string): number {
  const ra = runs(a)
  const rb = runs(b)
  let zeros = 0
  for (let i = 0; i < Math.min(ra.length, rb.length); i++) {
    const [da, sa] = ra[i]
    const [db, sb] = rb[i]
    let o: number
    if (da && db) {
      const ta = sa.replace(/^0+/, '')
      const tb = sb.replace(/^0+/, '')
      if (zeros === 0) zeros = Math.sign(sa.length - sb.length)
      o = Math.sign(ta.length - tb.length) || byCodePoint(ta, tb)
    } else {
      o = byCodePoint(sa.toLowerCase(), sb.toLowerCase())
    }
    if (o) return o
  }
  return Math.sign(ra.length - rb.length) || zeros || byCodePoint(a, b)
}
