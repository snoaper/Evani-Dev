#!/usr/bin/env node
// One command to refresh the Asset & Style Studio live preview (see PREVIEW.md):
//
//   node tools/studio/preview-refresh.mjs [--force] [--ref studio] [--port 8123]
//   node tools/studio/preview-refresh.mjs --ensure     (no rebuild: just make sure the server answers 200)
//   --restart  also stops the running server (by the PID in server.pid) and starts a fresh one
//
// 1. Checks out the current `studio` head in a private build worktree in a scratch folder
//    (<root>/src), never in the shared studio worktree.
// 2. Builds the material-view app (`cargo xtask build material-view --no-diff`) with
//    CARGO_BUILD_JOBS=1, CARGO_TARGET_DIR and TEMP in that folder, at below-normal priority.
// 3. Only when the build worked: copies the page into sites/<sha>-<time>, adds the
//    library badge (preview-overlay.js + the catalogue summary), then points current.txt at it.
//    A failed build leaves the last good preview up and prints why.
// 4. Makes sure the static server (preview-server.mjs) runs on ONE fixed port
//    (pid in server.pid), starting it detached if needed. Prints the URL.

import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath } from 'node:url'

const here = path.dirname(fileURLToPath(import.meta.url))
const argv = process.argv.slice(2)
const flag = n => argv.includes(n)
const opt = (n, d) => (argv.includes(n) && argv[argv.indexOf(n) + 1]) || d

const ROOT = path.resolve(process.env.STUDIO_PREVIEW_ROOT || path.join(os.tmpdir(), 'evani-studio-preview'))
const PORT = Number(opt('--port', process.env.STUDIO_PREVIEW_PORT || '8123'))
const REF = opt('--ref', 'studio')
const APP = 'material-view'
const RUN_LOW = process.env.EVANI_RUN_LOW || (process.env.APPDATA ? path.join(process.env.APPDATA, 'Evani.App', 'tools', 'run-low.mjs') : '')
const SRC = path.join(ROOT, 'src')
const TARGET = path.join(ROOT, 'target')
const TMP = path.join(ROOT, 'tmp')
const SITES = path.join(ROOT, 'sites')
const URL_ = `http://localhost:${PORT}/`

const log = (...a) => console.log('[studio-preview]', ...a)
const git = (cwd, ...args) => {
  const r = spawnSync('git', args, { cwd, encoding: 'utf8', windowsHide: true })
  if (r.status !== 0) throw new Error(`git ${args.join(' ')} failed: ${(r.stderr || r.stdout || '').trim()}`)
  return r.stdout.trim()
}
// Whether `pid` runs: asked of the OS by that one PID (no signal is sent).
const alive = pid => {
  const n = Number(pid)
  if (!Number.isInteger(n) || n <= 0) return false
  if (process.platform === 'win32') {
    const r = spawnSync('tasklist', ['/FI', `PID eq ${n}`, '/NH', '/FO', 'CSV'], { encoding: 'utf8', windowsHide: true })
    return r.status === 0 && r.stdout.includes(`"${n}"`)
  }
  return fs.existsSync(`/proc/${n}`)
}
const writeAtomic = (file, text) => {
  fs.writeFileSync(file + '.tmp', text)
  fs.renameSync(file + '.tmp', file)
}

async function serverUp() {
  try {
    const r = await fetch(`http://127.0.0.1:${PORT}/__health`, { signal: AbortSignal.timeout(1500) })
    return r.ok ? await r.json() : null
  } catch {
    return null
  }
}

