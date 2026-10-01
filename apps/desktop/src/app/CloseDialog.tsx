// SPDX-License-Identifier: GPL-3.0-or-later
// Closing during an Operation always asks (INTERACTION_SPEC §10; Settings › General "LOCKED").
// Keep going is the default; the other choice stops after the current file and then closes.

import { useApp } from '../state/store'
import { useT } from '../i18n'
import { Dialog } from '../components/Dialog'
import { cancelOperation } from './actions'

export function CloseDialog() {
  const t = useT()
  const asked = useApp((s) => s.closeAsked)
  const after = useApp((s) => s.closeAfter)
  if (!asked) return null
  const keep = () => useApp.setState({ closeAsked: false })
  return (
    <Dialog
      title={t('close.title')}
      glyph="!"
      onCancel={keep}
      footer={
        <>
          <span className="note">{after ? t('close.stopping') : ''}</span>
          <button
            className="btn dlg"
            disabled={after}
            onClick={() => {
              useApp.setState({ closeAfter: true })
              cancelOperation()
            }}
          >
            {t('close.stop')}
          </button>
          <button className="btn dlg primary" disabled={after} onClick={keep}>
            {t('op.keep_going')}
          </button>
        </>
      }
    >
      <p className="note">{t('close.lead')}</p>
      <p className="note">✓ {t('op.cancel_finished')}</p>
      <p className="note">↺ {t('op.cancel_current')}</p>
    </Dialog>
  )
}
