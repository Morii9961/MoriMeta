// SPDX-License-Identifier: GPL-3.0-or-later
// Dialog frame (DESIGN_SYSTEM ConfirmDialog): scrim, 36 px title with glyph and question, body,
// footer. Esc always cancels; focus stays inside while it is open; never two at a time.

import { useEffect, useRef, type ReactNode } from 'react'

export function Dialog({
  title,
  glyph,
  children,
  footer,
  onCancel,
  width,
}: {
  title: string
  glyph?: string
  children: ReactNode
  footer: ReactNode
  onCancel: () => void
  width?: number
}) {
  const ref = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const prev = document.activeElement as HTMLElement | null
    const first = ref.current?.querySelector<HTMLElement>('input, button:not(:disabled)')
    first?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.preventDefault()
        e.stopPropagation()
        onCancel()
      } else if (e.key === 'Tab' && ref.current) {
        const f = [...ref.current.querySelectorAll<HTMLElement>('input, button:not(:disabled), select, textarea')]
        if (!f.length) return
        const i = f.indexOf(document.activeElement as HTMLElement)
        if (e.shiftKey && i <= 0) {
          e.preventDefault()
          f[f.length - 1].focus()
        } else if (!e.shiftKey && i === f.length - 1) {
          e.preventDefault()
          f[0].focus()
        }
      }
    }
    window.addEventListener('keydown', onKey, true)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      prev?.focus()
    }
  }, [onCancel])
  return (
    <div className="dialog-scrim" role="presentation">
      <div className="dialog" role="dialog" aria-modal="true" aria-label={title} ref={ref} style={width ? { width } : undefined}>
        <div className="dialog-title">
          {glyph && <span className="glyph-warn">{glyph}</span>}
          <span>{title}</span>
        </div>
        <div className="dialog-body">{children}</div>
        <div className="dialog-footer">{footer}</div>
      </div>
    </div>
  )
}
