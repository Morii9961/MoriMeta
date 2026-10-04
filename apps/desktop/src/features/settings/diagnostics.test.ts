// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import type { About, Setting } from '../../ipc/types'
import { diagnostics } from './diagnostics'

const about: About = { version: '0.1.0', exiftool: '13.59', registry_version: 0, webview2: '141.0', os: 'windows x86_64', dev: false }
const s = (name: string, value: string, def = ''): Setting => ({ name, value, default: def, about: '' }) as Setting

describe('diagnostics', () => {
  it('reports versions and settings but never a name, path or template', () => {
    const text = diagnostics(about, null, [
      s('metadata.default_creator', 'Morii Zhou'),
      s('metadata.copyright_template', '© {year} Morii'),
      s('backup.root', 'E:\\Photo backups\\2026'),
      s('exec.workers', '4', ''),
      s('backup.max_age_days', '30', '30'),
    ])
    expect(text).toContain('MoriMeta 0.1.0')
    expect(text).toContain('ExifTool: 13.59')
    expect(text).toContain('metadata.default_creator: (set)')
    expect(text).toContain('backup.root: (set)')
    expect(text).toContain('exec.workers: 4')
    expect(text).toContain('backup.max_age_days: 30 (default)')
    expect(text).not.toMatch(/Morii|Photo backups|\{year\}/)
  })
})
