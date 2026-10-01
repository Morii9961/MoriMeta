// SPDX-License-Identifier: GPL-3.0-or-later
// High-risk ConfirmDialog (DESIGN_SYSTEM ConfirmDialog; INTERACTION_SPEC §4–5): counts by kind,
// where each type is written, and one acknowledgement per required category. "Apply N" stays
// disabled until every acknowledgement is ticked; Esc cancels. The acknowledgements are recorded
// with the Operation.

import { useState } from 'react'
import type { PlanEntry, PlanView } from '../../ipc/types'
import { useT, fieldLabel } from '../../i18n'
import { Dialog } from '../../components/Dialog'
import { removalCount } from './model'

export function ConfirmApply({
  plan,
  entries,
  onCancel,
  onConfirm,
}: {
  plan: PlanView
  entries: PlanEntry[]
  onCancel: () => void
  onConfirm: (acks: string[]) => void
}) {
  const t = useT()
  const [ticked, setTicked] = useState<Set<string>>(new Set())
  const all = plan.required_acks.every((a) => ticked.has(a))
  const ackText = (a: string) => {
    if (a.startsWith('remove:')) {
      const f = a.slice(7)
      return t('confirm.ack_remove', { field: fieldLabel(t, f), n: removalCount(entries, f) })
    }
    if (a === 'unsupported') return t('confirm.ack_unsupported', { n: plan.kinds.unsupported })
    if (a === 'large') return t('confirm.ack_large', { n: plan.summary.ready })
    return a
  }
  const s = plan.summary
  return (
    <Dialog
      title={t('confirm.title', { n: s.changes, files: s.ready })}
      glyph="!"
      onCancel={onCancel}
      footer={
        <>
          <span className="note">{t('confirm.note')}</span>
          <button className="btn dlg" onClick={onCancel}>
            {t('confirm.back')}
          </button>
          <button className={`btn dlg${all ? ' primary' : ''}`} disabled={!all} onClick={() => onConfirm(plan.required_acks)}>
            {t('confirm.apply', { n: s.changes })}
          </button>
        </>
      }
    >
      <div className="consequence">
        <span className="glyph-add">+</span>
        <span>{t('kind.add')}</span>
        <span className="mono">{plan.kinds.add}</span>
      </div>
      <div className="consequence">
        <span className="glyph-mod">~</span>
        <span>{t('kind.modify')}</span>
        <span className="mono">{plan.kinds.modify}</span>
      </div>
      <div className="consequence">
        <span className="glyph-rem">−</span>
        <span>{t('kind.remove')}</span>
        <span className="mono">{plan.kinds.remove}</span>
      </div>
      <div className="consequence">
        <span className="glyph-uns">⊘</span>
        <span>{t('confirm.not_written')}</span>
        <span className="mono">{plan.kinds.unsupported + plan.kinds.blocked}</span>
      </div>
      <div className="consequence muted">
        <span>·</span>
        <span>{t('confirm.where', { in_file: s.targets.in_file, sidecar: s.targets.sidecar, new_sidecar: s.targets.new_sidecar })}</span>
        <span />
      </div>
      <div className="consequence muted">
        <span>·</span>
        <span>{t('confirm.backup')}</span>
        <span />
      </div>
      {plan.required_acks.map((a) => (
        <label key={a} className="ack">
          <input
            type="checkbox"
            className="checkbox dlg"
            checked={ticked.has(a)}
            onChange={(e) => {
              const next = new Set(ticked)
              if (e.target.checked) next.add(a)
              else next.delete(a)
              setTicked(next)
            }}
          />
          <span>{ackText(a)}</span>
        </label>
      ))}
    </Dialog>
  )
}
