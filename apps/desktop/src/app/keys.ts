// SPDX-License-Identifier: GPL-3.0-or-later
// Global keyboard shortcuts (INTERACTION_SPEC §18 Global).

import { useEffect } from 'react'
import { useApp, isLocked } from '../state/store'
import { addFiles, addFolder, openPreview } from './actions'

function typing(e: KeyboardEvent): boolean {
  const el = e.target as HTMLElement | null
  if (!el) return false
  return el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.tagName === 'SELECT' || el.isContentEditable
}

export function useGlobalKeys() {
  useEffect(() => {
    const on = (e: KeyboardEvent) => {
      const s = useApp.getState()
      const free = s.stage.kind === 'library' && !isLocked(s.stage)
      const ctrl = e.ctrlKey || e.metaKey
      if (document.querySelector('.dialog-scrim')) return // a dialog owns the keyboard
      if (ctrl && e.key.toLowerCase() === 'o' && free) {
        e.preventDefault()
        if (e.shiftKey) addFolder()
        else addFiles()
      } else if (ctrl && e.key === ',') {
        e.preventDefault()
        s.setSettingsOpen(true)
      } else if (ctrl && e.key.toLowerCase() === 'b') {
        e.preventDefault()
        s.toggleSidebar()
      } else if (ctrl && e.key.toLowerCase() === 'f') {
        e.preventDefault()
        document.getElementById('global-search')?.focus()
      } else if (ctrl && e.key === 'Enter' && s.stage.kind === 'library') {
        e.preventDefault()
        openPreview()
      } else if (!ctrl && !e.altKey && !typing(e) && free) {
        if (e.key === 'i' || e.key === 'I') s.toggleInspector()
        else if ((e.key === 't' || e.key === 'T') && s.selection.size > 0) s.setTimeToolsOpen(true)
      } else if (e.key === 'F5' || (ctrl && e.key.toLowerCase() === 'r')) {
        e.preventDefault() // no page reload inside the app
      }
    }
    window.addEventListener('keydown', on)
    return () => window.removeEventListener('keydown', on)
  }, [])
}
