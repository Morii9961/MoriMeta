// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'

// every stylesheet and component as text, read from disk (Vitest gives CSS imports no content)
async function sources(): Promise<Record<string, string>> {
  const name = 'node:fs'
  const fs = await import(/* @vite-ignore */ name)
  const dir = new URL('.', import.meta.url)
  const out: Record<string, string> = {}
  for (const f of fs.readdirSync(dir, { recursive: true }) as string[]) {
    if (/\.(css|tsx?)$/.test(f) && !/\.test\.ts$/.test(f)) out[f] = fs.readFileSync(new URL(f.replaceAll('\\', '/'), dir), 'utf8')
  }
  return out
}

describe('styles', () => {
  it('use only custom properties that are defined (an undefined var() silently drops the rule)', async () => {
    const files = await sources()
    const all = Object.values(files).join('\n')
    const defined = new Set([...all.matchAll(/(--[a-z0-9-]+)\s*:/g), ...all.matchAll(/['"](--[a-z0-9-]+)['"]\s*[:\]]/g)].map((m) => m[1]))
    expect(Object.keys(files).filter((f) => f.endsWith('.css')).length).toBeGreaterThan(5)
    expect(defined.size).toBeGreaterThan(50)
    const missing = new Set<string>()
    for (const [file, text] of Object.entries(files)) {
      for (const m of text.matchAll(/var\((--[a-z0-9-]+)\s*([,)])/g)) {
        // a var() with a fallback may name an undefined property
        if (m[2] === ')' && !defined.has(m[1])) missing.add(`${m[1]} in ${file}`)
      }
    }
    expect([...missing]).toEqual([])
  })
})
