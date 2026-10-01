// SPDX-License-Identifier: GPL-3.0-or-later
// Library (SCREEN_SPEC §1): Sources + facets | MetadataTable | Inspector slot (session summary,
// one file, batch panel, or the capture-time tools).

import { useApp } from '../../state/store'
import { useT } from '../../i18n'
import { useVisibleItems } from './hooks'
import { Sidebar } from './Sidebar'
import { MetadataTable } from './MetadataTable'
import { EmptyLibrary } from './EmptyLibrary'
import { InspectorSlot } from '../inspector/InspectorSlot'
import { api } from '../../ipc'
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
  const visible = useVisibleItems()
  const filtered = search.trim() !== '' || Object.values(facets).some((v) => v.size > 0)

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
            <button
              className="link"
              onClick={() => {
                clearFacets()
                setSearch('')
              }}
            >
              {t('table.clear_filter')}
            </button>
          )}
          <div className="toolbar-spacer" />
          <span className="faint">{t('table.hint')}</span>
        </div>
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
        {assets.length === 0 ? (
          <EmptyLibrary />
        ) : visible.length === 0 ? (
          <div className="table-empty">
            <span className="muted">{t('table.no_match')}</span>
            <button
              className="link"
              onClick={() => {
                clearFacets()
                setSearch('')
              }}
            >
              {t('table.clear_filter')}
            </button>
          </div>
        ) : (
          <MetadataTable items={visible} />
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
