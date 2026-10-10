// SPDX-License-Identifier: GPL-3.0-or-later
// MetadataTable (DESIGN_SYSTEM): virtualised, 22 px zebra rows, frozen flags + name columns,
// multi-level sort (click; Shift-click adds a level), selection with the mouse and the keyboard
// (INTERACTION_SPEC §18 Table). Values are plain text; unread values show "…", empty ones "—".
// SCREEN_SPEC 1#sort / 1#columns: rows grouped by a column (24 px group rows with a count), a
// header context menu, columns resized by dragging the header edge with a live readout.

import { useCallback, useEffect, useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useApp } from '../../state/store'
import { useT, type T } from '../../i18n'
import { COLUMNS, flagsOf, middleTruncate, type Item } from './data'
import { clampWidth, itemsInOrder, visibleColumns, withGroups } from './layout'
import { rowHeight } from '../../app/uiPrefs'
import { PopupMenu } from '../../components/PopupMenu'

const GROUP_ROW = 24
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

interface HeaderMenu {
  key: string
  x: number
  y: number
}

export function MetadataTable({ items, onColumns }: { items: Item[]; onColumns: () => void }) {
  const t = useT()
  const selection = useApp((s) => s.selection)
  const focus = useApp((s) => s.focus)
  const anchor = useApp((s) => s.anchor)
  const select = useApp((s) => s.select)
  const sort = useApp((s) => s.sort)
  const setSort = useApp((s) => s.setSort)
  const layout = useApp((s) => s.layout)
  const setLayout = useApp((s) => s.setLayout)
  const groupBy = useApp((s) => s.groupBy)
  const setGroupBy = useApp((s) => s.setGroupBy)
  const ROW = rowHeight(useApp((s) => s.prefs.density))
  const scroller = useRef<HTMLDivElement>(null)
  const [menu, setMenu] = useState<HeaderMenu | null>(null)
  const [resizing, setResizing] = useState<{ key: string; from: number; to: number } | null>(null)

  const columns = useMemo(() => visibleColumns(layout), [layout])
  const display = useMemo(() => withGroups(items, groupBy), [items, groupBy])
  // selection ranges and the keyboard follow the drawn order
  const ordered = useMemo(() => itemsInOrder(display), [display])
  const v = useVirtualizer({
    count: display.length,
    getScrollElement: () => scroller.current,
    estimateSize: (i) => (display[i]?.kind === 'group' ? GROUP_ROW : ROW),
    overscan: 20,
  })
  useEffect(() => v.measure(), [display, v, ROW])
  // where the scan has got to, in the order shown: the first row still unread (SCREEN_SPEC 1#large)
  const scanning = useApp((s) => s.scan.running)
  const scanLine = useMemo(() => {
    if (!scanning || display.length === 0) return null
    const at = display.findIndex((d) => d.kind === 'item' && !d.it.row)
    return at < 0 ? null : at / display.length
  }, [scanning, display])
  const width = FLAGS_W + columns.reduce((a, c) => a + (resizing?.key === c.key ? resizing.to : c.width), 0)
  const index = useMemo(() => new Map(ordered.map((it, i) => [it.asset.id, i])), [ordered])
  const row = useMemo(() => new Map(display.map((d, i) => [d.kind === 'item' ? d.it.asset.id : -1 - i, i])), [display])

  // keep the focused row in view
  useEffect(() => {
    if (focus === null) return
    const i = row.get(focus)
    if (i !== undefined) v.scrollToIndex(i, { align: 'auto' })
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [focus])

  const closeMenu = useCallback(() => setMenu(null), [])

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

  const startResize = (e: MouseEvent, key: string, from: number) => {
    e.preventDefault()
    e.stopPropagation()
    const x0 = e.clientX
    let to = from
    setResizing({ key, from, to })
    const move = (m: globalThis.MouseEvent) => {
      to = clampWidth(from + m.clientX - x0)
      setResizing({ key, from, to })
    }
    const up = () => {
      window.removeEventListener('mousemove', move)
      window.removeEventListener('mouseup', up)
      setResizing(null)
      if (to !== from) setLayout({ ...layout, widths: { ...layout.widths, [key]: to } })
    }
    window.addEventListener('mousemove', move)
    window.addEventListener('mouseup', up)
  }

  const range = (a: number, b: number) => {
    const [lo, hi] = a < b ? [a, b] : [b, a]
    return ordered.slice(lo, hi + 1).map((it) => it.asset.id)
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
    if (!ordered.length) return
    const cur = focus !== null && index.has(focus) ? index.get(focus)! : -1
    const page = Math.max(1, Math.floor((scroller.current?.clientHeight ?? 400) / ROW) - 1)
    let next = cur
    switch (e.key) {
      case 'ArrowDown':
        next = Math.min(ordered.length - 1, cur + 1)
        break
      case 'ArrowUp':
        next = Math.max(0, cur - 1)
        break
      case 'PageDown':
        next = Math.min(ordered.length - 1, cur + page)
        break
      case 'PageUp':
        next = Math.max(0, cur - page)
        break
      case 'Home':
        next = 0
        break
      case 'End':
        next = ordered.length - 1
        break
      case ' ': {
        e.preventDefault()
        if (cur < 0) return
        const id = ordered[cur].asset.id
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
          select(ordered.map((it) => it.asset.id))
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
    const id = ordered[next].asset.id
    if (e.shiftKey) {
      const a = anchor !== null && index.has(anchor) ? index.get(anchor)! : cur < 0 ? next : cur
      select(range(a, next), id, ordered[a].asset.id)
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

  const menuItems = (key: string) => {
    const label = t(COLUMNS.find((c) => c.key === key)!.label)
    const items: { text: string; run: () => void; disabled?: boolean; checked?: boolean }[] = [
      { text: t('cols.sort_asc'), run: () => setSort([{ key, dir: 1 }]) },
      { text: t('cols.sort_desc'), run: () => setSort([{ key, dir: -1 }]) },
      {
        text: t('cols.sort_add'),
        run: () => setSort([...sort.filter((s) => s.key !== key), { key, dir: 1 }]),
        disabled: sort.length === 1 && sort[0].key === key,
      },
      groupBy === key
        ? { text: t('cols.ungroup'), run: () => setGroupBy(null), checked: true }
        : { text: t('cols.group_by', { name: label }), run: () => setGroupBy(key), disabled: key === 'name' || key === 'size' },
      {
        text: t('cols.hide', { name: label }),
        run: () => setLayout({ ...layout, hidden: [...layout.hidden, key] }),
        disabled: key === 'name',
      },
      { text: t('cols.chooser'), run: onColumns },
    ]
    return items
  }

  return (
    <div className="mtable-wrap">
    <div
      className="mtable"
      ref={scroller}
      tabIndex={0}
      role="grid"
      aria-rowcount={ordered.length}
      aria-multiselectable
      aria-label={t('table.label')}
      onKeyDown={onKey}
    >
      <div className="mtable-inner" style={{ width, height: v.getTotalSize() + 24 }}>
        <div className="mtable-header" role="row" style={{ width }}>
          <div className="mth frozen" style={{ width: FLAGS_W, left: 0 }} role="columnheader">
            <span className="secondary">{t('col.flags')}</span>
          </div>
          {columns.map((c, i) => {
            const w = resizing?.key === c.key ? resizing.to : c.width
            return (
              <div
                key={c.key}
                role="columnheader"
                aria-sort={sort[0]?.key === c.key ? (sort[0].dir === 1 ? 'ascending' : 'descending') : undefined}
                className={`mth${i === 0 ? ' frozen frozen-edge' : ''}${sort.some((s) => s.key === c.key) ? ' sorted' : ''}${i === columns.length - 1 ? ' flex' : ''}`}
                style={{ width: w, left: i === 0 ? FLAGS_W : undefined, textAlign: c.align }}
                onClick={(e) => onHeader(e, c.key)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  setMenu({ key: c.key, x: e.clientX, y: e.clientY })
                }}
                title={t('table.sort_hint')}
              >
                <span className="ellipsis">{t(c.label)}</span>
                {groupBy === c.key && <span className="group-mark" title={t('cols.grouped')}>⊟</span>}
                {sortMark(c.key)}
                <span
                  className="col-resize"
                  role="separator"
                  aria-orientation="vertical"
                  aria-label={t('cols.resize', { name: t(c.label) })}
                  onMouseDown={(e) => startResize(e, c.key, c.width)}
                  onClick={(e) => e.stopPropagation()}
                />
              </div>
            )
          })}
        </div>
        {v.getVirtualItems().map((vr) => {
          const d = display[vr.index]
          if (d.kind === 'group') {
            const col = COLUMNS.find((c) => c.key === groupBy)
            const { text } = cellText(t, groupBy ?? '', d.label)
            return (
              <div
                key={`g-${vr.index}`}
                role="row"
                className="mrow group-row"
                style={{ transform: `translateY(${vr.start + 24}px)`, width, height: GROUP_ROW }}
              >
                <div className="mtd group-cell" role="gridcell">
                  <span className="secondary">{col ? t(col.label) : ''}</span>
                  <span className="mono strong">{text}</span>
                  <span className="mono faint">{d.count}</span>
                </div>
              </div>
            )
          }
          const it = d.it
          const id = it.asset.id
          const selected = selection.has(id)
          const focused = focus === id
          const flags = flagsOf(it)
          return (
            <div
              key={id}
              role="row"
              aria-selected={selected}
              className={`mrow${d.index % 2 ? ' alt' : ''}${selected ? ' selected' : ''}${focused ? ' focused' : ''}${it.asset.writable ? '' : ' readonly'}`}
              style={{ transform: `translateY(${vr.start + 24}px)`, width, height: ROW }}
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
              {columns.map((c, i) => {
                const raw = c.value(it)
                const { text, cls } = cellText(t, c.key, raw)
                const w = resizing?.key === c.key ? resizing.to : c.width
                const shown = c.key === 'name' ? middleTruncate(text, Math.max(8, Math.floor(w / 6.8))) : text
                return (
                  <div
                    key={c.key}
                    role="gridcell"
                    className={`mtd${c.mono ? ' mono' : ''}${i === 0 ? ' frozen frozen-edge' : ''}${i === columns.length - 1 ? ' flex' : ''} ${cls}`}
                    style={{ width: w, left: i === 0 ? FLAGS_W : undefined, textAlign: c.align }}
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
      {resizing && (
        <div className="resize-readout mono" role="status">
          {t(COLUMNS.find((c) => c.key === resizing.key)!.label)} · {resizing.from} → {resizing.to} px
        </div>
      )}
      {menu && <PopupMenu className="header-menu" x={menu.x} y={menu.y} items={menuItems(menu.key)} onClose={closeMenu} />}
    </div>
    {scanLine !== null && (
      <div className="scan-track" aria-hidden>
        <div className="scan-line" style={{ top: `${scanLine * 100}%` }} title={t('scan.line')} />
      </div>
    )}
    </div>
  )
}
