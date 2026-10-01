// SPDX-License-Identifier: GPL-3.0-or-later
// MetadataTable (DESIGN_SYSTEM): virtualised, 22 px zebra rows, frozen flags + name columns,
// multi-level sort (click; Shift-click adds a level), selection with the mouse and the keyboard
// (INTERACTION_SPEC §18 Table). Values are plain text; unread values show "…", empty ones "—".

import { useEffect, useRef, type KeyboardEvent, type MouseEvent } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useApp } from '../../state/store'
import { useT, type T } from '../../i18n'
import { COLUMNS, flagsOf, middleTruncate, type Item } from './data'

const ROW = 22
const FLAGS_W = 64

function cellText(t: T, key: string, v: string | null | undefined): { text: string; cls: string } {
  if (v === undefined) return { text: '…', cls: 'faint' }
  if (v === null || v === '') return { text: '—', cls: 'empty-value' }
  if (key === 'writes_to') {
    const map: Record<string, Parameters<T>[0]> = {
      in_file: 'writes.in_file',
      sidecar: 'writes.sidecar',
      new_sidecar: 'writes.new_sidecar',
      read_only: 'writes.read_only',
    }
    return { text: map[v] ? t(map[v]) : v, cls: v === 'read_only' ? 'faint' : '' }
  }
  return { text: v, cls: '' }
}

