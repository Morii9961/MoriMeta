// SPDX-License-Identifier: GPL-3.0-or-later
// EmptyState, session empty (SCREEN_SPEC 1#empty): what to do, with the safety lines and the
// supported formats.

import { useT } from '../../i18n'
import { addFiles, addFolder } from '../../app/actions'

export function EmptyLibrary() {
  const t = useT()
  return (
    <div className="library-empty">
      <div className="empty-state">
        <h2>{t('empty.title')}</h2>
        <p>{t('empty.lead')}</p>
        <div className="actions">
          <button className="btn accent" onClick={addFolder}>
            {t('menu.add_folder')} <span className="mono faint">Ctrl ⇧ O</span>
          </button>
          <button className="btn" onClick={addFiles}>
            {t('menu.add_files')} <span className="mono faint">Ctrl O</span>
          </button>
        </div>
        <ul>
          <li>{t('empty.safe1')}</li>
          <li>{t('empty.safe2')}</li>
          <li>{t('empty.safe3')}</li>
        </ul>
        <p className="faint mono formats">{t('empty.formats')}</p>
      </div>
    </div>
  )
}
