// SPDX-License-Identifier: GPL-3.0-or-later
// Settings (SCREEN_SPEC §12): left nav, form rows `210 label | control + help`, LOCKED / 1.x
// tags. Values go through the backend's settings registry, which checks each before keeping it.

import { useEffect, useState } from 'react'
import { api, errorText } from '../../ipc'
import type { Setting } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, type MessageKey } from '../../i18n'
import { sizeText } from '../library/data'
import './settings.css'
import { UpdatesPanel } from './UpdatesPanel'
import { BackupsPanel } from './BackupsPanel'
import { AdvancedTools } from './AdvancedTools'
import { SCALES } from '../../app/uiPrefs'
import { diagnostics } from './diagnostics'

type Page = 'general' | 'metadata' | 'raw' | 'backup' | 'privacy' | 'updates' | 'advanced'
const PAGES: { key: Page; label: MessageKey }[] = [
  { key: 'general', label: 'set.general' },
  { key: 'metadata', label: 'set.metadata' },
  { key: 'raw', label: 'set.raw' },
  { key: 'backup', label: 'set.backup' },
  { key: 'privacy', label: 'set.privacy' },
  { key: 'updates', label: 'set.updates' },
  { key: 'advanced', label: 'set.advanced' },
]

function Row({ label, help, children, tag }: { label: string; help?: string; children: React.ReactNode; tag?: string }) {
  return (
    <div className="set-row">
      <div className="set-label">
        {label} {tag && <span className="tag">{tag}</span>}
      </div>
      <div className="set-control">
        {children}
        {help && <div className="note">{help}</div>}
      </div>
    </div>
  )
}

function TextSetting({ s, onSaved, mono = true }: { s: Setting | undefined; onSaved: () => void; mono?: boolean }) {
  const t = useT()
  const notify = useApp((x) => x.notify)
  const [v, setV] = useState(s?.value ?? '')
  const [err, setErr] = useState<string | null>(null)
  useEffect(() => setV(s?.value ?? ''), [s?.value])
  if (!s) return null
  const dirty = v !== s.value
  return (
    <div className="set-inline">
      <input className={`input${mono ? '' : ' ui'}${err ? ' invalid' : ''}`} value={v} placeholder={s.default} onChange={(e) => { setV(e.target.value); setErr(null) }} />
      <button
        className="btn small"
        disabled={!dirty}
        onClick={() =>
          api
            .settingSet(s.name, v)
            .then(() => {
              notify('success', t('set.saved'))
              onSaved()
            })
            .catch((e) => setErr(errorText(e)))
        }
      >
        {t('set.save')}
      </button>
      {err && <div className="field-note glyph-fail">{err}</div>}
    </div>
  )
}