export function MetadataTable({ items }: { items: Item[] }) {
  const t = useT()
  const selection = useApp((s) => s.selection)
  const focus = useApp((s) => s.focus)
  const anchor = useApp((s) => s.anchor)
  const select = useApp((s) => s.select)
  const sort = useApp((s) => s.sort)
  const setSort = useApp((s) => s.setSort)
  const scroller = useRef<HTMLDivElement>(null)
  const v = useVirtualizer({
    count: items.length,
    getScrollElement: () => scroller.current,
    estimateSize: () => ROW,
    overscan: 20,
  })
  const width = FLAGS_W + COLUMNS.reduce((a, c) => a + c.width, 0)
  const index = new Map(items.map((it, i) => [it.asset.id, i]))

  // keep the focused row in view
  useEffect(() => {
    if (focus === null) return
    const i = index.get(focus)
    if (i !== undefined) v.scrollToIndex(i, { align: 'auto' })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus])

  const onHeader = (e: MouseEvent, key: string) => {
    const at = sort.findIndex((s) => s.key === key)
    if (e.shiftKey) {
      if (at >= 0) setSort(sort.map((s, i) => (i === at ? { ...s, dir: (s.dir * -1) as 1 | -1 } : s)))
      else setSort([...sort, { key, dir: 1 }])
    } else if (at === 0 && sort.length === 1) {
      setSort([{ key, dir: (sort[0].dir * -1) as 1 | -1 }])
    } else {
      setSort([{ key, dir: 1 }])
    }
  }

  const range = (a: number, b: number) => {
    const [lo, hi] = a < b ? [a, b] : [b, a]
    return items.slice(lo, hi + 1).map((it) => it.asset.id)
  }

  const onRow = (e: MouseEvent, id: number) => {
    const i = index.get(id)!
    if (e.shiftKey && anchor !== null && index.has(anchor)) {
      const ids = range(index.get(anchor)!, i)
      select(e.ctrlKey ? [...new Set([...selection, ...ids])] : ids, id, anchor)
    } else if (e.ctrlKey || e.metaKey) {
      const next = new Set(selection)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      select([...next], id, id)
    } else {
      select([id], id, id)
    }
    scroller.current?.focus()
  }

  const onKey = (e: KeyboardEvent) => {
    if (!items.length) return
    const cur = focus !== null && index.has(focus) ? index.get(focus)! : -1
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / ROW) - 1)
    let next = cur
    switch (e.key) {
      case 'ArrowDown':
        next = Math.min(items.length - 1, cur + 1)
        break
      case 'ArrowUp':
        next = Math.max(0, cur - 1)
        break
      case 'PageDown':
        next = Math.min(items.length - 1, cur + page)
        break
      case 'PageUp':
        next = Math.max(0, cur - page)
        break
      case 'Home':
        next = 0
        break
      case 'End':
        next = items.length - 1
        break
      case ' ': {
        e.preventDefault()
        if (cur < 0) return
        const id = items[cur].asset.id
        const s = new Set(selection)
        if (s.has(id)) s.delete(id)
        else s.add(id)
        select([...s], id, id)
        return
      }
      case 'a':
      case 'A':
        if (e.ctrlKey) {
          e.preventDefault()
          select(items.map((it) => it.asset.id))
        }
        return
      case 'Escape':
        select([], focus)
        return
      default:
        return
    }
    e.preventDefault()
    if (next < 0) next = 0
    const id = items[next].asset.id
    if (e.shiftKey) {
      const a = anchor !== null && index.has(anchor) ? index.get(anchor)! : cur < 0 ? next : cur
      select(range(a, next), id, items[a].asset.id)
    } else if (e.ctrlKey) {
      select([...selection], id)
    } else {
      select([id], id, id)
    }
  }

  const sortMark = (key: string) => {
    const at = sort.findIndex((s) => s.key === key)
    if (at < 0) return null
    const arrow = sort[at].dir === 1 ? '▲' : '▼'
    return <span className="sort-mark mono">{sort.length > 1 ? `${at + 1}${arrow}` : arrow}</span>
  }

  return (
    <div
      className="mtable"
      ref={scroller}
      tabIndex={0}
      role="grid"
      aria-rowcount={items.length}
      aria-multiselectable
      aria-label={t('table.label')}
      onKeyDown={onKey}
    >
      <div className="mtable-inner" style={{ width, height: v.getTotalSize() + 24 }}>
        <div className="mtable-header" role="row" style={{ width }}>
          <div className="mth frozen" style={{ width: FLAGS_W, left: 0 }} role="columnheader">
            <span className="secondary">{t('col.flags')}</span>
          </div>
          {COLUMNS.map((c, i) => (
            <div
              key={c.key}
              role="columnheader"
              aria-sort={sort[0]?.key === c.key ? (sort[0].dir === 1 ? 'ascending' : 'descending') : undefined}
              className={`mth${i === 0 ? ' frozen frozen-edge' : ''}${sort.some((s) => s.key === c.key) ? ' sorted' : ''}${i === COLUMNS.length - 1 ? ' flex' : ''}`}
              style={{ width: c.width, left: i === 0 ? FLAGS_W : undefined, textAlign: c.align }}
              onClick={(e) => onHeader(e, c.key)}
              title={t('table.sort_hint')}
            >
              <span className="ellipsis">{t(c.label)}</span>
              {sortMark(c.key)}
            </div>
          ))}
        </div>
        {v.getVirtualItems().map((vr) => {
          const it = items[vr.index]
          const id = it.asset.id
          const selected = selection.has(id)
          const focused = focus === id
          const flags = flagsOf(it)
          return (
            <div
              key={id}
              role="row"
              aria-selected={selected}
              className={`mrow${vr.index % 2 ? ' alt' : ''}${selected ? ' selected' : ''}${focused ? ' focused' : ''}${it.asset.writable ? '' : ' readonly'}`}
              style={{ transform: `translateY(${vr.start + 24}px)`, width }}
              onMouseDown={(e) => {
                if (e.button === 0) onRow(e, id)
              }}
              onDoubleClick={() => useApp.getState().inspectorOpen || useApp.getState().toggleInspector()}
            >
              <div className="mtd frozen flags" style={{ width: FLAGS_W, left: 0 }} role="gridcell">
                {flags.map((f) => (
                  <span key={f.text} className={`flag ${f.tone}`}>
                    {f.text}
                  </span>
                ))}
              </div>
              {COLUMNS.map((c, i) => {
                const raw = c.value(it)
                const { text, cls } = cellText(t, c.key, raw)
                const shown = c.key === 'name' ? middleTruncate(text, 34) : text
                return (
                  <div
                    key={c.key}
                    role="gridcell"
                    className={`mtd${c.mono ? ' mono' : ''}${i === 0 ? ' frozen frozen-edge' : ''}${i === COLUMNS.length - 1 ? ' flex' : ''} ${cls}`}
                    style={{ width: c.width, left: i === 0 ? FLAGS_W : undefined, textAlign: c.align }}
                    title={text !== shown || text.length > 24 ? text : undefined}
                  >
                    {shown}
                  </div>
                )
              })}
            </div>
          )
        })}
      </div>
    </div>
  )
}
