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

/** `35.6895, 139.6917[, 40 m]` (the GPS field's display) → degrees, minutes, seconds. */
export function dms(value: string | null | undefined): string | null {
  const m = /^(-?\d+(?:\.\d+)?),\s*(-?\d+(?:\.\d+)?)/.exec(value ?? '')
  if (!m) return null
  const part = (v: number, pos: string, neg: string) => {
    const a = Math.abs(v)
    let d = Math.floor(a)
    let mi = Math.floor((a - d) * 60)
    let s = Math.round(((a - d) * 60 - mi) * 600) / 10
    if (s >= 60) {
      s -= 60
      mi += 1
    }
    if (mi >= 60) {
      mi -= 60
      d += 1
    }
    return `${d}°${String(mi).padStart(2, '0')}′${s.toFixed(1).padStart(4, '0')}″${v < 0 ? neg : pos}`
  }
  return `${part(Number(m[1]), 'N', 'S')} ${part(Number(m[2]), 'E', 'W')}`
}

async function copyText(text: string, done: string) {
  const s = useApp.getState()
  try {
    await navigator.clipboard.writeText(text)
    s.notify('success', done)
  } catch (e) {
    s.notify('error', errorText(e))
  }
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

function FieldBlock({ f, t, id }: { f: FieldView; t: T; id?: number }) {
  const bt = useBT()
  const [kept, setKept] = useState(false)
  const stageEdit = useApp((s) => s.stageEdit)
  const notify = useApp((s) => s.notify)
  // SCREEN_SPEC 1#i-creator: settle a conflict with the effective value, through the Preview
  const settle =
    f.conflicting && f.value && id !== undefined && (f.field === 'creator' || f.field === 'copyright')
      ? () => {
          if (f.field === 'creator') stageEdit({ creator: { op: 'set', values: f.value!.split('; ') } })
          else stageEdit({ copyright: { op: 'set', value: f.value! } })
          notify('info', t('insp.settle_staged'))
        }
      : null
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
          {settle && !kept && (
            <div className="conflict-actions">
              <button className="btn small" onClick={settle}>{t('insp.use_everywhere', { value: f.value! })}</button>
              <button className="btn small plain" onClick={() => setKept(true)}>{t('insp.keep_as_is')}</button>
            </div>
          )}
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
  const [group, setGroup] = useState('')
  const [showSerial, setShowSerial] = useState(false)
  const [preview, setPreview] = useState<string | null | undefined>(undefined)

  useEffect(() => {
    setShowSerial(false)
    setPreview(undefined)
  }, [id])
  useEffect(() => {
    if (section !== 'basic' || preview !== undefined || row?.not_downloaded) return
    let alive = true
    api
      .assetPreview(id)
      .then((p) => alive && setPreview(p && p.startsWith('data:image/jpeg;base64,') ? p : null))
      .catch(() => alive && setPreview(null))
    return () => {
      alive = false
    }
  }, [section, preview, id, row?.not_downloaded])

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
            {field('capture_time') && <FieldBlock f={field('capture_time')!} t={t} id={id} />}
            <Line t={t} label={t('insp.offset')} value={tag(tags, 'OffsetTimeOriginal')?.[1]} k={tag(tags, 'OffsetTimeOriginal')?.[0]} />
            <Line t={t} label={t('insp.subsec')} value={tag(tags, 'SubSecTimeOriginal')?.[1]} k={tag(tags, 'SubSecTimeOriginal')?.[0]} lock />
            <Line t={t} label={t('insp.created')} value={tag(tags, 'CreateDate')?.[1]} k={tag(tags, 'CreateDate')?.[0]} />
            <Line t={t} label={t('insp.modified')} value={tag(tags, 'ModifyDate')?.[1]} k={tag(tags, 'ModifyDate')?.[0]} lock />
            <Line t={t} label={t('insp.gps_utc')} value={[tag(tags, 'GPSDateStamp')?.[1], tag(tags, 'GPSTimeStamp')?.[1]].filter(Boolean).join(' ') || null} k={tag(tags, 'GPSDateStamp')?.[0]} lock />
            <p className="note">{t('insp.capture_note')}</p>
            <div className="insp-actions">
              <button className="btn small" onClick={() => useApp.getState().setTimeToolsOpen(true)}>
                {t('menu.time_tools')} <span className="mono faint">T</span>
              </button>
              <button className="btn small" disabled={!field('capture_time')?.value} onClick={() => void copyText(field('capture_time')?.value ?? '', t('insp.copied'))}>
                {t('insp.copy_time')}
              </button>
            </div>
          </div>
        )}
        {detail && section === 'camera' && (() => {
          const lines = (
            [
              ['insp.make', ['Make'], false],
              ['insp.model', ['Model'], false],
              ['insp.serial', ['SerialNumber', 'InternalSerialNumber', 'BodySerialNumber'], true],
              ['insp.firmware', ['FirmwareVersion', 'Firmware', 'Software'], false],
              ['insp.lens', ['LensModel', 'Lens', 'LensID'], false],
              ['insp.lens_serial', ['LensSerialNumber'], true],
              ['insp.focal', ['FocalLength'], false],
              ['insp.aperture', ['FNumber', 'Aperture'], false],
              ['insp.shutter', ['ExposureTime', 'ShutterSpeed'], false],
              ['insp.iso', ['ISO'], false],
              ['insp.ec', ['ExposureCompensation'], false],
              ['insp.metering', ['MeteringMode'], false],
              ['insp.flash', ['Flash'], false],
              ['insp.shutter_count', ['ShutterCount', 'ImageCount'], false],
            ] as [MessageKey, string[], boolean][]
          ).map(([label, names, secret]) => {
            const hit = tag(tags, ...names)
            const shown = secret && hit && !showSerial ? `••••${hit[1].slice(-3)}` : hit?.[1]
            return { label, hit, shown, secret }
          })
          const anySecret = lines.some((l) => l.secret && l.hit)
          return (
            <div className="insp-section">
              {lines.map((l) => (
                <Line key={l.label} t={t} label={t(l.label)} value={l.shown} k={l.hit?.[0]} lock />
              ))}
              <div className="insp-actions">
                {anySecret && (
                  <button className="btn small plain" aria-pressed={showSerial} onClick={() => setShowSerial((v) => !v)}>
                    {showSerial ? t('insp.hide_serials') : t('insp.show_serials')}
                  </button>
                )}
                <button
                  className="btn small"
                  onClick={() =>
                    void copyText(
                      lines
                        .filter((l) => l.hit)
                        .map((l) => `${t(l.label)}: ${l.shown}`)
                        .join('\n'),
                      t('insp.copied'),
                    )
                  }
                >
                  {t('insp.copy_all')}
                </button>
              </div>
              <p className="note">{t('insp.camera_note')}</p>
            </div>
          )
        })()}
        {detail && section === 'creator' && (
          <div className="insp-section">
            {field('creator') && <FieldBlock f={field('creator')!} t={t} id={id} />}
            {field('copyright') && <FieldBlock f={field('copyright')!} t={t} id={id} />}
            <Line t={t} label={t('insp.credit')} value={tag(tags, 'Credit')?.[1]} k={tag(tags, 'Credit')?.[0]} lock />
            <Line t={t} label={t('insp.website')} value={tag(tags, 'CreatorWorkURL', 'WebStatement')?.[1]} k={tag(tags, 'CreatorWorkURL', 'WebStatement')?.[0]} lock />
            <Line t={t} label={t('insp.email')} value={tag(tags, 'CreatorWorkEmail')?.[1]} k={tag(tags, 'CreatorWorkEmail')?.[0]} lock />
            <p className="note">{t('insp.creator_note')}</p>
          </div>
        )}
        {detail && section === 'location' && (
          <div className="insp-section">
            {field('gps') && <FieldBlock f={field('gps')!} t={t} id={id} />}
            {dms(field('gps')?.value) && <Line t={t} label={t('insp.dms')} value={dms(field('gps')?.value)} />}
            <Line t={t} label={t('insp.altitude')} value={tag(tags, 'GPSAltitude')?.[1]} k={tag(tags, 'GPSAltitude')?.[0]} />
            <Line t={t} label={t('insp.country')} value={tag(tags, 'Country', 'Country-PrimaryLocationName')?.[1]} k={tag(tags, 'Country', 'Country-PrimaryLocationName')?.[0]} lock />
            <Line t={t} label={t('insp.region')} value={tag(tags, 'State', 'Province-State')?.[1]} k={tag(tags, 'State', 'Province-State')?.[0]} lock />
            <Line t={t} label={t('insp.city')} value={tag(tags, 'City')?.[1]} k={tag(tags, 'City')?.[0]} lock />
            <Line t={t} label={t('insp.place')} value={tag(tags, 'Location', 'Sub-location')?.[1]} k={tag(tags, 'Location', 'Sub-location')?.[0]} lock />
            {writesTo === 'sidecar' || writesTo === 'new_sidecar' ? (
              <div className="tone warn insp-error">{t('insp.raw_gps_note')}</div>
            ) : null}
          </div>
        )}
        {detail && section === 'basic' && (
          <div className="insp-section">
            <div className="embedded-preview">
              {preview ? (
                <img src={preview} alt={t('insp.preview_alt')} width={112} />
              ) : (
                <span className="faint">{preview === undefined ? t('insp.reading') : t('insp.preview_none')}</span>
              )}
            </div>
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
            <div className="adv-tools">
              <input className="input ui adv-filter" placeholder={t('insp.filter_tags')} value={filter} onChange={(e) => setFilter(e.target.value)} />
              <select className="input ui" value={group} onChange={(e) => setGroup(e.target.value)} aria-label={t('insp.group')}>
                <option value="">{t('insp.all_groups')}</option>
                {[...new Set(Object.keys(tags).map((k) => k.split(':')[0]))].sort().map((g) => (
                  <option key={g} value={g}>
                    {g}
                  </option>
                ))}
              </select>
            </div>
            <div className="adv-list">
              {Object.entries(tags)
                .filter(([k]) => !group || k.split(':')[0] === group)
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
                    <button className="btn plain adv-copy" title={t('insp.copy')} aria-label={`${t('insp.copy')} ${k}`} onClick={() => void copyText(`${k}: ${v}`, t('insp.copied'))}>
                      ⧉
                    </button>
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
