// SPDX-License-Identifier: GPL-3.0-or-later
// Actions shared by menus, toolbar, panels and keyboard shortcuts. Every write goes Plan →
// Preview → confirmation → Operation (INTERACTION_SPEC §3); nothing here writes on its own.

import { api, errorText } from '../ipc'
import type { PlanView } from '../ipc/types'
import { stagedCount, useApp } from '../state/store'
import { translate } from '../i18n'

const st = () => useApp.getState()
const tr = (k: Parameters<typeof translate>[1], p?: Record<string, string | number>) => translate(st().lang, k, p)

export function addFiles() {
  api.importDialog('files').catch((e) => st().notify('error', errorText(e)))
}

export function addFolder() {
  api.importDialog('folder').catch((e) => st().notify('error', errorText(e)))
}

export function clearSession() {
  if (st().stage.kind !== 'library') return
  api
    .sessionClear()
    .then(() => st().clearSession())
    .catch((e) => st().notify('error', errorText(e)))
}

/** Whether Preview can be opened, and why not (INTERACTION_SPEC §3 step 1). */
export function previewBlocker(): string | null {
  const s = st()
  if (stagedCount(s.staged) === 0) return tr('preview.need_staged')
  if (s.selection.size === 0) return tr('preview.need_selection')
  return null
}

/** Ctrl ↵ from editing: build `Plan N v1` for the selection and open Preview in place. */
export async function openPreview() {
  const s = st()
  if (previewBlocker() || s.stage.kind !== 'library') return
  const ids = s.assets.filter((a) => s.selection.has(a.id)).map((a) => a.id)
  s.setStage({ kind: 'planning', done: 0, total: ids.length, stage: 'files' })
  try {
    const plan = await api.planBatch(ids, { ...s.staged, title: editTitle() })
    st().setStage({ kind: 'preview', plan, origin: 'edit' })
  } catch (e) {
    st().setStage({ kind: 'library' })
    st().notify('error', errorText(e))
  }
}

/** A short title for History, from what is staged. */
function editTitle(): string {
  const e = st().staged
  const parts: string[] = []
  if (e.creator) parts.push(e.creator.op === 'set' ? tr('title.set_creator') : tr('title.clear_creator'))
  if (e.copyright) parts.push(e.copyright.op === 'set' ? tr('title.set_copyright') : tr('title.clear_copyright'))
  if (e.gps) parts.push(e.gps.op === 'set' ? tr('title.set_gps') : tr('title.remove_gps'))
  if (e.time) parts.push(tr(`title.time_${e.time.mode}` as Parameters<typeof translate>[1]))
  return parts.join(' · ')
}

export function cancelPlanning() {
  api.planCancel().catch(() => {})
}

/** Esc in Preview: back to the staging surface with the edits intact. */
export function backToEdit() {
  const s = st()
  if (s.stage.kind !== 'preview') return
  s.setStage({ kind: 'library' })
  if (s.stage.origin !== 'edit') s.setModule('history')
}

/** Discard plan: clears what was staged as well. */
export function discardPlan() {
  const s = st()
  const origin = s.stage.kind === 'preview' ? s.stage.origin : 'edit'
  s.setStage({ kind: 'library' })
  if (origin === 'edit') s.discardStaged()
  else s.setModule('history')
}

export function showPreview(plan: PlanView, origin: 'edit' | 'undo' | 'retry') {
  st().setStage({ kind: 'preview', plan, origin })
}

/** Confirmed in Preview (with the acknowledgements the dialog collected): run it. */
export async function apply(plan: PlanView, acks: string[]) {
  const s = st()
  let token: string
  try {
    token = await api.planConfirm(plan.id, plan.version, acks)
  } catch (e) {
    s.notify('error', errorText(e))
    return
  }
  const started = Date.now()
  s.setStage({ kind: 'applying', plan, progress: null, started, cancelling: false })
  try {
    const report = await api.opExecute(plan.id, plan.version, token)
    st().setStage({ kind: 'done', plan, report, started, finished: Date.now() })
    if (plan.kind !== 'undo' && st().stage.kind === 'done') st().discardStaged()
    refreshAfterOperation(report.files.map((f) => f.path))
  } catch (e) {
    st().setStage({ kind: 'preview', plan, origin: 'edit' })
    st().notify('error', errorText(e))
  }
  if (st().closeAfter) api.appClose().catch(() => {})
  api.appInfo().then(st().setInfo).catch(() => {})
}

/** The rows of the files an Operation touched are read again. */
function refreshAfterOperation(paths: string[]) {
  const s = st()
  const wanted = new Set(paths.map((p) => p.toLowerCase()))
  const ids = s.assets
    .filter((a) => {
      const p = a.path.toLowerCase()
      return wanted.has(p) || wanted.has(p.replace(/\.[^.\\]+$/, '.xmp'))
    })
    .map((a) => a.id)
  if (ids.length) api.rescan(ids).catch(() => {})
}

/** Files changed while the Preview was open: the same edit is planned again from a fresh read. */
export async function planAgain(origin: 'edit' | 'undo' | 'retry', plan: PlanView) {
  if (origin === 'edit' && plan.kind !== 'undo') {
    st().setStage({ kind: 'library' })
    await openPreview()
    return
  }
  st().setStage({ kind: 'library' })
  st().setModule('history')
  st().notify('info', tr('preview.plan_again_history'))
}

export function cancelOperation() {
  const s = st()
  if (s.stage.kind !== 'applying') return
  s.setStage({ ...s.stage, cancelling: true })
  api.opCancel().catch(() => {})
}

export async function planUndo(opId: string) {
  try {
    showPreview(await api.undoPlan(opId), 'undo')
  } catch (e) {
    st().notify('error', errorText(e))
  }
}

export async function planRetry(opId: string) {
  try {
    showPreview(await api.retryPlan(opId), 'retry')
  } catch (e) {
    st().notify('error', errorText(e))
  }
}

/** Done in the completion summary. */
export function finishOperation() {
  st().setStage({ kind: 'library' })
}
