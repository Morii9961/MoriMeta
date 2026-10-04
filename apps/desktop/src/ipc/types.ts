// SPDX-License-Identifier: GPL-3.0-or-later
// The IPC contract as the frontend sees it: mirrors `src-tauri/src/dto.rs` and the serde shapes of
// the mm-core / mm-domain types those commands return. Paths are for display only (ARCHITECTURE
// §5.1): files are referred to by asset id, Plans by id + version.

export type FieldName = 'creator' | 'copyright' | 'capture_time' | 'gps'

export interface ExifToolState {
  starting: boolean
  version: string | null
  error: string | null
  integrity: string | null
  package: string | null
}

export interface RecoverySummary {
  op_id: string
  title: string
  status: string
  done: number
  remaining: number
  attention: number
  other: number
}

/** Find history in a backup folder… (`history::HistoryImport`). */
export interface HistoryImport {
  location: string
  imported: string[]
  skipped: [string, string][]
  recovered_files: number
}

export interface StartupInfo {
  recovered_files: number
  recovery_waiting: [string, string][]
  needs_decision: RecoverySummary[]
  prunes_left: [string, string][]
  elevated: boolean
  backup_problem: string | null
  error: string | null
}

export interface BackupInfo {
  root: string
  bytes: number
  operations: number
  free: number | null
  sync_warning: string | null
  problem: string | null
}

export interface AppInfo {
  version: string
  data_dir: string
  launched: boolean
  exiftool: ExifToolState
  startup: StartupInfo
  writes_refused: string | null
  backup: BackupInfo
  dev: boolean
}

export interface Asset {
  id: number
  path: string
  name: string
  folder: string
  ext: string
  writable: boolean
  size: number
}

export interface ImportSummary {
  added: number
  read_only: number
  duplicates: number
  failed: [string, string][]
  other_files: number
  not_followed: number
  placeholders: number
  skipped_folders: number
  orphan_sidecars: number
  backup_files: number
}

export type WritesTo = 'in_file' | 'sidecar' | 'new_sidecar' | 'read_only'

export interface Row {
  id: number
  writes_to: WritesTo
  creator: string | null
  copyright: string | null
  capture_time: string | null
  gps: string | null
  conflicts: FieldName[]
  invalid: FieldName[]
  make: string | null
  model: string | null
  lens: string | null
  not_downloaded: boolean
  error: string | null
}

export interface ExecProgress {
  total: number
  done: number
  ok: number
  failed: number
  skipped: number
  last_seq: number | null
  last_state: string | null
}

export type AppEvent =
  | { kind: 'status' }
  | { kind: 'imported'; assets: Asset[]; summary: ImportSummary }
  | { kind: 'rows'; rows: Row[] }
  | { kind: 'scan_progress'; done: number; total: number }
  | { kind: 'scan_done'; cancelled: boolean; error: string | null }
  | { kind: 'plan_progress'; stage: 'files' | 'metadata'; done: number; total: number }
  | ({ kind: 'exec_progress' } & ExecProgress)
  | { kind: 'close_blocked' }

export interface FieldView {
  field: FieldName
  value: string | null
  sources: [string, string][]
  conflicting: boolean
  error: string | null
}

export interface AssetDetail {
  path: string
  sidecar: string | null
  fields: FieldView[]
  tags: Record<string, string>
  sidecar_tags: Record<string, string> | null
}

export interface FieldAggregate {
  field: FieldName
  files: number
  values: [string, number][]
  more_values: number
  empty: number
  conflicting: number
  unreadable: number
  not_downloaded: number
}

export interface Attention {
  read_only: number[]
  conflicts: number[]
  cloud_placeholders: number[]
  cloud_files: number[]
  darktable_sidecars: number[]
  c2pa: number[]
  links: number[]
  unreadable: number[]
  long_paths: number[]
  removable: number[]
  network: number[]
  other_file_system: number[]
  changed_since_import: number[]
}

// --- batch edit (INTERACTION_SPEC §1): a field left out is "Leave"

export type ListEdit = { op: 'set'; values: string[] } | { op: 'clear' }
export type TextEdit = { op: 'set'; value: string } | { op: 'clear' }
export type GpsEdit = { op: 'set'; position: string } | { op: 'remove' }
export type TimeEdit =
  | { mode: 'absolute'; to: string }
  | { mode: 'shift'; by: string }
  | { mode: 'sequence'; start: string; step: string; order: 'time' | 'name' }
  | { mode: 'preserve'; anchor: number; to: string }

export interface BatchEdit {
  creator?: ListEdit
  copyright?: TextEdit
  gps?: GpsEdit
  time?: TimeEdit
  digitized?: boolean
  title?: string
}

// --- Plan and Preview

export interface WriteTargets {
  in_file: number
  sidecar: number
  new_sidecar: number
}

