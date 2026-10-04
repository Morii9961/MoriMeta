// SPDX-License-Identifier: GPL-3.0-or-later
// Interface density and scale (SCREEN_SPEC 5#s-general): per-viewer conveniences kept in browser
// storage, applied to the document root. Reading or writing storage may fail; defaults apply.

export type Density = 'compact' | 'comfortable'

export interface UiPrefs {
  density: Density
  /** Percent; one of SCALES. */
  scale: number
}

export const SCALES = [90, 100, 110, 125] as const

export const DEFAULT_PREFS: UiPrefs = { density: 'compact', scale: 100 }

const KEY = 'mm.ui'

export function loadPrefs(): UiPrefs {
  try {
    const v = JSON.parse(localStorage.getItem(KEY) ?? 'null') as Partial<UiPrefs> | null
    return {
      density: v?.density === 'comfortable' ? 'comfortable' : 'compact',
      scale: SCALES.includes(v?.scale as (typeof SCALES)[number]) ? (v!.scale as number) : 100,
    }
  } catch {
    return DEFAULT_PREFS
  }
}

export function savePrefs(p: UiPrefs): void {
  try {
    localStorage.setItem(KEY, JSON.stringify(p))
  } catch {
    // a per-viewer convenience only
  }
}

/** Density on the document root; the scale is the window's own zoom, set by the backend. */
export function applyDensity(p: UiPrefs): void {
  document.documentElement.dataset.density = p.density
}

/** Table row height for the density (DESIGN_SYSTEM MetadataTable: 22 px compact). */
export function rowHeight(d: Density): number {
  return d === 'comfortable' ? 26 : 22
}
