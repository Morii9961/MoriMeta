// SPDX-License-Identifier: GPL-3.0-or-later
// Checks and downloads are explicit actions; installation has its own confirmation.
import { useEffect, useRef, useState } from 'react'
import { Dialog } from '../../components/Dialog'
import { api, errorText, isMock } from '../../ipc'
import type { UpdateInfo } from '../../ipc/types'
import { useBT, useT } from '../../i18n'
import { useApp } from '../../state/store'
import { sizeText } from '../library/data'

export function UpdatesPanel() {
  const t = useT()
  const bt = useBT()
  const applying = useApp((s) => s.stage.kind === 'applying' || s.cleanProgress !== null || s.scan.running)
  const [info, setInfo] = useState<UpdateInfo | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [working, setWorking] = useState(false)
  const [ask, setAsk] = useState(false)
  const revision = useRef(0)
  const mounted = useRef(false)
  useEffect(() => {
    mounted.current = true
    let timer: ReturnType<typeof setTimeout>
    let stopped = false
    const poll = async () => {
      const before = revision.current
      let delay = 5000
      try {
        const next = await api.updateStatus()
        if (!stopped && before === revision.current) setInfo(next)
        if (next.busy) delay = 500
      } catch (e) {
        if (!stopped && before === revision.current) setError(errorText(e))
      }
      if (!stopped) timer = setTimeout(poll, delay)
    }
    void poll()
    return () => { stopped = true; mounted.current = false; clearTimeout(timer) }
  }, [working])

  const run = async (phase: string, action: () => Promise<UpdateInfo | void>) => {
    ++revision.current
    setWorking(true)
    setError(null)
    setInfo((previous) => previous && { ...previous, busy: true, phase, ...(phase === 'downloading' ? { bytes: 0, total: null } : {}) })
    try {
      const next = await action()
      if (mounted.current && next) setInfo(next)
    } catch (e) {
      if (mounted.current) {
        setError(errorText(e))
        try { const next = await api.updateStatus(); if (mounted.current) setInfo(next) } catch { /* Keep the actionable command error. */ }
      }
    } finally {
      ++revision.current
      if (mounted.current) setWorking(false)
    }
  }
  const busy = working || info?.busy === true
  const offer = info?.offer
  return (
    <div className="set-update" aria-live="polite">
      {isMock && <p className="note">{t('update.simulated')}</p>}
      {!info ? <span className="muted">{t('common.loading')}</span> : !info.configured ? (
        <p className="note">{t('update.unavailable')}</p>
      ) : (
        <>
          <div className="set-inline">
            <button className="btn small" disabled={busy} onClick={() => void run('checking', () => api.updateCheck())}>{t('update.check')}</button>
            {busy && info.phase !== 'installing' && <button className="btn small" onClick={() => void api.updateCancel().catch((e) => setError(errorText(e)))}>{t('common.cancel')}</button>}
            {info.phase === 'checking' && <span className="muted">{t('update.checking')}</span>}
            {info.phase === 'current' && <span className="glyph-ok">{t('update.current')}</span>}
          </div>
          {offer && (
            <div className="set-update-offer">
              <strong>{t('update.available', { version: offer.version })}</strong>
              {offer.notes && <p className="set-release-notes selectable">{offer.notes}</p>}
              {info.phase === 'downloading' && (
                <div>
                  <progress aria-label={t('update.downloading')} value={info.total ? info.bytes : undefined} max={info.total ?? 1} />
                  <span className="mono"> {sizeText(info.bytes)}{info.total ? ` / ${sizeText(info.total)}` : ''}</span>
                </div>
              )}
              <div className="set-inline">
                {info.downloaded ? (
                  <button className="btn small" disabled={busy || applying} onClick={() => setAsk(true)}>{t('update.install')}</button>
                ) : (
                  <button className="btn small" disabled={busy} onClick={() => void run('downloading', () => api.updateDownload(offer.id))}>{t('update.download')}</button>
                )}
                {info.downloaded && <span className="glyph-ok">{t('update.verified')}</span>}
              </div>
              {info.downloaded && applying && <p className="note">{t('update.wait')}</p>}
              {info.phase === 'simulated_install' && <p className="note">{t('update.simulated_done')}</p>}
            </div>
          )}
        </>
      )}
      {(error || info?.error) && <p className="tone error" role="alert">{bt(error ?? info?.error)}</p>}
      {ask && offer && (
        <Dialog title={t('update.confirm', { version: offer.version })} onCancel={() => setAsk(false)} footer={
          <>
            <button className="btn" onClick={() => setAsk(false)}>{t('common.cancel')}</button>
            <button className="btn primary" disabled={busy || applying} onClick={() => { setAsk(false); void run('installing', async () => { await api.updateInstall(offer.id); return api.updateStatus() }) }}>{t('update.install')}</button>
          </>
        }>
          <p>{t('update.install_note')}</p>
        </Dialog>
      )}
    </div>
  )
}
