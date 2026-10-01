// SPDX-License-Identifier: GPL-3.0-or-later
// Session summary (SCREEN_SPEC 1#default): where edits are written, Needs attention (each with
// an action), recent operations.

import { useEffect, useMemo, useState } from 'react'
import { api } from '../../ipc'
import type { Attention, OpSummary } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, type MessageKey } from '../../i18n'

const ATTENTION: { key: keyof Attention; label: MessageKey; tone: 'warn' | 'error' | 'info' | 'neutral' }[] = [
  { key: 'read_only', label: 'att.read_only', tone: 'warn' },
  { key: 'conflicts', label: 'att.conflicts', tone: 'warn' },
  { key: 'changed_since_import', label: 'att.changed', tone: 'warn' },
  { key: 'cloud_placeholders', label: 'att.cloud_placeholders', tone: 'info' },
  { key: 'cloud_files', label: 'att.cloud_files', tone: 'warn' },
  { key: 'darktable_sidecars', label: 'att.darktable', tone: 'neutral' },
  { key: 'c2pa', label: 'att.c2pa', tone: 'warn' },
  { key: 'links', label: 'att.links', tone: 'warn' },
  { key: 'unreadable', label: 'att.unreadable', tone: 'error' },
  { key: 'removable', label: 'att.removable', tone: 'warn' },
  { key: 'other_file_system', label: 'att.other_fs', tone: 'warn' },
  { key: 'network', label: 'att.network', tone: 'info' },
  { key: 'long_paths', label: 'att.long_paths', tone: 'neutral' },
]

export function SessionSummary() {
  const t = useT()
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  const scan = useApp((s) => s.scan)
  const select = useApp((s) => s.select)
  const setModule = useApp((s) => s.setModule)
  const [att, setAtt] = useState<Attention | null>(null)
  const [recent, setRecent] = useState<OpSummary[]>([])

  const targets = useMemo(() => {
    const c = { in_file: 0, sidecar: 0, new_sidecar: 0, read_only: 0 }
    for (const a of assets) {
      if (!a.writable) c.read_only++
      else {
        const r = rows.get(a.id)
        if (r) c[r.writes_to]++
      }
    }
    return c
  }, [assets, rows])

  // "Needs attention" reads every file once more: only when the Session is fully read
  useEffect(() => {
    if (scan.running || assets.length === 0) {
      setAtt(null)
      return
    }
    let alive = true
    api
      .attention(assets.map((a) => a.id))
      .then((a) => alive && setAtt(a))
      .catch(() => {})
    return () => {
      alive = false
    }
  }, [assets, scan.running])

  useEffect(() => {
    api
      .historyList(0)
      .then((h) => setRecent(h.slice(0, 5)))
      .catch(() => {})
  }, [])

  return (
    <div className="pane-scroll insp">
      <div className="insp-section">
        <div className="section-label">{t('summary.title')}</div>
        <div className="summary-lead">
          <span className="mono strong count-big">{assets.length}</span> <span>{t('summary.files_in_session')}</span>
        </div>
      </div>
      <div className="insp-section">
        <div className="section-label">{t('summary.where')}</div>
        <div className="kv">
          <span>{t('writes.in_file')}</span>
          <span className="mono">{targets.in_file}</span>
          <span>{t('writes.sidecar')}</span>
          <span className="mono">{targets.sidecar}</span>
          <span>{t('writes.new_sidecar')}</span>
          <span className="mono">{targets.new_sidecar}</span>
          <span>{t('writes.read_only')}</span>
          <span className="mono faint">{targets.read_only}</span>
        </div>
        <p className="note">{t('summary.where_note')}</p>
      </div>
      <div className="insp-section">
        <div className="section-label">{t('summary.attention')}</div>
        {assets.length === 0 ? (
          <p className="note">{t('summary.nothing_yet')}</p>
        ) : !att ? (
          <p className="note">{t('summary.checking')}</p>
        ) : (
          (() => {
            const present = ATTENTION.filter((a) => att[a.key].length > 0)
            if (!present.length) return <p className="note glyph-ok">✓ {t('summary.no_attention')}</p>
            return present.map((a) => (
              <div key={a.key} className="att-row">
                <span className={`glyph-${a.tone === 'error' ? 'fail' : a.tone === 'warn' ? 'warn' : a.tone === 'info' ? 'info' : 'skip'}`}>
                  {a.tone === 'error' ? '×' : a.tone === 'warn' ? '!' : 'i'}
                </span>
                <span>{t(a.label)}</span>
                <span className="mono">{att[a.key].length}</span>
                <button className="link" onClick={() => select(att[a.key], att[a.key][0], att[a.key][0])}>
                  {t('common.select')}
                </button>
              </div>
            ))
          })()
        )}
      </div>
      <div className="insp-section">
        <div className="section-label">{t('summary.recent')}</div>
        {recent.length === 0 ? (
          <p className="note">{t('summary.no_ops')}</p>
        ) : (
          recent.map((o) => (
            <div key={o.id} className="recent-row" title={o.id}>
              <span className={o.status === 'completed' ? 'glyph-ok' : 'glyph-warn'}>{o.status === 'completed' ? '✓' : '!'}</span>
              <span className="ellipsis">{o.title}</span>
              <span className="mono faint">{new Date(o.created_ms).toLocaleTimeString()}</span>
            </div>
          ))
        )}
        <button className="link" onClick={() => setModule('history')}>
          {t('summary.open_history')}
        </button>
      </div>
    </div>
  )
}
