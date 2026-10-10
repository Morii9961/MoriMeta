// SPDX-License-Identifier: GPL-3.0-or-later
// Check a release build as it will run once installed: the bundled ExifTool is found next to the
// program, passes its manifest (key files at launch, every file afterwards), starts as
// `perl.exe exiftool.pl`, and writes are allowed. Uses a throwaway data folder; touches no photo.
//
//   node scripts/release-check.mjs <work-folder> [path to morimeta.exe]
//
// MM_RELEASE_CHECK_ELEVATED=1: on an elevated CI runner, accept that writes are refused for that reason.

import { spawn } from 'node:child_process'
import { mkdirSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const work = resolve(process.argv[2] ?? '.')
const here = dirname(fileURLToPath(import.meta.url))
const exe = resolve(process.argv[3] ?? join(here, '..', 'src-tauri', 'target', 'release', 'morimeta.exe'))
mkdirSync(work, { recursive: true })
const PORT = 9334
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const app = spawn(exe, [], {
  env: { ...process.env, MM_DATA: join(work, 'data'), WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS: `--remote-debugging-port=${PORT}` },
  stdio: 'inherit',
})
const fail = (m) => {
  console.error(`FAIL: ${m}`)
  app.kill()
  process.exit(1)
}

let target
for (let i = 0; i < 60 && !target; i++) {
  await sleep(500)
  try {
    target = (await (await fetch(`http://127.0.0.1:${PORT}/json`)).json()).find((t) => t.type === 'page')
  } catch {
    // not up yet
  }
}
if (!target) fail('no WebView DevTools target')
const ws = new WebSocket(target.webSocketDebuggerUrl)
await new Promise((r) => ws.addEventListener('open', r))
let id = 1
const pending = new Map()
ws.addEventListener('message', (m) => {
  const msg = JSON.parse(m.data)
  pending.get(msg.id)?.(msg)
})
const js = (expression) =>
  new Promise((r) => {
    const n = id++
    pending.set(n, (msg) => r(msg.result?.result?.value))
    ws.send(JSON.stringify({ id: n, method: 'Runtime.evaluate', params: { expression, awaitPromise: true, returnByValue: true } }))
  })

// ask the backend itself, through the page's IPC
let info
for (let i = 0; i < 120; i++) {
  info = await js(`window.__TAURI_INTERNALS__.invoke('app_info')`)
  if (info?.launched) break
  await sleep(500)
}
if (!info?.launched) fail('the launch sequence did not finish')
console.log(`MoriMeta ${info.version} (${info.dev ? 'development' : 'release'} build)`)
console.log(`ExifTool: ${info.exiftool.version ?? '—'} from ${info.exiftool.package}`)
if (info.dev) fail('this is a development build')
if (info.exiftool.error) fail(`ExifTool: ${info.exiftool.error}`)
if (!info.exiftool.package?.replaceAll('\\', '/').includes(dirname(exe).replaceAll('\\', '/'))) fail('ExifTool was not taken from the bundle')
// the whole-package check runs after launch
await sleep(3000)
info = await js(`window.__TAURI_INTERNALS__.invoke('app_info')`)
if (info.exiftool.integrity) fail(`integrity: ${info.exiftool.integrity}`)
// GitHub's Windows runners are elevated and MoriMeta then refuses to write (SECURITY_MODEL §4.1):
// with MM_RELEASE_CHECK_ELEVATED=1 that refusal, and only that one, is expected
const elevated = process.env.MM_RELEASE_CHECK_ELEVATED === '1' && info.writes_refused?.includes('administrator rights')
if (info.writes_refused && !elevated) fail(`writes refused: ${info.writes_refused}`)
console.log(`bundled ExifTool intact (key files and every file), ${elevated ? 'writes refused only for administrator rights' : 'writes allowed'}`)
console.log('PASS')
app.kill()
process.exit(0)
