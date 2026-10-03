// SPDX-License-Identifier: GPL-3.0-or-later
import { useEffect, useState } from 'react'
import { Dialog } from '../../components/Dialog'
import { showPreview } from '../../app/actions'
import { api, errorText } from '../../ipc'
import type { PlanEntry, PlanView } from '../../ipc/types'
import { useBT, useT } from '../../i18n'

/** File choices refer to this exact immutable plan, including its conflict fingerprints. */
export function UndoDialog({ opId, onClose }: { opId: string; onClose: () => void }) {
  const t = useT()
  const bt = useBT()
  const [plan, setPlan] = useState<PlanView | null>(null)
  const [entries, setEntries] = useState<PlanEntry[]>([])
  const [chosen, setChosen] = useState<Set<number>>(new Set())
  const [ack, setAck] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  useEffect(() => {
    let alive = true
    ;(async () => {
      const p = await api.undoPlan(opId)
      const all: PlanEntry[] = []
      for (let page = 0; ; page++) {
        const result = await api.planPage(p.id, p.version, 'all', page)
        all.push(...result.entries)
        if (all.length >= result.matching || !result.entries.length) break
      }
      if (alive) {
        setPlan(p); setEntries(all)
        setChosen(new Set(all.filter((e) => e.status.status === 'ready' && !e.excluded).map((e) => e.seq)))
      }
    })().catch((e) => { if (alive) setError(errorText(e)) })
    return () => { alive = false }
  }, [opId])
  const forced = entries.filter((e) => chosen.has(e.seq) && e.forced_undo).length
  const preview = async () => {
    if (!plan || busy || !chosen.size || (forced > 0 && !ack)) return
    setBusy(true); setError(null)
    try {
      let next = plan
      const included = entries.filter((e) => chosen.has(e.seq)).map((e) => e.seq)
      const excluded = entries.filter((e) => !chosen.has(e.seq)).map((e) => e.seq)
      if (included.length) next = await api.planExclude(next.id, next.version, included, false)
      // Remember the new version if the second request fails; retry cannot reuse stale state.
      setPlan(next)
      if (excluded.length) next = await api.planExclude(next.id, next.version, excluded, true)
      showPreview(next, 'undo'); onClose()
    } catch (e) { setError(errorText(e)) }
    finally { setBusy(false) }
  }
  return <Dialog title={t('history.choose_undo')} onCancel={() => { if (!busy) onClose() }} footer={<>
    <button className="btn" disabled={busy} onClick={onClose}>{t('common.cancel')}</button>
    <button className="btn primary" disabled={!plan || busy || !chosen.size || (forced > 0 && !ack)} onClick={preview}>{t('history.preview_undo', { n: chosen.size })}</button>
  </>}>
    <p>{t('history.undo_choices_note')}</p>
    {!plan && !error && <p className="note">{t('common.loading')}</p>}
    <ul className="undo-choices">{entries.map((e) => <li key={e.seq}>
      <label><input type="checkbox" disabled={busy || e.status.status !== 'ready'} checked={chosen.has(e.seq)} onChange={(event) => {
        setAck(false)
        setChosen((old) => { const next = new Set(old); if (event.target.checked) next.add(e.seq); else next.delete(e.seq); return next })
      }} /><span className="mono selectable" title={e.path}>{e.name}</span></label>
      {e.forced_undo ? <p className="tone warning">{t('history.undo_conflict')}</p> : e.status.status === 'blocked' || e.status.status === 'unsupported' ? <p className="note">{bt(e.status.reason)}</p> : e.status.status === 'no_change' ? <p className="note">{t('kind.no_change')}</p> : null}
    </li>)}</ul>
    {forced > 0 && <label className="checkline"><input type="checkbox" disabled={busy} checked={ack} onChange={(e) => setAck(e.target.checked)} />{t('history.undo_force_ack', { n: forced })}</label>}
    {error && <p className="tone error" role="alert">{bt(error)}</p>}
  </Dialog>
}
