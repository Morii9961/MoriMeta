// SPDX-License-Identifier: GPL-3.0-or-later
// UI smoke test of the desktop app against the real backend and ExifTool, through the WebView's
// DevTools protocol (no screen control needed). Development build only.
//
//   node scripts/ui-smoke.mjs <photos-folder> <work-folder>
//
// Needs the Vite dev server on :1420 (`npm run dev`) and a debug build
// (`cargo build` in src-tauri). It copies nothing: <photos-folder> must hold disposable copies.
// Flow: launch with MM_DEV_IMPORT → rows read → select all → stage Creator → Preview → review →
// Apply (+ acknowledgements) → completion → files changed as planned (JPEG written, NEF
// untouched, sidecars created) → Undo through Preview → every file back byte for byte.
// Screenshots go to <work-folder>/shots.

import { spawn } from 'node:child_process'
import { createHash } from 'node:crypto'
import { mkdirSync, readdirSync, readFileSync, writeFileSync, existsSync } from 'node:fs'
import { join, dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const [photos, work] = process.argv.slice(2).map((p) => resolve(p))
if (!photos || !work) {
  console.error('usage: node scripts/ui-smoke.mjs <photos-folder> <work-folder>')
  process.exit(2)
}
const here = dirname(fileURLToPath(import.meta.url))
const exe = join(here, '..', 'src-tauri', 'target', 'debug', 'morimeta.exe')
const shots = join(work, 'shots')
mkdirSync(shots, { recursive: true })
const PORT = 9333

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
const hashes = () =>
  Object.fromEntries(
    readdirSync(photos)
      .sort()
      .map((f) => [f, createHash('sha256').update(readFileSync(join(photos, f))).digest('hex')]),
  )

function fail(msg) {
  console.error(`FAIL: ${msg}`)
  app?.kill()
  process.exit(1)
}

const before = hashes()
const app = spawn(exe, [], {
  env: {
    ...process.env,
    MM_DATA: join(work, 'data'),
    MM_DEV_IMPORT: photos,
    WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}`,
  },
  stdio: 'inherit',
})

// --- DevTools connection
let target
for (let i = 0; i < 60 && !target; i++) {
  await sleep(500)
  try {
    const list = await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()
    target = list.find((t) => t.type === 'page')
  } catch {
    // not up yet
  }
}
if (!target) fail('no WebView DevTools target')
const ws = new WebSocket(target.webSocketDebuggerUrl)
await new Promise((r) => ws.addEventListener('open', r))
let nextId = 1
const pending = new Map()
ws.addEventListener('message', (m) => {
  const msg = JSON.parse(m.data)
  if (msg.id && pending.has(msg.id)) {
    pending.get(msg.id)(msg)
    pending.delete(msg.id)
  }
})
const send = (method, params = {}) =>
  new Promise((r) => {
    const id = nextId++
    pending.set(id, r)
    ws.send(JSON.stringify({ id, method, params }))
  })
async function js(expr) {
  const r = await send('Runtime.evaluate', { expression: expr, awaitPromise: true, returnByValue: true })
  if (r.result?.exceptionDetails) fail(`page error: ${r.result.exceptionDetails.exception?.description ?? JSON.stringify(r.result.exceptionDetails)}`)
  return r.result?.result?.value
}
async function shot(name) {
  const r = await send('Page.captureScreenshot', { format: 'png' })
  writeFileSync(join(shots, `${name}.png`), Buffer.from(r.result.data, 'base64'))
}
async function until(what, expr, ms = 60000) {
  const end = Date.now() + ms
  while (Date.now() < end) {
    if (await js(expr)) return
    await sleep(250)
  }
  await shot(`timeout-${what}`)
  fail(`timed out waiting for ${what}`)
}

// helpers inside the page
await js(`
  window.__t = {
    text: () => document.body.innerText,
    btn: (label) => [...document.querySelectorAll('button')].find(b => b.innerText.trim().startsWith(label) && !b.disabled),
    click: (label) => { const b = window.__t.btn(label); if (!b) throw new Error('no button ' + label); b.click(); return true },
    key: (el, key, opts = {}) => el.dispatchEvent(new KeyboardEvent('keydown', { key, bubbles: true, ...opts })),
    type: (el, value) => {
      const set = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, 'value').set
      set.call(el, value)
      el.dispatchEvent(new Event('input', { bubbles: true }))
    },
  }; true`)
// English UI for the text checks: View menu → English
await until('menu', `document.querySelectorAll('.menubar-item > button').length === 5`)
await js(`
  const view = document.querySelectorAll('.menubar-item > button')[2]
  view.dispatchEvent(new MouseEvent('mousedown', { bubbles: true }))
  true`)
await sleep(200)
await js(`[...document.querySelectorAll('.menu-popup button')].find(b => b.innerText.includes('English')).click(); true`)
const n = Object.keys(before).length
await until('ExifTool', `/ExifTool \\d/.test(document.querySelector('.statusbar')?.innerText ?? '')`)
console.log('app launched; ExifTool running')
await until('import', `document.querySelectorAll('.mrow').length > 0 || /Add photos/.test(document.body.innerText)`)
if (await js(`/Add photos/.test(document.body.innerText)`)) {
  await until('import', `document.querySelectorAll('.mrow').length > 0`)
}
await until('rows read', `![...document.querySelectorAll('.mrow')].some(r => r.innerText.includes('…'))`)
await shot('1-library')
console.log('library rows read')

// select all, stage Creator, open Preview
await js(`const t = document.querySelector('.mtable'); t.focus(); __t.key(t, 'a', { ctrlKey: true }); true`)
await until('batch panel', `/files selected/.test(document.body.innerText)`)
await js(`[...document.querySelectorAll('.mvf')][0].querySelectorAll('.mvf-ops button')[1].click(); true`)
await until('creator input', `!!document.querySelector('.mvf-input')`)
await js(`__t.type(document.querySelector('.mvf-input'), 'Morii Smoke'); true`)
await until('staged', `/1 staged/.test(document.body.innerText)`)
await shot('2-batch')
await js(`__t.key(document.body, 'Enter', { ctrlKey: true }); true`)
await until('preview', `!!document.querySelector('.preview') && !/Loading the plan/.test(document.body.innerText) && /Pre-flight/i.test(document.body.innerText) && !/Checking/.test(document.querySelector('.action-bar').innerText)`, 120000)
await shot('3-preview')
console.log('preview open')
// the same Preview in Chinese: UI texts and the backend's notes and reasons
const lang = async (label) => {
  await js(`document.querySelectorAll('.menubar-item > button')[2].dispatchEvent(new MouseEvent('mousedown', { bubbles: true })); true`)
  await sleep(200)
  await js(`[...document.querySelectorAll('.menu-popup button')].find(b => b.innerText.includes('${label}')).click(); true`)
  await sleep(300)
}
await lang('简体中文')
await shot('3b-preview-zh')
const untranslated = await js(`[...document.querySelectorAll('.dnote')].map(n => n.innerText).filter(t => /[a-z]{4,} [a-z]{3,} [a-z]{3,}/.test(t))`)
if (untranslated.length) console.log(`untranslated notes: ${JSON.stringify(untranslated)}`)
await lang('English')

// review every category the checklist asks for, then Apply
await js(`document.querySelectorAll('.review-item').forEach(b => b.click()); true`)
await sleep(300)
const applyLabel = await js(`[...document.querySelectorAll('.action-bar button')].pop().innerText`)
if (await js(`[...document.querySelectorAll('.action-bar button')].pop().disabled`)) {
  await shot('apply-blocked')
  fail(`Apply stayed blocked: ${await js(`document.querySelector('.action-note')?.innerText`)}`)
}
await js(`[...document.querySelectorAll('.action-bar button')].pop().click(); true`)
await sleep(300)
if (await js(`!!document.querySelector('.dialog')`)) {
  await shot('4-confirm')
  await js(`document.querySelectorAll('.dialog .ack input').forEach(c => c.click()); true`)
  await sleep(200)
  await js(`[...document.querySelectorAll('.dialog-footer button')].pop().click(); true`)
}
console.log(`applying (${applyLabel})`)
await until('completion', `/Done/.test(document.querySelector('.op-side')?.innerText ?? '')`, 180000)
await shot('5-done')
const tally = await js(`document.querySelector('.op-tally').innerText`)
console.log(`completed: ${tally.replace(/\\s+/g, ' ')}`)

// what happened on disk
const after = hashes()
const changed = Object.keys(before).filter((f) => before[f] !== after[f])
const created = Object.keys(after).filter((f) => !(f in before))
const nefTouched = Object.keys(before).filter((f) => /\.nef$/i.test(f) && before[f] !== after[f])
console.log(`changed: ${changed.join(', ') || '(none)'}`)
console.log(`created: ${created.join(', ') || '(none)'}`)
if (nefTouched.length) fail(`RAW files were written: ${nefTouched}`)
if (!changed.length) fail('no file was written')

// undo through Preview
await js(`__t.click('Undo operation'); true`)
await until('undo preview', `!!document.querySelector('.preview') && /Pre-flight/i.test(document.body.innerText) && !/Checking/.test(document.querySelector('.action-bar').innerText)`, 60000)
await shot('6-undo-preview')
await js(`document.querySelectorAll('.review-item').forEach(b => b.click()); true`)
await sleep(300)
if (await js(`[...document.querySelectorAll('.action-bar button')].pop().disabled`)) {
  fail(`Undo blocked: ${await js(`document.querySelector('.action-note')?.innerText`)}`)
}
await js(`[...document.querySelectorAll('.action-bar button')].pop().click(); true`)
await sleep(300)
if (await js(`!!document.querySelector('.dialog')`)) {
  await js(`document.querySelectorAll('.dialog .ack input').forEach(c => c.click()); true`)
  await sleep(200)
  await js(`[...document.querySelectorAll('.dialog-footer button')].pop().click(); true`)
}
await until('undo completion', `/Done/.test(document.querySelector('.op-side')?.innerText ?? '')`, 180000)
await shot('7-undo-done')
const back = hashes()
const differ = Object.keys(before).filter((f) => before[f] !== back[f])
const left = Object.keys(back).filter((f) => !(f in before))
if (differ.length) fail(`not restored: ${differ}`)
if (left.length) fail(`left behind: ${left}`)
console.log(`undo: all ${n} files back byte for byte, nothing left behind`)

// History shows both Operations
await js(`__t.click('Done'); true`)
await js(`[...document.querySelectorAll('.toolbar [role=tab]')].find(b => b.innerText === 'History').click(); true`)
await until('history', `document.querySelectorAll('.journal-entry').length >= 2`)
await shot('8-history')
console.log('history lists both operations')

// a built-in Preset on the same selection: a Plan in Preview, then discarded
await js(`[...document.querySelectorAll('.toolbar [role=tab]')].find(b => b.innerText === 'Presets').click(); true`)
await until('presets', `[...document.querySelectorAll('.preset-name')].some(n => n.innerText === 'Copyright Template')`)
await js(`[...document.querySelectorAll('.preset-row')].find(r => r.innerText.includes('Copyright Template')).click(); true`)
await sleep(200)
await js(`__t.click('Apply to'); true`)
await until('preset preview', `!!document.querySelector('.preview') && /Pre-flight/i.test(document.body.innerText) && !/Checking/.test(document.querySelector('.action-bar').innerText)`, 60000)
await shot('9-preset-preview')
const presetSummary = await js(`document.querySelector('.summary-strip').innerText`)
console.log(`preset preview: ${presetSummary.replace(/\s+/g, ' ')}`)
await js(`__t.click('Discard plan'); true`)
if (JSON.stringify(hashes()) !== JSON.stringify(before)) fail('the preset Preview changed files')

// Clean Export Preview of the same selection (exporting needs the folder dialog; the core's
// integration test covers it)
await js(`[...document.querySelectorAll('.toolbar [role=tab]')].find(b => b.innerText === 'Library').click(); true`)
await sleep(300)
await js(`__t.click('Clean export'); true`)
await until('clean preview', `document.querySelectorAll('.clean-row').length > 0 && !/Reading…/.test(document.querySelector('.summary-strip').innerText)`, 60000)
await until('clean detail', `/MAKER NOTES|SERIAL NUMBERS/i.test(document.querySelector('.pane-inspector').innerText)`)
await shot('10-clean-preview')
console.log(`clean export preview: ${(await js(`document.querySelector('.summary-strip').innerText`)).replace(/\s+/g, ' ')}`)
await js(`document.body.dispatchEvent(new KeyboardEvent('keydown', { key: 'Escape', bubbles: true })); true`)
if (JSON.stringify(hashes()) !== JSON.stringify(before)) fail('the Clean Export Preview changed files')
console.log('PASS')
app.kill()
process.exit(0)
