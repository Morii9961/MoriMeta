// SPDX-License-Identifier: GPL-3.0-or-later
// MetadataInspector for one file (DESIGN_SYSTEM, SCREEN_SPEC §2): header, layer toggle
// (Effective / In file / Sidecar), section strip, the expanded section. Only the four MVP fields
// are editable (DECISIONS H-5): editing stages into the plan, never writes (INTERACTION_SPEC §3).

import { useEffect, useMemo, useState } from 'react'
import { api, errorText } from '../../ipc'
import type { AssetDetail, FieldView } from '../../ipc/types'
import { useApp } from '../../state/store'
import { useT, useBT, fieldLabel, type MessageKey, type T } from '../../i18n'
import { sizeText } from '../library/data'
import { BatchFields } from './BatchPanel'

type Layer = 'effective' | 'file' | 'sidecar'
type Section = 'edit' | 'capture' | 'camera' | 'creator' | 'location' | 'basic' | 'advanced'

const SECTIONS: { key: Section; label: MessageKey }[] = [
  { key: 'edit', label: 'insp.sec_edit' },
  { key: 'capture', label: 'insp.sec_capture' },
  { key: 'camera', label: 'insp.sec_camera' },
  { key: 'creator', label: 'insp.sec_creator' },
  { key: 'location', label: 'insp.sec_location' },
  { key: 'basic', label: 'insp.sec_basic' },
  { key: 'advanced', label: 'insp.sec_advanced' },
]

/** The first tag whose name (without group) is one of `names`, in `names` order. */
function tag(tags: Record<string, string>, ...names: string[]): [string, string] | null {
  for (const n of names) {
    for (const [k, v] of Object.entries(tags)) {
      if (k === n || k.endsWith(`:${n}`)) return [k, v]
    }
  }
  return null
}

function Provenance({ k }: { k: string | null }) {
  if (!k) return <span className="prov faint">—</span>
  return (
    <span className="prov mono faint ellipsis" title={k}>
      {k.split(':')[0]}
    </span>
  )
}

function Line({ label, value, k, lock, t }: { label: string; value: string | null | undefined; k?: string | null; lock?: boolean; t: T }) {
  return (
    <div className="field-line">
      <span className="field-label ellipsis" title={label}>
        {label}
      </span>
      <span className={`field-value mono selectable ${value ? '' : 'empty-value'}`} title={value ?? undefined}>
        {value || '—'}
      </span>
      <span className="prov-cell">
        <Provenance k={k ?? null} />
        {lock && <span className="faint" title={t('insp.protected')}> · {t('insp.lock')}</span>}
      </span>
    </div>
  )
}

function FieldBlock({ f, t }: { f: FieldView; t: T }) {
  const bt = useBT()
  return (
    <div className={`field-block${f.conflicting ? ' conflict' : ''}`}>
      <div className="field-line">
        <span className="field-label">{fieldLabel(t, f.field)}</span>
        <span className={`field-value mono selectable ${f.value ? '' : 'empty-value'}`} title={f.value ?? undefined}>
          {f.conflicting && <span className="glyph-conflict">≠ </span>}
          {f.value || '—'}
        </span>
        <span className="prov-cell">
          <Provenance k={f.sources[0]?.[0] ?? null} />
        </span>
      </div>
      {f.error && <div className="field-note glyph-fail">{bt(f.error)}</div>}
      {f.sources.length > 1 && (
        <div className="sources">
          {f.sources.map(([k, v]) => (
            <div key={k} className="source-line">
              <span className="mono faint ellipsis" title={k}>
                {k}
              </span>
              <span className="mono ellipsis selectable" title={v}>
                {v}
              </span>
            </div>
          ))}
          {f.conflicting && <div className="field-note glyph-conflict">{t('insp.conflict_note')}</div>}
        </div>
      )}
    </div>
  )
}

