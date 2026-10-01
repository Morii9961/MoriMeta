// SPDX-License-Identifier: GPL-3.0-or-later
// The inspector slot (DESIGN.md §3): Session summary with nothing selected (SCREEN_SPEC
// 1#default), the Inspector for one file, the batch panel for several (SCREEN_SPEC §3), or the
// capture-time tools (§4), which take the slot at 440/500 px.

import { useApp } from '../../state/store'
import { SessionSummary } from './SessionSummary'
import { FileInspector } from './FileInspector'
import { BatchPanel } from './BatchPanel'
import { TimeTools } from './TimeTools'
import './inspector.css'

export function InspectorSlot() {
  const selection = useApp((s) => s.selection)
  const timeToolsOpen = useApp((s) => s.timeToolsOpen)
  if (timeToolsOpen && selection.size > 0) {
    return (
      <aside className="pane-inspector wide">
        <TimeTools />
      </aside>
    )
  }
  return (
    <aside className="pane-inspector">
      {selection.size === 0 ? (
        <SessionSummary />
      ) : selection.size === 1 ? (
        <FileInspector id={[...selection][0]} />
      ) : (
        <BatchPanel />
      )}
    </aside>
  )
}
