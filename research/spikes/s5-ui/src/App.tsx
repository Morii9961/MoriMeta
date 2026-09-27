// S5 UI technology spike — NOT product UI (no visual design decisions; neutral styling only).
// Measures: IPC transfer of 5,000 rows x 30 fields, virtualized ARIA grid scroll fps, sort/filter
// latency, Channel progress rate. Manual checks (IME, Narrator/NVDA) use the same screen.
import { useEffect, useRef, useState } from 'react'
import { Channel, invoke } from '@tauri-apps/api/core'
import { useVirtualizer } from '@tanstack/react-virtual'
import {
  columnFilteringFeature,
  createColumnHelper,
  createFilteredRowModel,
  createSortedRowModel,
  filterFn_includesString,
  globalFilteringFeature,
  rowSortingFeature,
  sortFn_alphanumeric,
  sortFn_text,
  tableFeatures,
  useTable,
} from '@tanstack/react-table'
import type { Row } from './bindings/Row'
import type { Progress } from './bindings/Progress'

const features = tableFeatures({
  rowSortingFeature,
  sortedRowModel: createSortedRowModel(),
  sortFns: { alphanumeric: sortFn_alphanumeric, text: sortFn_text },
  columnFilteringFeature,
  globalFilteringFeature,
  filteredRowModel: createFilteredRowModel(),
  filterFns: { includesString: filterFn_includesString },
})

const HEADERS = [
  'File', 'Type', 'Captured', 'Offset', 'Camera', 'Lens', 'ISO', 'Aperture', 'Shutter', 'Focal',
  'GPS', 'Creator', 'Copyright', 'Rating', 'Target', 'F16', 'F17', 'F18', 'F19', 'F20',
]
const COLS = HEADERS.length
const WIDTHS = HEADERS.map((_, i) => (i === 0 || i === 5 ? 190 : 110))
const ROW_H = 24
const helper = createColumnHelper<typeof features, Row>()
const columns = helper.columns(
  HEADERS.map((h, i) =>
    helper.accessor((r: Row) => r.fields[i], { id: `c${i}`, header: h }),
  ),
)

type SortingState = { id: string; desc: boolean }[]
const nextPaint = () => new Promise<void>((r) => requestAnimationFrame(() => requestAnimationFrame(() => r())))

