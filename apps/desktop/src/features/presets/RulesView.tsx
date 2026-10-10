// SPDX-License-Identifier: GPL-3.0-or-later
// Rule builder (SCREEN_SPEC §5, DESIGN_SYSTEM RuleRow, INTERACTION_SPEC §16): the Preset's rules
// run top to bottom against the files as they are now; a later rule setting the same field wins.
// IF lines (all must hold) and THEN lines; enable, reorder (Alt ↑/↓), delete; Save (Ctrl S).
// Built-in Presets are shown read-only.

import { Fragment, useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import { useApp } from '../../state/store'
import { useT, useBT, fieldLabel, type MessageKey } from '../../i18n'
import { usePresets } from './PresetsView'
import { useVisibleItems } from '../library/hooks'
import { showPreview } from '../../app/actions'
import type { DryRun } from '../../ipc/types'
import {
  ACTION_KINDS,
  CONDITION_KINDS,
  FIELDS,
  actionValue,
  newAction,
  newCondition,
  moveRule,
  ruleProblems,
  withActionValue,
  type Action,
  type Condition,
  type Preset,
  type Rule,
  type RuleField,
} from './model'
import './presets.css'

const EMPTY: Preset = { schema_version: 1, name: '', rules: [] }

function ConditionEditor({ c, onChange, onRemove, ro }: { c: Condition; onChange: (c: Condition) => void; onRemove: () => void; ro: boolean }) {
  const t = useT()
  return (
    <div className="rule-line">
      <select className="input" disabled={ro} value={c.if} onChange={(e) => onChange(newCondition(e.target.value as Condition['if']))}>
        {CONDITION_KINDS.map((k) => (
          <option key={k} value={k}>
            {t(`rules.c_${k}` as MessageKey)}
          </option>
        ))}
      </select>
      {'field' in c && (
        <select className="input" disabled={ro} value={c.field} onChange={(e) => onChange({ ...c, field: e.target.value as RuleField })}>
          {FIELDS.map((f) => (
            <option key={f} value={f}>
              {fieldLabel(t, f)}
            </option>
          ))}
        </select>
      )}
      {(c.if === 'equals' || c.if === 'contains') && (
        <input className="input staged rule-value" disabled={ro} value={c.value} placeholder={t('rules.value')} onChange={(e) => onChange({ ...c, value: e.target.value })} />
      )}
      {c.if === 'extension' && (
        <input
          className="input staged rule-value"
          disabled={ro}
          value={c.any.join(', ')}
          placeholder="jpg, nef"
          onChange={(e) => onChange({ ...c, any: e.target.value.split(',').map((x) => x.trim().replace(/^\./, '')) })}
        />
      )}
      {c.if === 'kind' && (
        <select className="input" disabled={ro} value={c.kind} onChange={(e) => onChange({ ...c, kind: e.target.value as 'jpeg' | 'raw' | 'xmp' })}>
          {(['jpeg', 'raw', 'xmp'] as const).map((k) => (
            <option key={k} value={k}>
              {t(`rules.kind_${k}` as MessageKey)}
            </option>
          ))}
        </select>
      )}
      {!ro && (
        <button className="link" onClick={onRemove} aria-label={t('rules.remove')}>
          ×
        </button>
      )}
    </div>
  )
}

function ActionEditor({ a, onChange, onRemove, ro }: { a: Action; onChange: (a: Action) => void; onRemove: () => void; ro: boolean }) {
  const t = useT()
  const v = actionValue(a)
  return (
    <div className="rule-line">
      <select className="input" disabled={ro} value={a.do} onChange={(e) => onChange(newAction(e.target.value as Action['do']))}>
        {ACTION_KINDS.map((k) => (
          <option key={k} value={k}>
            {t(`rules.a_${k}` as MessageKey)}
          </option>
        ))}
      </select>
      {v !== null && (
        <input
          className="input staged rule-value"
          disabled={ro}
          value={v}
          placeholder={
            a.do === 'set_time' ? 'YYYY:MM:DD HH:MM:SS' : a.do === 'shift_time' ? '+01:00:00' : a.do === 'set_gps' ? '35.6586,139.7454' : t('rules.value')
          }
          onChange={(e) => onChange(withActionValue(a, e.target.value))}
        />
      )}
      {!ro && (
        <button className="link" onClick={onRemove} aria-label={t('rules.remove')}>
          ×
        </button>
      )}
    </div>
  )
}

export function RulesView() {
  const t = useT()
  const bt = useBT()
  const notify = useApp((s) => s.notify)
  const editPreset = useApp((s) => s.editPreset)
  const { list, reload } = usePresets()
  const [id, setId] = useState<string | null>(editPreset)
  const [draft, setDraft] = useState<Preset>(EMPTY)
  const [saved, setSaved] = useState<string>(JSON.stringify(EMPTY))
  const [error, setError] = useState<string | null>(null)
  const cur = list?.find((p) => p.id === id) ?? null
  const ro = !!cur?.builtin

  useEffect(() => {
    if (!list) return
    const p = list.find((x) => x.id === id)
    const base = p ? p.preset : { ...EMPTY, name: t('rules.new_name') }
    setDraft(base)
    setSaved(JSON.stringify(base))
    setError(null)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, list])

  const dirty = JSON.stringify(draft) !== saved
  // Dry run (SCREEN_SPEC §5): the draft against some files of the Session, read only
  const assets = useApp((s) => s.assets)
  const selection = useApp((s) => s.selection)
  const visible = useVisibleItems()
  const [against, setAgainst] = useState<'selection' | 'filter' | 'sample'>('selection')
  const [dry, setDry] = useState<DryRun | null>(null)
  const [dryBusy, setDryBusy] = useState(false)
  const [dryError, setDryError] = useState<string | null>(null)
  const targets = {
    selection: assets.filter((a) => selection.has(a.id)).map((a) => a.id),
    filter: visible.map((it) => it.asset.id),
    sample: assets.slice(0, 100).map((a) => a.id),
  }
  useEffect(() => {
    setDry(null)
    setDryError(null)
  }, [draft, against])
  const problems = useMemo(() => draft.rules.map((r) => ruleProblems(t, r)), [draft, t])
  const enabled = draft.rules.filter((r) => r.enabled).length
  // incomplete rules are left out of the dry run, as the footer says
  const runnable: Preset = { ...draft, rules: draft.rules.filter((r, i) => r.enabled && problems[i].length === 0) }
  const leftOut = draft.rules.map((r, i) => (r.enabled && problems[i].length ? i + 1 : 0)).filter(Boolean)
  const runDry = async () => {
    setDryBusy(true)
    setDryError(null)
    try {
      setDry(await api.presetDryRun(targets[against], runnable))
    } catch (e) {
      setDryError(errorText(e))
    } finally {
      setDryBusy(false)
    }
  }
  const previewPlan = async () => {
    if (!id || dirty) return
    const ids = targets[against]
    useApp.getState().setStage({ kind: 'planning', done: 0, total: ids.length, stage: 'files' })
    try {
      showPreview(await api.planPreset(ids, id), 'edit')
    } catch (e) {
      useApp.getState().setStage({ kind: 'library' })
      notify('error', errorText(e))
    }
  }

  const setRule = (i: number, r: Rule) => setDraft({ ...draft, rules: draft.rules.map((x, k) => (k === i ? r : x)) })
  const move = (i: number, d: number) => {
    const j = i + d
    if (j < 0 || j >= draft.rules.length) return
    const rules = [...draft.rules]
    ;[rules[i], rules[j]] = [rules[j], rules[i]]
    setDraft({ ...draft, rules })
  }
  // drag a rule by its handle; the drop line shows the position it will take (3#rules-drag)
  const [dragging, setDragging] = useState<number | null>(null)
  const [dropAt, setDropAt] = useState<number | null>(null)
  const endDrag = () => {
    setDragging(null)
    setDropAt(null)
  }
  const drop = () => {
    if (dragging !== null && dropAt !== null) setDraft({ ...draft, rules: moveRule(draft.rules, dragging, dropAt) })
    endDrag()
  }
  // the position a dropped rule takes; a drop next to the dragged rule changes nothing
  const landsAt = (at: number) => (dragging !== null && at > dragging ? at : at + 1)
  const dropLine = (at: number) =>
    dragging !== null && dropAt === at && at !== dragging && at !== dragging + 1 ? (
      <div className="rule-drop" aria-hidden>
        <span className="mono">{t('rules.drop_at', { n: String(landsAt(at)).padStart(2, '0') })}</span>
      </div>
    ) : null
  const save = async () => {
    if (ro || !dirty) return
    try {
      const newId = await api.presetSave(id && !cur?.builtin ? id : null, {
        ...draft,
        rules: draft.rules.map((r) => ({ ...r, then: r.then.map((a) => (a.do === 'set_creator' ? { ...a, names: a.names.filter(Boolean) } : a)) })),
      })
      setId(newId)
      useApp.setState({ editPreset: newId })
      await reload()
      notify('success', t('rules.saved'))
    } catch (e) {
      setError(errorText(e))
    }
  }

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 's') {
        e.preventDefault()
        save()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  })

  return (
    <>
      <nav className="pane-sidebar" aria-label={t('module.presets')}>
        <div className="side-group">
          <div className="side-group-header">
            <span className="section-label">{t('module.presets')}</span>
            <button
              className="link side-clear"
              onClick={() => {
                setId(null)
                useApp.setState({ editPreset: null })
              }}
            >
              {t('presets.new')}
            </button>
          </div>
          {(list ?? []).map((p) => (
            <button
              key={p.id}
              className={`kind-row${p.id === id ? ' on' : ''}`}
              onClick={() => {
                setId(p.id)
                useApp.setState({ editPreset: p.id })
              }}
            >
              <span className="faint">{p.builtin ? '·' : ''}</span>
              <span className="ellipsis">{p.name}</span>
              <span className="mono faint">{p.preset.rules.length}</span>
              <span />
            </button>
          ))}
        </div>
      </nav>
      <div className="pane-primary">
        <div className="rules-head">
          <input className="input ui rules-name" disabled={ro} value={draft.name} onChange={(e) => setDraft({ ...draft, name: e.target.value })} aria-label={t('rules.name')} />
          <span className="mono faint">{t('rules.summary', { n: draft.rules.length, m: enabled })}</span>
          {dirty && <span className="tag edited-tag">{t('rules.edited')}</span>}
          {ro && <span className="tag">{t('presets.builtin')}</span>}
          <div className="toolbar-spacer" />
          <button
            className="btn small"
            disabled={!dirty}
            onClick={() => {
              setDraft(JSON.parse(saved))
              setError(null)
            }}
          >
            {t('rules.revert')}
          </button>
          {ro ? (
            <button className="btn small accent" onClick={() => cur && api.presetDuplicate(cur.id).then((n) => { setId(n); reload() }).catch((e) => notify('error', errorText(e)))}>
              {t('rules.duplicate_to_edit')}
            </button>
          ) : (
            <button className={`btn small${dirty ? ' accent' : ''}`} disabled={!dirty || problems.some((p) => p.length > 0)} onClick={save}>
              {t('rules.save')} <span className="mono faint">Ctrl S</span>
            </button>
          )}
        </div>
        <div className="pane-scroll rules-body">
          {draft.rules.map((r, i) => (
            <Fragment key={i}>
            {dropLine(i)}
            <div
              className={`rule-row${r.enabled ? '' : ' disabled'}${problems[i].length ? ' incomplete' : ''}${dragging === i ? ' dragging' : ''}`}
              onDragOver={(e) => {
                if (dragging === null) return
                e.preventDefault()
                const box = e.currentTarget.getBoundingClientRect()
                setDropAt(e.clientY < box.top + box.height / 2 ? i : i + 1)
              }}
              onDrop={(e) => {
                e.preventDefault()
                drop()
              }}
            >
              <div className="rule-rail">
                <input type="checkbox" className="checkbox" disabled={ro} checked={r.enabled} onChange={(e) => setRule(i, { ...r, enabled: e.target.checked })} aria-label={t('rules.enabled')} />
                <span className="mono faint">{String(i + 1).padStart(2, '0')}</span>
                {!ro && draft.rules.length > 1 && (
                  <span
                    className="rule-grip"
                    draggable
                    title={t('rules.drag')}
                    aria-label={t('rules.drag')}
                    onDragStart={(e) => {
                      e.dataTransfer.effectAllowed = 'move'
                      e.dataTransfer.setData('text/plain', String(i))
                      setDragging(i)
                    }}
                    onDragEnd={endDrag}
                  >
                    ⋮⋮
                  </span>
                )}
                {!ro && (
                  <span className="rule-move">
                    <button className="link" onClick={() => move(i, -1)} disabled={i === 0} aria-label={t('rules.up')}>
                      ▲
                    </button>
                    <button className="link" onClick={() => move(i, 1)} disabled={i === draft.rules.length - 1} aria-label={t('rules.down')}>
                      ▼
                    </button>
                  </span>
                )}
              </div>
              <div
                className="rule-body"
                onKeyDown={(e) => {
                  if (e.altKey && (e.key === 'ArrowUp' || e.key === 'ArrowDown')) {
                    e.preventDefault()
                    move(i, e.key === 'ArrowUp' ? -1 : 1)
                  }
                }}
              >
                <input className="input ui rule-name" disabled={ro} value={r.name} placeholder={t('rules.rule_name')} onChange={(e) => setRule(i, { ...r, name: e.target.value })} />
                {r.when.map((c, k) => (
                  <div key={k} className="rule-kw-line">
                    <span className="kw-if mono">{k === 0 ? t('rules.if') : t('rules.and')}</span>
                    <ConditionEditor
                      ro={ro}
                      c={c}
                      onChange={(n) => setRule(i, { ...r, when: r.when.map((x, j) => (j === k ? n : x)) })}
                      onRemove={() => setRule(i, { ...r, when: r.when.filter((_, j) => j !== k) })}
                    />
                  </div>
                ))}
                {r.when.length === 0 && (
                  <div className="rule-kw-line">
                    <span className="kw-if mono">{t('rules.if')}</span>
                    <span className="faint">{t('rules.every_file')}</span>
                  </div>
                )}
                {!ro && (
                  <button className="link rule-add" onClick={() => setRule(i, { ...r, when: [...r.when, newCondition('empty')] })}>
                    + {t('rules.add_condition')}
                  </button>
                )}
                {r.then.map((a, k) => (
                  <div key={k} className="rule-kw-line">
                    <span className="kw-then mono">{k === 0 ? t('rules.then') : t('rules.and')}</span>
                    <ActionEditor
                      ro={ro}
                      a={a}
                      onChange={(n) => setRule(i, { ...r, then: r.then.map((x, j) => (j === k ? n : x)) })}
                      onRemove={() => setRule(i, { ...r, then: r.then.filter((_, j) => j !== k) })}
                    />
                  </div>
                ))}
                {!ro && (
                  <button className="link rule-add" onClick={() => setRule(i, { ...r, then: [...r.then, newAction('set_copyright')] })}>
                    + {t('rules.add_action')}
                  </button>
                )}
                {problems[i].map((p) => (
                  <div key={p} className="field-note glyph-fail">
                    {p}
                  </div>
                ))}
              </div>
              <div className="rule-side">
                {!ro && (
                  <button className="link" onClick={() => setDraft({ ...draft, rules: draft.rules.filter((_, k) => k !== i) })}>
                    {t('rules.delete_rule')}
                  </button>
                )}
              </div>
            </div>
            </Fragment>
          ))}
          {dropLine(draft.rules.length)}
          {!ro && (
            <button
              className="btn small rule-new"
              onClick={() => setDraft({ ...draft, rules: [...draft.rules, { name: '', enabled: true, when: [], then: [newAction('set_copyright')] }] })}
            >
              + {t('rules.add_rule')}
            </button>
          )}
          <p className="note rules-explain">{t('rules.explain')}</p>
        </div>
      </div>
      <aside className="pane-inspector">
        <div className="pane-scroll insp">
          <div className="insp-section dry-run">
            <div className="section-label">{t('rules.dry_run')}</div>
            <label className="popover-field">
              <span className="field-label">{t('rules.test_against')}</span>
              <select className="input ui" value={against} onChange={(e) => setAgainst(e.target.value as typeof against)}>
                <option value="selection">{t('rules.against_selection', { n: targets.selection.length })}</option>
                <option value="filter">{t('rules.against_filter', { n: targets.filter.length })}</option>
                <option value="sample">{t('rules.against_sample', { n: targets.sample.length })}</option>
              </select>
            </label>
            <button className="btn small" disabled={dryBusy || !targets[against].length || runnable.rules.length === 0} onClick={runDry}>
              {dryBusy ? t('rules.dry_running') : t('rules.dry_run_now')}
            </button>
            {!targets[against].length && <p className="note">{t('rules.dry_no_files')}</p>}
            {dryError && <div className="tone error selectable">{bt(dryError)}</div>}
            {dry && (
              <>
                <p className="mono">
                  {t('rules.dry_files', { n: dry.files })} · {t('rules.dry_changes', { n: dry.view.summary.ready })}
                </p>
                <ul className="facts">
                  <li>{t('rules.dry_kinds', { add: dry.view.kinds.add, modify: dry.view.kinds.modify, remove: dry.view.kinds.remove })}</li>
                  <li>{t('rules.dry_unmatched', { n: dry.view.kinds.no_change })}</li>
                  {dry.view.kinds.warnings > 0 && <li className="glyph-warn">{t('rules.dry_warnings', { n: dry.view.kinds.warnings })}</li>}
                  {dry.view.kinds.blocked > 0 && <li className="glyph-warn">{t('rules.dry_blocked', { n: dry.view.kinds.blocked })}</li>}
                  {dry.view.kinds.unsupported > 0 && <li className="faint">{t('rules.dry_unsupported', { n: dry.view.kinds.unsupported })}</li>}
                  <li className="glyph-ok">{t('rules.dry_protected')}</li>
                </ul>
                {dry.view.field_counts.length > 0 && (
                  <>
                    <div className="section-label">{t('rules.dry_by_field')}</div>
                    <ul className="facts">
                      {dry.view.field_counts.map(([f, n]) => (
                        <li key={f}>
                          {fieldLabel(t, f)}: <span className="mono">{n}</span>
                        </li>
                      ))}
                    </ul>
                  </>
                )}
                {dry.samples.length > 0 && (
                  <>
                    <div className="section-label">{t('rules.dry_samples')}</div>
                    {dry.samples.map((s) => (
                      <div key={s.name} className="dry-sample">
                        <div className="mono ellipsis">{s.name}</div>
                        {s.changes.map(([f, before, after]) => (
                          <div key={f} className="mono faint ellipsis" title={`${before} → ${after}`}>
                            {fieldLabel(t, f)}: {before || '—'} → <span className="glyph-mod">{after || '—'}</span>
                          </div>
                        ))}
                      </div>
                    ))}
                  </>
                )}
              </>
            )}
            {leftOut.length > 0 && <p className="note glyph-warn">{t('rules.left_out', { rules: leftOut.map((n) => String(n).padStart(2, '0')).join(', ') })}</p>}
            <button className="btn small accent" disabled={!id || dirty || !targets[against].length} title={dirty ? t('rules.save_first') : undefined} onClick={previewPlan}>
              {t('rules.preview_plan')}
            </button>
            {dirty && <p className="note">{t('rules.save_first')}</p>}
          </div>
          <div className="insp-section">
            <div className="section-label">{t('rules.checks')}</div>
            {error && <div className="tone error selectable">{bt(error)}</div>}
            {problems.every((p) => p.length === 0) && !error ? (
              <p className="note glyph-ok">✓ {t('rules.no_problems')}</p>
            ) : (
              problems.map((p, i) => (p.length ? <p key={i} className="note glyph-fail">{t('rules.rule_n', { n: i + 1 })}: {p.join(', ')}</p> : null))
            )}
            {cur?.lint.map((w, i) => (
              <div key={i} className="tone warn">
                {bt(w)}
              </div>
            ))}
          </div>
          <div className="insp-section">
            <div className="section-label">{t('rules.how')}</div>
            <p className="note">{t('rules.how_1')}</p>
            <p className="note">{t('rules.how_2')}</p>
            <p className="note">{t('rules.how_3')}</p>
          </div>
        </div>
      </aside>
    </>
  )
}
