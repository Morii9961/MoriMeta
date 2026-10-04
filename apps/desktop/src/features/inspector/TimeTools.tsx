// SPDX-License-Identifier: GPL-3.0-or-later
// Capture Time Tools (SCREEN_SPEC §4, INTERACTION_SPEC §17, PRODUCT_SPEC §6.5.1). The four MVP
// modes are executable; Change time zone, Sync from reference, Range and Random are listed and
// disabled (1.x; DECISIONS H-3). Parameters → a result preview → Add to plan.

import { useMemo, useState } from 'react'
import type { TimeEdit } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, type MessageKey, type T } from '../../i18n'
import { openPreview } from '../../app/actions'
import { formatExif, formatShift, localFromParts, naturalCompare, parseExif, parseShift } from './timeMath'

type Mode = 'absolute' | 'shift' | 'sequence' | 'preserve'

const MODES: { key: Mode | 'tz' | 'sync' | 'range' | 'random'; label: MessageKey; enabled: boolean }[] = [
  { key: 'absolute', label: 'time.absolute', enabled: true },
  { key: 'shift', label: 'time.shift', enabled: true },
  { key: 'sequence', label: 'time.sequence', enabled: true },
  { key: 'preserve', label: 'time.preserve', enabled: true },
  { key: 'tz', label: 'time.tz', enabled: false },
  { key: 'sync', label: 'time.sync', enabled: false },
  { key: 'range', label: 'time.range', enabled: false },
  { key: 'random', label: 'time.random', enabled: false },
]

export function timeSummary(t: T, e: TimeEdit): string {
  switch (e.mode) {
    case 'absolute':
      return t('time.sum_absolute', { to: e.to })
    case 'shift':
      return t('time.sum_shift', { by: e.by })
    case 'sequence':
      return t('time.sum_sequence', { start: e.start, step: e.step })
    case 'preserve':
      return t('time.sum_preserve', { to: e.to })
  }
}

/** `YYYY:MM:DD HH:MM:SS` → the panel's `YYYY-MM-DD` and `HH:MM:SS`. */
function exifParts(v: string): [string, string] {
  const [d = '', c = ''] = v.split(' ')
  return [d.replace(/:/g, '-'), c]
}

/** `[+|-][Nd]HH:MM:SS` → the shift fields. */
function shiftParts(by: string) {
  const m = /^([+-]?)(?:(\d+)d)?(\d{1,2}):(\d{2}):(\d{2})$/.exec(by.trim())
  if (!m) return { sign: '+', d: '0', h: '0', m: '0', s: '0' }
  return { sign: m[1] === '-' ? '-' : '+', d: String(Number(m[2] ?? 0)), h: String(Number(m[3])), m: String(Number(m[4])), s: String(Number(m[5])) }
}

function stepParts(step: string) {
  const p = shiftParts(step)
  return { h: String(Number(p.d) * 24 + Number(p.h)), m: p.m, s: p.s }
}

