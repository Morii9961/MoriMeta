// SPDX-License-Identifier: GPL-3.0-or-later
// WarningBanner (DESIGN_SYSTEM): one at a time, the most severe wins; session- or operation-wide
// only. Kinds: preview · applying · paused · blocking (red) · attention (amber).

import { useApp } from '../state/store'
import { useT } from '../i18n'
import { backToEdit, cancelPlanning } from './actions'

export function Banner() {
  const t = useT()
  const info = useApp((s) => s.info)
  const stage = useApp((s) => s.stage)

  if (stage.kind === 'planning') {
    return (
      <div className="banner b-preview" role="status">
        <span className="banner-glyph">●</span>
        <b>{t('banner.planning')}</b>
        <span className="mono">
          {stage.total > 0 ? `${Math.min(stage.done, stage.total)} / ${stage.total}` : ''}
        </span>
        <span className="muted">{t('banner.nothing_written')}</span>
        <div className="banner-spacer" />
        <button className="btn small" onClick={cancelPlanning}>
          {t('common.cancel')}
        </button>
      </div>
    )
  }
  if (stage.kind === 'preview') {
    const p = stage.plan
    return (
      <div className="banner b-preview" role="status">
        <span className="banner-glyph tag accent-tag">{t('banner.preview')}</span>
        <span className="mono">
          {p.title} · v{p.version}
        </span>
        <b>{t('banner.nothing_written_disk')}</b>
        <div className="banner-spacer" />
        <button className="btn small" onClick={backToEdit}>
          {t('preview.back_to_edit')} <span className="mono faint">Esc</span>
        </button>
      </div>
    )
  }
  if (stage.kind === 'applying') {
    return (
      <div className="banner b-preview" role="status">
        <span className="banner-glyph tag accent-tag">{t('banner.applying')}</span>
        <span className="mono">{stage.plan.title}</span>
        <span className="muted">{t('banner.applying_note')}</span>
      </div>
    )
  }
  if (!info) return null
  const blocking =
    info.startup.backup_problem ??
    (info.exiftool.error ? t('banner.exiftool_down') : null) ??
    (info.startup.elevated ? t('banner.elevated') : null)
  if (blocking) {
    return (
      <div className="banner b-blocking" role="alert">
        <span className="banner-glyph">×</span>
        <b>{t('banner.writes_blocked')}</b>
        <span className="ellipsis">{blocking}</span>
        <span className="muted">{t('banner.files_safe')}</span>
      </div>
    )
  }
  if (info.startup.recovery_waiting.length > 0) {
    return (
      <div className="banner b-paused" role="alert">
        <span className="banner-glyph">‖</span>
        <b>{t('banner.recovery_waiting', { n: info.startup.recovery_waiting.length })}</b>
        <span className="ellipsis">{info.startup.recovery_waiting[0][1]}</span>
      </div>
    )
  }
  if (info.exiftool.integrity) {
    return (
      <div className="banner b-blocking" role="alert">
        <span className="banner-glyph">×</span>
        <b>{t('banner.integrity')}</b>
        <span className="ellipsis">{info.exiftool.integrity}</span>
      </div>
    )
  }
  return null
}
