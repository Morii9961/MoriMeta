// SPDX-License-Identifier: GPL-3.0-or-later
// Recovery dialog (SCREEN_SPEC 6#e-recover, INTERACTION_SPEC §13, DECISIONS R-1): shown at launch
// before any write when an interrupted Operation still asks for a decision. Continue remaining
// (primary) · Undo the completed… · Keep as is and close.

import { useEffect, useState } from 'react'
import { api, errorText } from '../../ipc'
import type { RecoverySummary } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT } from '../../i18n'
import { Dialog } from '../../components/Dialog'
import { planUndo } from '../../app/actions'

export function RecoveryDialog() {
  const t = useT()
  const info = useApp((s) => s.info)
  const notify = useApp((s) => s.notify)
  const [pending, setPending] = useState<RecoverySummary[]>([])
  const [busy, setBusy] = useState(false)
  const launched = info?.launched ?? false
  // also asked again when an operation brought back from a backup folder needs a decision
  const asking = info?.startup.needs_decision.length ?? 0

  useEffect(() => {
    if (!launched) return
    api
      .recoveryStatus()
      .then(setPending)
      .catch(() => {})
  }, [launched, asking])

  const op = pending[0]
  if (!op) return null
  const next = () => setPending((p) => p.slice(1))
  const act = async (f: () => Promise<unknown>, done: string) => {
    setBusy(true)
    try {
      await f()
      notify('success', done)
      next()
    } catch (e) {
      notify('error', errorText(e))
    } finally {
      setBusy(false)
      api.appInfo().then(useApp.getState().setInfo).catch(() => {})
    }
  }
  return (
    <Dialog
      title={t('recovery.title')}
      glyph="!"
      onCancel={() => !busy && next()}
      footer={
        <>
          <span className="note">{t('recovery.note')}</span>
          <button className="btn dlg" disabled={busy} onClick={() => act(() => api.recoveryDismiss(op.op_id), t('recovery.kept'))}>
            {t('recovery.keep')}
          </button>
          <button
            className="btn dlg"
            disabled={busy || op.done === 0}
            onClick={() => {
              next()
              planUndo(op.op_id)
            }}
          >
            {t('recovery.undo')}…
          </button>
          <button
            className="btn dlg primary"
            disabled={busy || op.remaining === 0}
            onClick={() => act(() => api.recoveryResume(op.op_id), t('recovery.resumed'))}
          >
            {t('recovery.continue')}
          </button>
        </>
      }
    >
      <p>{t('recovery.lead', { title: op.title })}</p>
      <div className="consequence">
        <span className="glyph-ok">✓</span>
        <span>{t('recovery.done')}</span>
        <span className="mono">{op.done}</span>
      </div>
      <div className="consequence">
        <span className="glyph-skip">○</span>
        <span>{t('recovery.remaining')}</span>
        <span className="mono">{op.remaining}</span>
      </div>
      <div className="consequence">
        <span className="glyph-warn">!</span>
        <span>{t('recovery.attention')}</span>
        <span className="mono">{op.attention}</span>
      </div>
      <p className="note">{t('recovery.safe')}</p>
    </Dialog>
  )
}
