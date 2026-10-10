// SPDX-License-Identifier: GPL-3.0-or-later
// Development-only stand-in for the backend, used when the UI is opened in a plain browser
// (`npm run dev`) to look at screens. It invents a small Session; it never touches files and is
// not included in production builds (ipc/index.ts imports it only under import.meta.env.DEV).

import type {
  AppEvent,
  AppInfo,
  Asset,
  BatchEdit,
  FieldChange,
  OpReport,
  OpSummary,
  PlanEntry,
  PlanView,
  Row,
} from './types'
import { createMockUpdates } from './mockUpdates'
const mockUpdates = createMockUpdates()
import { createMockSettings } from './mockSettings'
const mockSettings = createMockSettings()
import { createMockBackups } from './mockBackups'
const mockBackups = createMockBackups()

let listener: (e: AppEvent) => void = () => {}
const assets: Asset[] = []
const rows = new Map<number, Row>()
const plans = new Map<string, { versions: PlanEntry[][]; view: PlanView }>()
const history: OpSummary[] = [{ id: 'op-preview-history', kind: 'apply', title: 'Preview: copyright edit', status: 'completed_with_errors', created_ms: Date.now() - 86_400_000, finished_ms: Date.now() - 86_390_000, undo_of: null, undone_by: [], files: 3, states: { done: 2, failed: 1 }, changes: 3, keep: false, backups_pruned: false, backups_unavailable: false, undoable: true, rolled_back: 0, warnings: 0 }]

const CAMERAS = [
  ['NIKON CORPORATION', 'NIKON Z 8', 'NIKKOR Z 24-120mm f/4 S'],
  ['NIKON CORPORATION', 'NIKON Z f', 'NIKKOR Z 40mm f/2'],
  ['NIKON CORPORATION', 'NIKON D850', 'AF-S NIKKOR 70-200mm f/2.8E FL ED VR'],
]

function info(): AppInfo {
  return {
    version: '0.1.0',
    data_dir: 'D:\\MoriMeta-dev',
    launched: true,
    exiftool: { starting: false, version: '13.59', error: null, integrity: null, package: 'mock' },
    startup: { recovered_files: 0, recovery_waiting: [], needs_decision: [], prunes_left: [], elevated: false, backup_problem: null, error: null },
    writes_refused: null,
    backup: { root: 'D:\\MoriMeta-dev\\backups', bytes: 48_000_000, operations: history.length, free: 210_000_000_000, sync_warning: null, problem: null },
    dev: true,
  }
}

function makeSession(n: number) {
  const start = assets.length
  const made: Asset[] = []
  for (let i = 0; i < n; i++) {
    const k = start + i
    const raw = k % 3 === 0
    const folder = k % 2 ? 'D:\\Photos\\2026-09 京都' : 'D:\\Photos\\2026-09 Hokkaido'
    const name = `DSC_${String(4100 + k).padStart(4, '0')}.${raw ? 'NEF' : 'JPG'}`
    made.push({ id: k + 1, path: `${folder}\\${name}`, name, folder, ext: raw ? 'NEF' : 'JPG', writable: k % 17 !== 5, size: raw ? 45_000_000 : 9_800_000 })
  }
  assets.push(...made)
  listener({ kind: 'imported', assets: made, summary: { added: n, read_only: 0, duplicates: 0, failed: [], other_files: 0, not_followed: 0, placeholders: 0, skipped_folders: 0, orphan_sidecars: 0, backup_files: 0 } })
  let done = 0
  const tick = () => {
    const batch = made.slice(done, done + 40).map((a): Row => {
      const k = a.id
      const [make, model, lens] = CAMERAS[k % 3]
      return {
        id: a.id,
        writes_to: a.ext === 'NEF' ? (k % 2 ? 'sidecar' : 'new_sidecar') : 'in_file',
        creator: k % 4 === 0 ? null : k % 7 === 0 ? 'M. Mori' : 'Morii',
        copyright: k % 3 === 1 ? null : '© 2026 Morii',
        capture_time: `2026:09:${String(10 + (k % 5)).padStart(2, '0')} 1${k % 9}:${String((k * 7) % 60).padStart(2, '0')}:${String((k * 13) % 60).padStart(2, '0')}+09:00`,
        gps: k % 5 === 0 ? null : `${(43.06 + k / 1000).toFixed(5)}, ${(141.35 + k / 1000).toFixed(5)}`,
        conflicts: k % 11 === 0 ? ['creator'] : [],
        invalid: [],
        make,
        model,
        lens,
        not_downloaded: false,
        error: null,
      }
    })
    for (const r of batch) rows.set(r.id, r)
    done += batch.length
    listener({ kind: 'rows', rows: batch })
    listener({ kind: 'scan_progress', done, total: made.length })
    if (done < made.length) setTimeout(tick, 120)
    else listener({ kind: 'scan_done', cancelled: false, error: null })
  }
  setTimeout(tick, 200)
}

