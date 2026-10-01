// SPDX-License-Identifier: GPL-3.0-or-later
// Library table data: columns, sort keys, search and facets (SCREEN_SPEC 1#filters, 1#sort;
// DESIGN_SYSTEM Sidebar: facets are OR within a group and AND across groups).

import type { Asset, Row } from '../../ipc/types'
import type { MessageKey } from '../../i18n'
import type { SortKey } from '../../state/store'

export interface Item {
  asset: Asset
  row: Row | undefined
}

export interface Column {
  key: string
  label: MessageKey
  width: number
  mono: boolean
  /** The value shown and searched; null = empty (—), undefined = not read yet (…). */
  value: (it: Item) => string | null | undefined
  align?: 'right'
}

const camera = (r: Row | undefined): string | null | undefined => {
  if (!r) return undefined
  if (!r.model && !r.make) return null
  // "NIKON CORPORATION NIKON Z 8" reads as "NIKON Z 8": the model usually carries the brand
  if (r.model && r.make && r.model.toLowerCase().startsWith(r.make.split(' ')[0].toLowerCase())) return r.model
  return [r.make, r.model].filter(Boolean).join(' ')
}

export function sizeText(bytes: number): string {
  if (bytes >= 1 << 30) return `${(bytes / (1 << 30)).toFixed(1)} GB`
  if (bytes >= 1 << 20) return `${(bytes / (1 << 20)).toFixed(1)} MB`
  if (bytes >= 1 << 10) return `${Math.round(bytes / (1 << 10))} KB`
  return `${bytes} B`
}

export const COLUMNS: Column[] = [
  { key: 'name', label: 'col.name', width: 230, mono: true, value: (it) => it.asset.name },
  { key: 'type', label: 'col.type', width: 56, mono: true, value: (it) => it.asset.ext },
  {
    key: 'writes_to',
    label: 'col.writes_to',
    width: 104,
    mono: false,
    value: (it) => (it.asset.writable ? (it.row ? it.row.writes_to : undefined) : 'read_only'),
  },
  { key: 'capture_time', label: 'col.capture_time', width: 176, mono: true, value: (it) => (it.row ? it.row.capture_time : undefined) },
  { key: 'camera', label: 'col.camera', width: 140, mono: true, value: (it) => camera(it.row) },
  { key: 'lens', label: 'col.lens', width: 190, mono: true, value: (it) => (it.row ? it.row.lens : undefined) },
  { key: 'creator', label: 'col.creator', width: 140, mono: true, value: (it) => (it.row ? it.row.creator : undefined) },
  { key: 'copyright', label: 'col.copyright', width: 170, mono: true, value: (it) => (it.row ? it.row.copyright : undefined) },
  { key: 'gps', label: 'col.gps', width: 170, mono: true, value: (it) => (it.row ? it.row.gps : undefined) },
  { key: 'size', label: 'col.size', width: 76, mono: true, value: (it) => sizeText(it.asset.size), align: 'right' },
  { key: 'folder', label: 'col.folder', width: 260, mono: true, value: (it) => it.asset.folder },
]

/** Natural order: "IMG_2" before "IMG_10". */
const collator = new Intl.Collator(undefined, { numeric: true, sensitivity: 'base' })

function sortValue(it: Item, key: string): string | number | null | undefined {
  if (key === 'size') return it.asset.size
  const c = COLUMNS.find((x) => x.key === key)
  return c ? c.value(it) : null
}

export function sortItems(items: Item[], sort: SortKey[]): Item[] {
  const keys = sort.length ? sort : [{ key: 'name', dir: 1 as const }]
  return [...items].sort((a, b) => {
    for (const k of keys) {
      const va = sortValue(a, k.key)
      const vb = sortValue(b, k.key)
      // empty and unread values sort last in either direction
      const ea = va === null || va === undefined || va === ''
      const eb = vb === null || vb === undefined || vb === ''
      if (ea !== eb) return ea ? 1 : -1
      if (ea && eb) continue
      const d =
        typeof va === 'number' && typeof vb === 'number' ? va - vb : collator.compare(String(va), String(vb))
      if (d !== 0) return d * k.dir
    }
    return collator.compare(a.asset.name, b.asset.name)
  })
}

