// SPDX-License-Identifier: GPL-3.0-or-later
// Batch edit (SCREEN_SPEC §3, INTERACTION_SPEC §1–2): selection header, one MixedValueField per
// writable field, the fields that are not editable in 1.0, and a footer with the staged count,
// "0 written", Discard and Preview. Nothing is written from here.

import { useMemo, useState } from 'react'
import type { Row } from '../../ipc/types'
import { useApp, stagedCount } from '../../state/store'
import { useT, fieldLabel, type T } from '../../i18n'
import { openPreview, previewBlocker } from '../../app/actions'
import { timeSummary } from './TimeTools'

type Field = 'creator' | 'copyright' | 'gps'

interface Agg {
  files: number
  values: [string, number][]
  empty: number
  unread: number
  conflicting: number
  /** RAWs (written through a sidecar) that hold a value. */
  rawWithValue: number
}

function aggregate(ids: number[], rows: Map<number, Row>, writable: Set<number>, field: Field | 'capture_time'): Agg {
  const counts = new Map<string, number>()
  const a: Agg = { files: 0, values: [], empty: 0, unread: 0, conflicting: 0, rawWithValue: 0 }
  for (const id of ids) {
    if (!writable.has(id)) continue
    a.files++
    const r = rows.get(id)
    if (!r || r.not_downloaded) {
      a.unread++
      continue
    }
    const v = r[field]
    if (r.conflicts.includes(field)) a.conflicting++
    if (v === null || v === '') a.empty++
    else {
      counts.set(v, (counts.get(v) ?? 0) + 1)
      if (r.writes_to === 'sidecar' || r.writes_to === 'new_sidecar') a.rawWithValue++
    }
  }
  a.values = [...counts.entries()].sort((x, y) => y[1] - x[1])
  return a
}

function aggText(t: T, a: Agg): { text: string; mixed: boolean } {
  const present = a.files - a.empty - a.unread
  if (a.files === 0) return { text: t('batch.no_writable'), mixed: false }
  if (present === 0) return { text: t('batch.all_empty'), mixed: false }
  if (a.values.length === 1 && a.empty === 0) return { text: t('batch.same', { v: a.values[0][0] }), mixed: false }
  if (a.values.length > 1) return { text: t('batch.mixed', { n: a.values.length }), mixed: true }
  return { text: t('batch.present_in', { a: present, b: a.files }), mixed: false }
}

/** Impact of the staged action before Preview: + added, ~ modified, = unchanged, − removed, ⊘. */
function impact(field: Field, op: 'set' | 'clear' | 'remove', value: string, a: Agg) {
  const present = a.files - a.empty - a.unread
  if (op === 'set') {
    const same = a.values.find(([v]) => v === value)?.[1] ?? 0
    return { add: a.empty, mod: present - same, same, rem: 0, uns: 0 }
  }
  // removing GPS from a RAW is not possible through its sidecar (PRODUCT_SPEC §6.8.2)
  const uns = field === 'gps' ? a.rawWithValue : 0
  return { add: 0, mod: 0, same: a.empty, rem: present - uns, uns }
}

