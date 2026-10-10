// SPDX-License-Identifier: GPL-3.0-or-later
// A context menu at a point (the table header's, a preset row's): closes on a click outside, on
// Escape and after a choice; ↑/↓ move between items, the first enabled item takes the focus.

import { useEffect } from 'react'

export interface MenuItem {
  text: string
  run: () => void
  disabled?: boolean
  checked?: boolean
  keys?: string
}

export function PopupMenu({ x, y, items, onClose, className = '' }: { x: number; y: number; items: MenuItem[]; onClose: () => void; className?: string }) {
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    window.addEventListener('mousedown', onClose)
    window.addEventListener('keydown', onKey)
    return () => {
      window.removeEventListener('mousedown', onClose)
      window.removeEventListener('keydown', onKey)
    }
  }, [onClose])
  const first = items.findIndex((m) => !m.disabled)
  return (
    <div className={`menu-popup ${className}`} role="menu" style={{ position: 'fixed', left: x, top: y }} onMouseDown={(e) => e.stopPropagation()}>
      {items.map((m, k) => (
        <button
          key={m.text}
          role="menuitem"
          disabled={m.disabled}
          autoFocus={k === first}
          onClick={() => {
            onClose()
            m.run()
          }}
          onKeyDown={(e) => {
            const all = [...(e.currentTarget.parentElement?.querySelectorAll('button:not(:disabled)') ?? [])] as HTMLElement[]
            const at = all.indexOf(e.currentTarget)
            if (e.key === 'ArrowDown') all[(at + 1) % all.length]?.focus()
            if (e.key === 'ArrowUp') all[(at - 1 + all.length) % all.length]?.focus()
          }}
        >
          <span className="menu-check">{m.checked ? '✓' : ''}</span>
          <span className="menu-label">{m.text}</span>
          <span className="menu-keys">{m.keys ?? ''}</span>
        </button>
      ))}
    </div>
  )
}
