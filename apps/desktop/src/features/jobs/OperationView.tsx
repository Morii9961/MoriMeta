// SPDX-License-Identifier: GPL-3.0-or-later
// Apply progress (SCREEN_SPEC §8) and the completion summary (§9). Cancel asks "Stop after the
// current file?" with Keep going as the default (INTERACTION_SPEC §10, DECISIONS R-2).

import { useEffect, useMemo, useState } from 'react'
import type { FileOutcome } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, useBT, type MessageKey } from '../../i18n'
import { cancelOperation, finishOperation, planRetry, planUndo } from '../../app/actions'
import { Dialog } from '../../components/Dialog'
import './jobs.css'

const STATE: Record<string, { glyph: string; cls: string; label: MessageKey }> = {
  done: { glyph: '✓', cls: 'glyph-ok', label: 'state.done' },
  failed: { glyph: '×', cls: 'glyph-fail', label: 'state.failed' },
  skipped: { glyph: '–', cls: 'glyph-skip', label: 'state.skipped' },
  conflict: { glyph: '≠', cls: 'glyph-conflict', label: 'state.conflict' },
  cancelled: { glyph: '○', cls: 'glyph-skip', label: 'state.cancelled' },
  not_started: { glyph: '○', cls: 'glyph-skip', label: 'state.not_started' },
  attention: { glyph: '!', cls: 'glyph-warn', label: 'state.attention' },
}

function stateOf(s: string) {
  return STATE[s] ?? { glyph: '·', cls: 'faint', label: 'state.other' as MessageKey }
}

function elapsed(ms: number): string {
  const s = Math.round(ms / 1000)
  return s < 60 ? `${s}s` : `${Math.floor(s / 60)}m ${String(s % 60).padStart(2, '0')}s`
}

