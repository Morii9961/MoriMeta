// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { messages } from './messages'

const params = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort()

describe('UI texts', () => {
  const entries = Object.entries(messages) as [string, readonly string[]][]

  it('have an English and a Chinese text each', () => {
    const bad = entries.filter(([, v]) => v.length !== 2 || v.some((x) => typeof x !== 'string' || x.trim() === ''))
    expect(bad.map(([k]) => k)).toEqual([])
  })

  it('use the same parameters in both languages', () => {
    const bad = entries.filter(([, [en, zh]]) => params(en).join() !== params(zh).join())
    expect(bad.map(([k, [en, zh]]) => `${k}: ${params(en)} / ${params(zh)}`)).toEqual([])
  })

  it('are translated (a Chinese text equal to the English one is a name or a code)', () => {
    // texts that stay the same: product and component names, formats, symbols, examples
    const same = entries.filter(([, [en, zh]]) => en === zh).map(([, [en]]) => en)
    const untranslated = same.filter((en) => /[a-z]{3,} [a-z]{3,}/.test(en))
    expect(untranslated).toEqual([])
  })
})
