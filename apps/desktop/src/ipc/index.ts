// SPDX-License-Identifier: GPL-3.0-or-later
// The only module that calls `invoke` (ARCHITECTURE §4.2 `ipc/`). In a development build opened
// in a plain browser (no Tauri), a mock backend stands in so the UI can be looked at; it is
// never part of a production build.

import type { Preset, PresetInfo } from '../features/presets/model'
import type {
  AppEvent,
  AppInfo,
  BackupUsage,
  PrunePreview,
  AssetDetail,
  Attention,
  BatchEdit,
  CleanPlan,
  Exported,
  KeepSpec,
  Prediction,
  EntryFilter,
  FieldAggregate,
  OpDetail,
  OpReport,
  OpSummary,
  PlanPage,
  PlanView,
  Preflight,
  RecoverySummary,
  Setting,
  UpdateInfo,
} from './types'

type Invoke = <T>(cmd: string, args?: Record<string, unknown>) => Promise<T>
type Subscribe = (on: (e: AppEvent) => void) => Promise<void>

interface Transport {
  invoke: Invoke
  subscribe: Subscribe
}

const inTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window

let transport: Promise<Transport> | null = null

function getTransport(): Promise<Transport> {
  if (!transport) {
    transport = (async (): Promise<Transport> => {
      if (inTauri) {
        const core = await import('@tauri-apps/api/core')
        return {
          invoke: (cmd, args) => core.invoke(cmd, args),
          subscribe: async (on) => {
            const ch = new core.Channel<AppEvent>()
            ch.onmessage = on
            await core.invoke('subscribe', { onEvent: ch })
          },
        }
      }
      if (import.meta.env.DEV) {
        const mock = await import('./mock')
        return mock.transport
      }
      throw new Error('MoriMeta runs inside its desktop app')
    })()
  }
  return transport
}

async function call<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  const t = await getTransport()
  return t.invoke<T>(cmd, args)
}

export const isMock = !inTauri

export const api = {
  updateStatus: () => call<UpdateInfo>('update_status'),
  updateCheck: (manual = true) => call<UpdateInfo>('update_check', { manual }),
  updateDownload: (id: string) => call<UpdateInfo>('update_download', { id }),
  updateCancel: () => call<void>('update_cancel'),
  updateInstall: (id: string) => call<void>('update_install', { id }),
  subscribe: async (on: (e: AppEvent) => void) => (await getTransport()).subscribe(on),
  appInfo: () => call<AppInfo>('app_info'),
  importDialog: (kind: 'files' | 'folder') => call<void>('import_dialog', { kind }),
  scanCancel: () => call<void>('scan_cancel'),
  rescan: (ids: number[]) => call<void>('rescan', { ids }),
  sessionClear: () => call<void>('session_clear'),
  assetDetail: (id: number) => call<AssetDetail>('asset_detail', { id }),
  selectionAggregate: (ids: number[]) => call<FieldAggregate[]>('selection_aggregate', { ids }),
  attention: (ids: number[]) => call<Attention>('attention', { ids }),
  clearReadOnly: (id: number) => call<boolean>('clear_read_only', { id }),
  planBatch: (ids: number[], edit: BatchEdit) => call<PlanView>('plan_batch', { ids, edit }),
  planCancel: () => call<void>('plan_cancel'),
  planView: (id: string, version: number) => call<PlanView>('plan_view', { id, version }),
  planPage: (id: string, version: number, filter: EntryFilter, page: number) =>
    call<PlanPage>('plan_page', { id, version, filter, page }),
  planExclude: (id: string, version: number, seqs: number[], excluded: boolean) =>
    call<PlanView>('plan_exclude', { id, version, seqs, excluded }),
  planExcludeField: (
    id: string,
    version: number,
    seqs: number[] | null,
    field: string,
    excluded: boolean,
  ) => call<PlanView>('plan_exclude_field', { id, version, seqs, field, excluded }),
  planPreflight: (id: string, version: number) => call<Preflight>('plan_preflight', { id, version }),
  planConfirm: (id: string, version: number, acks: string[]) =>
    call<string>('plan_confirm', { id, version, acks }),
  opExecute: (id: string, version: number, token: string) =>
    call<OpReport>('op_execute', { id, version, token }),
  opCancel: () => call<void>('op_cancel'),
  historyList: (page: number) => call<OpSummary[]>('history_list', { page }),
  opDetail: (opId: string) => call<OpDetail>('op_detail', { opId }),
  undoPlan: (opId: string, forceSeqs: number[] | null = null) => call<PlanView>('undo_plan', { opId, forceSeqs }),
  retryPlan: (opId: string) => call<PlanView>('retry_plan', { opId }),
  planAgain: (opId: string) => call<PlanView>('plan_again', { opId }),
  recoveryKeep: (opId: string, seqs: number[]) => call<void>('recovery_keep', { opId, seqs }),
  exportLog: (opId: string, includePaths: boolean, includeValues: boolean) =>
    call<string | null>('export_log', { opId, includePaths, includeValues }),
  restoreTo: (opId: string) =>
    call<{ folder: string; restored: number; without_backup: number; notes: string[] } | null>('restore_to', { opId }),
  recoveryStatus: () => call<RecoverySummary[]>('recovery_status'),
  recoveryDismiss: (opId: string) => call<void>('recovery_dismiss', { opId }),
  recoveryResume: (opId: string) => call<OpReport>('recovery_resume', { opId }),
  presetsList: () => call<PresetInfo[]>('presets_list'),
  presetSave: (id: string | null, preset: Preset) => call<string>('preset_save', { id, preset }),
  presetDuplicate: (id: string) => call<string>('preset_duplicate', { id }),
  presetDelete: (id: string) => call<void>('preset_delete', { id }),
  presetImport: () => call<string | null>('preset_import'),
  presetExport: (id: string) => call<string | null>('preset_export', { id }),
  planPreset: (ids: number[], presetId: string) => call<PlanView>('plan_preset', { ids, presetId }),
  cleanPlan: (ids: number[], spec: KeepSpec) => call<CleanPlan>('clean_plan', { ids, spec }),
  cleanEntry: (seq: number) => call<Prediction>('clean_entry', { seq }),
  cleanExport: (planId: string, numberTaken: boolean) => call<Exported[] | null>('clean_export', { planId, numberTaken }),
  chooseBackupFolder: () => call<AppInfo['backup'] | null>('choose_backup_folder'),
  nowVsAfter: (opId: string, seqs: number[]) => call<[number, string][]>('now_vs_after', { opId, seqs }),
  appClose: () => call<void>('app_close'),
  settingsList: () => call<Setting[]>('settings_list'),
  settingSet: (name: string, value: string) => call<void>('setting_set', { name, value }),
  settingsReset: () => call<void>('settings_reset'),
  settingsMigrations: () => call<[number, string][]>('settings_migrations'),
  backupUsage: () => call<BackupUsage>('backup_usage'),
  backupKeep: (opId: string, keep: boolean) => call<void>('backup_keep', { opId, keep }),
  prunePreview: (opIds: string[] | null) => call<PrunePreview>('prune_preview', { opIds }),
  pruneExecute: (token: string) => call<string[]>('prune_execute', { token }),
}

/** A command's error as text (commands reject with the backend's message). */
export function errorText(e: unknown): string {
  if (typeof e === 'string') return e
  if (e instanceof Error) return e.message
  return String(e)
}
