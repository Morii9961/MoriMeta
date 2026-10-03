// SPDX-License-Identifier: GPL-3.0-or-later
// Development-only update simulation. It never downloads an artifact or starts an installer.
import type { UpdateInfo } from './types'

export function createMockUpdates() {
  let generation = 0
  let state: UpdateInfo = { configured: true, busy: false, phase: 'idle', offer: null, downloaded: false, bytes: 0, total: null, error: null }
  const snapshot = () => structuredClone(state)
  const begin = (phase: string) => {
    if (state.busy) throw 'an update request is already running'
    state = { ...state, phase, busy: true, error: null }
    return ++generation
  }
  const selected = (id: string) => {
    if (state.offer?.id !== id) throw 'the update offer changed; check for updates again'
  }
  const wait = () => new Promise<void>((resolve) => setTimeout(resolve, 250))
  return {
    status: snapshot,
    check: async () => {
      const job = begin('checking')
      await wait()
      if (job !== generation) throw 'update check cancelled'
      state = { ...state, busy: false, phase: 'available', offer: { id: `mock-update-${job}`, version: '0.2.0-preview', notes: 'Simulated release notes. No network request, file download, or installation takes place.', date: null }, downloaded: false, bytes: 0, total: null }
      return snapshot()
    },
    download: async (id: string) => {
      selected(id)
      const job = begin('downloading')
      state = { ...state, bytes: 0, total: 4_000_000, downloaded: false }
      for (let i = 1; i <= 8; ++i) {
        await wait()
        if (job !== generation) throw 'update download cancelled or too large'
        state.bytes = i * 500_000
      }
      state = { ...state, busy: false, phase: 'ready', downloaded: true }
      return snapshot()
    },
    cancel: () => {
      if (!state.busy) return
      ++generation
      state = { ...state, busy: false, phase: 'failed', error: 'update check cancelled' }
    },
    install: (id: string) => {
      selected(id)
      if (!state.downloaded) throw 'download and verify the update first'
      state = { ...state, phase: 'simulated_install', downloaded: false }
    },
  }
}
