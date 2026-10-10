// SPDX-License-Identifier: GPL-3.0-or-later
import { describe, expect, it } from 'vitest'
import { backendText, compile } from './backend'
import catalog from './backend.zh.json'

describe('backend texts', () => {
  it('compiles Rust format strings with literal braces', () => {
    const { re } = compile('{{{name}}} has no value for this file (give a default: {{{name}|…}})')
    expect(re.exec('{creator} has no value for this file (give a default: {creator|…})')?.slice(1)).toEqual(['creator', 'creator'])
  })

  it('translates a plain sentence and one with values', () => {
    expect(backendText('zh', 'file contains more than one IPTC record; not written')).toBe('文件包含多条 IPTC 记录；不写入')
    expect(backendText('zh', 'replace failed (Win32 5); original unchanged')).toBe('替换失败（Win32 5）；原文件未改动')
  })

  it('translates the values inside a sentence when they are texts of their own', () => {
    expect(
      backendText('zh', 'copyright not changed: {creator} has no value for this file (give a default: {creator|…})'),
    ).toBe('版权未修改：{creator} 在此文件中没有值（可给默认值：{creator|…}）')
    expect(backendText('zh', 'sidecar: read-only attribute is set (treated as locked by the user)')).toBe(
      'sidecar：已设置只读属性（视为用户锁定）',
    )
  })

  it('keeps the warning prefix for the UI to show as a glyph', () => {
    expect(
      backendText(
        'zh',
        'warning: no photo of this name is next to this XMP sidecar: the change is written to the sidecar alone and shows only in programs that open it',
      ),
    ).toBe('warning: 此 XMP sidecar 旁没有同名照片：修改只写入该 sidecar，只有打开它的程序才会显示')
  })

  it('uses every value of each sentence, and only those, in its Chinese text', () => {
    const entries = [...Object.entries(catalog.inventory), ...Object.entries(catalog.extra)] as [string, string][]
    const bad: string[] = []
    let withValues = 0
    for (const [en, zh] of entries) {
      // the number of values the English template captures, as the UI matches it
      const groups = new RegExp(`${compile(en).re.source}|`).exec('')!.length - 1
      if (groups) withValues++
      const used = new Set([...zh.matchAll(/\{(\d+)w?\}/g)].map((m) => Number(m[1])))
      const want = new Set(Array.from({ length: groups }, (_, i) => i + 1))
      if ([...used].sort().join() !== [...want].sort().join()) bad.push(`${en} → ${zh}`)
    }
    expect(withValues).toBeGreaterThan(100)
    expect(bad).toEqual([])
  })

  it('leaves unknown texts and English alone', () => {
    expect(backendText('zh', 'something new from the backend')).toBe('something new from the backend')
    expect(backendText('en', 'file contains more than one IPTC record; not written')).toBe(
      'file contains more than one IPTC record; not written',
    )
  })
})
