// SPDX-License-Identifier: GPL-3.0-or-later
// Library (SCREEN_SPEC §1): Sources + facets | MetadataTable | Inspector slot (session summary,
// one file, batch panel, or the capture-time tools).

import { useState } from 'react'
import { useApp } from '../../state/store'
import { useT } from '../../i18n'
import { useVisibleItems } from './hooks'
import { Sidebar } from './Sidebar'
import { MetadataTable } from './MetadataTable'
import { EmptyLibrary } from './EmptyLibrary'
import { InspectorSlot } from '../inspector/InspectorSlot'
import { TimePreview } from '../inspector/TimePreview'
import { api } from '../../ipc'
import { ColumnChooser } from './ColumnChooser'
import { ConditionRow } from './Conditions'
import { COLUMNS } from './data'
import './library.css'

export function LibraryView() {
  const t = useT()
  const sidebarOpen = useApp((s) => s.sidebarOpen)
  const toggleSidebar = useApp((s) => s.toggleSidebar)
  const inspectorOpen = useApp((s) => s.inspectorOpen)
  const toggleInspector = useApp((s) => s.toggleInspector)
  const timeToolsOpen = useApp((s) => s.timeToolsOpen)
  const assets = useApp((s) => s.assets)
  const scan = useApp((s) => s.scan)
  const selection = useApp((s) => s.selection)
  const search = useApp((s) => s.search)
  const facets = useApp((s) => s.facets)
  const clearFacets = useApp((s) => s.clearFacets)
  const setSearch = useApp((s) => s.setSearch)
  const conditions = useApp((s) => s.conditions)
  const setConditions = useApp((s) => s.setConditions)
  const groupBy = useApp((s) => s.groupBy)
  const setGroupBy = useApp((s) => s.setGroupBy)
  const [chooser, setChooser] = useState(false)
  const visible = useVisibleItems()
  const filtered = search.trim() !== '' || Object.values(facets).some((v) => v.size > 0) || conditions.length > 0
  const clearAll = () => {
    clearFacets()
    setSearch('')
    setConditions([])
  }
  const groupCol = COLUMNS.find((c) => c.key === groupBy)

  return (
    <>
      {sidebarOpen ? (
        <Sidebar />
      ) : (
        <button className="pane-rail left" onClick={toggleSidebar} title={t('menu.sidebar')}>
          <span>{t('sidebar.rail')}</span>
        </button>
      )}
      <div className="pane-primary">
        <div className="table-toolbar">
          <span className="mono strong">
            {filtered
              ? t('table.count_filtered', { shown: visible.length, n: assets.length })
              : t('table.count', { n: assets.length })}
          </span>
          {selection.size > 0 && <span className="mono secondary">· {t('table.selected', { n: selection.size })}</span>}
          {filtered && (
            <button className="link" onClick={clearAll}>
              {t('table.clear_filter')}
            </button>
          )}
          {groupCol && (
            <span className="group-chip">
              {t('cols.grouped_by', { name: t(groupCol.label) })}
              <button className="btn plain tiny" aria-label={t('cols.ungroup')} onClick={() => setGroupBy(null)}>
                ×
              </button>
            </span>
          )}
          <div className="toolbar-spacer" />
          <span className="faint">{t('table.hint')}</span>
          {assets.length > 0 && (
            <>
              <button className="btn small plain" onClick={() => useApp.setState({ addingCondition: true })}>
                + {t('cond.add')}
              </button>
              <span className="chooser-wrap">
                <button className="btn small plain columns-button" aria-expanded={chooser} onClick={() => setChooser((v) => !v)}>
                  {t('cols.button')}
                </button>
                {chooser && <ColumnChooser onClose={() => setChooser(false)} />}
              </span>
            </>
          )}
        </div>
        <ConditionRow />
        {scan.running && (
          <div className="scan-strip" role="progressbar" aria-valuemin={0} aria-valuemax={scan.total} aria-valuenow={scan.done}>
            <span className="mono">
              {t('scan.reading', { done: Math.min(scan.done, scan.total), total: scan.total })}
            </span>
            <span className="mono faint">{scan.total ? Math.floor((100 * Math.min(scan.done, scan.total)) / scan.total) : 0}%</span>
            <div className="scan-bar">
              <div style={{ width: `${scan.total ? (100 * Math.min(scan.done, scan.total)) / scan.total : 0}%` }} />
            </div>
            <span className="muted">{t('scan.read_only')}</span>
            <button className="btn small" onClick={() => api.scanCancel()}>
              {t('scan.cancel')} <span className="mono faint">Esc</span>
            </button>
          </div>
        )}
        {timeToolsOpen && assets.length > 0 ? (
          <TimePreview />
        ) : assets.length === 0 ? (
          <EmptyLibrary />
        ) : visible.length === 0 ? (
          <div className="table-empty">
            <span className="muted">{t('table.no_match')}</span>
            <button className="link" onClick={clearAll}>
              {t('table.clear_filter')}
            </button>
          </div>
        ) : (
          <MetadataTable items={visible} onColumns={() => setChooser(true)} />
        )}
      </div>
      {inspectorOpen || timeToolsOpen ? (
        <InspectorSlot />
      ) : (
        <button className="pane-rail right" onClick={toggleInspector} title={t('menu.inspector')}>
          <span>{t('insp.rail')}</span>
        </button>
      )}
    </>
  )
}
