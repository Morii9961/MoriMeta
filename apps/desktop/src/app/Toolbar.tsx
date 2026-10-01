// SPDX-License-Identifier: GPL-3.0-or-later
// Toolbar (38 px, DESIGN_SYSTEM Toolbar): module segmented control · text tools · search ·
// Preview button (grey outline with nothing staged, accent outline when a plan is staged).

import { useRef } from 'react'
import { useApp, stagedCount, isLocked, type Module } from '../state/store'
import { useT, type MessageKey } from '../i18n'
import { addFiles, addFolder, openPreview, previewBlocker } from './actions'

const MODULES: { key: Module; label: MessageKey }[] = [
  { key: 'library', label: 'module.library' },
  { key: 'presets', label: 'module.presets' },
  { key: 'rules', label: 'module.rules' },
  { key: 'history', label: 'module.history' },
]

export function Toolbar() {
  const t = useT()
  const module = useApp((s) => s.module)
  const settingsOpen = useApp((s) => s.settingsOpen)
  const setModule = useApp((s) => s.setModule)
  const stage = useApp((s) => s.stage)
  const staged = useApp((s) => s.staged)
  const selection = useApp((s) => s.selection)
  const search = useApp((s) => s.search)
  const setSearch = useApp((s) => s.setSearch)
  const setTimeToolsOpen = useApp((s) => s.setTimeToolsOpen)
  const addRef = useRef<HTMLDetailsElement>(null)
  const locked = isLocked(stage) || stage.kind !== 'library'
  const n = stagedCount(staged)
  const blocker = previewBlocker()
  void selection // the Preview button reflects the selection too

  return (
    <div className="toolbar" role="toolbar">
      <div className="segmented" role="tablist" aria-label={t('module.label')}>
        {MODULES.map((m) => (
          <button
            key={m.key}
            role="tab"
            aria-pressed={!settingsOpen && module === m.key}
            aria-selected={!settingsOpen && module === m.key}
            disabled={isLocked(stage)}
            onClick={() => setModule(m.key)}
          >
            {t(m.label)}
          </button>
        ))}
      </div>
      <div className="toolbar-divider" />
      <details className="dropdown" ref={addRef}>
        <summary className={`btn plain${locked ? ' disabled' : ''}`} aria-disabled={locked}>
          {t('toolbar.add')} ▾
        </summary>
        {!locked && (
          <div className="menu-popup" role="menu">
            <button role="menuitem" onClick={() => { addRef.current?.removeAttribute('open'); addFiles() }}>
              <span className="menu-check" />
              <span className="menu-label">{t('menu.add_files')}</span>
              <span className="menu-keys mono">Ctrl O</span>
            </button>
            <button role="menuitem" onClick={() => { addRef.current?.removeAttribute('open'); addFolder() }}>
              <span className="menu-check" />
              <span className="menu-label">{t('menu.add_folder')}</span>
              <span className="menu-keys mono">Ctrl ⇧ O</span>
            </button>
          </div>
        )}
      </details>
      <button className="btn plain" disabled={locked || selection.size === 0} onClick={() => setTimeToolsOpen(true)}>
        {t('toolbar.time_tools')}
      </button>
      <button className="btn plain" disabled={locked} onClick={() => setModule('presets')}>
        {t('toolbar.apply_preset')} ▾
      </button>
      <button
        className="btn plain"
        disabled={locked || selection.size === 0}
        title={selection.size === 0 ? t('preview.need_selection') : t('toolbar.clean_export_tip')}
        onClick={() => useApp.getState().setStage({ kind: 'clean' })}
      >
        {t('toolbar.clean_export')}
      </button>
      {isLocked(stage) && <span className="toolbar-locked">{t('toolbar.locked')}</span>}
      <div className="toolbar-spacer" />
      <input
        className="input ui toolbar-search"
        type="search"
        placeholder={t('toolbar.search')}
        value={search}
        disabled={module !== 'library' || settingsOpen}
        onChange={(e) => setSearch(e.target.value)}
        aria-label={t('toolbar.search')}
        id="global-search"
      />
      <button
        className={`btn${n > 0 && !blocker && !locked ? ' accent' : ''}`}
        disabled={!!blocker || locked}
        title={blocker ?? undefined}
        onClick={() => openPreview()}
      >
        {t('toolbar.preview')}
        {n > 0 && <span className="mono">· {n}</span>}
        <span className="mono faint">Ctrl ↵</span>
      </button>
    </div>
  )
}
