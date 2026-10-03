// SPDX-License-Identifier: GPL-3.0-or-later
// Cleanup always previews the exact operations and confirms loss of their undo backups.
import { useEffect, useRef, useState } from 'react'
import { Dialog } from '../../components/Dialog'
import { api, errorText, isMock } from '../../ipc'
import type { BackupUsage, OperationBackup, PrunePreview } from '../../ipc/types'
import { useBT, useT } from '../../i18n'
import { useApp } from '../../state/store'
import { sizeText } from '../library/data'

export function BackupsPanel() {
  const t = useT()
  const bt = useBT()
  const lang = useApp((s) => s.lang)
  const [usage, setUsage] = useState<BackupUsage | null>(null)
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [preview, setPreview] = useState<PrunePreview | null>(null)
  const [ack, setAck] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [done, setDone] = useState<number | null>(null)
  const mounted = useRef(false)

  const reload = async () => {
    const [next, info] = await Promise.all([api.backupUsage(), api.appInfo()])
    if (!mounted.current) return
    setUsage(next)
    useApp.getState().setInfo(info)
    const eligible = new Set(next.ops.filter((o) => !o.pruned && o.protection !== 'unfinished').map((o) => o.op_id))
    setSelected((old) => new Set([...old].filter((id) => eligible.has(id))))
  }
  const run = async (action: () => Promise<void>) => {
    setBusy(true)
    setError(null)
    setDone(null)
    try { await action() } catch (e) { if (mounted.current) setError(errorText(e)) }
    finally { if (mounted.current) setBusy(false) }
  }
  useEffect(() => {
    mounted.current = true
    void run(reload)
    return () => { mounted.current = false }
  }, [])
  const protection = (o: OperationBackup) => o.pruned ? t('backup.pruned') : o.protection === 'unfinished' ? t('backup.unfinished') : o.keep ? t('backup.kept') : o.protection === 'recent' ? t('backup.recent') : t('backup.eligible')
  const prepare = (ids: string[] | null) => run(async () => {
    const next = await api.prunePreview(ids)
    if (mounted.current) { setAck(false); setPreview(next) }
  })
  return (
    <section className="set-backups" aria-label={t('backup.manage')}>
      <h2 className="section-label">{t('backup.manage')}</h2>
      {isMock && <p className="note">{t('backup.simulated')}</p>}
      <p className="note">{t('backup.help')}</p>
      <div className="set-inline">
        <button className="btn small" disabled={busy} onClick={() => void run(reload)}>{t('backup.refresh')}</button>
        <button className="btn small" disabled={busy || !usage?.ops.some((o) => !o.pruned)} onClick={() => void prepare(null)}>{t('backup.policy')}</button>
        <button className="btn small" disabled={busy || !selected.size} onClick={() => void prepare([...selected])}>{t('backup.selected', { n: selected.size })}</button>
      </div>
      {usage?.sync_warning && <p className="tone warn">{bt(usage.sync_warning)}</p>}
      {error && <p role="alert" className="tone error">{bt(error)}</p>}
      {done !== null && <p role="status" className="glyph-ok">{t('backup.done', { n: done })}</p>}
      {!usage ? <p className="muted">{t('common.loading')}</p> : !usage.ops.length ? <p className="muted">{t('backup.empty')}</p> : (
        <div className="set-backup-scroll">
          <table className="set-backup-table">
            <thead><tr><th>{t('backup.select')}</th><th>{t('backup.operation')}</th><th>{t('backup.size')}</th><th>{t('backup.protection')}</th><th>{t('backup.retention')}</th></tr></thead>
            <tbody>{usage.ops.map((o) => (
              <tr key={o.op_id}>
                <td><input type="checkbox" aria-label={t('backup.select_op', { title: o.title })} checked={selected.has(o.op_id)} disabled={busy || o.pruned || o.protection === 'unfinished'} onChange={(e) => {
                  const checked = e.target.checked
                  setSelected((old) => { const next = new Set(old); if (checked) next.add(o.op_id); else next.delete(o.op_id); return next })
                }} /></td>
                <td><div>{o.title}</div><div className="faint mono">{new Date(o.created_ms).toLocaleDateString(lang === 'zh' ? 'zh-CN' : 'en-US')}</div></td>
                <td className="mono">{sizeText(o.bytes)}</td>
                <td>{protection(o)}</td>
                <td><button className="btn small" aria-pressed={o.keep} disabled={busy || o.pruned} onClick={() => void run(async () => { await api.backupKeep(o.op_id, !o.keep); await reload() })}>{o.keep ? t('backup.release') : t('backup.keep')}</button></td>
              </tr>
            ))}</tbody>
          </table>
        </div>
      )}
      {preview && (
        <Dialog title={t('backup.confirm', { n: preview.operations.length, size: sizeText(preview.bytes) })} glyph="!" onCancel={() => { if (!busy) setPreview(null) }} width={640} footer={
          <>
            <button className="btn" disabled={busy} onClick={() => setPreview(null)}>{t('common.cancel')}</button>
            <button className="btn primary" disabled={busy || !ack} onClick={() => void run(async () => {
              try {
                const removed = await api.pruneExecute(preview.token)
                if (mounted.current) { setSelected(new Set()); setDone(removed.length) }
              } finally { if (mounted.current) setPreview(null) }
              await reload()
            })}>{t('backup.cleanup')}</button>
          </>
        }>
          <p>{t('backup.consequence')}</p>
          {preview.operations.some((o) => o.keep || o.protection === 'recent') && <p className="tone warn">{t('backup.override')}</p>}
          <ul className="set-prune-list">{preview.operations.map((o) => <li key={o.op_id}>{o.title} <span className="mono muted">{sizeText(o.bytes)}</span></li>)}</ul>
          <label className="check-row"><input type="checkbox" checked={ack} disabled={busy} onChange={(e) => setAck(e.target.checked)} />{t('backup.ack')}</label>
          {error && <p role="alert" className="tone error">{bt(error)}</p>}
        </Dialog>
      )}
    </section>
  )
}
