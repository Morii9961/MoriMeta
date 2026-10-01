// SPDX-License-Identifier: GPL-3.0-or-later
// Preview / Diff (SCREEN_SPEC §7): sidebar (Show changes · Edits in plan · Write targets ·
// Excluded) | summary strip + DiffTable (By file / By change) | Checks / File detail; action bar
// with the review checklist and Apply. Every exclusion makes a new Plan version.

import { useCallback, useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import type { PlanEntry, PlanView, Preflight } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, fieldLabel, type MessageKey } from '../../i18n'
import { apply, backToEdit, discardPlan } from '../../app/actions'
import { counts, GLYPH, GLYPH_CLASS, KINDS, kindsOf, REVIEW, type Kind } from './model'
import { DiffTable } from './DiffTable'
import { ConfirmApply } from './ConfirmApply'
import './preview.css'

const KIND_LABEL: Record<Kind, MessageKey> = {
  add: 'kind.add',
  modify: 'kind.modify',
  remove: 'kind.remove',
  warning: 'kind.warning',
  unsupported: 'kind.unsupported',
  no_change: 'kind.no_change',
  excluded: 'kind.excluded',
}

export function PreviewView() {
  const t = useT()
  const stage = useApp((s) => s.stage)
  const setStage = useApp((s) => s.setStage)
  const notify = useApp((s) => s.notify)
  const plan = stage.kind === 'preview' ? stage.plan : null
  const [entries, setEntries] = useState<PlanEntry[] | null>(null)
  const [filter, setFilter] = useState<Kind | 'all'>('all')
  const [view, setView] = useState<'file' | 'change'>('file')
  const [reviewed, setReviewed] = useState<Set<Kind>>(new Set())
  const [focus, setFocus] = useState<number | null>(null)
  const [pane, setPane] = useState<'checks' | 'detail'>('checks')
  const [preflight, setPreflight] = useState<Preflight | null>(null)
  const [busy, setBusy] = useState(false)
  const [confirming, setConfirming] = useState(false)

  // every page of this version (5,000 entries are 25 pages)
  useEffect(() => {
    if (!plan) return
    let alive = true
    ;(async () => {
      const all: PlanEntry[] = []
      for (let page = 0; ; page++) {
        const p = await api.planPage(plan.id, plan.version, 'all', page)
        all.push(...p.entries)
        if (all.length >= p.matching || p.entries.length === 0) break
      }
      if (alive) setEntries(all)
    })().catch((e) => notify('error', errorText(e)))
    api
      .planPreflight(plan.id, plan.version)
      .then((p) => alive && setPreflight(p))
      .catch((e) => notify('error', errorText(e)))
    return () => {
      alive = false
    }
  }, [plan?.id, plan?.version, notify, plan])

  const replace = useCallback(
    (next: PlanView) => {
      if (stage.kind === 'preview') setStage({ ...stage, plan: next })
    },
    [stage, setStage],
  )

  const run = useCallback(
    async (f: () => Promise<PlanView>) => {
      if (busy) return
      setBusy(true)
      try {
        replace(await f())
      } catch (e) {
        notify('error', errorText(e))
      } finally {
        setBusy(false)
      }
    },
    [busy, replace, notify],
  )

  const c = useMemo(() => (entries ? counts(entries) : null), [entries])
  const shown = useMemo(() => {
    if (!entries) return []
    if (filter === 'all') return entries
    return entries.filter((e) => kindsOf(e).has(filter))
  }, [entries, filter])

  if (!plan) return null
  const needReview = REVIEW.filter((k) => (c?.[k] ?? 0) > 0)
  const unreviewed = needReview.filter((k) => !reviewed.has(k))
  // an undo Plan restores files: its entries carry no field changes
  const writable = plan.summary.ready > 0 && (plan.kind === 'undo' || plan.summary.changes > 0)
  const pfOk = preflight?.ok ?? false
  let blocked: string | null = null
  if (!entries || !preflight) blocked = t('preview.checking')
  else if (!writable) blocked = t('preview.nothing_to_write')
  else if (!pfOk) blocked = preflight.backup ? t('preview.blocked_backup') : preflight.rescan.length ? t('preview.blocked_rescan') : t('preview.blocked_checks')
  else if (unreviewed.length) blocked = t('preview.review_first', { what: unreviewed.map((k) => t(KIND_LABEL[k])).join(', ') })
  const canApply = blocked === null && !busy
  const highRisk = plan.required_acks.length > 0

  const onFilter = (k: Kind | 'all') => {
    setFilter(k)
    if (k !== 'all') setReviewed((r) => new Set([...r, k]))
  }

  const doApply = () => {
    if (!canApply) return
    if (highRisk) setConfirming(true)
    else apply(plan, [])
  }

  const onKey = (e: React.KeyboardEvent) => {
    if (confirming || (e.target as HTMLElement).tagName === 'INPUT') return
    if (e.key === 'Escape') {
      e.preventDefault()
      backToEdit()
    } else if (e.ctrlKey && e.key === 'Enter') {
      e.preventDefault()
      doApply()
    } else if (e.key === 'v' || e.key === 'V') {
      setView((v) => (v === 'file' ? 'change' : 'file'))
    } else if (e.key === '0') onFilter('all')
    else if (/^[1-7]$/.test(e.key)) onFilter(KINDS[Number(e.key) - 1])
  }

  const focusedEntry = entries?.find((e) => e.seq === focus) ?? null
  const fieldCount = new Map(plan.field_counts)

  return (
    <div className="preview" tabIndex={-1} onKeyDown={onKey} autoFocus>
      <div className="preview-body">
        <nav className="pane-sidebar" aria-label={t('preview.sidebar')}>
          <div className="side-group">
            <div className="side-group-header">
              <span className="section-label">{t('preview.show_changes')}</span>
              {filter !== 'all' ? (
                <button className="link side-clear" onClick={() => onFilter('all')}>
                  {t('preview.all')}
                </button>
              ) : (
                <span className="faint side-any">{t('preview.all')}</span>
              )}
            </div>
            {KINDS.map((k, i) => {
              const n = c?.[k] ?? 0
              const review = REVIEW.includes(k) && n > 0
              return (
                <button
                  key={k}
                  className={`kind-row${filter === k ? ' on' : ''}`}
                  onClick={() => onFilter(filter === k ? 'all' : k)}
                  disabled={n === 0}
                  title={`${i + 1}`}
                >
                  <span className={`kind-glyph mono ${GLYPH_CLASS[k]}`}>{GLYPH[k]}</span>
                  <span>{t(KIND_LABEL[k])}</span>
                  <span className="mono faint">{n}</span>
                  <span className={`review-mark${review ? (reviewed.has(k) ? ' done' : '') : ' none'}`}>
                    {review ? (reviewed.has(k) ? '✓' : '○') : ''}
                  </span>
                </button>
              )
            })}
          </div>
          <div className="side-group">
            <div className="side-group-header">
              <span className="section-label">{t('preview.edits_in_plan')}</span>
            </div>
            {plan.fields.map((f) => {
              const off = plan.excluded_fields.includes(f)
              return (
                <label key={f} className="facet-row">
                  <input
                    type="checkbox"
                    className="checkbox"
                    checked={!off}
                    disabled={busy || plan.kind === 'undo'}
                    onChange={() => run(() => api.planExcludeField(plan.id, plan.version, null, f, !off))}
                  />
                  <span className={off ? 'faint' : ''} style={off ? { textDecoration: 'line-through' } : undefined}>
                    {fieldLabel(t, f)}
                  </span>
                  <span className="mono faint count">{fieldCount.get(f) ?? 0}</span>
                </label>
              )
            })}
            {plan.fields.length === 0 && <div className="side-empty muted">—</div>}
          </div>
          <div className="side-group">
            <div className="side-group-header">
              <span className="section-label">{t('preview.write_targets')}</span>
            </div>
            <div className="kv side-kv">
              <span>{t('writes.in_file')}</span>
              <span className="mono">{plan.summary.targets.in_file}</span>
              <span>{t('writes.sidecar')}</span>
              <span className="mono">{plan.summary.targets.sidecar}</span>
              <span>{t('writes.new_sidecar')}</span>
              <span className="mono">{plan.summary.targets.new_sidecar}</span>
            </div>
          </div>
          <div className="side-group">
            <div className="side-group-header">
              <span className="section-label">{t('preview.excluded')}</span>
            </div>
            <div className="side-kv kv">
              <span>{t('preview.excluded_files')}</span>
              <span className="mono">{plan.summary.excluded}</span>
              <span>{t('preview.excluded_edits')}</span>
              <span className="mono">{plan.excluded_fields.length}</span>
            </div>
            {(plan.summary.excluded > 0 || (c?.excluded ?? 0) > 0) && entries && (
              <button
                className="link side-restore"
                disabled={busy}
                onClick={() =>
                  run(async () => {
                    let p = plan
                    const files = entries.filter((e) => e.excluded).map((e) => e.seq)
                    if (files.length) p = await api.planExclude(p.id, p.version, files, false)
                    for (const f of p.fields) {
                      p = await api.planExcludeField(p.id, p.version, null, f, false)
                    }
                    return p
                  })
                }
              >
                {t('preview.restore_all')}
              </button>
            )}
          </div>
        </nav>
        <div className="pane-primary">
          <div className="summary-strip">
            <span className="mono strong">{t('preview.files', { n: plan.summary.files })}</span>
            <span className="mono">
              {plan.kind === 'undo' ? t('preview.to_restore', { n: plan.summary.ready }) : t('preview.changes', { n: plan.summary.changes })}
            </span>
            {(['add', 'modify', 'remove', 'warning', 'unsupported'] as Kind[]).map((k) =>
              c && c[k] > 0 ? (
                <span key={k} className={`mono ${GLYPH_CLASS[k]}`}>
                  {GLYPH[k]}
                  {c[k]}
                </span>
              ) : null,
            )}
            <div className="toolbar-spacer" />
            <div className="segmented small" role="tablist">
              <button aria-pressed={view === 'file'} onClick={() => setView('file')}>
                {t('preview.by_file')}
              </button>
              <button aria-pressed={view === 'change'} onClick={() => setView('change')}>
                {t('preview.by_change')}
              </button>
            </div>
          </div>
          {!entries ? (
            <div className="table-empty muted">{t('preview.loading')}</div>
          ) : (
            <DiffTable
              plan={plan}
              entries={shown}
              view={view}
              filter={filter}
              focus={focus}
              busy={busy || plan.kind === 'undo'}
              onFocus={(seq) => {
                setFocus(seq)
                setPane('detail')
              }}
              onExcludeFile={(seq, excluded) => run(() => api.planExclude(plan.id, plan.version, [seq], excluded))}
              onExcludeChange={(seq, field, excluded) => run(() => api.planExcludeField(plan.id, plan.version, [seq], field, excluded))}
            />
          )}
        </div>
        <aside className="pane-inspector">
          <div className="segmented small pane-tabs">
            <button aria-pressed={pane === 'checks'} onClick={() => setPane('checks')}>
              {t('preview.checks')}
            </button>
            <button aria-pressed={pane === 'detail'} onClick={() => setPane('detail')}>
              {t('preview.file_detail')}
            </button>
          </div>
          <div className="pane-scroll insp">
            {pane === 'checks' ? (
              <Checks preflight={preflight} plan={plan} />
            ) : (
              <FileDetail entry={focusedEntry} />
            )}
          </div>
        </aside>
      </div>
      <div className="action-bar">
        <span className="secondary">{t('preview.review_before')}</span>
        {needReview.length === 0 && <span className="faint">{t('preview.nothing_to_review')}</span>}
        {needReview.map((k) => (
          <button key={k} className={`review-item${reviewed.has(k) ? ' done' : ''}`} onClick={() => onFilter(k)}>
            <span>{reviewed.has(k) ? '✓' : '○'}</span> {t(KIND_LABEL[k])} <span className="mono">{c?.[k]}</span>
          </button>
        ))}
        <div className="toolbar-spacer" />
        {blocked && <span className="action-note">{blocked}</span>}
        <button className="btn" onClick={discardPlan}>
          {t('preview.discard')}
        </button>
        <button className="btn" onClick={backToEdit}>
          {t('preview.back_to_edit')} <span className="mono faint">Esc</span>
        </button>
        <button className={`btn ${canApply ? 'primary' : ''}`} disabled={!canApply} onClick={doApply}>
          {plan.kind === 'undo'
            ? t('preview.apply_undo', { n: plan.summary.ready })
            : t('preview.apply', { n: plan.summary.changes })}
          …
        </button>
      </div>
      {confirming && entries && (
        <ConfirmApply
          plan={plan}
          entries={entries}
          onCancel={() => setConfirming(false)}
          onConfirm={(acks) => {
            setConfirming(false)
            apply(plan, acks)
          }}
        />
      )}
    </div>
  )
}

