// SPDX-License-Identifier: GPL-3.0-or-later
// Condition row (SCREEN_SPEC 1#filters): `Where [Camera is NIKON Z 8] and [GPS is present] …`,
// + Add condition (field → operator → value, with a live match count before adding), Clear all,
// and Save as smart filter… Conditions are AND-ed with the facets and the search; reading files
// is never affected, only what the table shows.

import { useMemo, useState } from 'react'
import { useApp } from '../../state/store'
import { useT, type MessageKey, type T } from '../../i18n'
import { COLUMNS } from './data'
import { CONDITION_FIELDS, OPS, matches, matchesAll, needsValue, valuesOf, type Condition, type Op } from './layout'
import { useItems } from './hooks'

export function fieldName(t: T, field: string): string {
  const c = COLUMNS.find((x) => x.key === field)
  return c ? t(c.label) : field
}

export function conditionText(t: T, c: Condition): string {
  const op = t(`cond.op_${c.op}` as MessageKey)
  return needsValue(c.op) ? `${fieldName(t, c.field)} ${op} ${c.value}` : `${fieldName(t, c.field)} ${op}`
}

export function ConditionRow() {
  const t = useT()
  const conditions = useApp((s) => s.conditions)
  const setConditions = useApp((s) => s.setConditions)
  const saveSmartFilter = useApp((s) => s.saveSmartFilter)
  const adding = useApp((s) => s.addingCondition)
  const setAdding = (v: boolean) => useApp.setState({ addingCondition: v })
  const [naming, setNaming] = useState(false)
  const [name, setName] = useState('')

  if (conditions.length === 0 && !adding) return null
  return (
    <div className="condition-row" role="group" aria-label={t('cond.label')}>
      <span className="secondary">{t('cond.where')}</span>
      {conditions.map((c, i) => (
        <span key={`${c.field}-${c.op}-${c.value}-${i}`} className="cond-chip-wrap">
          {i > 0 && <span className="faint">{t('cond.and')}</span>}
          <span className="cond-chip mono">
            {conditionText(t, c)}
            <button className="btn plain tiny" aria-label={t('cond.remove', { what: conditionText(t, c) })} onClick={() => setConditions(conditions.filter((_, j) => j !== i))}>
              ×
            </button>
          </span>
        </span>
      ))}
      <span className="cond-add-wrap">
        <button className="link" onClick={() => setAdding(!adding)} aria-expanded={adding}>
          + {t('cond.add')}
        </button>
        {adding && (
          <AddCondition
            onAdd={(c) => {
              setConditions([...conditions, c])
              setAdding(false)
            }}
            onClose={() => setAdding(false)}
          />
        )}
      </span>
      {conditions.length > 0 && (
        <>
          <span className="faint">·</span>
          <button className="link" onClick={() => setConditions([])}>
            {t('cond.clear')}
          </button>
          <span className="faint">·</span>
          {naming ? (
            <form
              className="cond-save"
              onSubmit={(e) => {
                e.preventDefault()
                if (!name.trim()) return
                saveSmartFilter(name.trim(), conditions)
                setNaming(false)
                setName('')
              }}
            >
              <input className="input ui" autoFocus value={name} maxLength={40} placeholder={t('cond.smart_name')} aria-label={t('cond.smart_name')} onChange={(e) => setName(e.target.value)} onKeyDown={(e) => e.key === 'Escape' && setNaming(false)} />
              <button className="btn small" type="submit" disabled={!name.trim()}>
                {t('cond.save')}
              </button>
            </form>
          ) : (
            <button className="link" onClick={() => setNaming(true)}>
              {t('cond.save_smart')}
            </button>
          )}
        </>
      )}
    </div>
  )
}

function AddCondition({ onAdd, onClose }: { onAdd: (c: Condition) => void; onClose: () => void }) {
  const t = useT()
  const all = useItems()
  const existing = useApp((s) => s.conditions)
  // the count is what the table will show with this condition added to the others
  const items = useMemo(() => all.filter((it) => matchesAll(it, existing)), [all, existing])
  const [field, setField] = useState(CONDITION_FIELDS.includes('camera') ? 'camera' : CONDITION_FIELDS[0])
  const [op, setOp] = useState<Op>('is')
  const [value, setValue] = useState('')
  const suggestions = useMemo(() => valuesOf(items, field), [items, field])
  const draft: Condition = { field, op, value }
  const ready = !needsValue(op) || value.trim() !== ''
  const count = useMemo(() => (ready ? items.filter((it) => matches(it, draft)).length : null), [items, field, op, value, ready]) // eslint-disable-line react-hooks/exhaustive-deps
  const listId = `cond-values-${field}`

  return (
    <form
      className="popover cond-popover"
      role="dialog"
      aria-label={t('cond.add')}
      onSubmit={(e) => {
        e.preventDefault()
        if (ready) onAdd({ field, op, value: needsValue(op) ? value.trim() : '' })
      }}
      onKeyDown={(e) => {
        if (e.key === 'Escape') {
          e.stopPropagation()
          onClose()
        }
      }}
    >
      <label className="popover-field">
        <span className="field-label">{t('cond.field')}</span>
        <select className="input ui" autoFocus value={field} onChange={(e) => { setField(e.target.value); setValue('') }}>
          {CONDITION_FIELDS.map((f) => (
            <option key={f} value={f}>
              {fieldName(t, f)}
            </option>
          ))}
        </select>
      </label>
      <label className="popover-field">
        <span className="field-label">{t('cond.operator')}</span>
        <select className="input ui" value={op} onChange={(e) => setOp(e.target.value as Op)}>
          {OPS.map((o) => (
            <option key={o} value={o}>
              {t(`cond.op_${o}` as MessageKey)}
            </option>
          ))}
        </select>
      </label>
      {needsValue(op) && (
        <label className="popover-field">
          <span className="field-label">{t('cond.value')}</span>
          <input className="input ui" list={listId} value={value} onChange={(e) => setValue(e.target.value)} />
          <datalist id={listId}>
            {suggestions.map(([v]) => (
              <option key={v} value={v} />
            ))}
          </datalist>
        </label>
      )}
      <div className="popover-row">
        <span className="mono secondary" role="status">
          {count === null ? t('cond.need_value') : t('cond.match_count', { n: count })}
        </span>
        <span className="toolbar-spacer" />
        <button type="button" className="btn small plain" onClick={onClose}>
          {t('common.cancel')}
        </button>
        <button type="submit" className="btn small primary" disabled={!ready}>
          {t('cond.add_short')}
        </button>
      </div>
    </form>
  )
}
