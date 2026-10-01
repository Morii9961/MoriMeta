// SPDX-License-Identifier: GPL-3.0-or-later
// StatusBar (24 px, permanent; DESIGN.md §3): RAW safety · backup · undo · scan · ExifTool.

import { useApp } from '../state/store'
import { useT } from '../i18n'
import { sizeText } from '../features/library/data'

export function StatusBar() {
  const t = useT()
  const info = useApp((s) => s.info)
  const scan = useApp((s) => s.scan)
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  if (!info) return <div className="statusbar" />
  const b = info.backup
  const et = info.exiftool
  const backupBad = b.problem !== null
  const refused = info.writes_refused
  return (
    <div className="statusbar" role="status">
      <div className="status-seg raw-safe" title={t('status.raw_safe_tip')}>
        <span className="glyph-ok">●</span> {t('status.raw_safe')}
        <span className="faint"> · NEF → XMP sidecar</span>
      </div>
      <div className={`status-seg${backupBad ? ' bad' : ''}`} title={b.problem ?? b.root}>
        {backupBad ? (
          <>
            <span className="glyph-fail">×</span> {t('status.backup_unavailable')}
          </>
        ) : (
          <>
            <span className="secondary">{t('status.backup')}</span>
            <span className="mono ellipsis status-path">{b.root}</span>
            {b.free !== null && <span className="mono faint">{t('status.free', { size: sizeText(b.free) })}</span>}
          </>
        )}
      </div>
      <div className="status-seg" title={t('status.undo_tip')}>
        <span className="secondary">{t('status.undo')}</span>
        <span className="mono">{t('status.ops_backed_up', { n: b.operations })}</span>
      </div>
      <div className="status-spacer" />
      {refused && (
        <div className="status-seg bad" title={refused}>
          <span className="glyph-fail">×</span> {t('status.writes_off')}
        </div>
      )}
      <div className="status-seg">
        {scan.running ? (
          <span className="mono">
            {t('status.reading', { done: Math.min(scan.done, scan.total), total: scan.total })}
          </span>
        ) : (
          <span className="mono secondary">
            {t('status.files', { n: assets.length })}
            {assets.length > 0 && rows.size < assets.length ? ` · ${t('status.unread', { n: assets.length - rows.size })}` : ''}
          </span>
        )}
      </div>
      <div className={`status-seg${et.error || et.integrity ? ' bad' : ''}`} title={et.error ?? et.integrity ?? et.package ?? ''}>
        {et.starting ? (
          <span className="muted">{t('status.exiftool_starting')}</span>
        ) : et.error ? (
          <>
            <span className="glyph-fail">×</span> {t('status.exiftool_unavailable')}
          </>
        ) : et.integrity ? (
          <>
            <span className="glyph-fail">×</span> {t('status.exiftool_integrity')}
          </>
        ) : (
          <>
            <span className="glyph-ok">✓</span> <span className="mono">ExifTool {et.version}</span>
          </>
        )}
      </div>
    </div>
  )
}