export interface FacetGroup {
  key: string
  label: MessageKey
  values: { value: string; label: string; count: number; attention?: boolean }[]
}

/** The facet value(s) of an item in a group; undefined when not read yet. */
function facetValues(it: Item, group: string): string[] | undefined {
  const r = it.row
  switch (group) {
    case 'type':
      return [it.asset.ext]
    case 'writes_to':
      return [it.asset.writable ? (r ? r.writes_to : 'pending') : 'read_only']
    case 'camera': {
      const c = camera(r)
      return c === undefined ? undefined : [c ?? '']
    }
    case 'lens':
      return r ? [r.lens ?? ''] : undefined
    case 'gps':
      return r ? [r.gps ? 'present' : 'empty'] : undefined
    case 'copyright':
      return r ? [r.copyright ? 'present' : 'empty'] : undefined
    case 'creator':
      return r ? [r.creator ? 'present' : 'empty'] : undefined
    case 'attention': {
      if (!r) return undefined
      const v: string[] = []
      if (r.conflicts.length) v.push('conflict')
      if (r.not_downloaded) v.push('cloud')
      if (r.error || r.invalid.length) v.push('bad_meta')
      return v
    }
    default:
      return []
  }
}

export const FACET_GROUPS: { key: string; label: MessageKey }[] = [
  { key: 'attention', label: 'facet.attention' },
  { key: 'type', label: 'facet.type' },
  { key: 'writes_to', label: 'facet.writes_to' },
  { key: 'camera', label: 'facet.camera' },
  { key: 'lens', label: 'facet.lens' },
  { key: 'creator', label: 'facet.creator' },
  { key: 'copyright', label: 'facet.copyright' },
  { key: 'gps', label: 'facet.gps' },
]

export function facetGroups(items: Item[]): { key: string; counts: Map<string, number> }[] {
  return FACET_GROUPS.map((g) => {
    const counts = new Map<string, number>()
    for (const it of items) {
      for (const v of facetValues(it, g.key) ?? []) counts.set(v, (counts.get(v) ?? 0) + 1)
    }
    return { key: g.key, counts }
  })
}

export function filterItems(items: Item[], search: string, facets: Record<string, Set<string>>): Item[] {
  const q = search.trim().toLowerCase()
  const active = Object.entries(facets).filter(([, v]) => v.size > 0)
  return items.filter((it) => {
    for (const [group, values] of active) {
      const fv = facetValues(it, group)
      if (!fv || !fv.some((v) => values.has(v))) return false
    }
    if (!q) return true
    return COLUMNS.some((c) => {
      const v = c.value(it)
      return typeof v === 'string' && v.toLowerCase().includes(q)
    })
  })
}

/** Flags shown in the first column (DESIGN_SYSTEM MetadataTable). */
export function flagsOf(it: Item): { text: string; tone: 'warn' | 'error' | 'neutral' | 'info' }[] {
  const r = it.row
  const f: { text: string; tone: 'warn' | 'error' | 'neutral' | 'info' }[] = []
  if (!r) return f
  if (r.conflicts.length) f.push({ text: 'CONF', tone: 'warn' })
  if (r.not_downloaded) f.push({ text: 'CLOUD', tone: 'info' })
  if (r.error || r.invalid.length) f.push({ text: 'BAD META', tone: 'error' })
  return f
}

/** Middle truncation that keeps the end (number + extension; DESIGN_SYSTEM MetadataTable Names). */
export function middleTruncate(name: string, max: number): string {
  let width = 0
  for (const ch of name) width += ch.charCodeAt(0) > 0x2e80 ? 2 : 1
  if (width <= max) return name
  const chars = [...name]
  const keepEnd = Math.floor(max * 0.45)
  const keepStart = max - keepEnd - 1
  const take = (arr: string[], w: number) => {
    const out: string[] = []
    let used = 0
    for (const ch of arr) {
      const cw = ch.charCodeAt(0) > 0x2e80 ? 2 : 1
      if (used + cw > w) break
      out.push(ch)
      used += cw
    }
    return out
  }
  const start = take(chars, keepStart).join('')
  const end = take([...chars].reverse(), keepEnd).reverse().join('')
  return `${start}…${end}`
}
