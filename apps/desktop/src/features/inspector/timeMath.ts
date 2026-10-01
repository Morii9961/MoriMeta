// SPDX-License-Identifier: GPL-3.0-or-later
// Wall-clock arithmetic for the time tools' result preview. The Plan made by the backend is what
// is written; this only lets the panel show "now → new" while parameters are typed.

/** `YYYY:MM:DD HH:MM:SS[.ss][±HH:MM]` → seconds since 0001 on a naive clock (UTC arithmetic). */
export function parseExif(s: string | null | undefined): number | null {
  if (!s) return null
  const m = /^(\d{4}):(\d{2}):(\d{2}) (\d{2}):(\d{2}):(\d{2})/.exec(s)
  if (!m) return null
  const [, y, mo, d, h, mi, se] = m.map(Number)
  const t = Date.UTC(y, mo - 1, d, h, mi, se)
  return Number.isFinite(t) ? t / 1000 : null
}

export function formatExif(sec: number): string {
  const d = new Date(sec * 1000)
  const p = (n: number, w = 2) => String(n).padStart(w, '0')
  return `${p(d.getUTCFullYear(), 4)}:${p(d.getUTCMonth() + 1)}:${p(d.getUTCDate())} ${p(d.getUTCHours())}:${p(d.getUTCMinutes())}:${p(d.getUTCSeconds())}`
}

/** `[+|-][Nd]HH:MM:SS` → seconds. */
export function parseShift(s: string): number | null {
  const m = /^([+-]?)(?:(\d+)d)?(\d{1,2}):(\d{2}):(\d{2})$/.exec(s.trim())
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

/** Natural file-name order, as the backend's Sequence uses it. */
export const naturalCompare = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' }).compare
