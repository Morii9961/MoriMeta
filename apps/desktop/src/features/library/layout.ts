// SPDX-License-Identifier: GPL-3.0-or-later
// Library table layout and condition filters (SCREEN_SPEC 1#columns, 1#sort, 1#filters): which
// columns show in which order and width, the grouping column, AND-ed conditions, and saved
// layouts and smart filters. All of it is a per-viewer convenience kept in localStorage; reading
// or writing it may fail (private window, blocked storage) and the table then uses the defaults.

import { COLUMNS, type Column, type Item } from './data'

export interface Layout {
  /** Column keys in display order; `name` is always first and always shown. */
  order: string[]
  hidden: string[]
  widths: Record<string, number>
}

export const DEFAULT_LAYOUT: Layout = { order: COLUMNS.map((c) => c.key), hidden: [], widths: {} }

export const MIN_WIDTH = 48
export const MAX_WIDTH = 640

/** A layout read back from storage, repaired: unknown keys dropped, missing ones appended. */
export function normalizeLayout(l: Partial<Layout> | null | undefined): Layout {
  const known = new Set(COLUMNS.map((c) => c.key))
  const order = (Array.isArray(l?.order) ? l!.order : []).filter((k) => known.has(k))
  for (const c of COLUMNS) if (!order.includes(c.key)) order.push(c.key)
  // the file name stays first: it is the frozen column
  const rest = order.filter((k) => k !== 'name')
  const hidden = (Array.isArray(l?.hidden) ? l!.hidden : []).filter((k) => known.has(k) && k !== 'name')
  const widths: Record<string, number> = {}
  for (const [k, w] of Object.entries(l?.widths ?? {})) {
    if (known.has(k) && typeof w === 'number' && Number.isFinite(w)) widths[k] = clampWidth(w)
  }
  return { order: ['name', ...rest], hidden: [...new Set(hidden)], widths }
}

export function clampWidth(w: number): number {
  return Math.round(Math.min(MAX_WIDTH, Math.max(MIN_WIDTH, w)))
}

/** The columns to draw, in order, with their widths. */
export function visibleColumns(l: Layout): Column[] {
  return l.order
    .filter((k) => !l.hidden.includes(k))
    .map((k) => COLUMNS.find((c) => c.key === k)!)
    .filter(Boolean)
    .map((c) => ({ ...c, width: l.widths[c.key] ?? c.width }))
}

/** Move a column one place left or right among the movable ones (not the file name). */
export function moveColumn(l: Layout, key: string, by: -1 | 1): Layout {
  if (key === 'name') return l
  const order = [...l.order]
  const i = order.indexOf(key)
  const j = i + by
  if (i < 1 || j < 1 || j >= order.length) return l
  ;[order[i], order[j]] = [order[j], order[i]]
  return { ...l, order }
}

// --- conditions (1#filters)

export type Op = 'is' | 'is_not' | 'contains' | 'empty' | 'present'

export interface Condition {
  field: string
  op: Op
  value: string
}

/** Fields a condition can test: the table's columns except the size. */
export const CONDITION_FIELDS = COLUMNS.filter((c) => c.key !== 'size').map((c) => c.key)

export const OPS: Op[] = ['is', 'is_not', 'contains', 'empty', 'present']

export function needsValue(op: Op): boolean {
  return op === 'is' || op === 'is_not' || op === 'contains'
}

/** Whether a file passes one condition; a value not read yet never matches. */
export function matches(it: Item, c: Condition): boolean {
  const col = COLUMNS.find((x) => x.key === c.field)
  if (!col) return true
  const v = col.value(it)
  if (v === undefined) return false
  const text = (v ?? '').trim()
  const want = c.value.trim().toLowerCase()
  switch (c.op) {
    case 'empty':
      return text === ''
    case 'present':
      return text !== ''
    case 'is':
      return text.toLowerCase() === want
    case 'is_not':
      return text.toLowerCase() !== want
    case 'contains':
      return want === '' || text.toLowerCase().includes(want)
  }
}

export function matchesAll(it: Item, conditions: Condition[]): boolean {
  return conditions.every((c) => matches(it, c))
}

/** The distinct values of a field among the files, most frequent first (for the value picker). */
export function valuesOf(items: Item[], field: string, limit = 50): [string, number][] {
  const col = COLUMNS.find((x) => x.key === field)
  if (!col) return []
  const m = new Map<string, number>()
  for (const it of items) {
    const v = col.value(it)
    if (typeof v === 'string' && v.trim() !== '') m.set(v, (m.get(v) ?? 0) + 1)
  }
  return [...m.entries()].sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0])).slice(0, limit)
}

// --- grouping (1#sort)

export type Display = { kind: 'group'; label: string | null; count: number } | { kind: 'item'; it: Item; index: number }

/**
 * The rows to draw: the sorted files, with a header row before each run of equal values of the
 * grouping column. Files keep their sorted order inside each group; groups follow the order in
 * which their first file appears, so the sort decides it (grouping by the first sort key keeps
 * groups contiguous). `index` is the file's position among the files, for selection.
 */
export function withGroups(items: Item[], groupBy: string | null): Display[] {
  if (!groupBy) return items.map((it, index) => ({ kind: 'item', it, index }))
  const col = COLUMNS.find((c) => c.key === groupBy)
  if (!col) return items.map((it, index) => ({ kind: 'item', it, index }))
  const key = (it: Item) => {
    const v = col.value(it)
    return v === undefined || v === null || v === '' ? null : v
  }
  const groups = new Map<string | null, Item[]>()
  for (const it of items) {
    const k = key(it)
    const g = groups.get(k)
    if (g) g.push(it)
    else groups.set(k, [it])
  }
  const out: Display[] = []
  let index = 0
  for (const [label, members] of groups) {
    out.push({ kind: 'group', label, count: members.length })
    for (const it of members) out.push({ kind: 'item', it, index: index++ })
  }
  return out
}

/** The files in drawing order (groups gather files, so selection ranges follow this order). */
export function itemsInOrder(display: Display[]): Item[] {
  return display.flatMap((d) => (d.kind === 'item' ? [d.it] : []))
}

// --- storage

export interface Saved<T> {
  name: string
  value: T
}

export function load<T>(key: string, fallback: T): T {
  try {
    const raw = localStorage.getItem(key)
    return raw ? (JSON.parse(raw) as T) : fallback
  } catch {
    return fallback
  }
}

export function save(key: string, value: unknown): void {
  try {
    localStorage.setItem(key, JSON.stringify(value))
  } catch {
    // a per-viewer convenience only
  }
}

/** Conditions read back from storage: only well-formed ones on known fields. */
export function normalizeConditions(v: unknown): Condition[] {
  if (!Array.isArray(v)) return []
  return v
    .filter(
      (c): c is Condition =>
        !!c &&
        typeof c === 'object' &&
        CONDITION_FIELDS.includes((c as Condition).field) &&
        OPS.includes((c as Condition).op) &&
        typeof (c as Condition).value === 'string',
    )
    .map((c) => ({ field: c.field, op: c.op, value: c.value }))
}
