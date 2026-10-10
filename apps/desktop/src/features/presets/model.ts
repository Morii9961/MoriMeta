// SPDX-License-Identifier: GPL-3.0-or-later
// Presets and Rules as the backend stores them (mm-domain::rules, schema_version 1), and their
// plain-sentence form for the Preset details (SCREEN_SPEC §6).

import type { T } from '../../i18n'
import { fieldLabel } from '../../i18n'

export type RuleField = 'creator' | 'copyright' | 'capture_time' | 'gps'

export type Condition =
  | { if: 'empty' | 'not_empty'; field: RuleField }
  | { if: 'equals' | 'contains'; field: RuleField; value: string }
  | { if: 'extension'; any: string[] }
  | { if: 'kind'; kind: 'jpeg' | 'raw' | 'xmp' }

export type Action =
  | { do: 'set_creator'; names: string[] }
  | { do: 'clear_creator' }
  | { do: 'set_copyright'; value: string }
  | { do: 'clear_copyright' }
  | { do: 'set_gps'; position: string }
  | { do: 'remove_gps' }
  | { do: 'set_time'; to: string; digitized?: boolean }
  | { do: 'shift_time'; by: string; digitized?: boolean }

export interface Rule {
  name: string
  enabled: boolean
  when: Condition[]
  then: Action[]
}

export interface Preset {
  schema_version: number
  name: string
  rules: Rule[]
}

export interface PresetInfo {
  id: string
  name: string
  builtin: boolean
  fields: RuleField[]
  last_used_ms: number | null
  untrusted: boolean
  preset: Preset
  lint: string[]
}

export const FIELDS: RuleField[] = ['creator', 'copyright', 'capture_time', 'gps']

export const CONDITION_KINDS: Condition['if'][] = ['empty', 'not_empty', 'equals', 'contains', 'extension', 'kind']

export const ACTION_KINDS: Action['do'][] = [
  'set_creator',
  'clear_creator',
  'set_copyright',
  'clear_copyright',
  'set_gps',
  'remove_gps',
  'set_time',
  'shift_time',
]

export function newCondition(kind: Condition['if']): Condition {
  switch (kind) {
    case 'empty':
    case 'not_empty':
      return { if: kind, field: 'copyright' }
    case 'equals':
    case 'contains':
      return { if: kind, field: 'creator', value: '' }
    case 'extension':
      return { if: 'extension', any: ['jpg'] }
    case 'kind':
      return { if: 'kind', kind: 'jpeg' }
  }
}

export function newAction(kind: Action['do']): Action {
  switch (kind) {
    case 'set_creator':
      return { do: kind, names: [''] }
    case 'set_copyright':
      return { do: kind, value: '' }
    case 'set_gps':
      return { do: kind, position: '' }
    case 'set_time':
      return { do: kind, to: '', digitized: true }
    case 'shift_time':
      return { do: kind, by: '+00:00:00', digitized: true }
    default:
      return { do: kind } as Action
  }
}

/** The text value an action carries, for its input. */
export function actionValue(a: Action): string | null {
  switch (a.do) {
    case 'set_creator':
      return a.names.join('; ')
    case 'set_copyright':
      return a.value
    case 'set_gps':
      return a.position
    case 'set_time':
      return a.to
    case 'shift_time':
      return a.by
    default:
      return null
  }
}

export function withActionValue(a: Action, v: string): Action {
  switch (a.do) {
    case 'set_creator':
      return { ...a, names: v.split(';').map((x) => x.trim()) }
    case 'set_copyright':
      return { ...a, value: v }
    case 'set_gps':
      return { ...a, position: v }
    case 'set_time':
      return { ...a, to: v }
    case 'shift_time':
      return { ...a, by: v }
    default:
      return a
  }
}

/** A rule's problems before saving (the backend checks again). */
export function ruleProblems(t: T, r: Rule): string[] {
  const out: string[] = []
  if (r.then.length === 0) out.push(t('rules.no_action'))
  for (const c of r.when) {
    if ((c.if === 'equals' || c.if === 'contains') && !c.value.trim()) out.push(t('rules.need_value'))
    if (c.if === 'extension' && !c.any.filter(Boolean).length) out.push(t('rules.need_extension'))
  }
  for (const a of r.then) {
    const v = actionValue(a)
    if (v !== null && !v.trim()) out.push(t('rules.need_value'))
  }
  return [...new Set(out)]
}

export function conditionText(t: T, c: Condition): string {
  switch (c.if) {
    case 'empty':
      return t('rules.s_empty', { field: fieldLabel(t, c.field) })
    case 'not_empty':
      return t('rules.s_not_empty', { field: fieldLabel(t, c.field) })
    case 'equals':
      return t('rules.s_equals', { field: fieldLabel(t, c.field), value: c.value })
    case 'contains':
      return t('rules.s_contains', { field: fieldLabel(t, c.field), value: c.value })
    case 'extension':
      return t('rules.s_extension', { list: c.any.join(', ') })
    case 'kind':
      return t('rules.s_kind', { kind: t(`rules.kind_${c.kind}` as Parameters<T>[0]) })
  }
}

export function actionText(t: T, a: Action): string {
  switch (a.do) {
    case 'set_creator':
      return t('rules.s_set', { field: fieldLabel(t, 'creator'), value: a.names.join('; ') })
    case 'set_copyright':
      return t('rules.s_set', { field: fieldLabel(t, 'copyright'), value: a.value })
    case 'set_gps':
      return t('rules.s_set', { field: fieldLabel(t, 'gps'), value: a.position })
    case 'set_time':
      return t('rules.s_set', { field: fieldLabel(t, 'capture_time'), value: a.to })
    case 'shift_time':
      return t('rules.s_shift', { by: a.by })
    case 'clear_creator':
      return t('rules.s_clear', { field: fieldLabel(t, 'creator') })
    case 'clear_copyright':
      return t('rules.s_clear', { field: fieldLabel(t, 'copyright') })
    case 'remove_gps':
      return t('rules.s_remove_gps')
  }
}

/** What kind of Preset this is (PresetRow badges): removal, time, or descriptive. */
export function presetKind(p: Preset): 'removal' | 'time' | 'descriptive' {
  const acts = p.rules.flatMap((r) => r.then)
  if (acts.some((a) => a.do === 'remove_gps' || a.do === 'clear_creator' || a.do === 'clear_copyright')) return 'removal'
  if (acts.some((a) => a.do === 'set_time' || a.do === 'shift_time')) return 'time'
  return 'descriptive'
}

/** `rules` with the rule at `from` moved to drop position `to` (0 = before the first rule,
 * `rules.length` = after the last), as dragging a rule does (SCREEN_SPEC 3#rules-drag). */
export function moveRule<R>(rules: R[], from: number, to: number): R[] {
  if (from < 0 || from >= rules.length || to < 0 || to > rules.length) return rules
  const out = [...rules]
  const [r] = out.splice(from, 1)
  out.splice(to > from ? to - 1 : to, 0, r)
  return out
}