function change(field: string, before: string | null, after: string | null): FieldChange | null {
  if (before === after) return null
  return { field, before: before ? [before] : null, after: after ? [after] : null, kind: !before ? 'add' : !after ? 'remove' : 'modify' }
}

function viewOf(id: string, version: number, entries: PlanEntry[], title: string, kind = 'apply'): PlanView {
  const ready = entries.filter((e) => !e.excluded && e.status.status === 'ready')
  const fields = [...new Set(entries.flatMap((e) => [...e.changes, ...(e.excluded_changes ?? [])].map((c) => c.field)))]
  const k = { add: 0, modify: 0, remove: 0, warnings: 0, unsupported: 0, blocked: 0, no_change: 0 }
  for (const e of entries) {
    if (e.excluded) continue
    if (e.status.status === 'ready') {
      for (const c of e.changes) k[c.kind]++
      if (e.warnings.length) k.warnings++
    } else if (e.status.status === 'unsupported') k.unsupported++
    else if (e.status.status === 'blocked') k.blocked++
    else k.no_change++
  }
  const acks = [...new Set(ready.flatMap((e) => e.changes.filter((c) => c.kind === 'remove').map((c) => `remove:${c.field}`)))]
  if (k.unsupported) acks.push('unsupported')
  return {
    id,
    version,
    title,
    kind,
    summary: {
      files: entries.length,
      ready: ready.length,
      no_change: k.no_change,
      blocked: k.blocked,
      unsupported: k.unsupported,
      excluded: entries.filter((e) => e.excluded).length,
      warnings: k.warnings,
      targets: {
        in_file: ready.filter((e) => e.target === 'in_file').length,
        sidecar: ready.filter((e) => e.target === 'sidecar').length,
        new_sidecar: ready.filter((e) => e.target === 'new_sidecar').length,
      },
      changes: ready.reduce((a, e) => a + e.changes.length, 0),
    },
    required_acks: acks,
    fields,
    field_counts: fields.map((f) => [f, entries.filter((e) => !e.excluded && e.changes.some((c) => c.field === f)).length]),
    excluded_fields: fields.filter((f) => !entries.some((e) => e.changes.some((c) => c.field === f))),
    kinds: k,
  }
}

function planBatch(ids: number[], edit: BatchEdit): PlanView {
  const entries: PlanEntry[] = ids.map((id, seq) => {
    const a = assets.find((x) => x.id === id)!
    const r = rows.get(id)!
    const base = { seq, path: a.path, name: a.name, notes: [] as string[], warnings: [] as string[], excluded: false, excluded_changes: [] as FieldChange[] }
    const target = r.writes_to === 'in_file' ? 'in_file' : r.writes_to === 'sidecar' ? 'sidecar' : 'new_sidecar'
    if (!a.writable) return { ...base, target: 'in_file', changes: [], status: { status: 'unsupported', reason: 'format is read-only in this build' } }
    const ch: FieldChange[] = []
    if (edit.creator) {
      const c = change('creator', r.creator, edit.creator.op === 'set' ? edit.creator.values.join('; ') : null)
      if (c) ch.push(c)
    }
    if (edit.copyright) {
      const v = edit.copyright.op === 'set' ? edit.copyright.value.replace('{year}', '2026').replace(/\{creator(\|[^}]*)?\}/, r.creator ?? 'Morii') : null
      const c = change('copyright', r.copyright, v)
      if (c) ch.push(c)
    }
    if (edit.gps) {
      if (edit.gps.op === 'remove' && target !== 'in_file' && r.gps) {
        return { ...base, target, changes: [], status: { status: 'unsupported', reason: 'GPS inside the RAW cannot be removed through its sidecar' } }
      }
      const c = change('gps', r.gps, edit.gps.op === 'set' ? edit.gps.position : null)
      if (c) ch.push(c)
    }
    if (edit.time) {
      const c = change('capture_time', r.capture_time, edit.time.mode === 'absolute' ? edit.time.to : `${r.capture_time} (${edit.time.mode})`)
      if (c) ch.push(c)
    }
    const warnings = r.conflicts.length && edit.creator ? ['XMP-dc:Creator differed from EXIF Artist; both are set'] : []
    return {
      ...base,
      target,
      changes: ch,
      warnings,
      notes: [...warnings.map((w) => `warning: ${w}`), ...(target !== 'in_file' ? ['written to the XMP sidecar; the RAW file is not modified'] : [])],
      status: ch.length ? { status: 'ready' } : { status: 'no_change' },
      action: ch.length ? { action: 'write', ops: ch.map((c) => (c.after ? { op: 'set' as const, tag: `XMP:${c.field}`, values: c.after } : { op: 'delete' as const, tag: `XMP:${c.field}` })) } : null,
    }
  })
  const id = `plan-mock-${plans.size + 1}`
  const view = viewOf(id, 1, entries, edit.title || 'Batch edit')
  plans.set(id, { versions: [entries], view })
  return view
}

