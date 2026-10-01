// SPDX-License-Identifier: GPL-3.0-or-later
// DiffTable (DESIGN_SYSTEM): By file (34 px rows; ✓ · File · Writes to · one column per edited
// field · notes) and By change (24 px rows; one row per change). Row checkbox excludes a file,
// a cell click excludes one change; unsupported and skipped cells are not toggleable.

import { useRef, type KeyboardEvent } from 'react'
import { useVirtualizer } from '@tanstack/react-virtual'
import type { FieldChange, PlanEntry, PlanView } from '../../ipc/types'
import { useT, useBT, fieldLabel, type MessageKey } from '../../i18n'
import { GLYPH, GLYPH_CLASS, kindsOf, valueText, type Kind } from './model'

interface Props {
  plan: PlanView
  entries: PlanEntry[]
  view: 'file' | 'change'
  filter: Kind | 'all'
  focus: number | null
  busy: boolean
  onFocus: (seq: number) => void
  onExcludeFile: (seq: number, excluded: boolean) => void
  onExcludeChange: (seq: number, field: string, excluded: boolean) => void
}

function ChangeCell({ c, excluded, dim, onToggle, disabled }: { c: FieldChange; excluded: boolean; dim: boolean; onToggle: () => void; disabled: boolean }) {
  const t = useT()
  const k: Kind = excluded ? 'excluded' : c.kind
  const before = valueText(c.before)
  const after = valueText(c.after)
  return (
    <button
      className={`dcell ${excluded ? 'excluded' : ''}${dim ? ' dim' : ''}`}
      onClick={(e) => {
        e.stopPropagation()
        onToggle()
      }}
      disabled={disabled}
      title={excluded ? t('preview.include_change') : t('preview.exclude_change')}
    >
      <span className={`mono dglyph ${GLYPH_CLASS[k]}`}>{GLYPH[k]}</span>
      <span className="dvals">
        {c.kind !== 'add' && (
          <span className="mono dbefore" title={before}>
            {before || '—'}
          </span>
        )}
        <span className={`mono dafter ${c.kind === 'add' ? 'glyph-add' : c.kind === 'remove' ? 'glyph-rem' : ''}`} title={after}>
          {c.kind === 'remove' ? t('preview.removed') : after}
        </span>
      </span>
    </button>
  )
}

