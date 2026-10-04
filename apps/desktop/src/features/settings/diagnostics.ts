// SPDX-License-Identifier: GPL-3.0-or-later
// Settings › Advanced › Copy diagnostics (SCREEN_SPEC 5#s-adv): what a bug report needs, without
// the user's name, paths or metadata values (SECURITY_MODEL §8). Personal settings (default
// Creator, copyright template, backup location) are reported only as set or not.

import type { About, AppInfo, Setting } from '../../ipc/types'

/** Settings whose values may name a person or a place on disk. */
const PERSONAL = new Set(['metadata.default_creator', 'metadata.copyright_template', 'backup.root'])

export function diagnostics(about: About, info: AppInfo | null, settings: Setting[]): string {
  const lines = [
    `MoriMeta ${about.version}${about.dev ? ' (development build)' : ''}`,
    `ExifTool: ${about.exiftool ?? 'not available'}`,
    `Field registry: ${about.registry_version}`,
    `WebView2: ${about.webview2 ?? 'not available'}`,
    `System: ${about.os}`,
  ]
  if (info) {
    lines.push(
      `ExifTool problem: ${info.exiftool.error ? 'start failed' : info.exiftool.integrity ? 'package check failed' : 'none'}`,
      `Writes refused: ${info.writes_refused ? 'yes' : 'no'}`,
      `Elevated: ${info.startup.elevated ? 'yes' : 'no'}`,
      `Recovered files at launch: ${info.startup.recovered_files}`,
      `Operations waiting for a backup drive: ${info.startup.recovery_waiting.length}`,
      `Operations needing a decision: ${info.startup.needs_decision.length}`,
      `Backup location: ${info.backup.problem ? 'problem' : 'ok'}${info.backup.sync_warning ? ', in a synced folder' : ''}`,
    )
  }
  for (const s of [...settings].sort((a, b) => a.name.localeCompare(b.name))) {
    const v = PERSONAL.has(s.name) ? (s.value && s.value !== s.default ? '(set)' : '(default)') : s.value === s.default ? `${s.value} (default)` : s.value
    lines.push(`${s.name}: ${v}`)
  }
  return lines.join('\n')
}
