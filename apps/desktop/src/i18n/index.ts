// SPDX-License-Identifier: GPL-3.0-or-later
// UI text in English and 简体中文 (DESIGN.md §6 Localisation). Every key carries both languages in
// one place so neither can fall behind. Backend texts (Preview notes, reasons, errors) are matched
// against their templates and translated by ./backend.ts.

import { useApp } from '../state/store'
import { messages } from './messages'
import { backendText } from './backend'

export type Lang = 'en' | 'zh'
export type MessageKey = keyof typeof messages

export function detectLang(): Lang {
  try {
    const saved = localStorage.getItem('mm.lang')
    if (saved === 'en' || saved === 'zh') return saved
  } catch {
    // storage may be unavailable; fall back to the system language
  }
  return navigator.language.toLowerCase().startsWith('zh') ? 'zh' : 'en'
}

export function translate(lang: Lang, key: MessageKey, params?: Record<string, string | number>): string {
  const m = messages[key]
  let s: string = m ? (lang === 'zh' ? m[1] : m[0]) : key
  if (params) {
    for (const [k, v] of Object.entries(params)) {
      s = s.split(`{${k}}`).join(typeof v === 'number' ? v.toLocaleString(lang === 'zh' ? 'zh-CN' : 'en-US') : v)
    }
  }
  return s
}

export type T = (key: MessageKey, params?: Record<string, string | number>) => string

export function useT(): T {
  const lang = useApp((s) => s.lang)
  return (key, params) => translate(lang, key, params)
}

/** Field names as the UI shows them. */
export function fieldLabel(t: T, field: string): string {
  switch (field) {
    case 'creator':
      return t('field.creator')
    case 'copyright':
      return t('field.copyright')
    case 'capture_time':
      return t('field.capture_time')
    case 'gps':
      return t('field.gps')
    default:
      return field
  }
}

/** Backend texts (Preview notes, reasons, errors) in the UI language. */
export function useBT(): (text: string | null | undefined) => string {
  const lang = useApp((s) => s.lang)
  return (text) => backendText(lang, text)
}
