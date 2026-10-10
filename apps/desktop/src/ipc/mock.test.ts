// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import ipcSource from './index.ts?raw'
import { mockCommands } from './mock'

describe('the development mock', () => {
  it('answers every command the UI calls', () => {
    const called = [...new Set([...ipcSource.matchAll(/\bcall(?:<[^>]*>)?\('([a-z_]+)'/g)].map((m) => m[1]))]
    expect(called.length).toBeGreaterThan(50)
    expect(called.filter((c) => !mockCommands.includes(c))).toEqual([])
  })
})
