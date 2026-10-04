// SPDX-License-Identifier: GPL-3.0-or-later
// Column chooser (SCREEN_SPEC 1#columns): show or hide columns, move them, the locked file name,
// widths, and saved layouts. Layouts are a per-viewer convenience kept in this browser storage.

import { useEffect, useRef, useState } from 'react'
import { useApp } from '../../state/store'
import { useT } from '../../i18n'
import { COLUMNS } from './data'
import { DEFAULT_LAYOUT, load, moveColumn, normalizeLayout, save, type Layout, type Saved } from './layout'

const SAVED_KEY = 'mm.layouts'

export function ColumnChooser({ onClose }: { onClose: () => void }) {
  const t = useT()
  const layout = useApp((s) => s.layout)
  const setLayout = useApp((s) => s.setLayout)
  const [saved, setSaved] = useState<Saved<Layout>[]>(() => load<Saved<Layout>[]>(SAVED_KEY, []).filter((s) => s && typeof s.name === 'string'))
  const [name, setName] = useState('')
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    ref.current?.querySelector<HTMLElement>('input, button')?.focus()
    const onKey = (e: KeyboardEvent) => {
      if (e.key === 'Escape') {
        e.stopPropagation()
        onClose()
      }
    }
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node) && !(e.target as HTMLElement).closest('.columns-button')) onClose()
    }
    window.addEventListener('keydown', onKey, true)
    window.addEventListener('mousedown', onDown)
    return () => {
      window.removeEventListener('keydown', onKey, true)
      window.removeEventListener('mousedown', onDown)
    }
  }, [onClose])

  const keep = (next: Saved<Layout>[]) => {
    setSaved(next)
    save(SAVED_KEY, next)
  }
  const toggle = (key: string) =>
    setLayout({ ...layout, hidden: layout.hidden.includes(key) ? layout.hidden.filter((k) => k !== key) : [...layout.hidden, key] })

  return (
    <div className="popover column-chooser" ref={ref} role="dialog" aria-label={t('cols.title')}>
      <div className="popover-title">{t('cols.title')}</div>
      <ul className="col-list">
        {layout.order.map((key, i) => {
          const col = COLUMNS.find((c) => c.key === key)!
          const locked = key === 'name'
          const label = t(col.label)
          return (
            <li key={key} className="col-item">
              <label>
                <input type="checkbox" className="checkbox" checked={!layout.hidden.includes(key)} disabled={locked} onChange={() => toggle(key)} />
                <span className="ellipsis">{label}</span>
                {locked && <span className="tag">{t('cols.locked')}</span>}
              </label>
              <span className="mono faint">{layout.widths[key] ?? col.width}px</span>
              <button className="btn plain tiny" disabled={locked || i <= 1} aria-label={t('cols.move_left', { name: label })} onClick={() => setLayout(moveColumn(layout, key, -1))}>
                ↑
              </button>
              <button className="btn plain tiny" disabled={locked || i === layout.order.length - 1} aria-label={t('cols.move_right', { name: label })} onClick={() => setLayout(moveColumn(layout, key, 1))}>
                ↓
              </button>
            </li>
          )
        })}
      </ul>
      <p className="note">{t('cols.resize_hint')}</p>
      <div className="popover-row">
        <button className="btn small" onClick={() => setLayout(normalizeLayout(DEFAULT_LAYOUT))}>
          {t('cols.reset')}
        </button>
      </div>
      <div className="section-label">{t('cols.saved')}</div>
      {saved.length === 0 && <p className="note">{t('cols.none_saved')}</p>}
      {saved.map((s) => (
        <div key={s.name} className="popover-row saved-row">
          <span className="ellipsis">{s.name}</span>
          <button className="btn small" onClick={() => setLayout(normalizeLayout(s.value))}>
            {t('cols.use')}
          </button>
          <button className="btn small plain" aria-label={t('cols.delete', { name: s.name })} onClick={() => keep(saved.filter((x) => x.name !== s.name))}>
            ×
          </button>
        </div>
      ))}
      <form
        className="popover-row"
        onSubmit={(e) => {
          e.preventDefault()
          const n = name.trim()
          if (!n) return
          keep([...saved.filter((x) => x.name !== n), { name: n, value: layout }])
          setName('')
        }}
      >
        <input className="input ui" value={name} maxLength={40} placeholder={t('cols.name')} aria-label={t('cols.name')} onChange={(e) => setName(e.target.value)} />
        <button className="btn small" type="submit" disabled={!name.trim()}>
          {t('cols.save')}
        </button>
      </form>
    </div>
  )
}
