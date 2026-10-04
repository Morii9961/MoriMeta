// SPDX-License-Identifier: GPL-3.0-or-later
import { useMemo } from 'react'
import { useApp } from '../../state/store'
import { filterItems, sortItems, type Item } from './data'
import { matchesAll } from './layout'

/** Every file of the Session with its row as last read. */
export function useItems(): Item[] {
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  return useMemo(() => assets.map((asset) => ({ asset, row: rows.get(asset.id) })), [assets, rows])
}

/** The table's rows: filtered by search, facets and conditions, then sorted. */
export function useVisibleItems(): Item[] {
  const items = useItems()
  const search = useApp((s) => s.search)
  const facets = useApp((s) => s.facets)
  const conditions = useApp((s) => s.conditions)
  const sort = useApp((s) => s.sort)
  return useMemo(
    () => sortItems(filterItems(items, search, facets).filter((it) => matchesAll(it, conditions)), sort),
    [items, search, facets, conditions, sort],
  )
}
