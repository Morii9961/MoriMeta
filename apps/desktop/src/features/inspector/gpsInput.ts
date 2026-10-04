// SPDX-License-Identifier: GPL-3.0-or-later
// What the batch editor accepts for Set GPS (SCREEN_SPEC 2#b-same): decimal `lat, lon[, alt]`,
// or degrees, minutes and seconds with hemisphere letters as maps and cameras print them. The
// same rules as the backend's `GeoPoint::parse`; the staged value is always decimal.

export interface GeoInput {
  lat: number
  lon: number
  alt: number | null
}

const DMS_MARK = /[NSEWnsew°º'"′″’”]/

function decimal(text: string): GeoInput | null {
  const parts = text.split(',').map((x) => x.trim())
  if (parts.length < 2 || parts.length > 3 || parts.some((p) => p === '')) return null
  const n = parts.map(Number)
  if (n.some((x) => !Number.isFinite(x))) return null
  return { lat: n[0], lon: n[1], alt: parts.length === 3 ? n[2] : null }
}

function dms(text: string): GeoInput | null {
  type Tok = { num: number; frac: boolean } | { hemi: string }
  const toks: Tok[] = []
  let num = ''
  const flush = () => {
    if (num) {
      const v = Number(num)
      if (!Number.isFinite(v)) return false
      toks.push({ num: v, frac: num.includes('.') })
      num = ''
    }
    return true
  }
  for (const c of text) {
    if (/[0-9.]/.test(c)) num += c
    else if (/[NSEWnsew]/.test(c)) {
      if (!flush()) return null
      toks.push({ hemi: c.toUpperCase() })
    } else if (/[\s,;°º'"′″’”]/.test(c)) {
      if (!flush()) return null
    } else return null
  }
  if (!flush()) return null
  const prefix = toks.length > 0 && 'hemi' in toks[0]
  const coords: { hemi: string; parts: { num: number; frac: boolean }[] }[] = []
  let nums: { num: number; frac: boolean }[] = []
  for (const t of toks) {
    if ('num' in t) nums.push(t)
    else if (prefix) {
      if (coords.length) coords[coords.length - 1].parts = nums
      nums = []
      coords.push({ hemi: t.hemi, parts: [] })
    } else {
      coords.push({ hemi: t.hemi, parts: nums })
      nums = []
    }
  }
  if (prefix && coords.length) {
    coords[coords.length - 1].parts = nums
    nums = []
  }
  if (coords.length !== 2 || nums.length) return null
  const value = (c: (typeof coords)[number]) => {
    const p = c.parts
    if (p.length < 1 || p.length > 3) return null
    if (p.slice(0, -1).some((x) => x.frac) || p.slice(1).some((x) => x.num >= 60)) return null
    const v = p.reduce((a, x, i) => a + x.num / [1, 60, 3600][i], 0)
    return c.hemi === 'S' || c.hemi === 'W' ? -v : v
  }
  const latFirst = coords[0].hemi === 'N' || coords[0].hemi === 'S'
  const [la, lo] = latFirst ? coords : [coords[1], coords[0]]
  if (!(la.hemi === 'N' || la.hemi === 'S') || !(lo.hemi === 'E' || lo.hemi === 'W')) return null
  const lat = value(la)
  const lon = value(lo)
  return lat === null || lon === null ? null : { lat, lon, alt: null }
}

/** The position typed or pasted, range-checked; null when it is not one. */
export function parseGpsInput(text: string): GeoInput | null {
  const p = DMS_MARK.test(text) ? dms(text) : decimal(text)
  if (!p) return null
  if (Math.abs(p.lat) > 90 || Math.abs(p.lon) > 180) return null
  if (p.alt !== null && Math.abs(p.alt) > 100_000) return null
  return p
}

/** The decimal form the backend is given (7 decimals, about 1 cm). */
export function gpsPosition(p: GeoInput): string {
  const r = (v: number) => String(Math.round(v * 1e7) / 1e7)
  return p.alt === null ? `${r(p.lat)},${r(p.lon)}` : `${r(p.lat)},${r(p.lon)},${p.alt}`
}