function MixedValueField({ field, ids, agg }: { field: Field; ids: number[]; agg: Agg }) {
  const t = useT()
  const staged = useApp((s) => s.staged)
  const stageEdit = useApp((s) => s.stageEdit)
  const unstage = useApp((s) => s.unstage)
  const select = useApp((s) => s.select)
  const rows = useApp((s) => s.rows)
  const cur = staged[field]
  // the mode is the editor's own: Set stays pressed while its value is still being typed
  const [op, setMode] = useState<'leave' | 'set' | 'clear'>(!cur ? 'leave' : cur.op === 'set' ? 'set' : 'clear')
  const stagedValue =
    cur && cur.op === 'set' ? ('values' in cur ? cur.values.join('; ') : 'value' in cur ? cur.value : cur.position) : ''
  const [draft, setDraft] = useState(stagedValue)
  const [invalid, setInvalid] = useState<string | null>(null)
  const { text, mixed } = aggText(t, agg)
  const clearLabel = field === 'gps' ? t('batch.remove') : t('batch.clear')

  const stageSet = (v: string) => {
    const value = v.trim()
    if (!value) {
      // INTERACTION_SPEC §1: Set with an empty value is not allowed; use Clear
      setInvalid(t('batch.empty_not_allowed'))
      unstage(field)
      return
    }
    setInvalid(null)
    if (field === 'creator') {
      stageEdit({ creator: { op: 'set', values: value.split(';').map((x) => x.trim()).filter(Boolean) } })
    } else if (field === 'copyright') {
      stageEdit({ copyright: { op: 'set', value } })
    } else {
      const parts = value.split(',').map((x) => x.trim())
      const nums = parts.map(Number)
      if (parts.length < 2 || parts.length > 3 || nums.some((n) => !Number.isFinite(n)) || Math.abs(nums[0]) > 90 || Math.abs(nums[1]) > 180) {
        setInvalid(t('batch.gps_format'))
        unstage(field)
        return
      }
      stageEdit({ gps: { op: 'set', position: parts.join(',') } })
    }
  }

  const setOp = (next: 'leave' | 'set' | 'clear') => {
    setInvalid(null)
    setMode(next)
    if (next === 'leave') unstage(field)
    else if (next === 'clear') {
      if (field === 'gps') stageEdit({ gps: { op: 'remove' } })
      else if (field === 'creator') stageEdit({ creator: { op: 'clear' } })
      else stageEdit({ copyright: { op: 'clear' } })
    } else {
      // Set on a field with one value pre-fills it (INTERACTION_SPEC §2)
      const pre = draft || (agg.values.length === 1 ? agg.values[0][0] : '')
      setDraft(pre)
      if (pre) stageSet(pre)
      else unstage(field) // nothing is staged until a value is typed
    }
  }

  const imp = op === 'leave' ? null : impact(field, op === 'set' ? 'set' : field === 'gps' ? 'remove' : 'clear', draft.trim(), agg)
  const total = agg.files || 1
  const pick = (value: string | null) =>
    select(
      ids.filter((id) => {
        const r = rows.get(id)
        const v = r ? r[field] : undefined
        return value === null ? v === null || v === '' : v === value
      }),
    )

  return (
    <div className={`mvf${cur ? ' staged' : ''}`}>
      <div className="mvf-head">
        <span className={`mvf-dot${cur ? ' on' : ''}`}>●</span>
        <span className="mvf-label">{fieldLabel(t, field)}</span>
        <span className={`mono mvf-agg${mixed ? ' glyph-warn' : ' secondary'}`}>{text}</span>
      </div>
      {agg.values.length > 0 && (mixed || agg.empty > 0) && (
        <div className="mvf-dist">
          {agg.values.slice(0, 3).map(([v, n]) => (
            <div key={v} className="dist-row">
              <span className="mono ellipsis" title={v}>
                {v}
              </span>
              <span className="mono faint">{n}</span>
              <span className="dist-bar">
                <span style={{ width: `${(56 * n) / total}px` }} />
              </span>
              <button className="link" onClick={() => pick(v)}>
                {t('common.select')}
              </button>
            </div>
          ))}
          {agg.empty > 0 && (
            <div className="dist-row">
              <span className="faint">({t('batch.empty')})</span>
              <span className="mono faint">{agg.empty}</span>
              <span className="dist-bar">
                <span style={{ width: `${(56 * agg.empty) / total}px` }} />
              </span>
              <button className="link" onClick={() => pick(null)}>
                {t('common.select')}
              </button>
            </div>
          )}
          {agg.values.length > 3 && <div className="faint mono">+{agg.values.length - 3}</div>}
        </div>
      )}
      <div className="segmented small mvf-ops">
        <button className="leave" aria-pressed={op === 'leave'} onClick={() => setOp('leave')}>
          {t('batch.leave')}
        </button>
        <button aria-pressed={op === 'set'} onClick={() => setOp('set')}>
          {t('batch.set')}
        </button>
        <button aria-pressed={op === 'clear'} onClick={() => setOp('clear')}>
          {clearLabel}
        </button>
      </div>
      {op === 'set' && (
        <>
          <input
            className={`input mvf-input staged${invalid ? ' invalid' : ''}`}
            value={draft}
            autoFocus
            placeholder={field === 'creator' ? t('batch.ph_creator') : field === 'copyright' ? t('batch.ph_copyright') : t('batch.ph_gps')}
            onChange={(e) => {
              setDraft(e.target.value)
              stageSet(e.target.value)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Escape') {
                setDraft(stagedValue)
                ;(e.target as HTMLInputElement).blur()
              }
            }}
            aria-invalid={!!invalid}
          />
          {invalid && <div className="field-note glyph-fail">{invalid}</div>}
          {field !== 'gps' && !invalid && <div className="field-note faint">{t('batch.template_hint')}</div>}
        </>
      )}
      {imp && (
        <>
          <div className="impact-bar" aria-hidden>
            {imp.add > 0 && <span className="add" style={{ flex: imp.add }} />}
            {imp.mod > 0 && <span className="mod" style={{ flex: imp.mod }} />}
            {imp.rem > 0 && <span className="rem" style={{ flex: imp.rem }} />}
            {imp.uns > 0 && <span className="uns" style={{ flex: imp.uns }} />}
            {imp.same > 0 && <span className="none" style={{ flex: imp.same }} />}
          </div>
          <div className="impact-line mono">
            {imp.add > 0 && <span className="glyph-add">+{imp.add} </span>}
            {imp.mod > 0 && <span className="glyph-mod">~{imp.mod} </span>}
            {imp.rem > 0 && <span className="glyph-rem">−{imp.rem} </span>}
            {imp.uns > 0 && <span className="glyph-uns">⊘{imp.uns} </span>}
            {imp.same > 0 && <span className="glyph-none">={imp.same}</span>}
          </div>
          {imp.uns > 0 && <div className="field-note glyph-uns">{t('batch.gps_raw_note', { n: imp.uns })}</div>}
          {op === 'set' && field === 'copyright' && <div className="field-note faint">{t('batch.estimate_note')}</div>}
        </>
      )}
    </div>
  )
}