function Checks({ preflight, plan }: { preflight: Preflight | null; plan: PlanView }) {
  const t = useT()
  const info = useApp((s) => s.info)
  if (!preflight) return <p className="note insp-section">{t('preview.checking')}</p>
  const row = (ok: boolean, label: string, detail: string | null) => (
    <div className="check-row">
      <span className={ok ? 'glyph-ok' : 'glyph-fail'}>{ok ? '✓' : '×'}</span>
      <span>{label}</span>
      {detail && <span className="check-detail selectable">{detail}</span>}
    </div>
  )
  return (
    <>
      <div className="insp-section">
        <div className="section-label">{t('preview.preflight')}</div>
        {row(!preflight.backup, t('preview.pf_backup'), preflight.backup ?? info?.backup.root ?? null)}
        {row(!preflight.space, t('preview.pf_space'), preflight.space)}
        {row(!preflight.exiftool, t('preview.pf_exiftool'), preflight.exiftool ?? (info?.exiftool.version ? `ExifTool ${info.exiftool.version}` : null))}
        {row(preflight.rescan.length === 0, t('preview.pf_rescan'), preflight.rescan.length ? t('preview.pf_rescan_n', { n: preflight.rescan.length }) : null)}
        {preflight.backup_warning && <div className="tone warn">{preflight.backup_warning}</div>}
      </div>
      <div className="insp-section">
        <div className="section-label">{t('preview.raw_safety')}</div>
        <p className="note">
          <span className="glyph-ok">●</span> {t('preview.raw_safety_note')}
        </p>
        {plan.summary.targets.new_sidecar > 0 && <p className="note">{t('preview.new_sidecars', { n: plan.summary.targets.new_sidecar })}</p>}
      </div>
      <div className="insp-section">
        <div className="section-label">{t('preview.backup_undo')}</div>
        <p className="note">{t('preview.backup_note')}</p>
        <p className="note">{t('preview.undo_note')}</p>
      </div>
    </>
  )
}

