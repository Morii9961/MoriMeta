// SPDX-License-Identifier: GPL-3.0-or-later
// The centre pane while the time tools are open (SCREEN_SPEC §4): every selected file with its
// time now and after (tinted), the change and its order status, and a 128 px timeline of now vs
// after with RAW+JPG pairs highlighted. Nothing is written from here.

import { useMemo, useRef } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import { useApp } from '../../state/store'
import { useT, type MessageKey } from '../../i18n'
import { parseExif } from './timeMath'
import { summarize, type TimeRow } from './timeRows'

const ROW = 22
/** The timeline draws at most this many files (evenly sampled); the table lists them all. */
const MAX_MARKS = 2000

export function TimePreview() {
  const t = useT()
  const rows = useApp((s) => s.timeRows)
  const scroller = useRef<HTMLDivElement>(null)
  const list = rows ?? []
  const v = useVirtualizer({ count: list.length, getScrollElement: () => scroller.current, estimateSize: () => ROW, overscan: 20 })
  const sum = useMemo(() => summarize(list), [list])

  if (!rows) {
    return (
      <div className="time-preview empty">
        <p className="note">{t('time.preview_waiting')}</p>
      </div>
    )
  }
  return (
    <div className="time-preview">
      <div className="time-summary" role="status">
        <span className="mono strong">{t('time.sum_files', { n: sum.files })}</span>
        <span className="mono">· {t('time.sum_changing', { n: sum.changing })}</span>
        <span className={sum.moved || sum.tied ? 'mono glyph-warn' : 'mono glyph-ok'}>
          · {sum.moved || sum.tied ? t('time.sum_order_changed', { moved: sum.moved, tied: sum.tied }) : t('time.sum_order_kept')}
        </span>
        {sum.noTime > 0 && <span className="mono faint">· {t('time.sum_no_time', { n: sum.noTime })}</span>}
        <span className="toolbar-spacer" />
        <span className="faint">{t('time.nothing_written')}</span>
      </div>
      <div className="mtable time-table" ref={scroller} role="grid" aria-rowcount={list.length} aria-label={t('time.table')}>
        <div className="mtable-inner" style={{ height: v.getTotalSize() + 24 }}>
          <div className="mtable-header" role="row">
            <div className="mth tcol-n" role="columnheader">#</div>
            <div className="mth tcol-name" role="columnheader">{t('col.name')}</div>
            <div className="mth tcol-to" role="columnheader">{t('col.writes_to')}</div>
            <div className="mth tcol-time" role="columnheader">{t('time.col_now')}</div>
            <div className="mth tcol-time tint" role="columnheader">{t('time.col_new')}</div>
            <div className="mth tcol-change" role="columnheader">{t('time.col_change')}</div>
            <div className="mth flex" role="columnheader">{t('time.col_order')}</div>
          </div>
          {v.getVirtualItems().map((vr) => {
            const r = list[vr.index]
            return (
              <div key={r.id} role="row" className={`mrow${vr.index % 2 ? ' alt' : ''}${r.pair ? ' paired' : ''}`} style={{ transform: `translateY(${vr.start + 24}px)` }}>
                <div className="mtd mono faint tcol-n" role="gridcell">{vr.index + 1}</div>
                <div className="mtd mono tcol-name" role="gridcell" title={r.name}>
                  {r.pair && <span className="pair-mark" title={t('time.pair')}>◆ </span>}
                  {r.name}
                </div>
                <div className="mtd tcol-to" role="gridcell">{r.writesTo ? t(`writes.${r.writesTo}` as MessageKey) : '…'}</div>
                <div className="mtd mono tcol-time" role="gridcell">{r.now ?? '—'}</div>
                <div className={`mtd mono tcol-time tint${r.next && r.next !== r.now ? ' glyph-mod' : ''}`} role="gridcell">{r.next ?? '—'}</div>
                <div className="mtd mono tcol-change" role="gridcell">{r.change ?? '—'}</div>
                <div className={`mtd flex ${r.order === 'kept' ? 'glyph-ok' : r.order === 'none' ? 'faint' : 'glyph-warn'}`} role="gridcell">
                  {t(`time.order_${r.order}` as MessageKey)}
                </div>
              </div>
            )
          })}
        </div>
      </div>
      <Timeline rows={list} />
    </div>
  )
}

function Timeline({ rows }: { rows: TimeRow[] }) {
  const t = useT()
  const marks = useMemo(() => {
    const timed = rows.filter((r) => parseExif(r.now) !== null || parseExif(r.next) !== null)
    const step = Math.max(1, Math.ceil(timed.length / MAX_MARKS))
    return timed.filter((_, i) => i % step === 0)
  }, [rows])
  const times = marks.flatMap((r) => [parseExif(r.now), parseExif(r.next)]).filter((x): x is number => x !== null)
  if (times.length === 0) return null
  const lo = Math.min(...times)
  const hi = Math.max(...times)
  const span = hi - lo || 1
  const W = 1000
  const x = (s: number) => 20 + ((s - lo) / span) * (W - 40)
  const fmt = (s: number) => new Date(s * 1000).toISOString().replace('T', ' ').slice(0, 19).replace(/-/g, ':')
  return (
    <figure className="timeline" aria-label={t('time.timeline')}>
      <svg viewBox={`0 0 ${W} 128`} preserveAspectRatio="none" role="img" aria-label={t('time.timeline_desc', { n: marks.length })}>
        <line className="tl-axis" x1={20} x2={W - 20} y1={34} y2={34} />
        <line className="tl-axis" x1={20} x2={W - 20} y1={94} y2={94} />
        {marks.map((r) => {
          const n = parseExif(r.now)
          const a = parseExif(r.next)
          const cls = r.pair ? 'tl-pair' : 'tl-mark'
          return (
            <g key={r.id} className={cls}>
              {n !== null && <line x1={x(n)} x2={x(n)} y1={26} y2={42} />}
              {a !== null && <line x1={x(a)} x2={x(a)} y1={86} y2={102} />}
              {n !== null && a !== null && <line className="tl-link" x1={x(n)} x2={x(a)} y1={42} y2={86} />}
            </g>
          )
        })}
      </svg>
      <figcaption className="timeline-legend">
        <span className="mono faint">{fmt(lo)}</span>
        <span>
          <b>{t('time.timeline_now')}</b> ↑ · ↓ <b>{t('time.timeline_after')}</b> · <span className="pair-mark">◆</span> {t('time.pair')}
        </span>
        <span className="mono faint">{fmt(hi)}</span>
      </figcaption>
    </figure>
  )
}
