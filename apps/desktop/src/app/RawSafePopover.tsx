// SPDX-License-Identifier: GPL-3.0-or-later
// RAW Safe Mode popover (SCREEN_SPEC 5#r-on, 5#r-warn): where this session's edits go, who sees
// sidecar edits, what each format does, and the notices that concern RAW files. RAW Safe Mode is
// always on in 1.0 (DECISIONS H-3 / SAFETY_MODEL §3): NEF/NRW files are never written.

import { useEffect, useMemo, useRef, useState } from 'react'
import { api } from '../ipc'
import type { Attention } from '../ipc/types'
import { useApp } from '../state/store'
import { useT } from '../i18n'

export function RawSafePopover({ onClose }: { onClose: () => void }) {
  const t = useT()
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  const [att, setAtt] = useState<Attention | null>(null)
  const ref = useRef<HTMLDivElement>(null)

  const counts = useMemo(() => {
    const c = { in_file: 0, sidecar: 0, new_sidecar: 0, read_only: 0 }
    for (const a of assets) {
      if (!a.writable) c.read_only++
      else {
        const w = rows.get(a.id)?.writes_to
        if (w === 'in_file' || w === 'sidecar' || w === 'new_sidecar') c[w]++
      }
    }
    return c
  }, [assets, rows])

  useEffect(() => {
    const raws = assets.filter((a) => /^(NEF|NRW)$/i.test(a.ext)).map((a) => a.id)
    if (raws.length) api.attention(raws).then(setAtt).catch(() => setAtt(null))
  }, [assets])

  useEffect(() => {
    ref.current?.focus()
    const onKey = (e: KeyboardEvent) => e.key === 'Escape' && onClose()
    const onDown = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node) && !(e.target as HTMLElement).closest('.raw-safe')) onClose()
    }
    window.addEventListener('keydown', onKey)
    window.addEventListener('mousedown', onDown)
    return () => {
      window.removeEventListener('keydown', onKey)
      window.removeEventListener('mousedown', onDown)
    }
  }, [onClose])

  const notices = att
    ? ([
        ['darktable_sidecars', 'raw.n_darktable'],
        ['changed_since_import', 'raw.n_changed'],
        ['cloud_files', 'raw.n_cloud'],
      ] as const).filter(([k]) => att[k].length > 0)
    : []

  return (
    <div className="popover raw-popover" ref={ref} tabIndex={-1} role="dialog" aria-label={t('raw.title')}>
      <div className="popover-title">
        <span className="glyph-ok">●</span> {t('raw.title')} <span className="tag">LOCKED</span>
      </div>
      <p className="note">{t('raw.lead')}</p>
      <div className="section-label">{t('raw.session')}</div>
      <ul className="facts">
        <li>{t('raw.in_file', { n: counts.in_file })}</li>
        <li>{t('raw.sidecar', { n: counts.sidecar + counts.new_sidecar, created: counts.new_sidecar })}</li>
        <li>{t('raw.read_only', { n: counts.read_only })}</li>
      </ul>
      <div className="section-label">{t('raw.who')}</div>
      <p className="note">{t('raw.who_text')}</p>
      <div className="section-label">{t('raw.formats')}</div>
      <ul className="facts">
        <li>{t('raw.fmt_nef')}</li>
        <li>{t('raw.fmt_jpeg')}</li>
        <li>{t('raw.fmt_xmp')}</li>
        <li>{t('raw.fmt_other')}</li>
      </ul>
      {notices.length > 0 && (
        <>
          <div className="section-label">{t('raw.notices')}</div>
          <ul className="facts">
            {notices.map(([k, label]) => (
              <li key={k} className="glyph-warn">
                {t(label, { n: att![k].length })}
              </li>
            ))}
          </ul>
        </>
      )}
    </div>
  )
}
