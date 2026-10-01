// SPDX-License-Identifier: GPL-3.0-or-later
// UI state (ARCHITECTURE §4.2 `state/`). The authoritative data lives in the backend: this holds
// the Session's rows as last read, the selection, what is staged, and which surface is open.

import { create } from 'zustand'
import type {
  AppInfo,
  Asset,
  BatchEdit,
  ExecProgress,
  ImportSummary,
  OpReport,
  PlanView,
  Row,
} from '../ipc/types'
import { detectLang, type Lang } from '../i18n'

export type Module = 'library' | 'presets' | 'rules' | 'history'

/** What the centre and right panes show (INTERACTION_SPEC §3). */
export type Stage =
  | { kind: 'library' }
  | { kind: 'planning'; done: number; total: number; stage: string }
  | { kind: 'preview'; plan: PlanView; origin: 'edit' | 'undo' | 'retry' }
  | { kind: 'applying'; plan: PlanView; progress: ExecProgress | null; started: number; cancelling: boolean }
  | { kind: 'done'; plan: PlanView; report: OpReport; started: number; finished: number }

export interface SortKey {
  key: string
  dir: 1 | -1
}

export interface Notice {
  id: number
  tone: 'info' | 'warn' | 'error' | 'success'
  text: string
}

interface AppState {
  lang: Lang
  info: AppInfo | null
  module: Module
  settingsOpen: boolean
  sidebarOpen: boolean
  inspectorOpen: boolean
  timeToolsOpen: boolean

  assets: Asset[]
  rows: Map<number, Row>
  scan: { running: boolean; done: number; total: number }
  lastImport: ImportSummary | null

  selection: Set<number>
  focus: number | null
  anchor: number | null
  sort: SortKey[]
  search: string
  facets: Record<string, Set<string>>

  staged: BatchEdit
  /** Bumped when staged edits are discarded, so editors start afresh. */
  stagedGen: number
  stage: Stage
  notices: Notice[]
  /** The window asked to close during an Operation (INTERACTION_SPEC §10). */
  closeAsked: boolean
  /** The Preset the rule builder shows. */
  editPreset: string | null
  /** Close once the Operation has stopped. */
  closeAfter: boolean

  setLang: (l: Lang) => void
  setInfo: (i: AppInfo) => void
  setModule: (m: Module) => void
  setSettingsOpen: (o: boolean) => void
  toggleSidebar: () => void
  toggleInspector: () => void
  setTimeToolsOpen: (o: boolean) => void

  addAssets: (a: Asset[], s: ImportSummary) => void
  putRows: (rows: Row[]) => void
  setScan: (s: Partial<AppState['scan']>) => void
  clearSession: () => void

  select: (ids: number[], focus?: number | null, anchor?: number | null) => void
  setSort: (s: SortKey[]) => void
  setSearch: (s: string) => void
  toggleFacet: (group: string, value: string) => void
  clearFacets: (group?: string) => void

  stageEdit: (e: Partial<BatchEdit>) => void
  unstage: (field: keyof BatchEdit) => void
  discardStaged: () => void
  setStage: (s: Stage) => void

  notify: (tone: Notice['tone'], text: string) => void
  dismiss: (id: number) => void
}

let noticeId = 1

export const useApp = create<AppState>((set) => ({
  lang: detectLang(),
  info: null,
  module: 'library',
  settingsOpen: false,
  sidebarOpen: true,
  inspectorOpen: true,
  timeToolsOpen: false,

  assets: [],
  rows: new Map(),
  scan: { running: false, done: 0, total: 0 },
  lastImport: null,

  selection: new Set(),
  focus: null,
  anchor: null,
  sort: [{ key: 'name', dir: 1 }],
  search: '',
  facets: {},

  staged: {},
  stagedGen: 0,
  stage: { kind: 'library' },
  notices: [],
  closeAsked: false,
  editPreset: null,
  closeAfter: false,

  setLang: (lang) => {
    try {
      localStorage.setItem('mm.lang', lang)
    } catch {
      // a per-viewer convenience only
    }
    document.documentElement.lang = lang === 'zh' ? 'zh-CN' : 'en'
    set({ lang })
  },
  setInfo: (info) => set({ info }),
  setModule: (module) => set({ module, settingsOpen: false }),
  setSettingsOpen: (settingsOpen) => set({ settingsOpen }),
  toggleSidebar: () => set((s) => ({ sidebarOpen: !s.sidebarOpen })),
  toggleInspector: () => set((s) => ({ inspectorOpen: !s.inspectorOpen })),
  setTimeToolsOpen: (timeToolsOpen) => set({ timeToolsOpen }),

  addAssets: (a, summary) =>
    set((s) => {
      const known = new Set(s.assets.map((x) => x.id))
      return { assets: [...s.assets, ...a.filter((x) => !known.has(x.id))], lastImport: summary }
    }),
  putRows: (rows) =>
    set((s) => {
      const next = new Map(s.rows)
      for (const r of rows) next.set(r.id, r)
      return { rows: next }
    }),
  setScan: (p) => set((s) => ({ scan: { ...s.scan, ...p } })),
  clearSession: () =>
    set({
      assets: [],
      rows: new Map(),
      selection: new Set(),
      focus: null,
      anchor: null,
      staged: {},
      facets: {},
      search: '',
      lastImport: null,
      stagedGen: useApp.getState().stagedGen + 1,
    }),

  select: (ids, focus, anchor) =>
    set((s) => ({
      selection: new Set(ids),
      focus: focus === undefined ? s.focus : focus,
      anchor: anchor === undefined ? s.anchor : anchor,
    })),
  setSort: (sort) => set({ sort }),
  setSearch: (search) => set({ search }),
  toggleFacet: (group, value) =>
    set((s) => {
      const cur = new Set(s.facets[group] ?? [])
      if (cur.has(value)) cur.delete(value)
      else cur.add(value)
      return { facets: { ...s.facets, [group]: cur } }
    }),
  clearFacets: (group) =>
    set((s) => {
      if (!group) return { facets: {} }
      const next = { ...s.facets }
      delete next[group]
      return { facets: next }
    }),

  stageEdit: (e) => set((s) => ({ staged: { ...s.staged, ...e } })),
  unstage: (field) =>
    set((s) => {
      const next = { ...s.staged }
      delete next[field]
      return { staged: next }
    }),
  discardStaged: () => set((s) => ({ staged: {}, stagedGen: s.stagedGen + 1 })),
  setStage: (stage) => set({ stage }),

  notify: (tone, text) =>
    set((s) => ({ notices: [...s.notices.slice(-3), { id: noticeId++, tone, text }] })),
  dismiss: (id) => set((s) => ({ notices: s.notices.filter((n) => n.id !== id) })),
}))

/** How many fields are staged (the Preview button turns accent when > 0). */
export function stagedCount(e: BatchEdit): number {
  return (['creator', 'copyright', 'gps', 'time'] as const).filter((k) => e[k] !== undefined).length
}

/** Whether an operation locks editing (DESIGN_SYSTEM AppShell `locked`). */
export function isLocked(stage: Stage): boolean {
  return stage.kind === 'applying' || stage.kind === 'planning'
}
