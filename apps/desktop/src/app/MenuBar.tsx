// SPDX-License-Identifier: GPL-3.0-or-later
// MenuBar (28 px, DESIGN.md §3): File · Edit · View · Tools · Help.

import { useEffect, useRef, useState } from 'react'
import { useApp, isLocked } from '../state/store'
import { useT, type MessageKey } from '../i18n'
import { addFiles, addFolder, clearSession } from './actions'

interface Item {
  label: MessageKey
  keys?: string
  run: () => void
  disabled?: boolean
  checked?: boolean
}

type Menu = { label: MessageKey; items: (Item | 'sep')[] }

export function MenuBar() {
  const t = useT()
  const [open, setOpen] = useState<number | null>(null)
  const ref = useRef<HTMLDivElement>(null)
  const stage = useApp((s) => s.stage)
  const lang = useApp((s) => s.lang)
  const sidebarOpen = useApp((s) => s.sidebarOpen)
  const inspectorOpen = useApp((s) => s.inspectorOpen)
  const assets = useApp((s) => s.assets)
  const locked = isLocked(stage) || stage.kind !== 'library'
  const s = useApp.getState

  const menus: Menu[] = [
    {
      label: 'menu.file',
      items: [
        { label: 'menu.add_files', keys: 'Ctrl O', run: addFiles, disabled: locked },
        { label: 'menu.add_folder', keys: 'Ctrl ⇧ O', run: addFolder, disabled: locked },
        'sep',
        { label: 'menu.clear_session', run: clearSession, disabled: locked || assets.length === 0 },
      ],
    },
    {
      label: 'menu.edit',
      items: [
        {
          label: 'menu.select_all',
          keys: 'Ctrl A',
          run: () => s().select(s().assets.map((a) => a.id)),
          disabled: locked,
        },
        { label: 'menu.select_none', run: () => s().select([], null), disabled: locked },
        'sep',
        { label: 'menu.discard_staged', run: () => s().discardStaged(), disabled: locked },
      ],
    },
    {
      label: 'menu.view',
      items: [
        { label: 'menu.sidebar', keys: 'Ctrl B', run: () => s().toggleSidebar(), checked: sidebarOpen },
        { label: 'menu.inspector', keys: 'I', run: () => s().toggleInspector(), checked: inspectorOpen },
        'sep',
        { label: 'menu.lang_en', run: () => s().setLang('en'), checked: lang === 'en' },
        { label: 'menu.lang_zh', run: () => s().setLang('zh'), checked: lang === 'zh' },
      ],
    },
    {
      label: 'menu.tools',
      items: [
        { label: 'menu.time_tools', keys: 'T', run: () => s().setTimeToolsOpen(true), disabled: locked },
        { label: 'menu.history', run: () => s().setModule('history') },
        'sep',
        { label: 'menu.settings', keys: 'Ctrl ,', run: () => s().setSettingsOpen(true) },
      ],
    },
    {
      label: 'menu.help',
      items: [{ label: 'menu.about', run: () => useApp.setState({ aboutOpen: true }) }],
    },
  ]

  useEffect(() => {
    if (open === null) return
    const close = (e: MouseEvent) => {
      if (!ref.current?.contains(e.target as Node)) setOpen(null)
    }
    const esc = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setOpen(null)
    }
    window.addEventListener('mousedown', close)
    window.addEventListener('keydown', esc)
    return () => {
      window.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', esc)
    }
  }, [open])

  return (
    <div className="menubar" ref={ref} role="menubar">
      {menus.map((m, i) => (
        <div key={m.label} className="menubar-item">
          <button
            role="menuitem"
            aria-haspopup="true"
            aria-expanded={open === i}
            className={open === i ? 'open' : ''}
            onMouseDown={(e) => {
              e.preventDefault()
              setOpen(open === i ? null : i)
            }}
            onMouseEnter={() => open !== null && setOpen(i)}
            onKeyDown={(e) => {
              if (e.key === 'Enter' || e.key === ' ' || e.key === 'ArrowDown') {
                e.preventDefault()
                setOpen(i)
              }
            }}
          >
            {t(m.label)}
          </button>
          {open === i && (
            <div className="menu-popup" role="menu">
              {m.items.map((it, k) =>
                it === 'sep' ? (
                  <div key={k} className="menu-sep" />
                ) : (
                  <button
                    key={it.label}
                    role="menuitem"
                    disabled={it.disabled}
                    autoFocus={k === 0}
                    onClick={() => {
                      setOpen(null)
                      it.run()
                    }}
                    onKeyDown={(e) => {
                      const items = [...(e.currentTarget.parentElement?.querySelectorAll('button:not(:disabled)') ?? [])] as HTMLElement[]
                      const at = items.indexOf(e.currentTarget)
                      if (e.key === 'ArrowDown') items[(at + 1) % items.length]?.focus()
                      if (e.key === 'ArrowUp') items[(at - 1 + items.length) % items.length]?.focus()
                    }}
                  >
                    <span className="menu-check">{it.checked ? '✓' : ''}</span>
                    <span className="menu-label">{t(it.label)}</span>
                    <span className="menu-keys mono">{it.keys ?? ''}</span>
                  </button>
                ),
              )}
            </div>
          )}
        </div>
      ))}
      <div className="menubar-title ellipsis">{t('app.title')}</div>
    </div>
  )
}
