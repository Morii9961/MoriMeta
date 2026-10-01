// SPDX-License-Identifier: GPL-3.0-or-later
// Presets and Rules (SCREEN_SPEC §5–6). The backend has Presets, Rules and their Plans; the
// manager and rule builder screens come next. Until then this page says what exists and points to
// the batch editor, which stages the same actions.

import { useApp } from '../../state/store'
import { useT } from '../../i18n'

export function PresetsView() {
  const t = useT()
  const setModule = useApp((s) => s.setModule)
  return (
    <div className="pane-primary">
      <div className="library-empty">
        <div className="empty-state">
          <h2>{t('presets.title')}</h2>
          <p>{t('presets.lead')}</p>
          <ul>
            <li>{t('presets.builtin_1')}</li>
            <li>{t('presets.builtin_2')}</li>
          </ul>
          <div className="actions">
            <button className="btn accent" onClick={() => setModule('library')}>
              {t('presets.to_library')}
            </button>
          </div>
        </div>
      </div>
    </div>
  )
}