export interface PlanSummary {
  files: number
  ready: number
  no_change: number
  blocked: number
  unsupported: number
  excluded: number
  warnings: number
  targets: WriteTargets
  changes: number
}

export interface KindCounts {
  add: number
  modify: number
  remove: number
  warnings: number
  unsupported: number
  blocked: number
  no_change: number
}

export interface PlanView {
  id: string
  version: number
  title: string
  kind: 'apply' | 'undo' | string
  summary: PlanSummary
  required_acks: string[]
  fields: string[]
  field_counts: [string, number][]
  excluded_fields: string[]
  kinds: KindCounts
}

export type ChangeKind = 'add' | 'modify' | 'remove'

export interface FieldChange {
  field: string
  before: string[] | null
  after: string[] | null
  kind: ChangeKind
}

export type EntryStatus =
  | { status: 'ready' }
  | { status: 'no_change' }
  | { status: 'blocked'; reason: string }
  | { status: 'unsupported'; reason: string }

export type TagOp =
  | { op: 'set'; tag: string; values: string[] }
  | { op: 'delete'; tag: string }
  | { op: 'update_iptc_digest' }

export type EntryAction =
  | { action: 'write'; ops: TagOp[] }
  | { action: 'create_file'; ops: TagOp[] }
  | { action: 'restore'; backup: string }
  | { action: 'recreate'; backup: string }
  | { action: 'move_to_backup_store' }

export interface PlanEntry {
  seq: number
  path: string
  raw?: string | null
  status: EntryStatus
  changes: FieldChange[]
  action?: EntryAction | null
  notes: string[]
  excluded: boolean
  excluded_changes?: FieldChange[]
  target: 'in_file' | 'sidecar' | 'new_sidecar'
  warnings: string[]
  name: string
  forced_undo?: boolean
}

export interface PlanPage {
  version: number
  matching: number
  entries: PlanEntry[]
}

export type EntryFilter = 'all' | 'ready' | 'no_change' | 'blocked' | 'unsupported' | 'excluded'

export interface Preflight {
  rescan: number[]
  backup: string | null
  space: string | null
  exiftool: string | null
  ok: boolean
  backup_warning: string | null
}

export interface FileOutcome {
  seq: number
  path: string
  name: string
  state: string
  reason: string | null
}

export interface OpReport {
  op_id: string
  status: string
  note: string | null
  files: FileOutcome[]
}

// --- History

export interface OpSummary {
  id: string
  kind: string
  title: string
  status: string
  created_ms: number
  finished_ms: number | null
  undo_of: string | null
  undone_by: string[]
  files: number
  states: Record<string, number>
  changes: number
  keep: boolean
  backups_pruned: boolean
  backups_unavailable: boolean
  undoable: boolean
  rolled_back: number
  warnings: number
}

export interface FileDetail {
  seq: number
  path: string
  state: string
  error: string | null
  h0: string | null
  h1: string | null
  changes: FieldChange[]
}

export interface OpDetail extends OpSummary {
  app_version: string
  exiftool_version: string
  acks: string[]
  files_detail: FileDetail[]
}

// --- Clean Export (D-15 (c))

export interface KeepSpec {
  camera: boolean
  lens: boolean
  exposure: boolean
  capture_time: boolean
  author: boolean
  descriptive: boolean
}

export interface RemovedSegment {
  label: string
  bytes: number
  unidentified: boolean
}

export interface CleanEntry {
  seq: number
  name: string
  source: string
  status: { status: 'ready' } | { status: 'blocked'; reason: string }
  categories: [string, number][]
  removed: number
  kept: number
  segments: RemovedSegment[]
  lens_lost: boolean
}

export interface CleanPlan {
  id: string
  entries: CleanEntry[]
  /** The keep choices this plan was built with (kept by the UI). */
  spec?: KeepSpec
}

export interface Prediction {
  keep: string[]
  remove: { key: string; category: string; value: string }[]
  remove_segments: RemovedSegment[]
  lens_lost: boolean
}

export interface Exported {
  seq: number
  source: string
  output: string | null
  status: 'exported' | 'skipped' | 'refused' | 'blocked' | 'failed' | 'cancelled'
  reasons: string[]
}

export interface Setting {
  name: string
  value: string
  default: string
  about: string
}

export interface OperationBackup {
  op_id: string
  title: string
  created_ms: number
  bytes: number
  pruned: boolean
  keep: boolean
  protection: 'unfinished' | 'kept' | 'recent' | null
}

export interface BackupUsage {
  ops: OperationBackup[]
  total_bytes: number
  volume_bytes: number
  sync_warning: string | null
}

export interface PrunePreview {
  token: string
  operations: OperationBackup[]
  bytes: number
  requested: boolean
}
export interface UpdateInfo {
  configured: boolean
  busy: boolean
  phase: string
  offer: { id: string; version: string; notes: string | null; date: string | null } | null
  downloaded: boolean
  bytes: number
  total: number | null
  error: string | null
}
