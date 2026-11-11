// Stage 1 gate evidence: the Evani.dev tab and the REAL Evani.Dev live server.
//
// A hidden test copy of Evani.App (built from Evani.App's own source, the same tab code as the
// installed app) opens an Evani.Dev engine project and must:
//   A. start the real live launcher (.vibedeck/dev-harness/live.mjs -> evani-live-bridge serve)
//      and connect by itself, never showing an error (so never a Try again), with the real
//      sandbox page joined on /engine and the tab's /evani-dev relay open;
//   A2. reconnect by itself after that live server stops (the tab starts it again);
//   B. show the plain "update" message when the live server's handshake reports another Live API
//      version: a proxy in front of the real bridge rewrites /live/status's `protocol` to
//      evani-live/2 (newer: "Update Evani.App.") and evani-live/0 (older: "Update the engine.");
//   C. connect again by itself once the address points at the real bridge again.
//
// Never on a screen, never takes focus, never uses the user's mouse or keyboard, never touches
// port 8096 (the real tab's engine). Every process it starts is ended at the end.
//
// Run (from anywhere):
//   node tools/live-check/tab-connect-proof.mjs --engine <engine root copy> --app <Evani.App build main/index.js>
//        --desktop <Evani.App source folder> --out <screenshots folder>
//
// Preparing (what the 2026-10-15 run did; evidence in .vibedeck/screenshots/gate1/tab-connect.md):
//   1. The test copy of Evani.App: in the Evani.App source folder,
//      node node_modules/electron-vite/bin/electron-vite.js build --outDir %TEMP%\evani-tabproof\out
//      then a junction %TEMP%\evani-tabproof\node_modules -> <Evani.App source>\node_modules
//      (the build leaves its libraries outside the bundle).
//   2. The engine copy: git worktree add --detach <a folder next to the engine> HEAD, then copy in
//      target\debug\evani-live-bridge.exe, target\evani-live\sandbox\dist and
//      target\evani-sizes\size-report.json (the dist's build stamp records the published size), and
//      touch the copies so they count as current.
// The engine root must not be the main checkout itself (the bridge rewrites its session file,
// which the real tab uses). With those copies the live server starts in about a second;
// EVANI_LIVE_HOT=0 keeps the bridge on the prebuilt Trunk build (no compile).

import { spawn } from 'node:child_process'
import fs from 'node:fs'
import http from 'node:http'
import net from 'node:net'
import os from 'node:os'
import path from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const arg = (name, dflt) => {
  const i = process.argv.indexOf(`--${name}`)
  return i > 0 && process.argv[i + 1] ? process.argv[i + 1] : dflt
}
const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..', '..')
const DESKTOP = path.resolve(arg('desktop', process.env.EVANI_APP_DIR || path.join(REPO, '..', 'claude-code-desktop')))
const MAIN = path.resolve(arg('app', path.join(os.tmpdir(), 'evani-tabproof', 'out', 'main', 'index.js')))
const ENG = path.resolve(arg('engine', path.join(REPO, '..', path.basename(REPO) + '-tabproof')))
const SHOTS = path.resolve(arg('out', path.join(os.tmpdir(), 'evani-tabproof', 'shots')))
const RUN = path.join(os.tmpdir(), 'evani-tabproof', 'run')
const PROFILE = path.join(RUN, 'profile')
const GARDEN = path.join(RUN, 'garden')

if (ENG.toLowerCase() === REPO.toLowerCase()) throw new Error('use a copy of the engine, not the main checkout (its session file belongs to the real tab)')
for (const f of [MAIN, path.join(ENG, '.vibedeck', 'dev-harness', 'live.mjs'), path.join(ENG, 'apps', 'sandbox', 'evani.toml'), path.join(ENG, 'target', 'debug', 'evani-live-bridge.exe'), path.join(ENG, 'target', 'evani-live', 'sandbox', 'dist', 'index.html')]) {
  if (!fs.existsSync(f)) throw new Error(`missing ${f}`)
}
const { guard, killTreeSync } = await import(pathToFileURL(path.join(DESKTOP, 'scripts', 'harnessGuard.mjs')).href)

