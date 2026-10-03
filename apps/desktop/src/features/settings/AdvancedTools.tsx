// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from 'react'
import { Dialog } from '../../components/Dialog'
import { api, errorText } from '../../ipc'
import { useBT, useT } from '../../i18n'
import { useApp } from '../../state/store'

export function AdvancedTools({ onReset }: { onReset: () => Promise<unknown> }) {
  const t = useT()
  const bt = useBT()
  const lang = useApp((s) => s.lang)
  const [migrations, setMigrations] = useState<[number, string][] | null>(null)
  const [asking, setAsking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [reset, setReset] = useState(false)
  useEffect(() => {
    let alive = true
    api.settingsMigrations().then((rows) => { if (alive) setMigrations(rows) }).catch((e) => { if (alive) setError(errorText(e)) })
    return () => { alive = false }
  }, [])
  return (
    <section className="set-backups" aria-label={t('set.maintenance')}>
      <h2 className="section-label">{t('set.migrations')}</h2>
      {!migrations ? <p className="note">{t('common.loading')}</p> : !migrations.length ? <p className="note">{t('set.no_migrations')}</p> : (
        <ul className="set-prune-list">{migrations.map(([time, description], index) => <li key={`${time}-${index}`}><span className="mono muted">{new Date(time).toLocaleString(lang === 'zh' ? 'zh-CN' : 'en-US')}</span> {description}</li>)}</ul>
      )}
      <button className="btn small" disabled={busy} onClick={() => { setError(null); setAsking(true) }}>{t('set.reset')}</button>
      {reset && <p className="glyph-ok" role="status">{t('set.reset_done')}</p>}
      {error && <p className="tone error" role="alert">{bt(error)}</p>}
      {asking && <Dialog title={t('set.reset_question')} onCancel={() => { if (!busy) setAsking(false) }} footer={
        <>
          <button className="btn" disabled={busy} onClick={() => setAsking(false)}>{t('common.cancel')}</button>
          <button className="btn primary" disabled={busy} onClick={async () => {
            setBusy(true)
            try { await api.settingsReset(); await onReset(); setReset(true); setAsking(false) }
            catch (e) { setError(errorText(e)) }
            finally { setBusy(false) }
          }}>{t('set.reset')}</button>
        </>
      }><p>{t('set.reset_note')}</p>{error && <p className="tone error">{bt(error)}</p>}</Dialog>}
    </section>
  )
}
