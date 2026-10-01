// SPDX-License-Identifier: GPL-3.0-or-later
import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '@fontsource/ibm-plex-sans/400.css'
import '@fontsource/ibm-plex-sans/500.css'
import '@fontsource/ibm-plex-sans/600.css'
import '@fontsource/ibm-plex-mono/400.css'
import '@fontsource/ibm-plex-mono/500.css'
import '@fontsource/ibm-plex-mono/600.css'
import './design/tokens.css'
import './design/base.css'
import './app/shell.css'
import { App } from './app/App'
import { useApp } from './state/store'

document.documentElement.lang = useApp.getState().lang === 'zh' ? 'zh-CN' : 'en'

// no browser context menu or reload shortcuts: this is an app window
window.addEventListener('contextmenu', (e) => {
  const t = e.target as HTMLElement
  if (!t.closest('input, textarea, .selectable')) e.preventDefault()
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