const sleep = (ms) => new Promise((r) => setTimeout(r, ms))
let failures = 0
const results = []
const ok = (what, pass, detail = '') => {
  if (!pass) failures++
  const line = `${pass ? 'PASS' : 'FAIL'}  ${what}${detail ? `  (${detail})` : ''}`
  results.push(line)
  console.log(line)
}
const freePort = () =>
  new Promise((resolve) => {
    const s = net.createServer().listen(0, '127.0.0.1', () => {
      const { port } = s.address()
      s.close(() => resolve(port))
    })
  })
const status = async (port) => {
  try {
    return await (await fetch(`http://127.0.0.1:${port}/live/status`, { cache: 'no-store', signal: AbortSignal.timeout(2000) })).json()
  } catch {
    return null
  }
}

// ---- Ports: the real bridge (started by the tab itself) and the handshake proxy ----

const P1 = await freePort()
let P2 = await freePort()
while (P2 === P1) P2 = await freePort()
if (P1 === 8096 || P2 === 8096) throw new Error('never use the real engine’s port')
const URL1 = `http://localhost:${P1}/?live=1`
const URL2 = `http://localhost:${P2}/?live=1`

// The proxy: everything goes to the real bridge on P1, byte for byte, except /live/status, whose
// `protocol` is rewritten to the version under test. WebSocket upgrades are piped through as is.
let fakeProtocol = 'evani-live/2'
let proxied = 0
const proxy = http.createServer((req, res) => {
  const up = http.request({ host: '127.0.0.1', port: P1, path: req.url, method: req.method, headers: { ...req.headers, host: `localhost:${P1}` } }, (r) => {
    if ((req.url || '').startsWith('/live/status')) {
      let body = ''
      r.setEncoding('utf8')
      r.on('data', (d) => (body += d))
      r.on('end', () => {
        proxied++
        let out = body
        try {
          const j = JSON.parse(body)
          j.protocol = fakeProtocol
          out = JSON.stringify(j)
        } catch {}
        res.writeHead(r.statusCode ?? 502, { 'content-type': 'application/json', 'cache-control': 'no-store' })
        res.end(out)
      })
    } else {
      res.writeHead(r.statusCode ?? 502, r.headers)
      r.pipe(res)
    }
  })
  up.on('error', () => {
    try {
      res.writeHead(502)
      res.end()
    } catch {}
  })
  req.pipe(up)
})
proxy.on('upgrade', (req, socket, head) => {
  const up = net.connect(P1, '127.0.0.1', () => {
    const lines = [`${req.method} ${req.url} HTTP/1.1`]
    for (let i = 0; i < req.rawHeaders.length; i += 2) lines.push(`${req.rawHeaders[i]}: ${/^host$/i.test(req.rawHeaders[i]) ? `localhost:${P1}` : req.rawHeaders[i + 1]}`)
    up.write(lines.join('\r\n') + '\r\n\r\n')
    if (head?.length) up.write(head)
    socket.pipe(up).pipe(socket)
  })
  up.on('error', () => socket.destroy())
  socket.on('error', () => up.destroy())
})
proxy.listen(P2, '127.0.0.1')

// ---- The projects and the profile ----

fs.rmSync(RUN, { recursive: true, force: true })
fs.rmSync(SHOTS, { recursive: true, force: true })
for (const d of [PROFILE, GARDEN, SHOTS]) fs.mkdirSync(d, { recursive: true })
fs.writeFileSync(path.join(GARDEN, 'package.json'), '{ "name": "garden" }\n')
const E = { id: 'proof-sandbox', name: 'Evani.Dev sandbox', path: ENG }
const G = { id: 'proof-garden', name: 'Garden site', path: GARDEN }
fs.writeFileSync(path.join(PROFILE, 'settings.json'), JSON.stringify({ projects: [G, E], accounts: [], selectedProjectId: G.id, settings: { computerUse: false, claudePath: '', projectsRoot: RUN, theme: 'dark' } }, null, 2))

// ---- The hidden app ----

