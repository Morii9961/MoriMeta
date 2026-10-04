// SPDX-License-Identifier: GPL-3.0-or-later
// AppShell (DESIGN.md §3, DESIGN_SYSTEM AppShell): menu bar, toolbar, optional banner, sidebar |
// primary pane | inspector, optional action bar, status bar.

import { useEffect } from 'react'
import { api, errorText } from '../ipc'
import type { AppEvent } from '../ipc/types'
import { useApp } from '../state/store'
import { useT } from '../i18n'
import { MenuBar } from './MenuBar'
import { Toolbar } from './Toolbar'
import { StatusBar } from './StatusBar'
import { Banner } from './Banner'
import { Notices } from './Notices'
import { LibraryView } from '../features/library/LibraryView'
import { PreviewView } from '../features/preview/PreviewView'
import { OperationView } from '../features/jobs/OperationView'
import { HistoryView } from '../features/history/HistoryView'
import { SettingsView } from '../features/settings/SettingsView'
import { PresetsView } from '../features/presets/PresetsView'
import { RulesView } from '../features/presets/RulesView'
import { CleanExportView } from '../features/clean/CleanExportView'
import { RecoveryDialog } from '../features/recovery/RecoveryDialog'
import { useGlobalKeys } from './keys'
import { CloseDialog } from './CloseDialog'
import { FirstLaunch } from '../features/launch/FirstLaunch'
import { AboutDialog } from './AboutDialog'

function onEvent(e: AppEvent) {
  const s = useApp.getState()
  switch (e.kind) {
    case 'status':
      api.appInfo().then(s.setInfo).catch(() => {})
      break
    case 'imported':
      s.addAssets(e.assets, e.summary)
      if (e.assets.length) s.setScan({ running: true, done: 0, total: e.assets.length })
      break
    case 'rows':
      s.putRows(e.rows)
      break
    case 'scan_progress':
      s.setScan({ done: e.done, total: e.total })
      break
    case 'scan_done':
      s.setScan({ running: false })
      if (e.error) s.notify('error', e.error)
      break
    case 'plan_progress':
      if (s.stage.kind === 'planning') s.setStage({ kind: 'planning', done: e.done, total: e.total, stage: e.stage })
      break
    case 'close_blocked':
      useApp.setState({ closeAsked: true })
      break
    case 'exec_progress': {
      const st = s.stage
      if (st.kind === 'clean') useApp.setState({ cleanProgress: { done: e.done, total: e.total } })
      if (st.kind === 'applying') {
        const { kind: _k, ...p } = e
        s.setStage({ ...st, progress: p })
      }
      break
    }
  }
}

export function App() {
  const t = useT()
  const info = useApp((s) => s.info)
  const module = useApp((s) => s.module)
  const settingsOpen = useApp((s) => s.settingsOpen)
  const stage = useApp((s) => s.stage)
  const notify = useApp((s) => s.notify)
  useGlobalKeys()

  useEffect(() => {
    let alive = true
    api
      .subscribe(onEvent)
      .then(() => api.appInfo())
      .then((i) => alive && useApp.getState().setInfo(i))
      .catch((e) => notify('error', errorText(e)))
    // Settings › General › Scale is the window's zoom; restore it at launch
    const scale = useApp.getState().prefs.scale
    if (scale !== 100) api.uiZoom(scale).catch(() => {})
    return () => {
      alive = false
    }
  }, [notify])

  // an Operation or Plan in progress takes the primary pane, whatever module was chosen
  const operation = stage.kind === 'applying' || stage.kind === 'done'
  const preview = stage.kind === 'preview'
  let body
  if (settingsOpen) body = <SettingsView />
  else if (operation) body = <OperationView />
  else if (preview) body = <PreviewView />
  else if (stage.kind === 'clean') body = <CleanExportView />
  else if (module === 'history') body = <HistoryView />
  else if (module === 'presets') body = <PresetsView />
  else if (module === 'rules') body = <RulesView />
  else body = <LibraryView />

  return (
    <div className="shell">
      <MenuBar />
      <Toolbar />
      <Banner />
      <div className="shell-body">{info ? body : <div className="shell-loading muted">{t('app.starting')}</div>}</div>
      <StatusBar />
      <Notices />
      {info && <RecoveryDialog />}
      <FirstLaunch />
      <CloseDialog />
      <AboutDialog />
    </div>
  )
}