export function TimeTools() {
  const t = useT()
  const selection = useApp((s) => s.selection)
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  const staged = useApp((s) => s.staged)
  const stageEdit = useApp((s) => s.stageEdit)
  const setOpen = useApp((s) => s.setTimeToolsOpen)
  // reopening the panel shows the staged parameters again
  const init = staged.time
  const initTo = init?.mode === 'absolute' || init?.mode === 'preserve' ? init.to : init?.mode === 'sequence' ? init.start : null
  const [mode, setMode] = useState<Mode>(init?.mode ?? 'shift')
  const [date, setDate] = useState(initTo ? exifParts(initTo)[0] : '')
  const [clock, setClock] = useState(initTo ? exifParts(initTo)[1] : '')
  const [shift, setShift] = useState(init?.mode === 'shift' ? shiftParts(init.by) : { sign: '+', d: '0', h: '0', m: '0', s: '0' })
  const [step, setStep] = useState(init?.mode === 'sequence' ? stepParts(init.step) : { h: '0', m: '0', s: '1' })
  const [order, setOrder] = useState<'time' | 'name'>(init?.mode === 'sequence' ? init.order : 'time')
  const [orderConfirmed, setOrderConfirmed] = useState(init?.mode === 'sequence')
  const [anchor, setAnchor] = useState<number | null>(init?.mode === 'preserve' ? init.anchor : null)
  const [digitized, setDigitized] = useState(staged.digitized ?? true)

  const files = useMemo(
    () =>
      assets
        .filter((a) => selection.has(a.id) && a.writable)
        .map((a) => ({ a, now: rows.get(a.id)?.capture_time ?? null })),
    [assets, selection, rows],
  )

  const shiftSec = parseShift(
    `${shift.sign}${Number(shift.d) ? `${Number(shift.d)}d` : ''}${String(Number(shift.h) || 0).padStart(2, '0')}:${String(Number(shift.m) || 0).padStart(2, '0')}:${String(Number(shift.s) || 0).padStart(2, '0')}`,
  )
  const stepSec = (Number(step.h) || 0) * 3600 + (Number(step.m) || 0) * 60 + (Number(step.s) || 0)
  const local = localFromParts(date, clock)
  const anchorFile = files.find((f) => f.a.id === anchor)

  // the edit these parameters make, or why not yet
  const built: { edit: TimeEdit | null; why: string | null } = (() => {
    switch (mode) {
      case 'absolute':
        return local ? { edit: { mode, to: local }, why: null } : { edit: null, why: t('time.need_datetime') }
      case 'shift':
        if (shiftSec === null || shiftSec === 0) return { edit: null, why: t('time.need_shift') }
        return { edit: { mode, by: formatShift(shiftSec) }, why: null }
      case 'sequence':
        if (!local) return { edit: null, why: t('time.need_start') }
        if (stepSec <= 0) return { edit: null, why: t('time.need_step') }
        if (!orderConfirmed) return { edit: null, why: t('time.need_order') }
        return { edit: { mode, start: local, step: formatShift(stepSec), order }, why: null }
      case 'preserve':
        if (!anchorFile) return { edit: null, why: t('time.need_anchor') }
        if (!parseExif(anchorFile.now)) return { edit: null, why: t('time.anchor_no_time') }
        if (!local) return { edit: null, why: t('time.need_datetime') }
        return { edit: { mode, anchor: anchorFile.a.id, to: local }, why: null }
    }
  })()

  // result preview: now → new for the first files
  const result = useMemo(() => {
    const out: { name: string; now: string | null; next: string | null; pair?: boolean }[] = []
    const e = built.edit
    if (!e) return out
    if (e.mode === 'sequence') {
      const key = (f: (typeof files)[number]) => `${f.a.folder}|${f.a.name.replace(/\.[^.]+$/, '').toLowerCase()}`
      const sorted = [...files].sort((x, y) => {
        if (e.order === 'time') {
          const a = parseExif(x.now) ?? Infinity
          const b = parseExif(y.now) ?? Infinity
          if (a !== b) return a - b
        }
        return naturalCompare(x.a.name, y.a.name)
      })
      const positions = new Map<string, number>()
      const start = parseExif(e.start)!
      for (const f of sorted) {
        const k = key(f)
        if (!positions.has(k)) positions.set(k, positions.size)
        out.push({ name: f.a.name, now: f.now, next: formatExif(start + positions.get(k)! * stepSec), pair: true })
      }
      return out
    }
    const delta =
      e.mode === 'shift'
        ? shiftSec!
        : e.mode === 'preserve'
          ? parseExif(e.to)! - parseExif(anchorFile!.now)!
          : null
    for (const f of files) {
      const now = parseExif(f.now)
      const next = e.mode === 'absolute' ? e.to : now === null || delta === null ? null : formatExif(now + delta)
      out.push({ name: f.a.name, now: f.now, next })
    }
    return out
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [JSON.stringify(built.edit), files, stepSec])

  const noTime = files.filter((f) => !parseExif(f.now)).length
  const addToPlan = () => {
    if (!built.edit) return
    stageEdit({ time: built.edit, digitized })
    setOpen(false)
  }

  return (
    <div className="time-tools">
      <div className="insp-header">
        <div className="insp-name strong">{t('menu.time_tools')}</div>
        <div className="insp-meta mono">{t('time.files', { n: files.length })}</div>
      </div>
      <div className="pane-scroll insp">
        <div className="mode-grid" role="radiogroup" aria-label={t('time.mode')}>
          {MODES.map((m) => (
            <label key={m.key} className={`mode-radio${m.enabled ? '' : ' disabled'}`} title={m.enabled ? undefined : t('time.later')}>
              <input
                type="radio"
                name="time-mode"
                disabled={!m.enabled}
                checked={mode === m.key}
                onChange={() => m.enabled && setMode(m.key as Mode)}
              />
              <span>{t(m.label)}</span>
              {!m.enabled && <span className="tag">1.x</span>}
            </label>
          ))}
        </div>

        {(mode === 'absolute' || mode === 'sequence' || mode === 'preserve') && (
          <div className="param-row">
            <span className="field-label">{mode === 'sequence' ? t('time.start') : mode === 'preserve' ? t('time.should_be') : t('time.set_to')}</span>
            <input className="input" type="text" placeholder="YYYY-MM-DD" value={date} onChange={(e) => setDate(e.target.value)} aria-label={t('time.date')} />
            <input className="input" type="text" placeholder="HH:MM:SS" value={clock} onChange={(e) => setClock(e.target.value)} aria-label={t('time.clock')} />
          </div>
        )}
        {mode === 'shift' && (
          <div className="param-row shift-row">
            <span className="field-label">{t('time.by')}</span>
            <div className="segmented small">
              {['+', '-'].map((sg) => (
                <button key={sg} aria-pressed={shift.sign === sg} onClick={() => setShift({ ...shift, sign: sg })}>
                  {sg === '+' ? '+' : '−'}
                </button>
              ))}
            </div>
            {(['d', 'h', 'm', 's'] as const).map((u) => (
              <label key={u} className="unit">
                <input className="input num" inputMode="numeric" value={shift[u]} onChange={(e) => setShift({ ...shift, [u]: e.target.value.replace(/\D/g, '') })} />
                <span className="faint">{u}</span>
              </label>
            ))}
          </div>
        )}
        {mode === 'sequence' && (
          <>
            <div className="param-row shift-row">
              <span className="field-label">{t('time.step')}</span>
              {(['h', 'm', 's'] as const).map((u) => (
                <label key={u} className="unit">
                  <input className="input num" inputMode="numeric" value={step[u]} onChange={(e) => setStep({ ...step, [u]: e.target.value.replace(/\D/g, '') })} />
                  <span className="faint">{u}</span>
                </label>
              ))}
            </div>
            <div className="param-row">
              <span className="field-label">{t('time.order_by')}</span>
              <div className="segmented small">
                <button aria-pressed={order === 'time'} onClick={() => { setOrder('time'); setOrderConfirmed(false) }}>
                  {t('time.order_time')}
                </button>
                <button aria-pressed={order === 'name'} onClick={() => { setOrder('name'); setOrderConfirmed(false) }}>
                  {t('time.order_name')}
                </button>
              </div>
            </div>
            <label className="ack small-ack">
              <input type="checkbox" className="checkbox" checked={orderConfirmed} onChange={(e) => setOrderConfirmed(e.target.checked)} />
              <span>{t('time.confirm_order')}</span>
            </label>
            <p className="note">{t('time.pairs_note')}</p>
          </>
        )}
        {mode === 'preserve' && (
          <div className="param-row">
            <span className="field-label">{t('time.reference')}</span>
            <select className="input ref-select" value={anchor ?? ''} onChange={(e) => setAnchor(e.target.value ? Number(e.target.value) : null)}>
              <option value="">{t('time.choose_reference')}</option>
              {files.map((f) => (
                <option key={f.a.id} value={f.a.id}>
                  {f.a.name} · {f.now ?? '—'}
                </option>
              ))}
            </select>
          </div>
        )}
        <label className="ack small-ack">
          <input type="checkbox" className="checkbox" checked={digitized} onChange={(e) => setDigitized(e.target.checked)} />
          <span>{t('time.digitized')}</span>
        </label>

        <div className="insp-section">
          <div className="section-label">{t('time.result')}</div>
          {built.why ? (
            <p className="note">{built.why}</p>
          ) : (
            <div className="result-list">
              {result.slice(0, 12).map((r, i) => (
                <div key={i} className="result-row">
                  <span className="mono ellipsis" title={r.name}>
                    {r.name}
                  </span>
                  <span className="mono faint">{r.now ?? '—'}</span>
                  <span className="mono glyph-mod">→ {r.next ?? '—'}</span>
                </div>
              ))}
              {result.length > 12 && <div className="faint mono">+{result.length - 12}</div>}
            </div>
          )}
          {noTime > 0 && mode !== 'absolute' && (
            <div className="tone warn">{t('time.no_time_files', { n: noTime })}</div>
          )}
          {mode === 'absolute' && files.length > 1 && <div className="tone warn">{t('time.shared_warning')}</div>}
        </div>
        <div className="insp-section">
          <p className="note">{t('time.note_gps')}</p>
          <p className="note">{t('time.note_offset')}</p>
          <p className="note">{t('time.note_sidecar')}</p>
        </div>
      </div>
      <div className="batch-footer">
        <button className="btn small" onClick={() => setOpen(false)}>
          {t('common.close')}
        </button>
        <div className="toolbar-spacer" />
        <button className="btn small" disabled={!built.edit} onClick={addToPlan} title={built.why ?? undefined}>
          {t('time.add_to_plan')}
        </button>
        <button
          className={`btn small${built.edit ? ' accent' : ''}`}
          disabled={!built.edit}
          onClick={() => {
            addToPlan()
            openPreview()
          }}
        >
          {t('batch.preview')} <span className="mono faint">Ctrl ↵</span>
        </button>
      </div>
    </div>
  )
}