function bump(id: string, f: (e: PlanEntry[]) => PlanEntry[]): PlanView {
  const p = plans.get(id)!
  const next = f(structuredClone(p.versions[p.versions.length - 1]))
  p.versions.push(next)
  p.view = viewOf(id, p.versions.length, next, p.view.title, p.view.kind)
  return p.view
}

const roCleared = new Set<number>()

const handlers: Record<string, (a: Record<string, unknown>) => unknown> = {
  backup_usage: () => mockBackups.usage(),
  backup_keep: (a) => mockBackups.keep(a.opId as string, a.keep as boolean),
  prune_preview: (a) => mockBackups.preview(a.opIds as string[] | null),
  prune_execute: (a) => mockBackups.execute(a.token as string),
  update_status: () => mockUpdates.status(),
  update_check: () => mockUpdates.check(),
  update_download: (a) => mockUpdates.download(a.id as string),
  update_cancel: () => mockUpdates.cancel(),
  update_install: (a) => mockUpdates.install(a.id as string),
  app_info: () => info(),
  import_dialog: (a) => makeSession(a.kind === 'folder' ? 240 : 12),
  scan_cancel: () => undefined,
  rescan: () => undefined,
  session_clear: () => {
    assets.length = 0
    rows.clear()
  },
  asset_detail: (a) => {
    const r = rows.get(a.id as number)!
    const as = assets.find((x) => x.id === a.id)!
    return {
      path: as.path,
      sidecar: as.ext === 'NEF' ? as.path.replace(/NEF$/, 'xmp') : null,
      fields: (['creator', 'copyright', 'capture_time', 'gps'] as const).map((f) => ({
        field: f,
        value: r[f],
        sources: r[f] ? [[f === 'creator' ? 'IFD0:Artist' : f === 'copyright' ? 'IFD0:Copyright' : f === 'gps' ? 'GPS:GPSLatitude' : 'ExifIFD:DateTimeOriginal', r[f]!], ...(r.conflicts.includes(f) ? [['XMP-dc:Creator', 'M. Mori']] : [])] : [],
        conflicting: r.conflicts.includes(f),
        error: null,
      })),
      tags: { 'IFD0:Make': r.make, 'IFD0:Model': r.model, 'ExifIFD:LensModel': r.lens, 'ExifIFD:ISO': '400', 'ExifIFD:FNumber': '5.6', 'ExifIFD:ExposureTime': '1/250', 'ExifIFD:OffsetTimeOriginal': '+09:00', 'ExifIFD:CreateDate': r.capture_time?.slice(0, 19) ?? '' },
      sidecar_tags: null,
      // a few JPEGs carry the read-only attribute until it is cleared
      read_only: as.ext === 'JPG' && as.id % 37 === 5 && !roCleared.has(as.id),
    }
  },
  attention: () => ({ read_only: [], conflicts: [...rows.values()].filter((r) => r.conflicts.length).map((r) => r.id), cloud_placeholders: [], cloud_files: [], darktable_sidecars: [], c2pa: [], links: [], unreadable: [], long_paths: [], removable: [], network: [], other_file_system: [], changed_since_import: [] }),
  plan_batch: (a) => new Promise((ok) => setTimeout(() => ok(planBatch(a.ids as number[], a.edit as BatchEdit)), 400)),
  plan_cancel: () => undefined,
  plan_view: (a) => plans.get(a.id as string)!.view,
  plan_page: (a) => {
    const p = plans.get(a.id as string)!
    const e = p.versions[(a.version as number) - 1]
    const page = a.page as number
    return { version: a.version, matching: e.length, entries: e.slice(page * 200, page * 200 + 200) }
  },
  plan_exclude: (a) =>
    bump(a.id as string, (es) => es.map((e) => ((a.seqs as number[]).includes(e.seq) ? { ...e, excluded: a.excluded as boolean } : e))),
  plan_exclude_field: (a) =>
    bump(a.id as string, (es) =>
      es.map((e) => {
        if (a.seqs && !(a.seqs as number[]).includes(e.seq)) return e
        const f = a.field as string
        if (a.excluded) {
          const c = e.changes.find((x) => x.field === f)
          if (!c) return e
          return { ...e, changes: e.changes.filter((x) => x !== c), excluded_changes: [...(e.excluded_changes ?? []), c] }
        }
        const c = e.excluded_changes?.find((x) => x.field === f)
        if (!c) return e
        return { ...e, changes: [...e.changes, c], excluded_changes: e.excluded_changes!.filter((x) => x !== c) }
      }),
    ),
  plan_preflight: () => ({ rescan: [], backup: null, space: null, exiftool: null, ok: true, backup_warning: null }),
  plan_confirm: () => 'token',
  op_execute: (a) =>
    new Promise<OpReport>((ok) => {
      const p = plans.get(a.id as string)!
      const es = p.versions[(a.version as number) - 1].filter((e) => !e.excluded && e.status.status === 'ready')
      let done = 0
      const tick = () => {
        done = Math.min(es.length, done + 7)
        listener({ kind: 'exec_progress', total: es.length, done, ok: done, failed: 0, skipped: 0, last_seq: null, last_state: 'done' })
        if (done < es.length) setTimeout(tick, 150)
        else {
          const op = `op-mock-${history.length + 1}`
          history.unshift({ id: op, kind: p.view.kind, title: p.view.title, status: 'completed', created_ms: Date.now(), finished_ms: Date.now(), undo_of: null, undone_by: [], files: es.length, states: { done: es.length }, changes: p.view.summary.changes, keep: false, backups_pruned: false, backups_unavailable: false, undoable: true, rolled_back: 0, warnings: 0 })
          ok({ op_id: op, status: 'completed', note: null, files: es.map((e) => ({ seq: e.seq, path: e.path, name: e.name, state: 'done', reason: null })) })
        }
      }
      setTimeout(tick, 150)
    }),
  op_cancel: () => undefined,
  history_list: () => history,
  op_detail: (a) => {
    const o = history.find((h) => h.id === a.opId)!
    return { ...o, app_version: '0.1.0', exiftool_version: '13.59', acks: [], files_detail: Array.from({ length: o.files }, (_, seq) => ({ seq, path: `preview-${seq + 1}.jpg`, state: seq === 2 ? 'failed' : 'done', error: seq === 2 ? 'Preview: simulated failure' : null, h0: null, h1: null, changes: [] })) }
  },
  undo_plan: (a) => {
    const o = history.find((h) => h.id === a.opId)!
    const id = `plan-mock-${plans.size + 1}`
    const entries: PlanEntry[] = Array.from({ length: o.id === 'op-preview-history' ? 2 : o.files }, (_, seq) => ({ seq, path: `preview-${seq + 1}.jpg`, name: `preview-${seq + 1}.jpg`, notes: [], warnings: [], excluded: seq === 1, forced_undo: seq === 1, target: 'in_file', changes: [], status: { status: 'ready' } }))
    const view = { ...viewOf(id, 1, entries, `Undo: ${o.title}`, 'undo') }
    view.summary.changes = o.files
    plans.set(id, { versions: [entries], view })
    return view
  },
  retry_plan: () => {
    throw 'nothing failed'
  },
  plan_again: () => {
    const id = `plan-mock-${plans.size + 1}`
    const entries: PlanEntry[] = [{ seq: 0, path: 'preview-3.jpg', name: 'preview-3.jpg', notes: [], warnings: [], excluded: false, target: 'in_file', changes: [{ field: 'copyright', before: null, after: ['Preview copyright'], kind: 'add' }], status: { status: 'ready' } }]
    const view = viewOf(id, 1, entries, 'Preview: re-read failed file')
    plans.set(id, { versions: [entries], view })
    return view
  },
  export_log: () => 'D:\\MoriMeta-op.json',
  ui_zoom: () => undefined,
  preset_dry_run: (a) => {
    const ids = (a.ids as number[]).slice(0, 500)
    const view = planBatch(ids, { copyright: { op: 'set', value: '© 2026 Morii' } })
    return { view, files: ids.length, samples: ids.slice(0, 3).map((id) => ({ name: assets.find((x) => x.id === id)?.name ?? '', changes: [['copyright', '', '© 2026 Morii']] })) }
  },
  about: () => ({ version: '0.1.0', exiftool: '13.59', registry_version: 0, webview2: '141.0.3537.71', os: 'windows x86_64', dev: true }),
  third_party_notices: () => '# Third-party notices\n\n(development mock: the real file is generated when the installer is staged)\n',
  asset_preview: () => null,
  history_import: () => ({ location: 'E:\\MoriMeta backups', imported: ['op-mock-1', 'op-mock-2'], skipped: [], recovered_files: 0 }),
  restore_to: () => ({ folder: 'D:\\restored', restored: 3, without_backup: 0, notes: [] }),
  presets_list: () => [
    { id: 'builtin:Copyright Template', name: 'Copyright Template', builtin: true, fields: ['copyright'], last_used_ms: null, untrusted: false, lint: [],
      preset: { schema_version: 1, name: 'Copyright Template', rules: [{ name: 'Copyright from creator and year where there is none', enabled: true, when: [{ if: 'empty', field: 'copyright' }], then: [{ do: 'set_copyright', value: '© {creator} {year}' }] }] } },
    { id: 'builtin:Remove GPS', name: 'Remove GPS', builtin: true, fields: ['gps'], last_used_ms: null, untrusted: false, lint: [],
      preset: { schema_version: 1, name: 'Remove GPS', rules: [{ name: '', enabled: true, when: [], then: [{ do: 'remove_gps' }] }] } },
  ],
  preset_save: () => 'preset-mock',
  preset_duplicate: () => 'preset-mock',
  preset_delete: () => undefined,
  preset_import: () => null,
  preset_export: () => null,
  plan_preset: (a) => planBatch(a.ids as number[], { copyright: { op: 'set', value: '© {creator} {year}' }, title: 'Copyright Template' }),
  clean_plan: (a) => ({ id: 'clean-mock', entries: (a.ids as number[]).map((id, seq) => {
    const as = assets.find((x) => x.id === id)!
    return as.ext === 'JPG'
      ? { seq, name: as.name, source: as.path, status: { status: 'ready' }, categories: [['gps', 9], ['maker_notes', 120], ['serial_numbers', 2]], removed: 131, kept: 24, segments: [{ label: 'APP2:MPF', bytes: 90, unidentified: false }], lens_lost: false }
      : { seq, name: as.name, source: as.path, status: { status: 'blocked', reason: 'only JPEG files are exported in this version' }, categories: [], removed: 0, kept: 0, segments: [], lens_lost: false }
  }) }),
  clean_entry: () => ({ keep: ['EXIF:IFD0:Make', 'EXIF:IFD0:Model'], remove: [{ key: 'EXIF:GPS:GPSLatitude', category: 'gps', value: '43 deg 3\' 51.12" N' }, { key: 'EXIF:ExifIFD:SerialNumber', category: 'serial_numbers', value: '6001234' }], remove_segments: [{ label: 'APP2:MPF', bytes: 90, unidentified: false }], lens_lost: false }),
  clean_export: () => [],
  choose_backup_folder: () => null,
  now_vs_after: (a) => (a.seqs as number[]).map((s) => [s, s % 5 ? 'as_written' : 'changed']),
  recovery_status: () => [],
  recovery_dismiss: () => undefined,
  recovery_resume: () => undefined,
  recovery_keep: () => undefined,
  // the browser has no window to close
  app_close: () => undefined,
  clear_read_only: (a) => {
    roCleared.add(a.id as number)
    return true
  },
  selection_aggregate: () => [],
  settings_list: () => mockSettings.list(),
  setting_set: (a) => mockSettings.set(a.name as string, a.value as string),
  settings_reset: () => mockSettings.reset(),
  settings_migrations: () => [],
}

/** The commands the mock answers (`mock.test.ts` compares them with what the UI calls). */
export const mockCommands = Object.keys(handlers)

export const transport = {
  invoke: async <T>(cmd: string, args?: Record<string, unknown>): Promise<T> => {
    const h = handlers[cmd]
    if (!h) throw `mock: no command ${cmd}`
    return (await h(args ?? {})) as T
  },
  subscribe: async (on: (e: AppEvent) => void) => {
    listener = on
  },
}
