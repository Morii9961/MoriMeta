// SPDX-License-Identifier: GPL-3.0-or-later
// First Launch (SCREEN_SPEC §13): three dialog steps over the real empty app, with a progress
// indicator and Skip setup. 1 what MoriMeta does + language; 2 the safety model + backup location;
// 3 privacy + the update-check choice, with nothing chosen in advance (DECISIONS D-4).

import { useEffect, useState } from 'react'
import { api, errorText } from '../../ipc'
import { useApp } from '../../state/store'
import { useT } from '../../i18n'
import { Dialog } from '../../components/Dialog'

export function FirstLaunch() {
  const t = useT()
  const info = useApp((s) => s.info)
  const lang = useApp((s) => s.lang)
  const setLang = useApp((s) => s.setLang)
  const notify = useApp((s) => s.notify)
  const [show, setShow] = useState(false)
  const [step, setStep] = useState(1)
  const [updates, setUpdates] = useState<'weekly' | 'never' | null>(null)

  useEffect(() => {
    if (!info?.launched) return
    api
      .settingsList()
      .then((s) => setShow(s.find((x) => x.name === 'ui.setup_done')?.value !== 'true'))
      .catch(() => {})
  }, [info?.launched])

  if (!show || !info) return null
  const finish = async (choice: 'weekly' | 'never' | null) => {
    try {
      if (choice) await api.settingSet('updates.check', choice)
      await api.settingSet('ui.setup_done', 'true')
    } catch (e) {
      notify('error', errorText(e))
    }
    setShow(false)
  }
  const dots = (
    <span className="setup-dots" aria-label={t('setup.step', { n: step })}>
      {[1, 2, 3].map((n) => (
        <span key={n} className={n === step ? 'on' : ''}>
          ●
        </span>
      ))}
    </span>
  )
  return (
    <Dialog
      title={t(step === 1 ? 'setup.t1' : step === 2 ? 'setup.t2' : 'setup.t3')}
      onCancel={() => finish(null)}
      width={540}
      footer={
        <>
          {dots}
          <button className="link" onClick={() => finish(null)}>
            {t('setup.skip')}
          </button>
          <span className="note" />
          {step > 1 && (
            <button className="btn dlg" onClick={() => setStep(step - 1)}>
              {t('setup.back')}
            </button>
          )}
          {step < 3 ? (
            <button className="btn dlg primary" onClick={() => setStep(step + 1)}>
              {t('setup.next')}
            </button>
          ) : (
            <button className="btn dlg primary" disabled={updates === null} onClick={() => finish(updates)}>
              {t('setup.start')}
            </button>
          )}
        </>
      }
    >
      {step === 1 && (
        <>
          <p>{t('setup.what')}</p>
          <p className="note">✓ {t('empty.safe1')}</p>
          <p className="note">✓ {t('empty.safe2')}</p>
          <p className="note">✓ {t('empty.safe3')}</p>
          <div className="setup-row">
            <span className="secondary">{t('set.language')}</span>
            <div className="segmented small">
              <button aria-pressed={lang === 'en'} onClick={() => setLang('en')}>
                English
              </button>
              <button aria-pressed={lang === 'zh'} onClick={() => setLang('zh')}>
                简体中文
              </button>
            </div>
          </div>
        </>
      )}
      {step === 2 && (
        <>
          <p>{t('setup.safety')}</p>
          <p className="note">1 · {t('op.step_backup')} → 2 · {t('op.step_temp')} → 3 · {t('op.step_verify')} → 4 · {t('op.step_swap')}</p>
          <p className="note">{t('setup.undo')}</p>
          <div className="setup-row">
            <span className="secondary">{t('set.backup_location')}</span>
            <span className="mono ellipsis selectable" title={info.backup.root}>
              {info.backup.root}
            </span>
            <button
              className="btn small"
              onClick={() =>
                api
                  .chooseBackupFolder()
                  .then((b) => b && api.appInfo().then(useApp.getState().setInfo))
                  .catch((e) => notify('error', errorText(e)))
              }
            >
              {t('setup.change')}…
            </button>
          </div>
          {info.backup.problem && <div className="tone error">{info.backup.problem}</div>}
          {info.backup.sync_warning && <div className="tone warn">{info.backup.sync_warning}</div>}
          <p className="note">{t('setup.backup_note')}</p>
        </>
      )}
      {step === 3 && (
        <>
          <p>{t('set.local_only_help')}</p>
          <p className="note">{t('set.log_detail_help')}</p>
          <div className="section-label">{t('setup.updates')}</div>
          <label className="ack">
            <input type="radio" name="updates" checked={updates === 'weekly'} onChange={() => setUpdates('weekly')} />
            <span>{t('setup.updates_weekly')}</span>
          </label>
          <label className="ack">
            <input type="radio" name="updates" checked={updates === 'never'} onChange={() => setUpdates('never')} />
            <span>{t('setup.updates_never')}</span>
          </label>
          <p className="note">{t('setup.updates_note')}</p>
        </>
      )}
    </Dialog>
  )
}