const safety = guard({ limitMs: 600_000, label: 'tab-connect-proof', cleanup: () => proxy.close() })
const electron = path.join(DESKTOP, 'node_modules', 'electron', 'dist', process.platform === 'win32' ? 'electron.exe' : 'electron')
async function connect(url) {
  const ws = new WebSocket(url)
  await new Promise((resolve, reject) => {
    ws.onopen = resolve
    ws.onerror = reject
  })
  let id = 0
  const waiting = new Map()
  ws.onmessage = (e) => {
    const msg = JSON.parse(e.data)
    if (msg.id && waiting.has(msg.id)) {
      waiting.get(msg.id)(msg)
      waiting.delete(msg.id)
    }
  }
  ws.onclose = () => {
    for (const w of waiting.values()) w({ error: { message: 'closed' } })
    waiting.clear()
  }
  const send = (method, params = {}) =>
    new Promise((resolve, reject) => {
      const n = ++id
      waiting.set(n, (m) => (m.error ? reject(new Error(m.error.message)) : resolve(m.result)))
      try {
        ws.send(JSON.stringify({ id: n, method, params }))
      } catch (e) {
        waiting.delete(n)
        reject(e)
      }
    })
  const evaluate = async (expression) => {
    const r = await send('Runtime.evaluate', { expression: `(async () => { ${expression} })()`, awaitPromise: true, returnByValue: true })
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description ?? r.exceptionDetails.text)
    return r.result.value
  }
  return { send, evaluate, close: () => ws.close() }
}
async function targets(port) {
  for (let i = 0; i < 150; i++) {
    try {
      return await (await fetch(`http://127.0.0.1:${port}/json/list`)).json()
    } catch {
      await sleep(200)
    }
  }
  throw new Error(`nothing on port ${port}`)
}
const isEngine = (url) => new RegExp(`^http://(localhost|127\\.0\\.0\\.1):(${P1}|${P2})`).test(url)

const inspectPort = await freePort()
const debugPort = await freePort()
const env = { ...process.env, VIBEDECK_HEADLESS: '1', VIBEDECK_PROFILE: PROFILE, VIBEDECK_EVANIDEV_TEST: '1', VIBEDECK_QUEUECHECK_TEST: '1', EVANI_LIVE_HOT: '0', CARGO_BUILD_JOBS: '2', NODE_PATH: path.join(DESKTOP, 'node_modules') }
for (const k of ['ELECTRON_RUN_AS_NODE', 'ELECTRON_RENDERER_URL', 'NODE_ENV']) delete env[k]
const child = spawn(electron, [MAIN, `--inspect=${inspectPort}`, `--remote-debugging-port=${debugPort}`, '--mute-audio', '--disable-features=CalculateNativeWinOcclusion', '--disable-backgrounding-occluded-windows', '--disable-renderer-backgrounding'], { cwd: DESKTOP, env, stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true })
safety.track(child)
try {
  if (child.pid) os.setPriority(child.pid, 10)
} catch {}
const log = fs.createWriteStream(path.join(RUN, 'app.log'))
child.stdout.pipe(log)
child.stderr.pipe(log)