export function FileInspector({ id }: { id: number }) {
  const t = useT()
  const bt = useBT()
  const assets = useApp((s) => s.assets)
  const rows = useApp((s) => s.rows)
  const asset = assets.find((a) => a.id === id)
  const row = rows.get(id)
  const [detail, setDetail] = useState<AssetDetail | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [layer, setLayer] = useState<Layer>('effective')
  const [section, setSection] = useState<Section>(asset?.writable === false ? 'capture' : 'edit')
  const [filter, setFilter] = useState('')

  useEffect(() => {
    let alive = true
    setDetail(null)
    setError(null)
    if (row?.not_downloaded) {
      setError(t('insp.not_downloaded'))
      return
    }
    api
      .assetDetail(id)
      .then((d) => alive && setDetail(d))
      .catch((e) => alive && setError(errorText(e)))
    return () => {
      alive = false
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id, row?.not_downloaded, row])

  const tags = useMemo(() => {
    if (!detail) return {}
    if (layer === 'file') return detail.tags
    if (layer === 'sidecar') return detail.sidecar_tags ?? {}
    return { ...detail.tags, ...(detail.sidecar_tags ?? {}) }
  }, [detail, layer])

  if (!asset) return null
  const index = assets.findIndex((a) => a.id === id)
  const writesTo = asset.writable ? row?.writes_to : 'read_only'
  const field = (n: string) => detail?.fields.find((f) => f.field === n)

  return (
    <div className="insp-file">
      <div className="insp-header">
        <div className="insp-name mono strong ellipsis" title={asset.name}>
          {asset.name}
        </div>
        <div className="insp-meta">
          <span className="mono">{asset.ext}</span>
          <span className="mono faint">· {sizeText(asset.size)}</span>
          <span className="faint">·</span>
          <span>{writesTo ? t(`writes.${writesTo}` as MessageKey) : '…'}</span>
          <span className="mono faint insp-pos">
            {index + 1} / {assets.length}
          </span>
        </div>
        <div className="insp-path mono faint ellipsis selectable" title={asset.path}>
          {asset.folder}
        </div>
        <div className="segmented small insp-layers" role="tablist">
          {(['effective', 'file', 'sidecar'] as Layer[]).map((l) => (
            <button
              key={l}
              aria-pressed={layer === l}
              disabled={l === 'sidecar' && !detail?.sidecar_tags}
              onClick={() => setLayer(l)}
            >
              {t(`insp.layer_${l}` as MessageKey)}
            </button>
          ))}
        </div>
      </div>
      <div className="section-strip" role="tablist">
        {SECTIONS.filter((s) => s.key !== 'edit' || asset.writable).map((s) => (
          <button key={s.key} role="tab" aria-selected={section === s.key} className={section === s.key ? 'on' : ''} onClick={() => setSection(s.key)}>
            {t(s.label)}
          </button>
        ))}
      </div>
      <div className="pane-scroll insp">
        {error && (
          <div className="tone error insp-error">
            <b>× {t('insp.cannot_read')}</b>
            <div className="selectable">{bt(error)}</div>
            <div className="muted">{t('insp.files_safe')}</div>
          </div>
        )}
        {!asset.writable && (
          <div className="tone neutral insp-error">
            <b>⊘ {t('insp.read_only_format', { ext: asset.ext })}</b>
            <div>{t('insp.read_only_note')}</div>
          </div>
        )}
        {!detail && !error && <p className="note">{t('insp.reading')}</p>}
        {section === 'edit' && asset.writable && <BatchFields ids={[id]} />}
        {detail && section === 'capture' && (
          <div className="insp-section">
            {field('capture_time') && <FieldBlock f={field('capture_time')!} t={t} />}
            <Line t={t} label={t('insp.offset')} value={tag(tags, 'OffsetTimeOriginal')?.[1]} k={tag(tags, 'OffsetTimeOriginal')?.[0]} />
            <Line t={t} label={t('insp.subsec')} value={tag(tags, 'SubSecTimeOriginal')?.[1]} k={tag(tags, 'SubSecTimeOriginal')?.[0]} lock />
            <Line t={t} label={t('insp.created')} value={tag(tags, 'CreateDate')?.[1]} k={tag(tags, 'CreateDate')?.[0]} />
            <Line t={t} label={t('insp.modified')} value={tag(tags, 'ModifyDate')?.[1]} k={tag(tags, 'ModifyDate')?.[0]} lock />
            <Line t={t} label={t('insp.gps_utc')} value={[tag(tags, 'GPSDateStamp')?.[1], tag(tags, 'GPSTimeStamp')?.[1]].filter(Boolean).join(' ') || null} k={tag(tags, 'GPSDateStamp')?.[0]} lock />
            <p className="note">{t('insp.capture_note')}</p>
            <button className="btn small" onClick={() => useApp.getState().setTimeToolsOpen(true)}>
              {t('menu.time_tools')} <span className="mono faint">T</span>
            </button>
          </div>
        )}
        {detail && section === 'camera' && (
          <div className="insp-section">
            {(
              [
                ['insp.make', ['Make']],
                ['insp.model', ['Model']],
                ['insp.serial', ['SerialNumber', 'InternalSerialNumber']],
                ['insp.lens', ['LensModel', 'Lens', 'LensID']],
                ['insp.focal', ['FocalLength']],
                ['insp.aperture', ['FNumber', 'Aperture']],
                ['insp.shutter', ['ExposureTime', 'ShutterSpeed']],
                ['insp.iso', ['ISO']],
                ['insp.ec', ['ExposureCompensation']],
                ['insp.flash', ['Flash']],
              ] as [MessageKey, string[]][]
            ).map(([label, names]) => {
              const hit = tag(tags, ...names)
              const masked = label === 'insp.serial' && hit ? `••••${hit[1].slice(-3)}` : hit?.[1]
              return <Line key={label} t={t} label={t(label)} value={masked} k={hit?.[0]} lock />
            })}
            <p className="note">{t('insp.camera_note')}</p>
          </div>
        )}
        {detail && section === 'creator' && (
          <div className="insp-section">
            {field('creator') && <FieldBlock f={field('creator')!} t={t} />}
            {field('copyright') && <FieldBlock f={field('copyright')!} t={t} />}
            <Line t={t} label={t('insp.credit')} value={tag(tags, 'Credit')?.[1]} k={tag(tags, 'Credit')?.[0]} lock />
            <Line t={t} label={t('insp.website')} value={tag(tags, 'CreatorWorkURL', 'WebStatement')?.[1]} k={tag(tags, 'CreatorWorkURL', 'WebStatement')?.[0]} lock />
            <Line t={t} label={t('insp.email')} value={tag(tags, 'CreatorWorkEmail')?.[1]} k={tag(tags, 'CreatorWorkEmail')?.[0]} lock />
            <p className="note">{t('insp.creator_note')}</p>
          </div>
        )}
        {detail && section === 'location' && (
          <div className="insp-section">
            {field('gps') && <FieldBlock f={field('gps')!} t={t} />}
            <Line t={t} label={t('insp.altitude')} value={tag(tags, 'GPSAltitude')?.[1]} k={tag(tags, 'GPSAltitude')?.[0]} />
            <Line t={t} label={t('insp.country')} value={tag(tags, 'Country', 'Country-PrimaryLocationName')?.[1]} k={tag(tags, 'Country', 'Country-PrimaryLocationName')?.[0]} lock />
            <Line t={t} label={t('insp.city')} value={tag(tags, 'City')?.[1]} k={tag(tags, 'City')?.[0]} lock />
            {writesTo === 'sidecar' || writesTo === 'new_sidecar' ? (
              <div className="tone warn insp-error">{t('insp.raw_gps_note')}</div>
            ) : null}
          </div>
        )}
        {detail && section === 'basic' && (
          <div className="insp-section">
            <Line t={t} label={t('insp.title')} value={tag(tags, 'Title', 'ObjectName')?.[1]} k={tag(tags, 'Title', 'ObjectName')?.[0]} lock />
            <Line t={t} label={t('insp.description')} value={tag(tags, 'Description', 'ImageDescription', 'Caption-Abstract')?.[1]} k={tag(tags, 'Description', 'ImageDescription', 'Caption-Abstract')?.[0]} lock />
            <Line t={t} label={t('insp.rating')} value={tag(tags, 'Rating')?.[1]} k={tag(tags, 'Rating')?.[0]} lock />
            <Line t={t} label={t('insp.label')} value={tag(tags, 'Label')?.[1]} k={tag(tags, 'Label')?.[0]} lock />
            <Line t={t} label={t('insp.keywords')} value={tag(tags, 'Subject', 'Keywords')?.[1]} k={tag(tags, 'Subject', 'Keywords')?.[0]} lock />
            <p className="note">{t('insp.basic_note')}</p>
          </div>
        )}
        {detail && section === 'advanced' && (
          <div className="insp-section">
            <input className="input ui adv-filter" placeholder={t('insp.filter_tags')} value={filter} onChange={(e) => setFilter(e.target.value)} />
            <div className="adv-list">
              {Object.entries(tags)
                .filter(([k, v]) => !filter || k.toLowerCase().includes(filter.toLowerCase()) || v.toLowerCase().includes(filter.toLowerCase()))
                .sort(([a], [b]) => a.localeCompare(b))
                .map(([k, v]) => (
                  <div key={k} className="adv-row">
                    <span className="mono faint ellipsis" title={k}>
                      {k}
                    </span>
                    <span className="mono ellipsis selectable" title={v}>
                      {v}
                    </span>
                  </div>
                ))}
            </div>
            <p className="note">{t('insp.advanced_note')}</p>
          </div>
        )}
      </div>
    </div>
  )
}
