// SPDX-License-Identifier: GPL-3.0-or-later
import { useMemo } from 'react'
import { useApp } from '../../state/store'
import { filterItems, sortItems, type Item } from './data'

/** Every file of the Session with its row as last read. */
export function useItems(): Item[] {
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  return useMemo(() => assets.map((asset) => ({ asset, row: rows.get(asset.id) })), [assets, rows])
}

/** The table's rows: filtered by search and facets, then sorted. */
export function useVisibleItems(): Item[] {
  const items = useItems()
  const search = useApp((s) => s.search)
  const facets = useApp((s) => s.facets)
  const sort = useApp((s) => s.sort)
  return useMemo(() => sortItems(filterItems(items, search, facets), sort), [items, search, facets, sort])
}