export function App() {
  const [data, setData] = useState<Row[]>([])
  const [sorting, setSorting] = useState<SortingState>([])
  const [globalFilter, setGlobalFilter] = useState('')
  const [active, setActive] = useState<{ r: number; c: number }>({ r: 0, c: 0 })
  const [editing, setEditing] = useState<{ r: number; c: number; v: string } | null>(null)
  const [edits, setEdits] = useState<Record<string, string>>({})
  const [status, setStatus] = useState('loading…')
  const scrollRef = useRef<HTMLDivElement>(null)

  const table = useTable({
    features,
    columns,
    data,
    state: { sorting, globalFilter },
    onSortingChange: setSorting as never,
    onGlobalFilterChange: setGlobalFilter as never,
    globalFilterFn: 'includesString',
  } as never) as any

  const rows = table.getRowModel().rows as any[]
  const virtualizer = useVirtualizer({
    count: rows.length,
    getScrollElement: () => scrollRef.current,
    estimateSize: () => ROW_H,
    overscan: 10,
  })

  // benchmark plumbing: state setters and the latest row count, reachable from async code
  const api = useRef({ setSorting, setGlobalFilter, setData, rowCount: 0 })
  api.current = { setSorting, setGlobalFilter, setData, rowCount: rows.length }

  useEffect(() => {
    let cancelled = false
    ;(async () => {
      const result: Record<string, unknown> = {
        userAgent: navigator.userAgent,
        viewport: [window.innerWidth, window.innerHeight],
        devicePixelRatio: window.devicePixelRatio,
      }
      // 1 IPC: 5,000 rows x 30 fields
      const ipc: number[] = []
      let loaded: Row[] = []
      for (let i = 0; i < 3; i++) {
        const t0 = performance.now()
        loaded = await invoke<Row[]>('rows', { count: 5000 })
        ipc.push(performance.now() - t0)
      }
      result.ipc_rows_ms = ipc.map((x) => Math.round(x))
      result.ipc_payload_bytes = new TextEncoder().encode(JSON.stringify(loaded)).length
      const t0 = performance.now()
      api.current.setData(loaded)
      await nextPaint()
      result.first_render_ms = Math.round(performance.now() - t0)
      if (cancelled) return
      setStatus(`${loaded.length} rows loaded`)
      const auto = await invoke<boolean>('autorun')
      if (!auto) return
      await new Promise((r) => setTimeout(r, 500))
      // 2 scroll: continuous programmatic scrolling for 5 s
      const el = scrollRef.current!
      const frames: number[] = []
      const t1 = performance.now()
      await new Promise<void>((done) => {
        const step = (ts: number) => {
          frames.push(ts)
          el.scrollTop = el.scrollTop + 90 >= el.scrollHeight - el.clientHeight ? 0 : el.scrollTop + 90
          if (performance.now() - t1 < 5000) requestAnimationFrame(step)
          else done()
        }
        requestAnimationFrame(step)
      })
      const gaps = frames.slice(1).map((f, i) => f - frames[i]).sort((a, b) => a - b)
      result.scroll = {
        frames: frames.length,
        fps: Math.round((frames.length / ((frames[frames.length - 1] - frames[0]) / 1000)) * 10) / 10,
        p95_frame_ms: Math.round(gaps[Math.floor(gaps.length * 0.95)] * 10) / 10,
        long_frames_over_33ms: gaps.filter((g) => g > 33).length,
      }
      el.scrollTop = 0
      // 3 sort and filter latency (state change -> painted)
      const timeIt = async (fn: () => void) => {
        const s = performance.now()
        fn()
        await nextPaint()
        return Math.round(performance.now() - s)
      }
      result.sort_ms = {
        captured_asc: await timeIt(() => api.current.setSorting([{ id: 'c2', desc: false }])),
        captured_desc: await timeIt(() => api.current.setSorting([{ id: 'c2', desc: true }])),
        lens_text: await timeIt(() => api.current.setSorting([{ id: 'c5', desc: false }])),
      }
      result.filter_ms = await timeIt(() => api.current.setGlobalFilter('Z 8'))
      result.filter_rows = api.current.rowCount
      await timeIt(() => api.current.setGlobalFilter(''))
      // 4 DOM / accessibility structure
      const grid = document.querySelector('[role=grid]')
      result.aria = {
        grid: !!grid,
        rowcount: grid?.getAttribute('aria-rowcount'),
        rendered_rows: document.querySelectorAll('[role=row]').length,
        gridcells: document.querySelectorAll('[role=gridcell]').length,
      }
      // 5 Channel progress events
      let received = 0
      const ch = new Channel<Progress>()
      ch.onmessage = () => {
        received++
      }
      const t2 = performance.now()
      const sent = await invoke<number>('progress', { total: 5000, onEvent: ch })
      while (received < sent && performance.now() - t2 < 10000) await new Promise((r) => setTimeout(r, 5))
      const dt = performance.now() - t2
      result.channel = { sent, received, ms: Math.round(dt), events_per_s: Math.round(received / (dt / 1000)) }
      // 6 least privilege: a core plugin command must be refused (no plugin permissions granted)
      try {
        await invoke('plugin:window|set_title', { label: 'main', value: 'should be denied' })
        result.plugin_command_denied = false
      } catch (e) {
        result.plugin_command_denied = true
        result.plugin_command_error = String(e).slice(0, 200)
      }
      await invoke('report', { json: JSON.stringify(result, null, 2) })
    })()
    return () => {
      cancelled = true
    }
  }, [])

  const cellKey = (r: number, c: number) => `${rows[r]?.id}:${c}`
  const onKey = (e: React.KeyboardEvent) => {
    if (editing) return
    const d: Record<string, [number, number]> = { ArrowDown: [1, 0], ArrowUp: [-1, 0], ArrowRight: [0, 1], ArrowLeft: [0, -1], PageDown: [20, 0], PageUp: [-20, 0] }
    if (e.key in d) {
      e.preventDefault()
      const [dr, dc] = d[e.key]
      const r = Math.max(0, Math.min(rows.length - 1, active.r + dr))
      const c = Math.max(0, Math.min(COLS - 1, active.c + dc))
      setActive({ r, c })
      virtualizer.scrollToIndex(r)
    } else if (e.key === 'F2' || e.key === 'Enter') {
      const v = edits[cellKey(active.r, active.c)] ?? String(rows[active.r]?.getAllCells()[active.c]?.getValue() ?? '')
      setEditing({ r: active.r, c: active.c, v })
    }
  }

  return (
    <div style={{ font: '13px system-ui, sans-serif', padding: 8 }}>
      <div style={{ display: 'flex', gap: 8, marginBottom: 6 }}>
        <label>
          Filter{' '}
          <input value={globalFilter} onChange={(e) => setGlobalFilter(e.target.value)} placeholder="type (IME test)" />
        </label>
        <span aria-live="polite">{status} · {rows.length} shown · F2/Enter edits a cell</span>
      </div>
      <div
        role="grid"
        aria-rowcount={rows.length + 1}
        aria-colcount={COLS}
        aria-label="Spike metadata table"
        tabIndex={0}
        onKeyDown={onKey}
        ref={scrollRef}
        style={{ height: 780, overflow: 'auto', border: '1px solid #999', position: 'relative' }}
      >
        <div role="row" aria-rowindex={1} style={{ display: 'flex', position: 'sticky', top: 0, background: '#eee', zIndex: 1 }}>
          {table.getHeaderGroups()[0].headers.map((h: any, i: number) => {
            const s = sorting.find((x) => x.id === h.column.id)
            return (
              <div
                key={h.id}
                role="columnheader"
                aria-colindex={i + 1}
                aria-sort={s ? (s.desc ? 'descending' : 'ascending') : 'none'}
                onClick={() => setSorting([{ id: h.column.id, desc: s ? !s.desc : false }])}
                style={{ width: WIDTHS[i], flex: 'none', padding: '2px 4px', fontWeight: 600, cursor: 'pointer' }}
              >
                {HEADERS[i]}
                {s ? (s.desc ? ' ▼' : ' ▲') : ''}
              </div>
            )
          })}
        </div>
        <div style={{ height: virtualizer.getTotalSize(), position: 'relative' }}>
          {virtualizer.getVirtualItems().map((vi) => {
            const row = rows[vi.index]
            return (
              <div
                key={row.id}
                role="row"
                aria-rowindex={vi.index + 2}
                aria-selected={vi.index === active.r}
                style={{ display: 'flex', position: 'absolute', top: 0, height: ROW_H, transform: `translateY(${vi.start}px)`, background: vi.index === active.r ? '#dde8f5' : undefined }}
              >
                {row.getAllCells().map((cell: any, c: number) => {
                  const k = cellKey(vi.index, c)
                  const isActive = vi.index === active.r && c === active.c
                  const isEditing = editing && editing.r === vi.index && editing.c === c
                  return (
                    <div
                      key={cell.id}
                      role="gridcell"
                      aria-colindex={c + 1}
                      onClick={() => setActive({ r: vi.index, c })}
                      onDoubleClick={() => setEditing({ r: vi.index, c, v: edits[k] ?? String(cell.getValue() ?? '') })}
                      style={{ width: WIDTHS[c], flex: 'none', padding: '2px 4px', overflow: 'hidden', whiteSpace: 'nowrap', textOverflow: 'ellipsis', outline: isActive ? '2px solid #3b6fb6' : undefined, fontVariantNumeric: 'tabular-nums' }}
                    >
                      {isEditing ? (
                        <input
                          autoFocus
                          value={editing.v}
                          onChange={(e) => setEditing({ ...editing, v: e.target.value })}
                          onKeyDown={(e) => {
                            // Enter while an IME composition is active must not commit (IME test)
                            if (e.key === 'Enter' && !e.nativeEvent.isComposing) {
                              setEdits({ ...edits, [k]: editing.v })
                              setEditing(null)
                            } else if (e.key === 'Escape') setEditing(null)
                          }}
                          style={{ width: '100%', font: 'inherit' }}
                        />
                      ) : (
                        (edits[k] ?? String(cell.getValue() ?? ''))
                      )}
                    </div>
                  )
                })}
              </div>
            )
          })}
        </div>
      </div>
    </div>
  )
}
