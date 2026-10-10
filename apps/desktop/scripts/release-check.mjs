// SPDX-License-Identifier: GPL-3.0-or-later
// Check a release build as it will run once installed: the bundled ExifTool is found next to the
// program, passes its manifest (key files at launch, every file afterwards), starts as
// `perl.exe exiftool.pl`, and writes are allowed. Uses a throwaway data folder; touches no photo.
// Reads the launch lines the program writes to its own log (no paths in them), so it needs no
// WebView DevTools port, which GitHub's Windows runners do not open.
//
//   node scripts/release-check.mjs <work-folder> [path to morimeta.exe]
//
// MM_RELEASE_CHECK_ELEVATED=1: on an elevated CI runner, accept that writes are refused for that reason.

import { spawn, spawnSync } from 'node:child_process'
import { mkdirSync, readFileSync, readdirSync, statSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'

const work = resolve(process.argv[2] ?? '.')
const here = dirname(fileURLToPath(import.meta.url))
const exe = resolve(process.argv[3] ?? join(here, '..', 'src-tauri', 'target', 'release', 'morimeta.exe'))
mkdirSync(work, { recursive: true })
const sleep = (ms) => new Promise((r) => setTimeout(r, ms))

const data = join(work, 'data')
const env = { ...process.env, MM_DATA: data }
delete env.MM_EXIFTOOL_PKG // the bundled package, not an override
const app = spawn(exe, [], { env, stdio: 'inherit' })
let exited
app.on('exit', (code) => (exited = code))

// what the data folder holds, with the end of each text file
const dump = (dir) => {
  let entries = []
  try {
    entries = readdirSync(dir, { recursive: true })
  } catch {
    return console.error(`(no data folder at ${dir})`)
  }
  for (const e of entries) {
    const p = join(dir, e)
    const s = statSync(p)
    if (!s.isFile()) continue
    console.error(`--- ${relative(dir, p)} (${s.size} bytes)`)
    if (/\.(log|txt|json|jsonl)$/i.test(p)) console.error(readFileSync(p, 'utf8').slice(-4000))
  }
}
const processes = () => {
  if (process.platform !== 'win32') return
  console.error(spawnSync('tasklist', ['/v', '/fo', 'list', '/fi', 'imagename eq morimeta.exe'], { encoding: 'utf8' }).stdout)
}
const fail = (m) => {
  console.error(`FAIL: ${m}`)
  console.error(exited === undefined ? 'the program was still running' : `the program exited with ${exited}`)
  dump(data)
  processes()
  app.kill()
  process.exit(1)
}

// `... info exiftool package checked app="0.1.0" build="release" exiftool="13.59" ...`
const launchLine = () => {
  let text = ''
  try {
    for (const f of readdirSync(join(data, 'logs'))) if (f.endsWith('.log')) text += readFileSync(join(data, 'logs', f), 'utf8')
  } catch {
    return undefined
  }
  const line = text.split('\n').find((l) => l.includes(' exiftool package checked '))
  if (!line) return undefined
  return Object.fromEntries([...line.matchAll(/ (\w+)="([^"]*)"/g)].map((m) => [m[1], m[2]]))
}

let info
// launch, recovery and the whole-package check; a cold start on a CI runner is slow
for (let i = 0; i < 240 && !info && exited === undefined; i++) {
  await sleep(500)
  info = launchLine()
}
if (!info) fail('the launch sequence did not log its ExifTool check')
console.log(`MoriMeta ${info.app} (${info.build} build), ExifTool ${info.exiftool} (${info.source})`)
if (info.build !== 'release') fail('this is a development build')
if (info.started !== 'true') fail('ExifTool did not start')
if (info.source !== 'bundled') fail(`ExifTool was not taken from the bundle (${info.source})`)
if (info.integrity !== 'ok') fail('the bundled ExifTool does not match its manifest')
// GitHub's Windows runners are elevated and MoriMeta then refuses to write (SECURITY_MODEL §4.1):
// with MM_RELEASE_CHECK_ELEVATED=1 that refusal, and only that one, is expected
const elevated = process.env.MM_RELEASE_CHECK_ELEVATED === '1' && info.writes === 'refused: administrator rights'
if (info.writes !== 'allowed' && !elevated) fail(`writes ${info.writes}`)
await sleep(1000)
if (exited !== undefined) fail('the program did not keep running')
console.log(`bundled ExifTool intact (key files and every file), ${elevated ? 'writes refused only for administrator rights' : 'writes allowed'}`)
console.log('PASS')
app.kill()
process.exit(0)