function FileDetail({ entry }: { entry: PlanEntry | null }) {
  const t = useT()
  if (!entry) return <p className="note insp-section">{t('preview.pick_file')}</p>
  const ops = entry.action && 'ops' in entry.action ? entry.action.ops : []
  return (
    <>
      <div className="insp-section">
        <div className="mono strong ellipsis" title={entry.path}>
          {entry.name}
        </div>
        <div className="mono faint ellipsis selectable" title={entry.path}>
          {entry.path}
        </div>
        <div className="secondary">{t(`writes.${entry.target}` as MessageKey)}</div>
        {entry.status.status !== 'ready' && entry.status.status !== 'no_change' && (
          <div className="tone neutral">
            <b>{entry.status.status === 'blocked' ? t('preview.blocked') : t('kind.unsupported')}</b>
            <div className="selectable">{entry.status.reason}</div>
          </div>
        )}
      </div>
      {ops.length > 0 && (
        <div className="insp-section">
          <div className="section-label">{t('preview.tags_written')}</div>
          {ops.map((o, i) => (
            <div key={i} className="tag-op">
              <span className={`mono ${o.op === 'delete' ? 'glyph-rem' : 'glyph-mod'}`}>{o.op === 'delete' ? '−' : '~'}</span>
              <span className="mono ellipsis" title={'tag' in o ? o.tag : 'Photoshop:IPTCDigest'}>
                {'tag' in o ? o.tag : 'Photoshop:IPTCDigest'}
              </span>
              <span className="mono ellipsis selectable" title={o.op === 'set' ? o.values.join('; ') : ''}>
                {o.op === 'set' ? o.values.join('; ') : o.op === 'delete' ? t('preview.deleted') : t('preview.updated')}
              </span>
            </div>
          ))}
        </div>
      )}
      {(entry.notes.length > 0 || entry.warnings.length > 0) && (
        <div className="insp-section">
          <div className="section-label">{t('preview.notes')}</div>
          {entry.notes.map((n, i) => {
            const warn = n.startsWith('warning: ')
            return (
              <p key={i} className={`note selectable${warn ? ' glyph-warn' : ''}`}>
                {warn ? `! ${n.slice(9)}` : n}
              </p>
            )
          })}
        </div>
      )}
    </>
  )
}
