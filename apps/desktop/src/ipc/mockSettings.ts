// SPDX-License-Identifier: GPL-3.0-or-later
// Development-only preferences are kept in memory for realistic screen interactions.
import type { Setting } from './types'
const DEFAULTS: Record<string, string> = {
  'backup.root': '', 'backup.max_age_days': '30', 'backup.max_share_of_volume': '0.1', 'backup.keep_latest': '10',
  'metadata.preserve_mtime': 'false', 'metadata.default_creator': '', 'metadata.copyright_template': '© {creator} {year}',
  'exec.workers': '', 'log.debug_since_ms': '', 'updates.check': 'ask', 'ui.setup_done': 'false',
}
export function createMockSettings() {
  const values = new Map<string, string>()
  return {
    list: (): Setting[] => Object.entries(DEFAULTS).map(([name, value]) => ({ name, default: value, value: values.get(name) ?? value, about: '' })),
    set: (name: string, value: string) => {
      if (!(name in DEFAULTS)) throw `unknown setting ${name}`
      if (value === '') values.delete(name); else values.set(name, value)
    },
    reset: () => values.clear(),
  }
}