async function ensureServer() {
  const h = await serverUp()
  if (h && flag('--restart')) {
    // Stop the old server only by the PID recorded in server.pid (and only if it is the one answering).
    let recorded = 0
    try {
      recorded = Number(fs.readFileSync(path.join(ROOT, 'server.pid'), 'utf8'))
    } catch {
      // no pid file
    }
    if (recorded && recorded === h.pid) {
      if (process.platform !== 'win32') {
        log(`--restart stops the server on Windows only; stop pid ${recorded} yourself`)
        return
      }
      spawnSync('taskkill', ['/PID', String(recorded), '/T', '/F'], { windowsHide: true })
      for (let i = 0; i < 20 && (await serverUp()); i++) await new Promise(r => setTimeout(r, 250))
      log(`stopped the old server (pid ${recorded})`)
    } else {
      log(`server.pid (${recorded}) does not match the server answering (${h.pid}); not stopping anything`)
      return
    }
  } else if (h) {
    fs.writeFileSync(path.join(ROOT, 'server.pid'), String(h.pid))
    log(`server already running (pid ${h.pid})`)
    return
  }
  const serverArgs = [path.join(here, 'preview-server.mjs'), '--root', ROOT, '--port', String(PORT), '--log', path.join(ROOT, 'server.log')]
  if (process.platform === 'win32') {
    // Win32_Process.Create starts the server outside this process's job object (its parent is the WMI
    // host), so it outlives the terminal or agent session that ran this script. A plain detached child
    // does not. Started directly (no cmd.exe wrapper, which a console Ctrl-C could take down), then
    // set to below-normal priority.
    const q = s => `"${s}"`
    const cmdline = [process.execPath, ...serverArgs].map(q).join(' ')
    const lit = s => `'${s.replace(/'/g, "''")}'`
    // Hidden from its start (ShowWindow 0): no console window on the owner's screen (SAFE-16).
    const ps =
      `$si = New-CimInstance -ClassName Win32_ProcessStartup -ClientOnly -Property @{ ShowWindow = [uint16]0 }; ` +
      `$r = Invoke-CimMethod -ClassName Win32_Process -MethodName Create -Arguments @{CommandLine=${lit(cmdline)}; CurrentDirectory=${lit(ROOT)}; ProcessStartupInformation=$si}; ` +
      `if ($r.ReturnValue -ne 0) { throw "Create failed: $($r.ReturnValue)" }; ` +
      `try { (Get-Process -Id $r.ProcessId).PriorityClass = 'BelowNormal' } catch {}; $r.ProcessId`
    const r = spawnSync('powershell.exe', ['-NoProfile', '-NonInteractive', '-WindowStyle', 'Hidden', '-Command', ps], { encoding: 'utf8', windowsHide: true })
    if (r.status !== 0) throw new Error(`could not start the server: ${r.stderr || r.stdout}`)
  } else {
    const out = fs.openSync(path.join(ROOT, 'server.log'), 'a')
    const child = spawn(process.execPath, serverArgs, { detached: true, stdio: ['ignore', out, out], windowsHide: true })
    child.unref()
  }
  for (let i = 0; i < 20; i++) {
    await new Promise(r => setTimeout(r, 250))
    const up = await serverUp()
    if (up) {
      fs.writeFileSync(path.join(ROOT, 'server.pid'), String(up.pid))
      log(`server started (pid ${up.pid})`)
      return
    }
  }
  throw new Error(`the static server did not come up on port ${PORT}; see ${path.join(ROOT, 'server.log')}`)
}

function librarySummary(srcDir) {
  const j = f => JSON.parse(fs.readFileSync(path.join(srcDir, f), 'utf8'))
  const mats = j('catalog/materials/materials.json').rows
  const assets = j('catalog/assets/assets.json').assets
  return {
    materials: mats.map(r => [r.id, r.name, `${r.category}/${r.subcategory}`, r.status === 'ready' || r.status === 'reviewed' ? 'ready' : 'planned']),
    assets: assets.map(r => [r.id, r.name, `${r.category}/${r.subcategory}`, r.status === 'ready' || r.status === 'reviewed' ? 'ready' : r.status || 'planned']),
  }
}

// ---- what's new ------------------------------------------------------------------------

