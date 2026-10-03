// SPDX-License-Identifier: GPL-3.0-or-later
// History / Undo (SCREEN_SPEC §10): journal grouped by day | selected Operation with its actions
// and per-file results | facts and how undo works. Undo and Retry build Plans that open in
// Preview (INTERACTION_SPEC §12, DECISIONS H-2).

import { useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import type { OpDetail, OpSummary } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, useBT, fieldLabel, type MessageKey } from '../../i18n'
import { planRetry, replanOperation } from '../../app/actions'
import { UndoDialog } from './UndoDialog'
import { valueText } from '../preview/model'
import { Dialog } from '../../components/Dialog'
import './history.css'

const STATUS: Record<string, { glyph: string; cls: string }> = {
  completed: { glyph: '✓', cls: 'glyph-ok' },
  completed_with_errors: { glyph: '!', cls: 'glyph-warn' },
  cancelled: { glyph: '○', cls: 'glyph-skip' },
  interrupted: { glyph: '!', cls: 'glyph-warn' },
  recovered: { glyph: '!', cls: 'glyph-warn' },
  running: { glyph: '●', cls: 'glyph-info' },
  paused: { glyph: '‖', cls: 'glyph-warn' },
}

export function HistoryView() {
  const t = useT()
  const bt = useBT()
  const lang = useApp((s) => s.lang)
  const notify = useApp((s) => s.notify)
  const [ops, setOps] = useState<OpSummary[] | null>(null)
  const [sel, setSel] = useState<string | null>(null)
  const [detail, setDetail] = useState<OpDetail | null>(null)
  const [tab, setTab] = useState<string>('all')
  const [dialog, setDialog] = useState<'export' | 'restore' | 'undo' | null>(null)
  const [now, setNow] = useState<Map<number, string>>(new Map())

  useEffect(() => {
    let alive = true
    api
      .historyList(0)
      .then((o) => {
        if (!alive) return
        setOps(o)
        if (o.length) setSel((s) => s ?? o[0].id)
      })
      .catch((e) => { if (alive) notify('error', errorText(e)) })
    return () => { alive = false }
  }, [notify])

  useEffect(() => {
    if (!sel) return
    let alive = true
    setDetail(null)
    setDialog(null)
    setTab('all')
    api
      .opDetail(sel)
      .then((value) => { if (alive) setDetail(value) })
      .catch((e) => { if (alive) notify('error', errorText(e)) })
    return () => { alive = false }
  }, [sel, notify])

  const days = useMemo(() => {
    const m = new Map<string, OpSummary[]>()
    for (const o of ops ?? []) {
      const d = new Date(o.created_ms).toLocaleDateString(lang === 'zh' ? 'zh-CN' : 'en-US', { year: 'numeric', month: 'short', day: 'numeric', weekday: 'short' })
      m.set(d, [...(m.get(d) ?? []), o])
    }
    return [...m.entries()]
  }, [ops, lang])

  const files = detail?.files_detail.filter((f) => tab === 'all' || f.state === tab) ?? []

  // Now vs. after operation, for the first 200 files shown (each is read to hash it)
  useEffect(() => {
    setNow(new Map())
    if (!detail || detail.backups_unavailable) return
    let alive = true
    const seqs = detail.files_detail.slice(0, 200).map((f) => f.seq)
    api
      .nowVsAfter(detail.id, seqs)
      .then((r) => alive && setNow(new Map(r)))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [detail])
  const states = detail ? Object.entries(detail.states).filter(([, n]) => n > 0) : []
  const failed = detail ? (detail.states.failed ?? 0) + (detail.states.skipped ?? 0) : 0
  const replannable = failed + (detail?.states.conflict ?? 0)

  return (
    <>
      <nav className="pane-sidebar history-journal" aria-label={t('history.journal')}>
        <div className="side-group-header">
          <span className="section-label">{t('history.journal')}</span>
        </div>
        {ops === null && <div className="side-empty muted">{t('common.loading')}</div>}
        {ops?.length === 0 && <div className="side-empty muted">{t('history.empty')}</div>}
        {days.map(([day, list]) => (
          <div key={day} className="journal-day">
            <div className="journal-date mono faint">{day}</div>
            {list.map((o) => {
              const st = STATUS[o.status] ?? { glyph: '·', cls: 'faint' }
              return (
                <button key={o.id} className={`journal-entry${sel === o.id ? ' on' : ''}`} onClick={() => setSel(o.id)}>
                  <span className="mono faint">{new Date(o.created_ms).toLocaleTimeString(lang === 'zh' ? 'zh-CN' : 'en-US', { hour: '2-digit', minute: '2-digit' })}</span>
                  <span className={st.cls}>{st.glyph}</span>
                  <span className="ellipsis journal-title">
                    {o.kind === 'undo' ? `↺ ${o.title}` : o.title}
                  </span>
                  <span className="journal-sub mono faint">
                    {t('history.files_changes', { files: o.files, changes: o.changes })}
                    {' · '}
                    {o.backups_pruned ? t('history.backup_expired') : o.backups_unavailable ? t('history.backup_away') : t('history.backup_kept')}
                  </span>
                </button>
              )
            })}
          </div>
        ))}
      </nav>
      <div className="pane-primary">
        {!detail ? (
          <div className="table-empty muted">{ops?.length ? t('common.loading') : t('history.empty_lead')}</div>
        ) : (
          <>
            <div className="history-head">
              <div className="history-title">
                <span className={(STATUS[detail.status] ?? { cls: 'faint' }).cls}>{(STATUS[detail.status] ?? { glyph: '·' }).glyph}</span>
                <span className="strong">{detail.title}</span>
                <span className="mono faint">{new Date(detail.created_ms).toLocaleString(lang === 'zh' ? 'zh-CN' : 'en-US')}</span>
              </div>
              <div className="history-actions">
                <button className="btn" disabled={!detail.undoable} onClick={() => setDialog('undo')} title={detail.undoable ? undefined : t('history.not_undoable')}>
                  {t('history.undo')}…
                </button>
                <button className={`btn${failed ? ' accent' : ''}`} disabled={!failed || detail.kind === 'undo'} onClick={() => planRetry(detail.id)}>
                  {t('op.retry')}
                </button>
                <button className="btn" disabled={!replannable || detail.status === 'running' || detail.status === 'interrupted'} onClick={() => replanOperation(detail.id)}>{t('history.replan')}</button>
                <button className="btn" disabled={detail.backups_pruned || detail.backups_unavailable} onClick={() => setDialog('restore')}>
                  {t('history.restore_to')}…
                </button>
                <button className="btn" onClick={() => setDialog('export')}>
                  {t('history.export_log')}…
                </button>
              </div>
            </div>
            <div className="table-toolbar">
              <div className="segmented small">
                <button aria-pressed={tab === 'all'} onClick={() => setTab('all')}>
                  {t('history.tab_all', { n: detail.files })}
                </button>
                {states.map(([s, n]) => (
                  <button key={s} aria-pressed={tab === s} onClick={() => setTab(s)}>
                    {t(`state.${s}` as MessageKey)} {n}
                  </button>
                ))}
              </div>
            </div>
            <div className="pane-scroll history-files">
              {files.map((f) => (
                <div key={f.seq} className="hfile">
                  <span className="mono faint">{f.seq + 1}</span>
                  <span className="mono ellipsis" title={f.path}>
                    {f.path.split(/[\\/]/).pop()}
                  </span>
                  <span className="secondary">{t(`state.${f.state}` as MessageKey)}</span>
                  <span className={`mono now-${now.get(f.seq) ?? 'unknown'}`} title={now.has(f.seq) ? t(`now.${now.get(f.seq)}_tip` as MessageKey) : ''}>
                    {now.has(f.seq) ? t(`now.${now.get(f.seq)}` as MessageKey) : '…'}
                  </span>
                  <span className="hchanges">
                    {f.changes.map((c, i) => (
                      <span key={i} className="hchange" title={`${valueText(c.before)} → ${valueText(c.after)}`}>
                        <span className={c.kind === 'add' ? 'glyph-add' : c.kind === 'remove' ? 'glyph-rem' : 'glyph-mod'}>
                          {c.kind === 'add' ? '+' : c.kind === 'remove' ? '−' : '~'}
                        </span>{' '}
                        {fieldLabel(t, c.field)}
                        <span className="mono faint"> {valueText(c.after) || valueText(c.before)}</span>
                      </span>
                    ))}
                  </span>
                  <span className="faint ellipsis selectable" title={f.error ?? ''}>
                    {bt(f.error)}
                  </span>
                </div>
              ))}
            </div>
          </>
        )}
      </div>
      <aside className="pane-inspector">
        <div className="pane-scroll insp">
          {detail && (
            <div className="insp-section">
              <div className="section-label">{t('history.facts')}</div>
              <div className="kv">
                <span>{t('history.files')}</span>
                <span className="mono">{detail.files}</span>
                <span>{t('history.changes')}</span>
                <span className="mono">{detail.changes}</span>
                <span>{t('history.warnings')}</span>
                <span className="mono">{detail.warnings}</span>
                <span>{t('history.exiftool')}</span>
                <span className="mono">{detail.exiftool_version}</span>
                <span>{t('history.backup')}</span>
                <span className="mono">{detail.backups_pruned ? t('history.backup_expired') : detail.keep ? t('history.kept_always') : t('history.backup_kept')}</span>
              </div>
              {detail.acks.length > 0 && (
                <>
                  <div className="section-label">{t('history.acks')}</div>
                  {detail.acks.map((a) => (
                    <div key={a} className="mono faint">
                      ✓ {a}
                    </div>
                  ))}
                </>
              )}
              {detail.undo_of && <p className="note">{t('history.undo_of', { id: detail.undo_of })}</p>}
              {detail.undone_by.length > 0 && <p className="note">{t('history.undone_by', { n: detail.undone_by.length })}</p>}
              <div className="mono faint selectable history-id">{detail.id}</div>
            </div>
          )}
          <div className="insp-section">
            <div className="section-label">{t('history.how_undo')}</div>
            <p className="note">{t('history.how_undo_1')}</p>
            <p className="note">{t('history.how_undo_2')}</p>
            <p className="note">{t('history.how_undo_3')}</p>
          </div>
        </div>
      </aside>
      {dialog === 'export' && detail && <ExportDialog opId={detail.id} onClose={() => setDialog(null)} />}
      {dialog === 'restore' && detail && <RestoreDialog opId={detail.id} files={detail.files} onClose={() => setDialog(null)} />}
      {dialog === 'undo' && detail && <UndoDialog key={detail.id} opId={detail.id} onClose={() => setDialog(null)} />}
    </>
  )
}

function ExportDialog({ opId, onClose }: { opId: string; onClose: () => void }) {
  const t = useT()
  const notify = useApp((s) => s.notify)
  const [paths, setPaths] = useState(false)
  const [values, setValues] = useState(false)
  return (
    <Dialog
      title={t('export.title')}
      onCancel={onClose}
      footer={
        <>
          <span className="note">{t('export.note')}</span>
          <button className="btn dlg" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            className="btn dlg primary"
            onClick={() =>
              api
                .exportLog(opId, paths, values)
                .then((p) => {
                  onClose()
                  if (p) notify('success', t('export.done', { path: p }))
                })
                .catch((e) => notify('error', errorText(e)))
            }
          >
            {t('export.choose')}
          </button>
        </>
      }
    >
      <p className="note">{t('export.lead')}</p>
      <label className="ack">
        <input type="checkbox" className="checkbox dlg" checked={paths} onChange={(e) => setPaths(e.target.checked)} />
        <span>{t('export.paths')}</span>
      </label>
      <label className="ack">
        <input type="checkbox" className="checkbox dlg" checked={values} onChange={(e) => setValues(e.target.checked)} />
        <span>{t('export.values')}</span>
      </label>
    </Dialog>
  )
}

function RestoreDialog({ opId, files, onClose }: { opId: string; files: number; onClose: () => void }) {
  const t = useT()
  const notify = useApp((s) => s.notify)
  return (
    <Dialog
      title={t('restore.title', { n: files })}
      onCancel={onClose}
      footer={
        <>
          <span className="note" />
          <button className="btn dlg" onClick={onClose}>
            {t('common.cancel')}
          </button>
          <button
            className="btn dlg primary"
            onClick={() =>
              api
                .restoreTo(opId)
                .then((r) => {
                  onClose()
                  if (r) notify('success', t('restore.done', { n: r.restored, folder: r.folder }))
                  r?.notes.forEach((n) => notify('warn', n))
                })
                .catch((e) => notify('error', errorText(e)))
            }
          >
            {t('restore.choose')}
          </button>
        </>
      }
    >
      <p className="note">✓ {t('restore.line1')}</p>
      <p className="note">✓ {t('restore.line2')}</p>
      <p className="note">✓ {t('restore.line3')}</p>
    </Dialog>
  )
}
