// SPDX-License-Identifier: GPL-3.0-or-later
// Backend texts in the UI language (DECISIONS §3 item 8). The backend keeps building English
// sentences (they are also what Plans, journals and logs store); each sentence's template is its
// message code. `backend.zh.json` maps every template to Chinese, and CI checks that it covers
// every template the backend has (`tools/message_inventory.py --check`). A text that matches no
// template is shown as it is.

import catalog from './backend.zh.json'
import type { Lang } from '.'

interface Rule {
  re: RegExp
  zh: string
  /** Length of the template's literal text: longer (more specific) templates are tried first. */
  weight: number
}

function escape(s: string): string {
  return s.replace(/[.*+?^${}()|[\]\\]/g, '\\$&')
}

/** Rust format string → anchored regex: `{{`/`}}` are literal braces, `{…}` a captured value. */
export function compile(template: string): { re: RegExp; weight: number } {
  let out = ''
  let weight = 0
  for (let i = 0; i < template.length; ) {
    const two = template.slice(i, i + 2)
    if (two === '{{' || two === '}}') {
      out += escape(two[0])
      weight++
      i += 2
    } else if (template[i] === '{') {
      const end = template.indexOf('}', i)
      out += '(.*?)'
      i = end < 0 ? template.length : end + 1
    } else {
      out += escape(template[i])
      weight++
      i++
    }
  }
  return { re: new RegExp(`^${out}$`, 's'), weight }
}

const RULES: Rule[] = [...Object.entries(catalog.inventory), ...Object.entries(catalog.extra)]
  .map(([en, zh]) => ({ ...compile(en), zh }))
  .sort((a, b) => b.weight - a.weight)

/** Field names and a few words that appear as values inside other texts. */
const WORDS: Record<string, string> = {
  creator: '作者',
  copyright: '版权',
  capture_time: '拍摄时间',
  'capture time': '拍摄时间',
  gps: 'GPS',
  GPS: 'GPS',
  interrupted: '已中断',
  running: '运行中',
  recovered: '已恢复',
  cancelled: '已取消',
  completed: '已完成',
}

const WARNING = 'warning: '
const cache = new Map<string, string>()

function translateZh(text: string, depth: number): string {
  if (depth > 3 || !text) return text
  for (const r of RULES) {
    const m = r.re.exec(text)
    if (!m) continue
    // {n}: the value, itself translated when it is a backend text; {nw}: a single word
    return r.zh.replace(/\{(\d+)(w?)\}/g, (_, n: string, w: string) => {
      const v = m[Number(n)] ?? ''
      return w ? (WORDS[v] ?? v) : translateZh(v, depth + 1)
    })
  }
  // several notes or reasons joined: "a; b"
  if (depth === 0 && text.includes('; ')) {
    const parts = text.split('; ')
    const t = parts.map((p) => translateZh(p, depth + 1))
    if (t.some((x, i) => x !== parts[i])) return t.join('；')
  }
  return text
}

/** A backend text in `lang`. A `warning: ` prefix stays as it is (the UI shows it as a glyph). */
export function backendText(lang: Lang, text: string | null | undefined): string {
  if (!text) return ''
  if (lang !== 'zh') return text
  const hit = cache.get(text)
  if (hit !== undefined) return hit
  let out: string
  if (text.startsWith(WARNING)) {
    // the prefix first: a generic template such as "{name}: {e}" would take "warning" as a name
    const rest = text.slice(WARNING.length)
    const t = translateZh(rest, 0)
    out = t !== rest ? WARNING + t : translateZh(text, 0)
  } else {
    out = translateZh(text, 0)
  }
  cache.set(text, out)
  return out
}
