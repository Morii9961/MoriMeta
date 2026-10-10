// SPDX-License-Identifier: GPL-3.0-or-later
// Preset manager (SCREEN_SPEC §6): source facets | Preset rows (last used first) | details with the
// rules as sentences, safety, actions, and "Apply to N selected files…", which builds a Plan and
// opens Preview. Built-in Presets are read-only (duplicate to change one).

import { useCallback, useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import { useApp } from '../../state/store'
import { useT, useBT, fieldLabel, type MessageKey } from '../../i18n'
import { useVisibleItems } from '../library/hooks'
import { Dialog } from '../../components/Dialog'
import { PopupMenu, type MenuItem } from '../../components/PopupMenu'
import { showPreview } from '../../app/actions'
import { actionText, conditionText, presetKind, type PresetInfo } from './model'
import './presets.css'

const KIND_GLYPH = { descriptive: '✎', removal: '−', time: '~' } as const

export function usePresets() {
  const notify = useApp((s) => s.notify)
  const [list, setList] = useState<PresetInfo[] | null>(null)
  const reload = useCallback(
    () =>
      api
        .presetsList()
        .then(setList)
        .catch((e) => notify('error', errorText(e))),
    [notify],
  )
  useEffect(() => {
    reload()
  }, [reload])
  return { list, reload }
}

export function PresetsView() {
  const t = useT()
  const bt = useBT()
  const lang = useApp((s) => s.lang)
  const notify = useApp((s) => s.notify)
  const selection = useApp((s) => s.selection)
  const setModule = useApp((s) => s.setModule)
  const setStage = useApp((s) => s.setStage)
  const { list, reload } = usePresets()
  const [source, setSource] = useState<'all' | 'builtin' | 'yours' | 'imported'>('all')
  const [sel, setSel] = useState<string | null>(null)
  const [renaming, setRenaming] = useState<string | null>(null)
  const [applying, setApplying] = useState<PresetInfo | null>(null)
  const assets = useApp((s) => s.assets)
  const [deleting, setDeleting] = useState<PresetInfo | null>(null)
  const [busy, setBusy] = useState(false)

  const shown = useMemo(() => {
    const l = (list ?? []).filter((p) =>
      source === 'all' ? true : source === 'builtin' ? p.builtin : source === 'imported' ? p.untrusted : !p.builtin,
    )
    return [...l].sort((a, b) => (b.last_used_ms ?? 0) - (a.last_used_ms ?? 0) || a.name.localeCompare(b.name))
  }, [list, source])
  const cur = list?.find((p) => p.id === sel) ?? shown[0] ?? null

  const run = async (f: () => Promise<unknown>) => {
    setBusy(true)
    try {
      await f()
      await reload()
    } catch (e) {
      notify('error', errorText(e))
    } finally {
      setBusy(false)
    }
  }

  const apply = async (p: PresetInfo, ids: number[]) => {
    setApplying(null)
    setStage({ kind: 'planning', done: 0, total: ids.length, stage: 'files' })
    try {
      const plan = await api.planPreset(ids, p.id)
      showPreview(plan, 'edit')
      reload()
    } catch (e) {
      setStage({ kind: 'library' })
      notify('error', errorText(e))
    }
  }

  const count = (f: (p: PresetInfo) => boolean) => (list ?? []).filter(f).length

  // a duplicate opens with its name ready to change (SCREEN_SPEC 3#p-menu)
  const duplicate = (p: PresetInfo) =>
    run(async () => {
      const copy = await api.presetDuplicate(p.id)
      setSel(copy)
      setRenaming(copy)
    })
  const editRules = (p: PresetInfo) => {
    useApp.setState({ editPreset: p.id })
    setModule('rules')
  }
  const [menu, setMenu] = useState<{ id: string; x: number; y: number } | null>(null)
  // the rename field opens with the whole name selected, ready to type over
  const selectOnMount = useCallback((el: HTMLInputElement | null) => {
    el?.focus()
    el?.select()
  }, [])
  const closeMenu = useCallback(() => setMenu(null), [])
  const menuItems = (p: PresetInfo): MenuItem[] => [
    { text: p.builtin ? t('presets.view_rules') : t('presets.edit_rules'), run: () => editRules(p) },
    { text: t('presets.duplicate'), run: () => duplicate(p), disabled: busy },
    { text: t('presets.rename'), keys: 'F2', run: () => setRenaming(p.id), disabled: busy || p.builtin },
    { text: `${t('presets.export')}…`, run: () => run(() => api.presetExport(p.id)), disabled: busy },
    { text: `${t('presets.delete')}…`, run: () => setDeleting(p), disabled: busy || p.builtin },
    { text: t('presets.apply'), run: () => setApplying(p), disabled: !assets.length },
  ]

  return (
    <>
      <nav className="pane-sidebar" aria-label={t('presets.sources')}>
        <div className="side-group">
          <div className="side-group-header">
            <span className="section-label">{t('presets.source')}</span>
          </div>
          {(
            [
              ['all', t('presets.all'), count(() => true)],
              ['builtin', t('presets.builtin'), count((p) => p.builtin)],
              ['yours', t('presets.yours'), count((p) => !p.builtin)],
              ['imported', t('presets.imported_new'), count((p) => p.untrusted)],
            ] as const
          ).map(([k, label, n]) => (
            <button key={k} className={`kind-row${source === k ? ' on' : ''}`} onClick={() => setSource(k)}>
              <span />
              <span>{label}</span>
              <span className="mono faint">{n}</span>
              <span />
            </button>
          ))}
        </div>
        <div className="side-group preset-side-actions">
          <button className="btn small" disabled={busy} onClick={() => run(() => api.presetImport())}>
            {t('presets.import')}…
          </button>
          <button className="btn small" onClick={() => setModule('rules')}>
            {t('presets.new')}
          </button>
        </div>
      </nav>
      <div className="pane-primary">
        <div className="table-toolbar">
          <span className="mono strong">{t('presets.count', { n: shown.length })}</span>
          <span className="faint">{t('presets.sorted')}</span>
        </div>
        <div className="pane-scroll preset-list" role="listbox" aria-label={t('module.presets')}>
          {list === null && <div className="table-empty muted">{t('common.loading')}</div>}
          {shown.map((p) => {
            const kind = presetKind(p.preset)
            return (
              <div
                key={p.id}
                role="option"
                aria-selected={cur?.id === p.id}
                tabIndex={0}
                className={`preset-row${cur?.id === p.id ? ' on' : ''}`}
                onClick={() => setSel(p.id)}
                onContextMenu={(e) => {
                  e.preventDefault()
                  setSel(p.id)
                  setMenu({ id: p.id, x: e.clientX, y: e.clientY })
                }}
                onKeyDown={(e) => {
                  if (e.key === 'F2' && !p.builtin) setRenaming(p.id)
                  if (e.key === 'ContextMenu' || (e.shiftKey && e.key === 'F10')) {
                    e.preventDefault()
                    const b = e.currentTarget.getBoundingClientRect()
                    setMenu({ id: p.id, x: b.left + 24, y: b.bottom })
                  }
                }}
              >
                <div className="preset-main">
                  {renaming === p.id ? (
                    <input
                      className="input ui"
                      ref={selectOnMount}
                      defaultValue={p.name}
                      onKeyDown={(e) => {
                        if (e.key === 'Escape') setRenaming(null)
                        if (e.key === 'Enter') {
                          const name = (e.target as HTMLInputElement).value.trim()
                          setRenaming(null)
                          if (name && name !== p.name) run(() => api.presetSave(p.id, { ...p.preset, name }))
                        }
                      }}
                      onBlur={() => setRenaming(null)}
                    />
                  ) : (
                    <span className="preset-name">{p.name}</span>
                  )}
                  <span className="faint preset-source">
                    {p.builtin ? t('presets.builtin') : p.untrusted ? t('presets.imported_new') : t('presets.yours')}
                  </span>
                </div>
                <span className="secondary ellipsis">{p.fields.map((f) => fieldLabel(t, f)).join(', ')}</span>
                <span className={`tag kind-${kind}`}>
                  {KIND_GLYPH[kind]} {t(`presets.kind_${kind}` as Parameters<typeof t>[0])}
                </span>
                <span className="mono faint">{t('presets.rules_n', { n: p.preset.rules.length })}</span>
                <span className="mono faint">
                  {p.last_used_ms ? new Date(p.last_used_ms).toLocaleDateString(lang === 'zh' ? 'zh-CN' : 'en-US') : '—'}
                </span>
                <button
                  className="link preset-more"
                  aria-label={t('presets.more')}
                  title={t('presets.more')}
                  onClick={(e) => {
                    e.stopPropagation()
                    setSel(p.id)
                    const b = e.currentTarget.getBoundingClientRect()
                    setMenu({ id: p.id, x: b.left, y: b.bottom })
                  }}
                  onMouseDown={(e) => e.stopPropagation()}
                >
                  ⋯
                </button>
              </div>
            )
          })}
        </div>
      </div>
      <aside className="pane-inspector">
        {cur && (
          <>
            <div className="insp-header">
              <div className="insp-name strong">{cur.name}</div>
              <div className="insp-meta">{cur.builtin ? t('presets.builtin_note') : cur.untrusted ? t('presets.imported_note') : t('presets.yours')}</div>
            </div>
            <div className="pane-scroll insp">
              <div className="insp-section">
                <div className="section-label">{t('presets.rules_in_order')}</div>
                {cur.preset.rules.map((r, i) => (
                  <div key={i} className={`rule-sentence${r.enabled ? '' : ' disabled'}`}>
                    <span className="mono faint">{String(i + 1).padStart(2, '0')}</span>
                    <span>
                      {r.when.length ? (
                        <>
                          <b className="kw-if">{t('rules.if')}</b> {r.when.map((c) => conditionText(t, c)).join(` ${t('rules.and')} `)}{' '}
                        </>
                      ) : null}
                      <b className="kw-then">{t('rules.then')}</b> {r.then.map((a) => actionText(t, a)).join(t('rules.list_sep'))}
                      {!r.enabled && <span className="faint"> · {t('rules.disabled')}</span>}
                    </span>
                  </div>
                ))}
                {cur.lint.map((w, i) => (
                  <div key={i} className="tone warn">
                    {bt(w)}
                  </div>
                ))}
              </div>
              <div className="insp-section">
                <div className="section-label">{t('presets.safety')}</div>
                <p className="note">{t('presets.safety_note')}</p>
              </div>
              <div className="insp-section preset-actions">
                <button className="btn" onClick={() => editRules(cur)}>
                  {cur.builtin ? t('presets.view_rules') : t('presets.edit_rules')}
                </button>
                <button className="btn" disabled={busy} onClick={() => duplicate(cur)}>
                  {t('presets.duplicate')}
                </button>
                <button className="btn" disabled={busy || cur.builtin} onClick={() => setRenaming(cur.id)}>
                  {t('presets.rename')} <span className="mono faint">F2</span>
                </button>
                <button className="btn" disabled={busy} onClick={() => run(() => api.presetExport(cur.id))}>
                  {t('presets.export')}…
                </button>
                <button className="btn" disabled={busy || cur.builtin} onClick={() => setDeleting(cur)}>
                  {t('presets.delete')}…
                </button>
              </div>
            </div>
            <div className="batch-footer">
              <span className="faint">{t('presets.apply_note')}</span>
              <div className="toolbar-spacer" />
              <button className={`btn small${assets.length ? ' accent' : ''}`} disabled={!assets.length} title={assets.length ? undefined : t('preview.need_selection')} onClick={() => setApplying(cur)}>
                {selection.size ? t('presets.apply_to', { n: selection.size }) : t('presets.apply')}
              </button>
            </div>
          </>
        )}
      </aside>
      {menu && (() => {
        const p = list?.find((x) => x.id === menu.id)
        return p ? <PopupMenu x={menu.x} y={menu.y} items={menuItems(p)} onClose={closeMenu} /> : null
      })()}
      {applying && <ApplyDialog preset={applying} onCancel={() => setApplying(null)} onBuild={(ids) => apply(applying, ids)} />}
      {deleting && (
        <Dialog
          title={t('presets.delete_q', { name: deleting.name })}
          onCancel={() => setDeleting(null)}
          footer={
            <>
              <button className="link" onClick={() => api.presetExport(deleting.id).catch((e) => notify('error', errorText(e)))}>
                {t('presets.export_first')}
              </button>
              <span className="note" />
              <button className="btn dlg" onClick={() => setDeleting(null)}>
                {t('common.cancel')}
              </button>
              <button
                className="btn dlg primary"
                onClick={() => {
                  const id = deleting.id
                  setDeleting(null)
                  setSel(null)
                  run(() => api.presetDelete(id))
                }}
              >
                {t('presets.delete')}
              </button>
            </>
          }
        >
          <p className="note">{t('presets.delete_note')}</p>
        </Dialog>
      )}
    </>
  )
}

type Scope = 'selection' | 'filter' | 'session'

/** Apply dialog (SCREEN_SPEC 3#p-apply): which files, the facts, Build preview. */
function ApplyDialog({ preset, onCancel, onBuild }: { preset: PresetInfo; onCancel: () => void; onBuild: (ids: number[]) => void }) {
  const t = useT()
  const assets = useApp((s) => s.assets)
  const selection = useApp((s) => s.selection)
  const visible = useVisibleItems()
  const sets: Record<Scope, number[]> = {
    selection: assets.filter((a) => selection.has(a.id)).map((a) => a.id),
    filter: visible.map((it) => it.asset.id),
    session: assets.map((a) => a.id),
  }
  const [scope, setScope] = useState<Scope>(sets.selection.length ? 'selection' : 'filter')
  const ids = sets[scope]
  const readOnly = assets.filter((a) => !a.writable && ids.includes(a.id)).length
  const enabled = preset.preset.rules.filter((r) => r.enabled).length
  return (
    <Dialog
      title={t('presets.apply_q', { name: preset.name })}
      onCancel={onCancel}
      footer={
        <>
          <span className="note">{t('presets.apply_writes_nothing')}</span>
          <button className="btn" onClick={onCancel}>
            {t('common.cancel')}
          </button>
          <button className="btn primary" disabled={!ids.length} onClick={() => onBuild(ids)}>
            {t('presets.build_preview')}
          </button>
        </>
      }
    >
      <div role="radiogroup" aria-label={t('presets.apply_scope')} className="scope-list">
        {(['selection', 'filter', 'session'] as Scope[]).map((k) => (
          <label key={k} className={`scope-row${sets[k].length ? '' : ' disabled'}`}>
            <input type="radio" name="apply-scope" checked={scope === k} disabled={!sets[k].length} onChange={() => setScope(k)} />
            <span>{t(`presets.scope_${k}` as MessageKey)}</span>
            <span className="mono faint">{t('presets.n_files', { n: sets[k].length })}</span>
          </label>
        ))}
      </div>
      <ul className="facts">
        <li>{t('presets.fact_rules', { n: preset.preset.rules.length, enabled })}</li>
        {readOnly > 0 && <li>{t('presets.fact_read_only', { n: readOnly })}</li>}
        <li>{t('presets.fact_order')}</li>
        {preset.untrusted && <li className="glyph-warn">{t('presets.fact_untrusted')}</li>}
      </ul>
    </Dialog>
  )
}