export function OperationView() {
  const t = useT()
  const bt = useBT()
  const stage = useApp((s) => s.stage)
  const [asking, setAsking] = useState(false)
  const [now, setNow] = useState(Date.now())
  const [filter, setFilter] = useState<'all' | 'issues'>('all')
  useEffect(() => {
    if (stage.kind !== 'applying') return
    const id = window.setInterval(() => setNow(Date.now()), 500)
    return () => clearInterval(id)
  }, [stage.kind])
  useEffect(() => {
    if (stage.kind !== 'applying') return
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !document.querySelector('.dialog-scrim')) setAsking(true)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [stage.kind])

  const files = stage.kind === 'done' ? stage.report.files : []
  const tally = useMemo(() => {
    const c: Record<string, number> = {}
    for (const f of files) c[f.state] = (c[f.state] ?? 0) + 1
    return c
  }, [files])
  if (stage.kind !== 'applying' && stage.kind !== 'done') return null

  if (stage.kind === 'applying') {
    const p = stage.progress
    const total = p?.total ?? stage.plan.summary.ready
    const done = p?.done ?? 0
    const pct = total ? (100 * done) / total : 0
    const rate = done > 0 ? done / ((now - stage.started) / 1000) : 0
    const eta = rate > 0 ? ((total - done) / rate) * 1000 : null
    return (
      <div className="op">
        <div className="op-main">
          <div className="op-tally">
            <span className="mono op-count strong">
              {done} / {total}
            </span>
            <span className="secondary">{t('op.files')}</span>
            <span className="mono faint">{Math.floor(pct)}%</span>
          </div>
          <div className="op-bar">
            <div style={{ width: `${pct}%` }} />
          </div>
          <div className="op-counts mono">
            <span className="glyph-ok">✓ {p?.ok ?? 0}</span>
            <span className="glyph-fail">× {p?.failed ?? 0}</span>
            <span className="glyph-skip">– {p?.skipped ?? 0}</span>
            <span className="faint">
              {t('op.queued')} {total - done}
            </span>
          </div>
          <div className="op-meta mono faint">
            {t('op.elapsed', { t: elapsed(now - stage.started) })}
            {eta !== null && ` · ${t('op.eta', { t: elapsed(eta) })}`}
            {rate > 0 && ` · ${t('op.rate', { n: rate.toFixed(1) })}`}
          </div>
          <div className="op-steps">
            <div className="section-label">{t('op.each_file')}</div>
            <div className="op-step-list">
              {(['op.step_backup', 'op.step_temp', 'op.step_verify', 'op.step_swap'] as MessageKey[]).map((k, i) => (
                <span key={k}>
                  <span className="mono faint">{i + 1}</span> {t(k)}
                </span>
              ))}
            </div>
          </div>
          {stage.cancelling && <div className="tone paused">{t('op.stopping')}</div>}
        </div>
        <aside className="pane-inspector op-side">
          <div className="insp-section">
            <div className="section-label">{t('op.if_cancel')}</div>
            <p className="note">✓ {t('op.cancel_finished')}</p>
            <p className="note">↺ {t('op.cancel_current')}</p>
            <p className="note">○ {t('op.cancel_queued')}</p>
            <button className="btn" disabled={stage.cancelling} onClick={() => setAsking(true)}>
              {t('op.cancel')}… <span className="mono faint">Esc</span>
            </button>
          </div>
        </aside>
        {asking && (
          <Dialog
            title={t('op.stop_q')}
            glyph="?"
            onCancel={() => setAsking(false)}
            footer={
              <>
                <span className="note" />
                <button
                  className="btn dlg"
                  onClick={() => {
                    setAsking(false)
                    cancelOperation()
                  }}
                >
                  {t('op.stop')}
                </button>
                <button className="btn dlg primary" onClick={() => setAsking(false)}>
                  {t('op.keep_going')}
                </button>
              </>
            }
          >
            <p className="note">✓ {t('op.cancel_finished')}</p>
            <p className="note">↺ {t('op.cancel_current')}</p>
            <p className="note">○ {t('op.cancel_queued')}</p>
          </Dialog>
        )}
      </div>
    )
  }

  const r = stage.report
  const issues: FileOutcome[] = r.files.filter((f) => f.state !== 'done')
  const shown = filter === 'issues' ? issues : r.files
  const total = r.files.length || 1
  const undo = stage.plan.kind === 'undo'
  return (
    <div className="op">
      <div className="op-main">
        <div className="op-tally">
          {(['done', 'failed', 'skipped', 'conflict', 'cancelled', 'attention'] as const).map((s) =>
            tally[s] ? (
              <span key={s} className={`op-tally-item ${stateOf(s).cls}`}>
                {stateOf(s).glyph} <span className="mono op-count">{tally[s]}</span> {t(stateOf(s).label)}
              </span>
            ) : null,
          )}
          <span className="mono faint">· {elapsed(stage.finished - stage.started)}</span>
        </div>
        <div className="op-bar seg">
          {(['done', 'failed', 'skipped', 'conflict', 'cancelled'] as const).map((s) =>
            tally[s] ? <div key={s} className={`seg-${s}`} style={{ width: `${(100 * tally[s]) / total}%` }} /> : null,
          )}
        </div>
        {r.note && <div className="tone warn selectable">{bt(r.note)}</div>}
        {stage.plan.kinds.unsupported + stage.plan.kinds.blocked > 0 && (
          <div className="glyph-uns">
            ⊘ {t('op.not_in_operation', { n: stage.plan.kinds.unsupported + stage.plan.kinds.blocked })}
          </div>
        )}
        {tally.done === r.files.length && r.files.length > 0 && (
          <div className="op-verified glyph-ok">✓ {undo ? t('op.undo_verified') : t('op.verified')}</div>
        )}
        {issues.length > 0 && (
          <div className="op-issues">
            <div className="section-label">{t('op.attention', { n: issues.length })}</div>
            {issues.slice(0, 50).map((f) => (
              <div key={f.seq} className="issue-row">
                <span className={stateOf(f.state).cls}>{stateOf(f.state).glyph}</span>
                <span className="mono ellipsis" title={f.path}>
                  {f.name}
                </span>
                <span className="secondary">{t(stateOf(f.state).label)}</span>
                <span className="issue-why selectable" title={f.reason ?? ''}>
                  {bt(f.reason)} <span className="faint">{t('op.file_safe')}</span>
                </span>
              </div>
            ))}
          </div>
        )}
        <div className="op-files-head">
          <div className="segmented small">
            <button aria-pressed={filter === 'all'} onClick={() => setFilter('all')}>
              {t('op.all_files', { n: r.files.length })}
            </button>
            <button aria-pressed={filter === 'issues'} disabled={!issues.length} onClick={() => setFilter('issues')}>
              {t('op.failed_skipped', { n: issues.length })}
            </button>
          </div>
        </div>
        <div className="op-files">
          {shown.map((f) => (
            <div key={f.seq} className="op-file">
              <span className="mono faint">{f.seq + 1}</span>
              <span className={stateOf(f.state).cls}>{stateOf(f.state).glyph}</span>
              <span className="mono ellipsis" title={f.path}>
                {f.name}
              </span>
              <span className="secondary">{t(stateOf(f.state).label)}</span>
              <span className="faint ellipsis" title={f.reason ?? ''}>
                {bt(f.reason)}
              </span>
            </div>
          ))}
        </div>
      </div>
      <aside className="pane-inspector op-side">
        <div className="insp-section">
          <div className="section-label">{t('op.actions')}</div>
          <button className="btn" disabled={!issues.length} onClick={() => setFilter('issues')}>
            {t('op.view_failed')}
          </button>
          {!undo && (
            <button className={`btn${issues.some((f) => f.state === 'failed' || f.state === 'skipped') ? ' accent' : ''}`} disabled={!issues.some((f) => f.state === 'failed' || f.state === 'skipped')} onClick={() => planRetry(r.op_id)}>
              {t('op.retry')}
            </button>
          )}
          <button
            className="btn"
            onClick={() => {
              finishOperation()
              useApp.getState().setModule('history')
            }}
          >
            {t('op.inspect_history')}
          </button>
          {!undo && (
            <button className="btn" disabled={!tally.done} onClick={() => planUndo(r.op_id)}>
              {t('op.undo')}…
            </button>
          )}
          <button className="btn primary" onClick={finishOperation}>
            {t('op.done')}
          </button>
        </div>
        <div className="insp-section">
          <div className="section-label">{t('op.backup_undo')}</div>
          <p className="note">{t('op.backup_fact')}</p>
          <p className="note mono faint selectable">{r.op_id}</p>
        </div>
      </aside>
    </div>
  )
}
