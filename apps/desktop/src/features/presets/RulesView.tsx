// SPDX-License-Identifier: GPL-3.0-or-later
// Rule builder (SCREEN_SPEC §5, DESIGN_SYSTEM RuleRow, INTERACTION_SPEC §16): the Preset's rules
// run top to bottom against the files as they are now; a later rule setting the same field wins.
// IF lines (all must hold) and THEN lines; enable, reorder (Alt ↑/↓), delete; Save (Ctrl S).
// Built-in Presets are shown read-only.

import { useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import { useApp } from '../../state/store'
import { useT, useBT, fieldLabel, type MessageKey } from '../../i18n'
import { usePresets } from './PresetsView'
import {
  ACTION_KINDS,
  CONDITION_KINDS,
  FIELDS,
  actionValue,
  newAction,
  newCondition,
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
  const problems = useMemo(() => draft.rules.map((r) => ruleProblems(t, r)), [draft, t])
  const enabled = draft.rules.filter((r) => r.enabled).length

  const setRule = (i: number, r: Rule) => setDraft({ ...draft, rules: draft.rules.map((x, k) => (k === i ? r : x)) })
  const move = (i: number, d: number) => {
    const j = i + d
    if (j < 0 || j >= draft.rules.length) return
    const rules = [...draft.rules]
    ;[rules[i], rules[j]] = [rules[j], rules[i]]
    setDraft({ ...draft, rules })
  }
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
            <div key={i} className={`rule-row${r.enabled ? '' : ' disabled'}${problems[i].length ? ' incomplete' : ''}`}>
              <div className="rule-rail">
                <input type="checkbox" className="checkbox" disabled={ro} checked={r.enabled} onChange={(e) => setRule(i, { ...r, enabled: e.target.checked })} aria-label={t('rules.enabled')} />
                <span className="mono faint">{String(i + 1).padStart(2, '0')}</span>
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
          ))}
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