function TimeField({ ids, agg }: { ids: number[]; agg: Agg }) {
  const t = useT()
  const staged = useApp((s) => s.staged)
  const unstage = useApp((s) => s.unstage)
  const setTimeToolsOpen = useApp((s) => s.setTimeToolsOpen)
  const { text, mixed } = aggText(t, agg)
  void ids
  return (
    <div className={`mvf${staged.time ? ' staged' : ''}`}>
      <div className="mvf-head">
        <span className={`mvf-dot${staged.time ? ' on' : ''}`}>●</span>
        <span className="mvf-label">{fieldLabel(t, 'capture_time')}</span>
        <span className={`mono mvf-agg${mixed ? ' glyph-warn' : ' secondary'}`}>{text}</span>
      </div>
      {staged.time ? (
        <div className="time-staged">
          <span className="mono glyph-mod">~ {timeSummary(t, staged.time)}</span>
          <button className="link" onClick={() => setTimeToolsOpen(true)}>
            {t('common.edit')}
          </button>
          <button className="link" onClick={() => unstage('time')}>
            {t('batch.leave')}
          </button>
        </div>
      ) : (
        <button className="btn small" onClick={() => setTimeToolsOpen(true)}>
          {t('menu.time_tools')}… <span className="mono faint">T</span>
        </button>
      )}
    </div>
  )
}

/** The editable fields for these files (also the Inspector's Edit section for one file). */
export function BatchFields({ ids }: { ids: number[] }) {
  const t = useT()
  const rows = useApp((s) => s.rows)
  const assets = useApp((s) => s.assets)
  const writable = useMemo(() => new Set(assets.filter((a) => a.writable).map((a) => a.id)), [assets])
  const aggs = useMemo(
    () => ({
      creator: aggregate(ids, rows, writable, 'creator'),
      copyright: aggregate(ids, rows, writable, 'copyright'),
      gps: aggregate(ids, rows, writable, 'gps'),
      time: aggregate(ids, rows, writable, 'capture_time'),
    }),
    [ids, rows, writable],
  )
  // discarding what is staged starts every editor afresh
  const gen = useApp((s) => s.stagedGen)
  return (
    <div className="batch-fields">
      {(['creator', 'copyright', 'gps'] as Field[]).map((f) => (
        <MixedValueField key={`${f}-${gen}`} field={f} ids={ids} agg={aggs[f]} />
      ))}
      <TimeField ids={ids} agg={aggs.time} />
      <div className="protected-group">
        <div className="section-label">{t('batch.protected')}</div>
        <p className="note">{t('batch.protected_note')}</p>
        <div className="section-label">{t('batch.coming')}</div>
        <p className="note">{t('batch.coming_note')}</p>
      </div>
    </div>
  )
}

export function BatchPanel() {
  const t = useT()
  const selection = useApp((s) => s.selection)
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  const staged = useApp((s) => s.staged)
  const discardStaged = useApp((s) => s.discardStaged)
  const ids = useMemo(() => assets.filter((a) => selection.has(a.id)).map((a) => a.id), [assets, selection])
  const split = useMemo(() => {
    const c = { in_file: 0, sidecar: 0, read_only: 0 }
    for (const id of ids) {
      const a = assets.find((x) => x.id === id)
      if (!a?.writable) c.read_only++
      else {
        const w = rows.get(id)?.writes_to
        if (w === 'in_file') c.in_file++
        else if (w) c.sidecar++
      }
    }
    return c
  }, [ids, assets, rows])
  const n = stagedCount(staged)
  const blocker = previewBlocker()
  return (
    <div className="batch">
      <div className="insp-header">
        <div className="insp-name strong">{t('batch.selected', { n: ids.length })}</div>
        <div className="insp-meta mono">
          <span>{t('batch.split_in_file', { n: split.in_file })}</span>
          <span className="faint">·</span>
          <span>{t('batch.split_sidecar', { n: split.sidecar })}</span>
          {split.read_only > 0 && (
            <>
              <span className="faint">·</span>
              <span className="faint">{t('batch.split_read_only', { n: split.read_only })}</span>
            </>
          )}
        </div>
        <div className="insp-meta faint">{t('batch.nothing_written')}</div>
      </div>
      {split.read_only > 0 && <div className="tone warn batch-note">{t('batch.read_only_note', { n: split.read_only })}</div>}
      <div className="pane-scroll insp">
        <BatchFields ids={ids} />
      </div>
      <div className="batch-footer">
        <span className="mono">{t('batch.staged', { n })}</span>
        <span className="mono faint">· {t('batch.written_zero')}</span>
        <div className="toolbar-spacer" />
        <button className="btn small" disabled={n === 0} onClick={discardStaged}>
          {t('batch.discard')}
        </button>
        <button className={`btn small${n > 0 && !blocker ? ' accent' : ''}`} disabled={!!blocker} title={blocker ?? undefined} onClick={() => openPreview()}>
          {t('batch.preview')} <span className="mono faint">Ctrl ↵</span>
        </button>
      </div>
      {n === 0 && <div className="batch-hint faint">{t('preview.need_staged')}</div>}
    </div>
  )
}