export function SettingsView() {
  const t = useT()
  const lang = useApp((s) => s.lang)
  const setLang = useApp((s) => s.setLang)
  const info = useApp((s) => s.info)
  const close = useApp((s) => s.setSettingsOpen)
  const notify = useApp((s) => s.notify)
  const [page, setPage] = useState<Page>('general')
  const [settings, setSettings] = useState<Setting[]>([])
  const load = () =>
    api
      .settingsList()
      .then(setSettings)
      .catch((e) => notify('error', errorText(e)))
  useEffect(() => {
    load()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape' && !document.querySelector('.dialog-scrim')) close(false)
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  const get = (n: string) => settings.find((s) => s.name === n)
  const prefs = useApp((s) => s.prefs)
  const setPrefs = useApp((s) => s.setPrefs)
  const copy = (text: string) =>
    navigator.clipboard
      .writeText(text)
      .then(() => notify('success', t('insp.copied')))
      .catch((e) => notify('error', errorText(e)))
  const refreshInfo = () => api.appInfo().then(useApp.getState().setInfo)

  return (
    <>
      <nav className="pane-sidebar" aria-label={t('menu.settings')}>
        <div className="side-group-header">
          <span className="section-label">{t('menu.settings')}</span>
          <button className="link side-clear" onClick={() => close(false)}>
            {t('common.close')} <span className="mono faint">Esc</span>
          </button>
        </div>
        {PAGES.map((p) => (
          <button key={p.key} className={`kind-row${page === p.key ? ' on' : ''}`} onClick={() => setPage(p.key)}>
            <span />
            <span>{t(p.label)}</span>
          </button>
        ))}
      </nav>
      <div className="pane-primary settings">
        <div className="pane-scroll set-page">
          <h1 className="set-title">{t(PAGES.find((p) => p.key === page)!.label)}</h1>
          {page === 'general' && (
            <>
              <Row label={t('set.language')} help={t('set.language_help')}>
                <div className="segmented small">
                  <button aria-pressed={lang === 'en'} onClick={() => setLang('en')}>
                    English
                  </button>
                  <button aria-pressed={lang === 'zh'} onClick={() => setLang('zh')}>
                    简体中文
                  </button>
                </div>
              </Row>
              <Row label={t('set.theme')} tag="LOCKED" help={t('set.theme_help')}>
                <span className="muted">{t('set.dark')}</span>
              </Row>
              <Row label={t('set.startup')} tag="LOCKED" help={t('set.startup_help')}>
                <span className="muted">{t('set.startup_value')}</span>
              </Row>
              <Row label={t('set.density')} help={t('set.density_help')}>
                <div className="segmented small">
                  {(['compact', 'comfortable'] as const).map((d) => (
                    <button key={d} aria-pressed={prefs.density === d} onClick={() => setPrefs({ ...prefs, density: d })}>
                      {t(`set.density_${d}` as MessageKey)}
                    </button>
                  ))}
                </div>
              </Row>
              <Row label={t('set.scale')} help={t('set.scale_help')}>
                <div className="segmented small">
                  {SCALES.map((sc) => (
                    <button
                      key={sc}
                      aria-pressed={prefs.scale === sc}
                      onClick={() =>
                        api
                          .uiZoom(sc)
                          .then(() => setPrefs({ ...prefs, scale: sc }))
                          .catch((e) => notify('error', errorText(e)))
                      }
                    >
                      {sc}%
                    </button>
                  ))}
                </div>
              </Row>
              <Row label={t('set.close_during')} tag="LOCKED" help={t('set.close_during_help')}>
                <span className="muted">{t('set.always_ask')}</span>
              </Row>
            </>
          )}
          {page === 'metadata' && (
            <>
              <Row label={t('set.default_creator')} help={t('set.default_creator_help')}>
                <TextSetting s={get('metadata.default_creator')} onSaved={load} />
              </Row>
              <Row label={t('set.copyright_template')} help={t('set.copyright_template_help')}>
                <TextSetting s={get('metadata.copyright_template')} onSaved={load} />
              </Row>
              <Row label={t('set.mtime')} help={t('set.mtime_help')}>
                <div className="segmented small">
                  {(['false', 'true'] as const).map((v) => (
                    <button
                      key={v}
                      aria-pressed={(get('metadata.preserve_mtime')?.value ?? 'false') === v}
                      onClick={() => api.settingSet('metadata.preserve_mtime', v).then(load).catch((e) => notify('error', errorText(e)))}
                    >
                      {v === 'false' ? t('set.mtime_update') : t('set.mtime_keep')}
                    </button>
                  ))}
                </div>
              </Row>
              <Row label={t('set.time_display')} tag="LOCKED" help={t('set.time_display_help')}>
                <span className="muted">{t('set.time_display_value')}</span>
              </Row>
              <Row label="ExifTool" help={info?.exiftool.package ?? ''}>
                <span className="mono">{info?.exiftool.version ?? '—'}</span>
                {info?.exiftool.integrity && <div className="tone error">{info.exiftool.integrity}</div>}
              </Row>
              <Row label={t('set.exiftool_path')} tag="1.x" help={t('set.exiftool_path_help')}>
                <span className="muted">{t('set.bundled')}</span>
              </Row>
            </>
          )}
          {page === 'raw' && (
            <>
              <Row label={t('set.raw_safe')} tag="LOCKED" help={t('set.raw_safe_help')}>
                <span className="glyph-ok">● {t('set.on')}</span>
              </Row>
              <Row label={t('set.sidecar_naming')} help={t('set.sidecar_naming_help')}>
                <span className="mono">IMG_0001.NEF → IMG_0001.xmp</span>
              </Row>
              <Row label={t('set.darktable')} help={t('set.darktable_help')}>
                <span className="muted">{t('set.read_only')}</span>
              </Row>
              <Row label={t('set.direct_nef')} tag="1.x" help={t('set.direct_nef_help')}>
                <span className="muted">{t('set.off')}</span>
              </Row>
            </>
          )}
          {page === 'backup' && (
            <>
              <Row label={t('set.backup_location')} help={t('set.backup_location_help')}>
                <div className="set-inline">
                  <span className="mono ellipsis selectable">{info?.backup.root}</span>
                  <button
                    className="btn small"
                    onClick={() =>
                      api
                        .chooseBackupFolder()
                        .then((b) => {
                          if (b) {
                            load()
                            refreshInfo()
                          }
                        })
                        .catch((e) => notify('error', errorText(e)))
                    }
                  >
                    {t('setup.change')}…
                  </button>
                </div>
                {info?.backup.problem && <div className="tone error">{info.backup.problem}</div>}
                {info?.backup.sync_warning && <div className="tone warn">{info.backup.sync_warning}</div>}
              </Row>
              <Row label={t('set.retention')} help={t('set.retention_help')}>
                <TextSetting s={get('backup.max_age_days')} onSaved={load} />
              </Row>
              <Row label={t('set.size_limit')} help={t('set.size_limit_help')}>
                <TextSetting s={get('backup.max_share_of_volume')} onSaved={load} />
              </Row>
              <Row label={t('set.keep_latest')} help={t('set.keep_latest_help')}>
                <TextSetting s={get('backup.keep_latest')} onSaved={load} />
              </Row>
              <Row label={t('set.usage')}>
                <span className="mono">
                  {sizeText(info?.backup.bytes ?? 0)} · {t('status.ops_backed_up', { n: info?.backup.operations ?? 0 })}
                  {info?.backup.free !== null && info?.backup.free !== undefined ? ` · ${t('status.free', { size: sizeText(info.backup.free) })}` : ''}
                </span>
              </Row>
              <Row label={t('set.backups_off')} tag="LOCKED" help={t('set.backups_off_help')}>
                <span className="muted">{t('set.cannot_switch_off')}</span>
              </Row>
              <BackupsPanel />
            </>
          )}
          {page === 'privacy' && (
            <>
              <Row label={t('set.local_only')} help={t('set.local_only_help')}>
                <span className="glyph-ok">✓ {t('set.local_only_value')}</span>
              </Row>
              <Row label={t('set.log_detail')} help={t('set.log_detail_help')}>
                <span className="muted">{t('set.log_anonymised')}</span>
              </Row>
              <Row label={t('set.data_folder')}>
                <span className="mono selectable">{info?.data_dir}</span>
              </Row>
              <Row label={t('set.log_folder')} help={t('set.log_folder_help')}>
                <div className="set-inline">
                  <span className="mono selectable">{info ? `${info.data_dir}\\logs` : ''}</span>
                  <button className="btn small" disabled={!info} onClick={() => info && copy(`${info.data_dir}\\logs`)}>
                    {t('set.copy_path')}
                  </button>
                </div>
              </Row>
              <Row label={t('set.network')} help={t('set.network_help')}>
                <span className="muted">{get('updates.check')?.value === 'weekly' ? t('set.network_weekly') : t('set.network_none')}</span>
              </Row>
            </>
          )}
          {page === 'updates' && (
            <Row label={t('set.update_check')} help={t('set.update_help')}>
              <div className="segmented small">
                {(['weekly', 'never'] as const).map((v) => (
                  <button
                    key={v}
                    aria-pressed={get('updates.check')?.value === v}
                    onClick={() => api.settingSet('updates.check', v).then(load).catch((e) => notify('error', errorText(e)))}
                  >
                    {v === 'weekly' ? t('setup.updates_weekly') : t('setup.updates_never')}
                  </button>
                ))}
              </div>
              <UpdatesPanel />
            </Row>
          )}
          {page === 'updates' && (
            <>
              <Row label={t('set.channel')} tag="LOCKED" help={t('set.channel_help')}>
                <span className="muted">{t('set.channel_stable')}</span>
              </Row>
              <Row label={t('set.check_sends')}>
                <span className="note">{t('set.check_sends_value')}</span>
              </Row>
            </>
          )}
          {page === 'advanced' && (
            <>
              <Row label={t('set.workers')} help={t('set.workers_help')}>
                <TextSetting s={get('exec.workers')} onSaved={load} />
              </Row>
              <Row label={t('set.verify')} tag="LOCKED" help={t('set.verify_help')}>
                <span className="glyph-ok">✓ {t('set.always')}</span>
              </Row>
              <Row label={t('set.debug_log')} help={t('set.debug_log_help')}>
                <div className="set-inline">
                  <span className="mono">{get('log.debug_since_ms')?.value ? t('set.on') : t('set.off')}</span>
                  <button
                    className="btn small"
                    onClick={() =>
                      api
                        .settingSet('log.debug_since_ms', get('log.debug_since_ms')?.value ? '' : String(Date.now()))
                        .then(load)
                        .catch((e) => notify('error', errorText(e)))
                    }
                  >
                    {get('log.debug_since_ms')?.value ? t('set.turn_off') : t('set.turn_on')}
                  </button>
                </div>
              </Row>
              <Row label={t('set.app_version')}>
                <span className="mono">
                  MoriMeta {info?.version}
                  {info?.dev ? ' (development build)' : ''}
                </span>
              </Row>
              <Row label={t('set.diagnostics')} help={t('set.diagnostics_help')}>
                <button
                  className="btn small"
                  onClick={() =>
                    api
                      .about()
                      .then((a) => copy(diagnostics(a, useApp.getState().info, settings)))
                      .catch((e) => notify('error', errorText(e)))
                  }
                >
                  {t('set.copy_diagnostics')}
                </button>
              </Row>
              <AdvancedTools onReset={() => Promise.all([load(), refreshInfo()])} />
            </>
          )}
        </div>
      </div>
    </>
  )
}