export function DiffTable(p: Props) {
  const t = useT()
  const bt = useBT()
  const scroller = useRef<HTMLDivElement>(null)
  const fields = p.plan.fields

  // By change: one row per change (entries without changes get one row for their status)
  const changeRows: { e: PlanEntry; c: FieldChange | null; excluded: boolean; first: boolean }[] = []
  if (p.view === 'change') {
    for (const e of p.entries) {
      const all = [
        ...e.changes.map((c) => ({ c, excluded: e.excluded })),
        ...(e.excluded_changes ?? []).map((c) => ({ c, excluded: true })),
      ]
      if (all.length === 0) changeRows.push({ e, c: null, excluded: e.excluded, first: true })
      all.forEach((x, i) => {
        if (p.filter !== 'all' && p.filter !== 'excluded' && p.filter !== 'warning' && p.filter !== 'unsupported' && !x.excluded && x.c.kind !== p.filter) return
        changeRows.push({ e, c: x.c, excluded: x.excluded, first: i === 0 })
      })
    }
  }
  const count = p.view === 'file' ? p.entries.length : changeRows.length
  const height = p.view === 'file' ? 34 : 24
  const v = useVirtualizer({ count, getScrollElement: () => scroller.current, estimateSize: () => height, overscan: 16 })

  const index = p.entries.findIndex((e) => e.seq === p.focus)
  const onKey = (e: KeyboardEvent) => {
    if (p.view !== 'file' || !p.entries.length) return
    if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
      e.preventDefault()
      const next = Math.max(0, Math.min(p.entries.length - 1, index + (e.key === 'ArrowDown' ? 1 : -1)))
      p.onFocus(p.entries[next].seq)
      v.scrollToIndex(next)
    } else if (e.key === ' ' && index >= 0) {
      e.preventDefault()
      const en = p.entries[index]
      if (!p.busy) p.onExcludeFile(en.seq, !en.excluded)
    }
  }

  const statusText = (e: PlanEntry): { text: string; cls: string } => {
    if (e.status.status === 'no_change') return { text: `= ${t('kind.no_change')}`, cls: 'glyph-none' }
    if (e.status.status === 'blocked') return { text: `× ${bt(e.status.reason)}`, cls: 'glyph-fail' }
    if (e.status.status === 'unsupported') return { text: `⊘ ${bt(e.status.reason)}`, cls: 'glyph-uns' }
    return { text: '', cls: '' }
  }

  const fileHeader = (
    <div className="dhead" style={{ gridTemplateColumns: `28px minmax(180px, 1.4fr) 104px repeat(${Math.max(1, fields.length)}, minmax(150px, 1fr)) minmax(160px, 1.2fr)` }}>
      <span />
      <span>{t('col.name')}</span>
      <span>{t('col.writes_to')}</span>
      {fields.map((f) => (
        <span key={f}>{fieldLabel(t, f)}</span>
      ))}
      {fields.length === 0 && <span />}
      <span>{t('preview.notes')}</span>
    </div>
  )

  return (
    <div className="dtable" ref={scroller} tabIndex={0} onKeyDown={onKey} role="grid" aria-label={t('preview.table')}>
      {p.view === 'file' ? (
        fileHeader
      ) : (
        <div className="dhead" style={{ gridTemplateColumns: '28px minmax(160px, 1.2fr) 110px 80px minmax(140px,1fr) minmax(140px,1fr) 104px minmax(160px,1.2fr)' }}>
          <span />
          <span>{t('col.name')}</span>
          <span>{t('preview.field')}</span>
          <span>{t('preview.change')}</span>
          <span>{t('preview.before')}</span>
          <span>{t('preview.after')}</span>
          <span>{t('col.writes_to')}</span>
          <span>{t('preview.notes')}</span>
        </div>
      )}
      <div style={{ height: v.getTotalSize(), position: 'relative' }}>
        {v.getVirtualItems().map((vr) => {
          if (p.view === 'file') {
            const e = p.entries[vr.index]
            const kinds = kindsOf(e)
            const st = statusText(e)
            const notes = [...e.warnings.map((w) => `! ${bt(w)}`), ...e.notes.filter((n) => !n.startsWith('warning: ')).map(bt)]
            const toggleable = e.status.status === 'ready' || e.excluded
            return (
              <div
                key={e.seq}
                role="row"
                className={`drow file${vr.index % 2 ? ' alt' : ''}${e.excluded ? ' excluded' : ''}${p.focus === e.seq ? ' focused' : ''}${kinds.has('warning') ? ' warn' : ''}`}
                style={{
                  transform: `translateY(${vr.start}px)`,
                  gridTemplateColumns: `28px minmax(180px, 1.4fr) 104px repeat(${Math.max(1, fields.length)}, minmax(150px, 1fr)) minmax(160px, 1.2fr)`,
                }}
                onMouseDown={() => p.onFocus(e.seq)}
              >
                <span className="dcheck">
                  <input
                    type="checkbox"
                    className="checkbox"
                    checked={!e.excluded}
                    disabled={p.busy || !toggleable}
                    onChange={() => p.onExcludeFile(e.seq, !e.excluded)}
                    aria-label={t('preview.include_file')}
                  />
                </span>
                <span className="mono ellipsis dname" title={e.path}>
                  {e.name}
                </span>
                <span className="secondary ellipsis">{t(`writes.${e.target}` as MessageKey)}</span>
                {fields.map((f) => {
                  const c = e.changes.find((x) => x.field === f)
                  const xc = e.excluded_changes?.find((x) => x.field === f)
                  if (c || xc) {
                    const change = (c ?? xc)!
                    const excluded = !c || e.excluded
                    const dim = p.filter !== 'all' && p.filter !== 'excluded' && p.filter !== 'warning' && change.kind !== p.filter
                    return (
                      <ChangeCell
                        key={f}
                        c={change}
                        excluded={excluded}
                        dim={dim}
                        disabled={p.busy || e.excluded || e.status.status !== 'ready'}
                        onToggle={() => p.onExcludeChange(e.seq, f, !!c)}
                      />
                    )
                  }
                  if (e.status.status === 'unsupported' || e.status.status === 'blocked') {
                    return (
                      <span key={f} className="dcell hatched">
                        <span className="mono dglyph glyph-uns">⊘</span>
                      </span>
                    )
                  }
                  return (
                    <span key={f} className="dcell">
                      <span className="mono dglyph glyph-none">=</span>
                    </span>
                  )
                })}
                {fields.length === 0 && <span />}
                <span className={`dnote ellipsis ${st.cls}`} title={[st.text, ...notes].filter(Boolean).join('\n')}>
                  {st.text || notes[0] || ''}
                  {notes.length > 1 && !st.text ? <span className="faint"> +{notes.length - 1}</span> : null}
                </span>
              </div>
            )
          }
          const r = changeRows[vr.index]
          const st = statusText(r.e)
          const k: Kind | null = r.c ? (r.excluded ? 'excluded' : r.c.kind) : null
          return (
            <div
              key={`${r.e.seq}-${r.c?.field ?? 'x'}-${vr.index}`}
              role="row"
              className={`drow change${vr.index % 2 ? ' alt' : ''}${r.excluded ? ' excluded' : ''}${p.focus === r.e.seq ? ' focused' : ''}`}
              style={{
                transform: `translateY(${vr.start}px)`,
                gridTemplateColumns: '28px minmax(160px, 1.2fr) 110px 80px minmax(140px,1fr) minmax(140px,1fr) 104px minmax(160px,1.2fr)',
              }}
              onMouseDown={() => p.onFocus(r.e.seq)}
              onClick={() => {
                if (r.c && !p.busy && !r.e.excluded && r.e.status.status === 'ready') p.onExcludeChange(r.e.seq, r.c.field, !r.excluded)
              }}
            >
              <span className="dcheck">
                {r.first && (
                  <input
                    type="checkbox"
                    className="checkbox"
                    checked={!r.e.excluded}
                    disabled={p.busy || !(r.e.status.status === 'ready' || r.e.excluded)}
                    onClick={(ev) => ev.stopPropagation()}
                    onChange={() => p.onExcludeFile(r.e.seq, !r.e.excluded)}
                    aria-label={t('preview.include_file')}
                  />
                )}
              </span>
              <span className="mono ellipsis" title={r.e.path}>
                {r.first ? r.e.name : ''}
              </span>
              <span>{r.c ? fieldLabel(t, r.c.field) : ''}</span>
              <span className={k ? GLYPH_CLASS[k] : st.cls}>{k ? `${GLYPH[k]} ${t(`kind.${k}` as MessageKey)}` : ''}</span>
              <span className="mono ellipsis dbefore-plain" title={r.c ? valueText(r.c.before) : ''}>
                {r.c ? valueText(r.c.before) || '—' : ''}
              </span>
              <span className={`mono ellipsis ${r.c?.kind === 'remove' ? 'glyph-rem' : ''}`} title={r.c ? valueText(r.c.after) : ''}>
                {r.c ? (r.c.kind === 'remove' ? t('preview.removed') : valueText(r.c.after)) : ''}
              </span>
              <span className="secondary ellipsis">{r.first ? t(`writes.${r.e.target}` as MessageKey) : ''}</span>
              <span className={`ellipsis dnote ${st.cls}`} title={st.text || r.e.notes.map(bt).join('\n')}>
                {r.first ? st.text || bt(r.e.warnings[0]) || bt(r.e.notes[0]) || '' : ''}
              </span>
            </div>
          )
        })}
      </div>
    </div>
  )
}
