// SPDX-License-Identifier: GPL-3.0-or-later
// Help › About MoriMeta (RELEASE_PLAN §6, §7.2): the versions a bug report needs, copyable, the
// license and the third-party notices shipped with the build.

import { useEffect, useState } from 'react'
import { api, errorText } from '../ipc'
import type { About } from '../ipc/types'
import { useApp } from '../state/store'
import { useBT, useT } from '../i18n'
import { Dialog } from '../components/Dialog'

export function AboutDialog() {
  const t = useT()
  const bt = useBT()
  const open = useApp((s) => s.aboutOpen)
  const notify = useApp((s) => s.notify)
  const [about, setAbout] = useState<About | null>(null)
  const [notices, setNotices] = useState<string | null>(null)
  const [showNotices, setShowNotices] = useState(false)
  const [error, setError] = useState<string | null>(null)

  useEffect(() => {
    if (!open) return
    setError(null)
    api.about().then(setAbout).catch((e) => setError(errorText(e)))
  }, [open])

  if (!open) return null
  const close = () => {
    setShowNotices(false)
    useApp.setState({ aboutOpen: false })
  }
  const unknown = t('about.unknown')
  const rows: [string, string][] = about
    ? [
        [t('about.version'), `${about.version}${about.dev ? ` (${t('about.dev_build')})` : ''}`],
        [t('about.exiftool'), about.exiftool ?? unknown],
        [t('about.registry'), String(about.registry_version)],
        [t('about.webview'), about.webview2 ?? unknown],
        [t('about.system'), about.os],
      ]
    : []
  const copy = async () => {
    const text = [`MoriMeta ${about?.version ?? ''}`, ...rows.slice(1).map(([k, v]) => `${k}: ${v}`)].join('\n')
    try {
      await navigator.clipboard.writeText(text)
      notify('success', t('about.copied'))
    } catch (e) {
      notify('error', errorText(e))
    }
  }
  const toggleNotices = async () => {
    if (showNotices) {
      setShowNotices(false)
      return
    }
    setShowNotices(true)
    if (notices === null) {
      try {
        setNotices(await api.thirdPartyNotices())
      } catch (e) {
        setNotices('')
        setError(errorText(e))
      }
    }
  }

  return (
    <Dialog
      title={t('about.title')}
      onCancel={close}
      width={showNotices ? 760 : 480}
      footer={
        <>
          <button className="btn" onClick={toggleNotices}>{showNotices ? t('about.hide_notices') : t('about.notices')}</button>
          <button className="btn" disabled={!about} onClick={copy}>{t('about.copy')}</button>
          <button className="btn primary" onClick={close}>{t('common.close')}</button>
        </>
      }
    >
      <p>{t('about.tagline')}</p>
      {about && (
        <dl className="about-facts">
          {rows.map(([k, v]) => (
            <div key={k}>
              <dt className="muted">{k}</dt>
              <dd className="mono">{v}</dd>
            </div>
          ))}
        </dl>
      )}
      <p className="note">{t('about.license')}</p>
      <p className="note">{t('about.source')}</p>
      {error && <p className="tone error" role="alert">{bt(error)}</p>}
      {showNotices && notices && <pre className="about-notices" tabIndex={0}>{notices}</pre>}
    </Dialog>
  )
}