let main, ui
// A test copy that can't load must fail at once, not wait for the time limit.
const loadFailed = setInterval(() => {
  try {
    if (/App threw an error during load/.test(fs.readFileSync(path.join(RUN, 'app.log'), 'utf8'))) {
      console.error('FAIL  the test copy of Evani.App did not load (see run/app.log)')
      killTreeSync(child.pid)
      proxy.close()
      process.exit(1)
    }
  } catch {}
}, 1000)
try {
  main = await connect((await targets(inspectPort))[0].webSocketDebuggerUrl)
  for (let i = 0; i < 150 && !(await main.evaluate('return !!process.mainModule.require("electron").BrowserWindow.getAllWindows()[0]').catch(() => false)); i++) await sleep(200)
  // The window: off every screen, no focus, no mouse, no taskbar, see-through.
  await main.evaluate(`const { BrowserWindow, screen } = process.mainModule.require('electron');
    const w = BrowserWindow.getAllWindows()[0];
    w.setFocusable(false); w.setIgnoreMouseEvents(true); w.setSkipTaskbar(true); w.setAlwaysOnTop(false); w.setOpacity(0);
    const ds = screen.getAllDisplays().map((d) => d.bounds);
    const x = Math.max(...ds.map((b) => b.x + b.width)) + 2000, y = Math.max(...ds.map((b) => b.y + b.height)) + 2000;
    w.setBounds({ x, y, width: 1680, height: 1000 });
    const touches = (b) => ds.some((d) => b.x < d.x + d.width && b.x + b.width > d.x && b.y < d.y + d.height && b.y + b.height > d.y);
    if (touches(w.getBounds())) { w.hide(); throw new Error('the test window would be on a screen') }
    w.showInactive();
    if (touches(w.getBounds())) { w.hide(); throw new Error('moved onto a screen when shown') }
    return true`)
  for (let i = 0; i < 100 && !ui; i++) {
    const t = (await targets(debugPort)).find((t) => t.type === 'page' && !t.url.startsWith('devtools') && !isEngine(t.url))
    if (t) {
      try {
        ui = await connect(t.webSocketDebuggerUrl)
        await ui.send('Page.enable')
      } catch {
        ui = null
      }
    }
    if (!ui) await sleep(200)
  }
  if (!ui) throw new Error('no window page')
  clearInterval(loadFailed)
  const q =(expr) => ui.evaluate(expr)
  const until = async (fn, ms = 30_000) => {
    const end = Date.now() + ms
    while (Date.now() < end) {
      if (await fn().catch(() => false)) return true
      await sleep(250)
    }
    return false
  }
  const T = 'globalThis.__evaniDevTest'
  await q(`Element.prototype.requestPointerLock = () => Promise.resolve(); Element.prototype.requestFullscreen = () => Promise.resolve(); if (navigator.keyboard) navigator.keyboard.lock = () => Promise.resolve(); return true`)
  // The engine page: the same stubs, once it exists.
  const stubEngine = async () => {
    const t = (await targets(debugPort)).find((t) => t.type === 'page' && isEngine(t.url))
    if (!t) return false
    const page = await connect(t.webSocketDebuggerUrl)
    await page.evaluate(`Element.prototype.requestPointerLock = () => Promise.resolve(); Element.prototype.requestFullscreen = () => Promise.resolve(); if (navigator.keyboard) navigator.keyboard.lock = () => Promise.resolve(); return true`).catch(() => {})
    page.close()
    return true
  }
  const shot = async (name, selector) => {
    const measure = () => q(`const el = document.querySelector(${JSON.stringify(selector)}); if (!el) return null; el.scrollIntoView({ block: 'start' }); const r = el.getBoundingClientRect(); return { x: r.x + scrollX, y: r.y + scrollY, width: r.width, height: r.height }`)
    let r = await measure()
    if (!r) throw new Error(`nothing at ${selector}`)
    for (let i = 0; i < 10; i++) {
      await sleep(150)
      const next = await measure()
      if (JSON.stringify(next) === JSON.stringify(r)) break
      r = next
    }
    const res = await ui.send('Page.captureScreenshot', { format: 'png', captureBeyondViewport: true, clip: { ...r, scale: 1 } })
    const out = path.join(SHOTS, `${name}.png`)
    fs.writeFileSync(out, Buffer.from(res.data, 'base64'))
    console.log('screenshot', out)
  }
  // The game page is a separate view over the window: picture it and lay the picture over its slot.
  const OVERLAY = 'tabproof-overlay'
  const composite = async () => {
    const b64 = await main.evaluate(`return await ${T}.capture()`)
    if (!b64) return false
    return q(`const slot = document.querySelector('.evanidev-slot'); if (!slot) return false;
      const r = slot.getBoundingClientRect(); let img = document.getElementById('${OVERLAY}');
      if (!img) { img = document.createElement('img'); img.id = '${OVERLAY}'; document.body.appendChild(img) }
      Object.assign(img.style, { position: 'fixed', left: r.x + 'px', top: r.y + 'px', width: r.width + 'px', height: r.height + 'px', pointerEvents: 'none', zIndex: 50 });
      img.src = 'data:image/png;base64,${b64}'; await img.decode().catch(() => {}); return true`)
  }
  const uncomposite = () => q(`document.getElementById('${OVERLAY}')?.remove(); return true`)
  const theme = (t) => q(`document.documentElement.dataset.theme = '${t}'; document.body.dataset.theme = '${t}'; document.documentElement.classList.remove('${t === 'light' ? 'dark' : 'light'}'); document.documentElement.classList.add('${t}'); return true`)
  const view = (id) => main.evaluate(`return ${T}.view(${JSON.stringify(id)})`)
  const activeTab = () => q(`return document.querySelector('.project-panel .tabs > .tab.active')?.textContent.trim() ?? ''`)
  const pickProject = (name) => q(`const li = [...document.querySelectorAll('li.project')].find((x) => x.textContent.includes(${JSON.stringify(name)})); if (!li) return false; (li.querySelector('button, .project-name, span') ?? li).click(); li.click(); return true`)
  const devPanel = () => q(`return !!document.querySelector('.evanidev-panel')`)
  const clickTab = (label) => q(`const b = [...document.querySelectorAll('.project-panel .tabs > button.tab')].find((x) => x.textContent.trim().startsWith(${JSON.stringify(label)})); if (!b) return false; b.click(); return true`)
  const retryShown = () => q(`return !!document.querySelector('[data-test="evanidev-retry"]')`)
  const setAddress = (url) => q(`localStorage.setItem('evaniDev:settings', JSON.stringify({ url: ${JSON.stringify(url)}, folder: '' })); return true`)
  const trackStates = (id) => {
    const states = []
    const retries = []
    const t = setInterval(() => {
      void view(id).then((v) => states.at(-1) !== v.state && states.push(v.state)).catch(() => {})
      void retryShown().then((r) => r && retries.push(Date.now())).catch(() => {})
    }, 150)
    return { states, retries, stop: () => clearInterval(t) }
  }

  ok('the hidden Evani.App loaded with both projects', await until(() => q(`const t = [...document.querySelectorAll('li.project')].map((li) => li.textContent).join('|'); return t.includes(${JSON.stringify(E.name)}) && t.includes(${JSON.stringify(G.name)})`)))
  ok('the test handles are there', await until(() => main.evaluate(`return !!${T}`)))
  ok(`nothing answers on the test port ${P1} yet (the tab must start the live server itself)`, !(await status(P1)))
  await setAddress(URL1)
  await q(`localStorage.setItem('projectTab', 'threads'); return true`)
  await sleep(1000)

  // ---- A. Opening the engine project: the tab starts the real live server and connects by itself ----
  let track = trackStates(E.id)
  const t0 = Date.now()
  await pickProject(E.name)
  ok('its Evani.dev tab is created by itself', await until(() => devPanel(), 15_000))
  const upA = await until(async () => (await view(E.id)).state === 'up', 90_000)
  const msA = Date.now() - t0
  ok('… and connects on its own to the real live server: state up', upA, `${(msA / 1000).toFixed(1)} s`)
  await until(() => main.evaluate(`return ${T}.relayOpen()`), 15_000)
  await until(async () => (await status(P1))?.engine_connected === true, 30_000)
  await stubEngine().catch(() => {})
  track.stop()
  ok('… it never showed an error, so never a Try again', !track.states.includes('error') && track.retries.length === 0, track.states.join(' → '))
  const sA = await status(P1)
  ok('… the real bridge answers (evani-live/1, this engine folder, game sandbox, built and idle)', sA?.protocol === 'evani-live/1' && sA?.engine === 'evani.dev' && path.resolve(sA?.root ?? '') === ENG && sA?.game === 'sandbox' && sA?.phase === 'idle',JSON.stringify({ protocol: sA?.protocol, root: sA?.root, game: sA?.game, phase: sA?.phase }))
  ok('… the tab started it itself (its own launcher, not one already running)', await main.evaluate(`return ${T}.hasServer()`))
  ok('… the real sandbox page joined the bridge on /engine', sA?.engine_connected === true)
  ok('… the tab’s news channel (/evani-dev) is open', await main.evaluate(`return ${T}.relayOpen()`))
  const vA = await view(E.id)
  ok('… the handshake read Live API v1 on both sides', vA.liveApi?.engine === 1 && (vA.liveApi?.app ?? []).includes(1), JSON.stringify(vA.liveApi))
  await shot('tab-connect-1-background-threads', '.project-panel')
  await clickTab('Evani.dev')
  ok('opening the tab shows the running game at once', await until(() => main.evaluate(`return ${T}.visible()`), 10_000))
  await sleep(4000) // the sandbox draws a few frames
  await composite()
  await shot('tab-connect-2-connected', '.evanidev-panel')
  await uncomposite()

  // ---- A2. The live server stops under the running page: the tab starts it again by itself ----
  const bridgePid = sA ? JSON.parse(fs.readFileSync(path.join(ENG, '.evani', 'live', 'session.json'), 'utf8')).pid : null
  track = trackStates(E.id)
  const t1 = Date.now()
  if (bridgePid) killTreeSync(bridgePid)
  ok('the live server was stopped from outside', await until(async () => !(await status(P1)), 15_000), `pid ${bridgePid}`)
  const backA2 = await until(async () => (await view(E.id)).state === 'up' && (await status(P1))?.engine_connected === true && (await main.evaluate(`return ${T}.relayOpen()`)), 120_000)
  track.stop()
  ok('… the tab started it again and reconnected by itself', backA2, `${((Date.now() - t1) / 1000).toFixed(1)} s; ${track.states.join(' → ') || 'stayed up'}`)
  ok('… with no Try again on screen at any point', track.retries.length === 0)
  await stubEngine().catch(() => {})
  await sleep(3000)
  await composite()
  await shot('tab-connect-3-reconnected-after-restart', '.evanidev-panel')
  await uncomposite()
  await clickTab('Threads')

  // ---- B. The handshake reports another Live API version ----
  for (const [proto, tag, re, fix] of [
    ['evani-live/2', 'newer', /This engine needs a newer Evani\.App\. It speaks Live API v2; Evani\.App speaks Live API v1\./, 'Update Evani.App.'],
    ['evani-live/0', 'older', /This engine is older than Evani\.App expects\. It speaks Live API v0; Evani\.App speaks Live API v1\./, 'Update the engine.']
  ]) {
    fakeProtocol = proto
    await pickProject(G.name)
    await until(async () => !(await devPanel()), 10_000)
    await setAddress(URL2)
    await sleep(800)
    await pickProject(E.name)
    // The tab keeps its last state per engine: wait for this version's own message.
    const seen = await until(async () => {
      const v = await view(E.id)
      return v.errorKind === 'version' && re.test(v.error ?? '')
    }, 60_000)
    const v = await view(E.id)
    ok(`B (${tag}): the live server reporting ${proto} gives the version message`, seen && re.test(v.error ?? '') && v.fix === fix, `${v.error} / Fix: ${v.fix}`)
    if (tag === 'newer') ok('… with a Restart & update action', v.action === 'update-app')
    await clickTab('Evani.dev')
    ok(`… the tab shows it in plain words${tag === 'newer' ? ', with Restart & update' : ''}`, await until(() => q(`const e = document.querySelector('[data-test="evanidev-error"]'); return !!e && /don.t match/.test(e.innerText) && e.innerText.includes(${JSON.stringify(fix)})${tag === 'newer' ? ` && !!document.querySelector('[data-test="evanidev-update-app"]')` : ''}`), 10_000))
    ok('… and the game page is not shown', !(await main.evaluate(`return ${T}.visible()`)))
    await shot(`version-mismatch-${tag === 'newer' ? '1-engine-newer' : '3-engine-older'}-dark`, '.evanidev-panel')
    if (tag === 'newer') {
      await theme('light')
      await shot('version-mismatch-2-engine-newer-light', '.evanidev-panel')
      await theme('dark')
    }
    await clickTab('Threads')
  }
  ok('the proxy really sat in front of the real bridge (its /live/status was read through it)', proxied > 0, `${proxied} reads`)

  // ---- C. Back on the real address: connected again by itself ----
  await pickProject(G.name)
  await until(async () => !(await devPanel()), 10_000)
  await setAddress(URL1)
  await sleep(800)
  track = trackStates(E.id)
  await pickProject(E.name)
  const backC = await until(async () => (await view(E.id)).state === 'up', 60_000)
  track.stop()
  ok('C: pointing back at the real live server connects again by itself', backC && !track.states.includes('error'), track.states.join(' → '))
  await clickTab('Evani.dev')
  await until(() => main.evaluate(`return ${T}.visible()`), 10_000)
  await stubEngine().catch(() => {})
  await sleep(3000)
  await composite()
  await shot('version-mismatch-4-restored-connected', '.evanidev-panel')
  await uncomposite()
  ok('port 8096 (the real tab’s engine) was never asked', !/(127\.0\.0\.1|localhost):8096/.test(fs.readFileSync(path.join(RUN, 'app.log'), 'utf8')))
} catch (e) {
  failures++
  results.push(`FAIL  ${e?.message ?? e}`)
  console.error('FAIL ', e?.stack ?? e)
} finally {
  clearInterval(loadFailed)
  try {
    ui?.close()
    main?.close()
  } catch {}
  killTreeSync(child.pid)
  proxy.close()
  await sleep(1500)
}
const left = await status(P1)
const leftLine = `${left ? 'FAIL' : 'PASS'}  nothing left running on the test port ${P1} after the run`
if (left) failures++
results.push(leftLine)
console.log(leftLine)
fs.writeFileSync(path.join(SHOTS, 'results.txt'), results.join('\n') + '\n')
console.log(failures ? `\n${failures} check(s) failed` : '\nAll checks passed')
process.exit(failures ? 1 : 0)
