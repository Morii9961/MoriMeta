// SPDX-License-Identifier: GPL-3.0-or-later
// Clean Export (D-15 (c), PRODUCT_SPEC §6.8.3, METADATA_MODEL §10.1): copies of JPEG files that
// keep only what the user chooses. The Preview lists every tag and segment each copy loses,
// grouped by category with the high-risk ones marked; unrecognised ones are "unidentified →
// removed". Each copy is checked before it gets its name; the originals are only read.

import { useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import type { CleanPlan, Exported, KeepSpec, Prediction } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, useBT, type MessageKey } from '../../i18n'
import './clean.css'

const HIGH_RISK = new Set(['gps', 'serial_numbers', 'owner', 'people', 'embedded_previews', 'comments', 'location_names'])

const KEEP_KEYS: (keyof KeepSpec)[] = ['camera', 'lens', 'exposure', 'capture_time', 'author', 'descriptive']

const DEFAULT_SPEC: KeepSpec = { camera: true, lens: true, exposure: true, capture_time: true, author: true, descriptive: false }

function catLabel(t: ReturnType<typeof useT>, c: string): string {
  return t(`cat.${c}` as MessageKey)
}

export function CleanExportView() {
  const t = useT()
  const bt = useBT()
  const notify = useApp((s) => s.notify)
  const setStage = useApp((s) => s.setStage)
  const selection = useApp((s) => s.selection)
  const assets = useApp((s) => s.assets)
  const progress = useApp((s) => s.cleanProgress)
  const [spec, setSpec] = useState<KeepSpec>(DEFAULT_SPEC)
  const [plan, setPlan] = useState<CleanPlan | null>(null)
  const [building, setBuilding] = useState(false)
  const [focus, setFocus] = useState<number | null>(null)
  const [detail, setDetail] = useState<Prediction | null>(null)
  const [numberTaken, setNumberTaken] = useState(true)
  const [exporting, setExporting] = useState(false)
  const [results, setResults] = useState<Exported[] | null>(null)
  const ids = useMemo(() => assets.filter((a) => selection.has(a.id)).map((a) => a.id), [assets, selection])

  const build = async () => {
    setBuilding(true)
    setResults(null)
    try {
      const p = await api.cleanPlan(ids, spec)
      setPlan({ ...p, spec })
      setFocus(p.entries.find((e) => e.status.status === 'ready')?.seq ?? null)
    } catch (e) {
      notify('error', errorText(e))
    } finally {
      setBuilding(false)
    }
  }

  useEffect(() => {
    build()
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

  useEffect(() => {
    setDetail(null)
    if (focus === null || !plan) return
    api.cleanEntry(focus).then(setDetail).catch(() => {})
  }, [focus, plan])

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !exporting && !document.querySelector('.dialog-scrim')) setStage({ kind: 'library' })
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [exporting, setStage])

  const ready = plan?.entries.filter((e) => e.status.status === 'ready').length ?? 0
  const specChanged = plan !== null && JSON.stringify(spec) !== JSON.stringify(plan.spec ?? spec)

  const doExport = async () => {
    if (!plan) return
    setExporting(true)
    useApp.setState({ cleanProgress: null })
    try {
      const r = await api.cleanExport(plan.id, numberTaken)
      if (r) setResults(r)
    } catch (e) {
      notify('error', errorText(e))
    } finally {
      setExporting(false)
    }
  }

  const tally = (s: string) => results?.filter((r) => r.status === s).length ?? 0
  const grouped = useMemo(() => {
    const m = new Map<string, { key: string; value: string }[]>()
    for (const r of detail?.remove ?? []) m.set(r.category, [...(m.get(r.category) ?? []), { key: r.key, value: r.value }])
    return [...m.entries()].sort((a, b) => Number(HIGH_RISK.has(b[0])) - Number(HIGH_RISK.has(a[0])) || b[1].length - a[1].length)
  }, [detail])

  return (
    <div className="clean">
      <div className="clean-body">
        <nav className="pane-sidebar" aria-label={t('clean.keep')}>
          <div className="side-group">
            <div className="side-group-header">
              <span className="section-label">{t('clean.keep')}</span>
            </div>
            {KEEP_KEYS.map((k) => (
              <label key={k} className="facet-row">
                <input type="checkbox" className="checkbox" checked={spec[k]} disabled={exporting} onChange={(e) => setSpec({ ...spec, [k]: e.target.checked })} />
                <span>{t(`clean.keep_${k}` as MessageKey)}</span>
                <span />
              </label>
            ))}
            <div className="clean-always faint">{t('clean.always')}</div>
            <div className="clean-always faint">{t('clean.never')}</div>
            <div className="clean-rebuild">
              <button className={`btn small${specChanged ? ' accent' : ''}`} disabled={building || exporting || ids.length === 0} onClick={build}>
                {t('clean.rebuild')}
              </button>
            </div>
          </div>
          <div className="side-group">
            <div className="side-group-header">
              <span className="section-label">{t('clean.guarantee')}</span>
            </div>
            <p className="note clean-note">{t('clean.guarantee_text')}</p>
          </div>
        </nav>
        <div className="pane-primary">
          <div className="summary-strip">
            <span className="mono strong">{t('clean.files', { n: plan?.entries.length ?? ids.length })}</span>
            <span className="mono">{t('clean.ready', { n: ready })}</span>
            {plan && plan.entries.length - ready > 0 && <span className="mono glyph-uns">⊘{plan.entries.length - ready}</span>}
            {building && <span className="muted">{t('clean.reading')}</span>}
          </div>
          <div className="pane-scroll clean-list">
            {results
              ? results.map((r) => {
                  const e = plan?.entries.find((x) => x.seq === r.seq)
                  const glyph = r.status === 'exported' ? ['✓', 'glyph-ok'] : r.status === 'skipped' || r.status === 'blocked' ? ['–', 'glyph-skip'] : ['×', 'glyph-fail']
                  return (
                    <div key={r.seq} className="clean-row">
                      <span className={glyph[1]}>{glyph[0]}</span>
                      <span className="mono ellipsis" title={r.source}>
                        {e?.name}
                      </span>
                      <span className="secondary">{t(`clean.st_${r.status}` as MessageKey)}</span>
                      <span className="ellipsis faint selectable" title={r.output ?? r.reasons.join('\n')}>
                        {r.output ?? r.reasons.map(bt).join('; ')}
                      </span>
                    </div>
                  )
                })
              : plan?.entries.map((e) => (
                  <div key={e.seq} className={`clean-row${focus === e.seq ? ' focused' : ''}`} onMouseDown={() => setFocus(e.seq)}>
                    <span className={e.status.status === 'ready' ? 'glyph-ok' : 'glyph-uns'}>{e.status.status === 'ready' ? '✓' : '⊘'}</span>
                    <span className="mono ellipsis" title={e.source}>
                      {e.name}
                    </span>
                    <span className="mono secondary">{e.status.status === 'ready' ? t('clean.removes', { n: e.removed + e.segments.length }) : ''}</span>
                    <span className="clean-cats">
                      {e.status.status === 'ready' ? (
                        <>
                          {e.categories
                            .filter(([c]) => HIGH_RISK.has(c))
                            .map(([c, n]) => (
                              <span key={c} className="tag risk">
                                {catLabel(t, c)} {n}
                              </span>
                            ))}
                          {e.segments.some((s) => s.unidentified) && <span className="tag risk">{t('clean.unidentified')}</span>}
                          {e.lens_lost && <span className="tag">{t('clean.lens_lost')}</span>}
                        </>
                      ) : (
                        <span className="faint">{bt(e.status.reason)}</span>
                      )}
                    </span>
                  </div>
                ))}
          </div>
        </div>
        <aside className="pane-inspector">
          <div className="pane-scroll insp">
            {results ? (
              <div className="insp-section">
                <div className="section-label">{t('clean.result')}</div>
                <p className="glyph-ok">✓ {t('clean.exported_n', { n: tally('exported') })}</p>
                {tally('refused') > 0 && <p className="glyph-fail">× {t('clean.refused_n', { n: tally('refused') })}</p>}
                {tally('skipped') + tally('blocked') > 0 && <p className="glyph-skip">– {t('clean.skipped_n', { n: tally('skipped') + tally('blocked') })}</p>}
                <p className="note">{t('clean.originals')}</p>
              </div>
            ) : !detail ? (
              <p className="note insp-section">{t('clean.pick_file')}</p>
            ) : (
              <>
                {detail.lens_lost && <div className="tone warn insp-error">{t('clean.lens_lost_note')}</div>}
                {grouped.map(([c, list]) => (
                  <div key={c} className="insp-section">
                    <div className={`section-label${HIGH_RISK.has(c) ? ' glyph-warn' : ''}`}>
                      {HIGH_RISK.has(c) ? '! ' : ''}
                      {catLabel(t, c)} · {list.length}
                    </div>
                    {list.slice(0, 40).map((r) => (
                      <div key={r.key} className="tag-op">
                        <span className="glyph-rem mono">−</span>
                        <span className="mono ellipsis" title={r.key}>
                          {r.key}
                        </span>
                        <span className="mono ellipsis faint selectable" title={r.value}>
                          {r.value}
                        </span>
                      </div>
                    ))}
                    {list.length > 40 && <div className="faint mono">+{list.length - 40}</div>}
                  </div>
                ))}
                {detail.remove_segments.length > 0 && (
                  <div className="insp-section">
                    <div className="section-label">{t('clean.segments')}</div>
                    {detail.remove_segments.map((s, i) => (
                      <div key={i} className="tag-op">
                        <span className="glyph-rem mono">−</span>
                        <span className="mono">{s.label === 'after the end of the image' ? t('clean.trailer') : s.label}</span>
                        <span className="mono faint">
                          {s.bytes} B{s.unidentified ? ` · ${t('clean.unidentified')}` : ''}
                        </span>
                      </div>
                    ))}
                  </div>
                )}
                <div className="insp-section">
                  <div className="section-label">{t('clean.kept_n', { n: detail.keep.length })}</div>
                  <p className="note mono">{detail.keep.slice(0, 30).join(', ')}</p>
                </div>
              </>
            )}
          </div>
        </aside>
      </div>
      <div className="action-bar">
        <span className="secondary">{t('clean.if_taken')}</span>
        <div className="segmented small">
          <button aria-pressed={numberTaken} onClick={() => setNumberTaken(true)}>
            {t('clean.number')}
          </button>
          <button aria-pressed={!numberTaken} onClick={() => setNumberTaken(false)}>
            {t('clean.skip')}
          </button>
        </div>
        {exporting && progress && (
          <span className="mono">
            {progress.done} / {progress.total}
          </span>
        )}
        <div className="toolbar-spacer" />
        {specChanged && <span className="action-note">{t('clean.rebuild_first')}</span>}
        <button className="btn" disabled={exporting} onClick={() => setStage({ kind: 'library' })}>
          {results ? t('op.done') : t('preview.back_to_edit')} <span className="mono faint">Esc</span>
        </button>
        {!results && (
          <button className={`btn${ready && !specChanged && !building ? ' primary' : ''}`} disabled={!ready || specChanged || building || exporting} onClick={doExport}>
            {t('clean.export', { n: ready })}
          </button>
        )}
      </div>
    </div>
  )
}
