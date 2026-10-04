// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from 'react'
import { Dialog } from '../../components/Dialog'
import { api, errorText } from '../../ipc'
import { useBT, useT } from '../../i18n'
import { useApp } from '../../state/store'
import type { HistoryImport } from '../../ipc/types'

export function AdvancedTools({ onReset }: { onReset: () => Promise<unknown> }) {
  const t = useT()
  const bt = useBT()
  const lang = useApp((s) => s.lang)
  const [migrations, setMigrations] = useState<[number, string][] | null>(null)
  const [asking, setAsking] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [reset, setReset] = useState(false)
  const [finding, setFinding] = useState(false)
  const [found, setFound] = useState<HistoryImport | null>(null)
  const findHistory = async () => {
    setError(null)
    setFinding(true)
    try {
      const r = await api.historyImport()
      if (r) setFound(r)
    } catch (e) {
      setError(errorText(e))
    } finally {
      setFinding(false)
    }
  }
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
      <h2 className="section-label">{t('set.find_history_title')}</h2>
      <p className="note">{t('set.find_history_note')}</p>
      <button className="btn small" disabled={finding} onClick={findHistory}>{t('set.find_history')}</button>
      {found && (
        <div role="status">
          <p className={found.imported.length ? 'glyph-ok' : 'note'}>
            {found.imported.length
              ? t('set.find_history_found', { n: found.imported.length, folder: found.location })
              : t('set.find_history_none', { folder: found.location })}
          </p>
          {found.recovered_files > 0 && <p className="note">{t('set.find_history_recovered', { n: found.recovered_files })}</p>}
          {found.skipped.length > 0 && (
            <>
              <p className="tone warn">{t('set.find_history_skipped', { n: found.skipped.length })}</p>
              <ul className="set-prune-list">{found.skipped.map(([id, why]) => <li key={id}><span className="mono">{id}</span> {bt(why)}</li>)}</ul>
            </>
          )}
        </div>
      )}
      <h2 className="section-label">{t('set.maintenance')}</h2>
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