const PATHS = {
  materials: 'catalog/materials/materials.json',
  assets: 'catalog/assets/assets.json',
  styles: 'kits/evani-kit-material-view/data/styles.json',
  lights: 'kits/evani-kit-material-view/data/lights.json',
}
const BUILT = s => s === 'ready' || s === 'reviewed'
const STYLE_BUILT = s => !!s && !['draft', 'planned', 'idea'].includes(s)

/** The built items of one commit: kind -> Map(id -> display name). Planned items are left out. */
function catalogAt(repo, ref) {
  const get = p => {
    const r = spawnSync('git', ['show', `${ref}:${p}`], { cwd: repo, encoding: 'utf8', maxBuffer: 256 * 1024 * 1024, windowsHide: true })
    return r.status === 0 ? JSON.parse(r.stdout) : null
  }
  const out = { material: new Map(), asset: new Map(), style: new Map(), lighting: new Map() }
  for (const r of get(PATHS.materials)?.rows || []) if (BUILT(r.status)) out.material.set(r.id, r.name)
  for (const r of get(PATHS.assets)?.assets || []) if (BUILT(r.status)) out.asset.set(r.id, r.name)
  for (const r of get(PATHS.styles)?.styles || []) if (STYLE_BUILT(r.status)) out.style.set(r.id, r.name)
  for (const p of get(PATHS.lights)?.presets || []) out.lighting.set(p.id || p.name, p.name || p.id)
  return out
}

/**
 * Entries for everything built that was not built before, one commit at a time along the
 * first-parent line from `prev` to `sha` (a merge commit's date and title say when and why).
 * A commit that adds no catalogue item but changes the code is listed as a `feature`.
 */
function whatsNewBetween(repo, prev, sha, landedAt) {
  let commits = [sha]
  const r = spawnSync('git', ['rev-list', '--first-parent', '--reverse', `${prev}..${sha}`], { cwd: repo, encoding: 'utf8', windowsHide: true })
  const anc = spawnSync('git', ['merge-base', '--is-ancestor', prev, sha], { cwd: repo, windowsHide: true })
  if (r.status === 0 && anc.status === 0 && r.stdout.trim()) commits = r.stdout.trim().split('\n')
  const entries = []
  let before = catalogAt(repo, prev)
  for (const c of commits) {
    const after = catalogAt(repo, c)
    const [date, title] = git(repo, 'log', '-1', '--format=%cI%x09%s', c).split('\t')
    let any = false
    for (const kind of Object.keys(after)) {
      for (const [id, name] of after[kind]) {
        if (before[kind].has(id)) continue
        any = true
        entries.push({ kind, id, name, addedAt: date, landedAt, title, sha: c.slice(0, 8) })
      }
    }
    if (!any) {
      const files = git(repo, 'diff', '--name-only', `${c}^1`, c).split('\n').filter(Boolean)
      if (files.some(f => !f.startsWith('docs/') && !f.startsWith('tools/studio/'))) {
        entries.push({ kind: 'feature', id: `commit:${c.slice(0, 8)}`, name: title, addedAt: date, landedAt, title, sha: c.slice(0, 8) })
      }
    }
    before = after
  }
  return entries.reverse() // newest first
}

function updateWhatsNew(repo, sha, landedAt, siteDir) {
  const file = path.join(ROOT, 'whats-new.json')
  const lastFile = path.join(ROOT, 'last-good.json')
  let old = []
  try {
    old = JSON.parse(fs.readFileSync(file, 'utf8')).entries || []
  } catch {
    // first run: nothing yet
  }
  let prev = null
  try {
    prev = JSON.parse(fs.readFileSync(lastFile, 'utf8')).sha
  } catch {
    // first good build is the baseline: nothing counts as new yet
  }
  let fresh = []
  if (prev && prev !== sha) {
    try {
      fresh = whatsNewBetween(repo, prev, sha, landedAt)
    } catch (e) {
      log(`what's new: could not compare with ${prev.slice(0, 8)} (${e.message})`)
    }
  }
  const seen = new Set()
  const entries = [...fresh, ...old]
    .filter(e => (seen.has(e.kind + ':' + e.id) ? false : seen.add(e.kind + ':' + e.id)))
    .slice(0, 200)
  const doc = JSON.stringify({ sha, builtAt: landedAt, entries }, null, 1)
  writeAtomic(file, doc)
  fs.writeFileSync(path.join(siteDir, 'whats-new.json'), doc)
  writeAtomic(lastFile, JSON.stringify({ sha }))
  return fresh.length
}

