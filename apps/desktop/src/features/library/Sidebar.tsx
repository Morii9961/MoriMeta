// SPDX-License-Identifier: GPL-3.0-or-later
// Library sidebar (DESIGN_SYSTEM Sidebar, mode `library`): Sources (the folders of this Session)
// and facets (OR within a group, AND across groups; amber counts = attention).

import { useMemo } from 'react'
import { useApp } from '../../state/store'
import { useT, type T } from '../../i18n'
import { FACET_GROUPS, facetGroups } from './data'
import { useItems } from './hooks'

function facetLabel(t: T, group: string, value: string): string {
  if (value === '') return t('facet.none')
  if (group === 'writes_to') {
    const k = `writes.${value}` as const
    return t(k as Parameters<T>[0])
  }
  if (group === 'gps' || group === 'copyright' || group === 'creator') {
    return value === 'present' ? t('facet.present') : t('facet.empty')
  }
  if (group === 'attention') {
    return t(`facet.att_${value}` as Parameters<T>[0])
  }
  return value
}

export function Sidebar() {
  const t = useT()
  const items = useItems()
  const facets = useApp((s) => s.facets)
  const toggleFacet = useApp((s) => s.toggleFacet)
  const clearFacets = useApp((s) => s.clearFacets)
  const assets = useApp((s) => s.assets)
  const groups = useMemo(() => facetGroups(items), [items])
  const folders = useMemo(() => {
    const m = new Map<string, number>()
    for (const a of assets) m.set(a.folder, (m.get(a.folder) ?? 0) + 1)
    return [...m.entries()].sort((a, b) => a[0].localeCompare(b[0]))
  }, [assets])

  return (
    <nav className="pane-sidebar" aria-label={t('sidebar.label')}>
      <div className="side-group">
        <div className="side-group-header">
          <span className="section-label">{t('sidebar.sources')}</span>
        </div>
        {folders.length === 0 && <div className="side-empty muted">{t('sidebar.no_folders')}</div>}
        {folders.map(([folder, n]) => (
          <div key={folder} className="source-row" title={folder}>
            <span className="source-glyph faint">▸</span>
            <span className="ellipsis mono">{folder.split(/[\\/]/).pop() || folder}</span>
            <span className="mono faint count">{n}</span>
          </div>
        ))}
      </div>
      {assets.length > 0 &&
        groups.map((g) => {
          const meta = FACET_GROUPS.find((x) => x.key === g.key)!
          const active = facets[g.key]
          if (g.counts.size === 0) return null
          const values = [...g.counts.entries()].sort((a, b) => b[1] - a[1]).slice(0, 12)
          return (
            <div key={g.key} className="side-group">
              <div className="side-group-header">
                <span className="section-label">{t(meta.label)}</span>
                {active && active.size > 0 ? (
                  <button className="link side-clear" onClick={() => clearFacets(g.key)}>
                    {t('sidebar.clear')}
                  </button>
                ) : (
                  <span className="faint side-any">{t('sidebar.any')}</span>
                )}
              </div>
              {values.map(([value, n]) => {
                const id = `facet-${g.key}-${value}`
                const attention = g.key === 'attention'
                return (
                  <label key={value} className="facet-row" htmlFor={id} title={value}>
                    <input
                      id={id}
                      type="checkbox"
                      className="checkbox"
                      checked={!!active?.has(value)}
                      onChange={() => toggleFacet(g.key, value)}
                    />
                    <span className={`ellipsis${value === '' ? ' faint' : ''}`}>{facetLabel(t, g.key, value)}</span>
                    <span className={`mono count${attention ? ' glyph-warn' : ' faint'}`}>{n}</span>
                  </label>
                )
              })}
            </div>
          )
        })}
    </nav>
  )
}
