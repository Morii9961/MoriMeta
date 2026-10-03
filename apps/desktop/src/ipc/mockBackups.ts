// SPDX-License-Identifier: GPL-3.0-or-later
// Development-only sample backup records. No paths are read and no files are removed.
import type { BackupUsage, OperationBackup, PrunePreview } from './types'

export function createMockBackups() {
  const now = Date.now()
  const operations: OperationBackup[] = [
    { op_id: 'mock-backup-old', title: 'Preview: old copyright edit', created_ms: now - 90 * 86_400_000, bytes: 8_000_000, pruned: false, keep: false, protection: null },
    { op_id: 'mock-backup-kept', title: 'Preview: kept creator edit', created_ms: now - 10 * 86_400_000, bytes: 12_000_000, pruned: false, keep: true, protection: 'kept' },
    { op_id: 'mock-backup-open', title: 'Preview: interrupted operation', created_ms: now, bytes: 4_000_000, pruned: false, keep: false, protection: 'unfinished' },
  ]
  let sequence = 0
  let pending: PrunePreview | null = null
  const usage = (): BackupUsage => ({ ops: structuredClone(operations), total_bytes: operations.reduce((sum, o) => sum + o.bytes, 0), volume_bytes: 1_000_000_000, sync_warning: null })
  return {
    usage,
    keep: (id: string, keep: boolean) => {
      const op = operations.find((o) => o.op_id === id)
      if (!op) throw `no operation ${id}`
      op.keep = keep
      if (op.protection !== 'unfinished') op.protection = keep ? 'kept' : null
    },
    preview: (ids: string[] | null) => {
      pending = null
      const chosen = ids === null ? operations.filter((o) => !o.pruned && o.protection === null) : [...new Set(ids)].map((id) => {
        const op = operations.find((o) => o.op_id === id)
        if (!op || op.pruned || op.protection === 'unfinished') throw 'nothing to prune'
        return op
      })
      if (!chosen.length) throw 'nothing to prune'
      pending = { token: `mock-prune-${++sequence}`, operations: structuredClone(chosen), bytes: chosen.reduce((sum, o) => sum + o.bytes, 0), requested: ids !== null }
      return structuredClone(pending)
    },
    execute: (token: string) => {
      if (pending?.token !== token) throw 'the plan was not confirmed; confirm it again'
      const preview = pending
      pending = null
      const chosen = preview.operations.map((p) => operations.find((o) => o.op_id === p.op_id)!)
      if (JSON.stringify(chosen) !== JSON.stringify(preview.operations)) throw 'the backup selection changed; preview it again'
      for (const op of chosen) { op.pruned = true; op.bytes = 0 }
      return chosen.map((o) => o.op_id)
    },
  }
}