function cleanup(keep) {
  if (!fs.existsSync(SITES)) return
  const dirs = fs.readdirSync(SITES).filter(n => fs.statSync(path.join(SITES, n)).isDirectory()).sort()
  for (const n of dirs.slice(0, Math.max(0, dirs.length - 3))) {
    if (n === keep) continue
    try {
      fs.rmSync(path.join(SITES, n), { recursive: true, force: true })
    } catch {
      // a file may still be open by the server; the next refresh removes it
    }
  }
}

async function main() {
  fs.mkdirSync(ROOT, { recursive: true })
  if (flag('--ensure')) {
    // Quick health fix: no rebuild, no lock. Starts the server only if it does not answer 200.
    const ok = await fetch(URL_, { signal: AbortSignal.timeout(2000) }).then(r => r.status === 200, () => false)
    if (ok && !flag('--restart') && (await serverUp())) {
      log(`already answering 200: ${URL_}`)
      return
    }
    await ensureServer()
    const ok2 = await fetch(URL_, { signal: AbortSignal.timeout(2000) }).then(r => r.status, () => 0)
    log(`${URL_} answers ${ok2}`)
    process.exit(ok2 === 200 ? 0 : 1)
  }
  for (const d of [SITES, TARGET, TMP]) fs.mkdirSync(d, { recursive: true })

  // One refresh at a time.
  const lock = path.join(ROOT, 'refresh.lock')
  if (fs.existsSync(lock)) {
    const pid = Number(fs.readFileSync(lock, 'utf8'))
    if (pid && alive(pid)) {
      log(`another refresh is running (pid ${pid}); wait for it and run this again.`)
      process.exit(2)
    }
  }
  fs.writeFileSync(lock, String(process.pid))
  const release = () => {
    try {
      fs.unlinkSync(lock)
    } catch {
      // already gone
    }
  }
  process.on('exit', release)

  const status = { startedAt: new Date().toISOString() }
  const fail = (why, tail) => {
    status.ok = false
    status.error = why
    status.finishedAt = new Date().toISOString()
    writeAtomic(path.join(ROOT, 'status.json'), JSON.stringify(status, null, 2))
    console.error(`\n[studio-preview] REFRESH FAILED: ${why}`)
    if (tail) console.error(tail)
    console.error(`[studio-preview] the last good preview (if any) is still served at ${URL_}`)
    process.exit(1)
  }

  try {
    // 1. The studio head, in a private worktree.
    const repo = git(here, 'rev-parse', '--show-toplevel')
    const sha = git(repo, 'rev-parse', `refs/heads/${REF}`)
    const subject = git(repo, 'log', '-1', '--format=%s', sha)
    status.sha = sha
    status.subject = subject
    const current = fs.existsSync(path.join(ROOT, 'current.txt')) ? fs.readFileSync(path.join(ROOT, 'current.txt'), 'utf8').trim() : ''
    log(`${REF} head ${sha.slice(0, 8)}: ${subject}`)
    if (!flag('--force') && current.startsWith(sha.slice(0, 12))) {
      log('already built from this commit (use --force to rebuild)')
      await ensureServer()
      log(`UP TO DATE: ${URL_}`)
      return
    }
    if (!fs.existsSync(path.join(SRC, '.git'))) {
      log(`creating the build worktree ${SRC}`)
      git(repo, 'worktree', 'add', '--detach', '--force', SRC, sha)
    } else {
      git(SRC, 'checkout', '--detach', '--force', sha)
    }

    // 2. Build, low priority, one job, everything under the preview root.
    const buildLog = path.join(ROOT, 'build.log')
    const fd = fs.openSync(buildLog, 'w')
    const env = {
      ...process.env,
      CARGO_BUILD_JOBS: '1',
      CARGO_TARGET_DIR: TARGET,
      TEMP: TMP,
      TMP: TMP,
      TMPDIR: TMP,
    }
    delete env.NO_COLOR
    log(`building ${APP} (one job, low priority; the first build takes a long while). Log: ${buildLog}`)
    const t0 = Date.now()
    const r = spawnSync(process.execPath, [RUN_LOW, 'cargo', 'xtask', 'build', APP, '--no-diff'], {
      cwd: SRC,
      env,
      stdio: ['ignore', fd, fd],
      windowsHide: true,
    })
    fs.closeSync(fd)
    const text = fs.readFileSync(buildLog, 'utf8')
    const tail = text.split(/\r?\n/).slice(-30).join('\n')
    if (r.status !== 0) return fail(`the ${APP} build failed (exit ${r.status}) after ${Math.round((Date.now() - t0) / 1000)} s`, tail)

    // 3. Swap in only a good build.
    const dist = path.join(SRC, 'target', 'evani-sizes', `${APP}-full`)
    const distTarget = fs.existsSync(path.join(dist, 'index.html')) ? dist : path.join(TARGET, 'evani-sizes', `${APP}-full`)
    if (!fs.existsSync(path.join(distTarget, 'index.html')) || !fs.readdirSync(distTarget).some(f => f.endsWith('.wasm'))) {
      return fail(`the build finished but ${dist} has no index.html and .wasm`, tail)
    }
    const stamp = new Date().toISOString().replace(/[-:]/g, '').replace(/\..+/, '')
    const name = `${sha.slice(0, 12)}-${stamp}`
    const stage = path.join(SITES, name + '.partial')
    fs.rmSync(stage, { recursive: true, force: true })
    fs.cpSync(distTarget, stage, { recursive: true })
    fs.copyFileSync(path.join(here, 'preview-overlay.js'), path.join(stage, 'studio-preview.js'))
    const summary = librarySummary(SRC)
    writeAtomic(
      path.join(stage, 'studio-preview-data.json'),
      JSON.stringify({ sha, subject, builtAt: new Date().toISOString().replace('T', ' ').slice(0, 16) + ' UTC', ...summary }),
    )
    const indexFile = path.join(stage, 'index.html')
    let html = fs.readFileSync(indexFile, 'utf8')
    const tag = '<script src="./studio-preview.js"></script>'
    html = html.includes('</body>') ? html.replace('</body>', `${tag}</body>`) : html + tag
    fs.writeFileSync(indexFile, html)
    fs.renameSync(stage, path.join(SITES, name))
    const landedAt = new Date().toISOString()
    const nNew = updateWhatsNew(repo, sha, landedAt, path.join(SITES, name))
    log(`what's new: ${nNew} new entr${nNew === 1 ? 'y' : 'ies'} since the last good build`)
    writeAtomic(path.join(ROOT, 'current.txt'), name)
    cleanup(name)

    status.ok = true
    status.site = name
    status.materials = { built: summary.materials.filter(m => m[3] === 'ready').length, planned: summary.materials.filter(m => m[3] !== 'ready').length }
    status.assets = { ready: summary.assets.filter(m => m[3] === 'ready').length, notReady: summary.assets.filter(m => m[3] !== 'ready').length }
    status.finishedAt = new Date().toISOString()
    writeAtomic(path.join(ROOT, 'status.json'), JSON.stringify(status, null, 2))
    log(`built in ${Math.round((Date.now() - t0) / 1000)} s; now serving ${name}`)
    log(`library: ${status.materials.built} materials built, ${status.materials.planned} planned; ${status.assets.ready} assets ready, ${status.assets.notReady} planned or being built`)
    await ensureServer()
    console.log(`\nStudio preview: ${URL_}`)
  } catch (e) {
    fail(e.message || String(e))
  }
}

main()
