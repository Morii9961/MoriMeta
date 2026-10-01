// SPDX-License-Identifier: GPL-3.0-or-later
// Preview data: the change kinds of an entry (DESIGN_SYSTEM §2), the kind filter, and the review
// checklist that gates Apply (INTERACTION_SPEC §4).

import type { FieldChange, PlanEntry } from '../../ipc/types'

export type Kind = 'add' | 'modify' | 'remove' | 'warning' | 'unsupported' | 'no_change' | 'excluded'

export const KINDS: Kind[] = ['add', 'modify', 'remove', 'warning', 'unsupported', 'no_change', 'excluded']

/** Categories that must be opened once before Apply (INTERACTION_SPEC §4 second box). */
export const REVIEW: Kind[] = ['warning', 'unsupported', 'remove']

export const GLYPH: Record<Kind, string> = {
  add: '+',
  modify: '~',
  remove: '−',
  warning: '!',
  unsupported: '⊘',
  no_change: '=',
  excluded: '↺',
}

export const GLYPH_CLASS: Record<Kind, string> = {
  add: 'glyph-add',
  modify: 'glyph-mod',
  remove: 'glyph-rem',
  warning: 'glyph-warn',
  unsupported: 'glyph-uns',
  no_change: 'glyph-none',
  excluded: 'glyph-uns',
}

export function kindsOf(e: PlanEntry): Set<Kind> {
  const k = new Set<Kind>()
  if (e.excluded) {
    k.add('excluded')
    return k
  }
  if ((e.excluded_changes?.length ?? 0) > 0) k.add('excluded')
  switch (e.status.status) {
    case 'ready':
      for (const c of e.changes) k.add(c.kind)
      if (e.warnings.length) k.add('warning')
      break
    case 'no_change':
      k.add('no_change')
      break
    default:
      k.add('unsupported')
  }
  return k
}

export function changeKind(c: FieldChange): Kind {
  return c.kind
}

export function counts(entries: PlanEntry[]): Record<Kind, number> {
  const c = Object.fromEntries(KINDS.map((k) => [k, 0])) as Record<Kind, number>
  for (const e of entries) for (const k of kindsOf(e)) c[k]++
  return c
}

export function valueText(v: string[] | null | undefined): string {
  if (!v || v.length === 0) return ''
  return v.join('; ')
}

/** Human text for an acknowledgement key from the backend (`remove:gps`, `unsupported`, `large`). */
export function removalCount(entries: PlanEntry[], field: string): number {
  return entries.filter((e) => !e.excluded && e.status.status === 'ready' && e.changes.some((c) => c.field === field && c.kind === 'remove')).length
}
