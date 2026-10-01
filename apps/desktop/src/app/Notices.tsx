// SPDX-License-Identifier: GPL-3.0-or-later
// Short-lived messages (errors from commands, import results), bottom right above the status bar.

import { useEffect } from 'react'
import { useApp } from '../state/store'
import { useT, useBT } from '../i18n'

const GLYPH = { info: 'i', warn: '!', error: '×', success: '✓' } as const

export function Notices() {
  const t = useT()
  const bt = useBT()
  const notices = useApp((s) => s.notices)
  const dismiss = useApp((s) => s.dismiss)
  useEffect(() => {
    // errors stay until dismissed; the rest leave on their own
    const timers = notices
      .filter((n) => n.tone !== 'error')
      .map((n) => window.setTimeout(() => dismiss(n.id), 8000))
    return () => timers.forEach(clearTimeout)
  }, [notices, dismiss])
  if (!notices.length) return null
  return (
    <div className="notices" aria-live="polite">
      {notices.map((n) => (
        <div key={n.id} className={`tone ${n.tone} notice`} role={n.tone === 'error' ? 'alert' : 'status'}>
          <span className="notice-glyph">{GLYPH[n.tone]}</span>
          <span className="notice-text selectable">{bt(n.text)}</span>
          <button className="link notice-close" onClick={() => dismiss(n.id)} aria-label={t('common.close')}>
            ×
          </button>
        </div>
      ))}
    </div>
  )
}
