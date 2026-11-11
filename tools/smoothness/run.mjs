// One smoothness run of one built game: serve the build folder, open it in
// headless, muted Chrome on the graphics card (tools/bench-runner/chrome.mjs:
// throwaway profile, one rendering Chrome at a time through the GPU lock),
// drive it with the config's synthetic keys, and record
//  - every presented frame (tools/live-check/frame-gaps.mjs FRAME_HOOK), and
//  - where the config's probe was drawn on every frame, through the live
//    API's `motion.trace` / `motion.read` (docs/LIVE_API.md 6.32).
// In networked mode a local authoritative host (tools/evani-host/server.mjs,
// the game's referee.wasm) runs behind a relay that adds delay and jitter
// (relay.mjs), and the page joins it as a player.
//
// Never the real mouse or keyboard: keys go in as CDP Input events, and the
// page's pointer lock, fullscreen and keyboard lock are stubbed out.

import { spawn } from 'node:child_process';
import { existsSync, readFileSync } from 'node:fs';
import http from 'node:http';
import net from 'node:net';
import { dirname, extname, join, normalize, resolve, sep } from 'node:path';
import { fileURLToPath } from 'node:url';
import { startChrome, STUBS, sleep } from '../bench-runner/chrome.mjs';
import { FRAME_HOOK } from '../live-check/frame-gaps.mjs';
import { profile, startRelay } from './relay.mjs';

const here = dirname(fileURLToPath(import.meta.url));

const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.json': 'application/json', '.css': 'text/css', '.png': 'image/png', '.jpg': 'image/jpeg', '.glb': 'model/gltf-binary', '.ktx2': 'image/ktx2', '.bin': 'application/octet-stream' };

/** Map a URL path to a file inside `root` (null outside it). */
export function resolveInside(root, urlPath) {
  const rel = decodeURIComponent(urlPath.split('?')[0]).replace(/^\/+/, '') || 'index.html';
  const full = normalize(join(root, rel));
  return full === root || full.startsWith(root.endsWith(sep) ? root : root + sep) ? full : null;
}

/** Serve a build folder on a free localhost port. */
export async function serveDir(dir) {
  const root = resolve(dir);
  const server = http.createServer((req, res) => {
    const f = resolveInside(root, new URL(req.url, 'http://x').pathname);
    if (!f) {
      res.writeHead(403).end();
      return;
    }
    let data;
    try {
      data = readFileSync(f);
    } catch {
      res.writeHead(404).end();
      return;
    }
    res.writeHead(200, { 'content-type': TYPES[extname(f)] || 'application/octet-stream', 'cache-control': 'no-store' });
    res.end(data);
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return { port: server.address().port, close: () => server.close() };
}

const freePort = () =>
  new Promise((ok, err) => {
    const s = net.createServer();
    s.on('error', err);
    s.listen(0, '127.0.0.1', () => {
      const p = s.address().port;
      s.close(() => ok(p));
    });
  });

/** Windows virtual key codes for CDP key events. */
export function keyInfo(code) {
  const named = { Space: [' ', 32], Enter: ['Enter', 13], Escape: ['Escape', 27], Tab: ['Tab', 9], Backspace: ['Backspace', 8], ShiftLeft: ['Shift', 16], ShiftRight: ['Shift', 16], ControlLeft: ['Control', 17], ControlRight: ['Control', 17], AltLeft: ['Alt', 18], ArrowLeft: ['ArrowLeft', 37], ArrowUp: ['ArrowUp', 38], ArrowRight: ['ArrowRight', 39], ArrowDown: ['ArrowDown', 40] };
  if (named[code]) return { key: named[code][0], vk: named[code][1], location: /Right$/.test(code) ? 2 : /Left$/.test(code) && !code.startsWith('Arrow') ? 1 : 0 };
  let m = /^Key([A-Z])$/.exec(code);
  if (m) return { key: m[1].toLowerCase(), vk: m[1].charCodeAt(0), location: 0 };
  m = /^Digit([0-9])$/.exec(code);
  if (m) return { key: m[1], vk: 48 + Number(m[1]), location: 0 };
  throw new Error(`unknown key code ${code} (use KeyboardEvent.code names: KeyW, ArrowLeft, Space, ShiftLeft, ...)`);
}

/**
 * The input script as timed events. `steps`: `[{ at, down?: [codes], up?: [codes], click?: selector }]`
 * (seconds from the start); `repeat_s` loops it until `duration_s`. Sorted by time.
 */
export function schedule(steps, duration_s, repeat_s = 0) {
  const out = [];
  const loops = repeat_s > 0 ? Math.ceil(duration_s / repeat_s) : 1;
  for (let k = 0; k < loops; k++) {
    for (const s of steps || []) {
      const at = s.at + k * (repeat_s || 0);
      if (at > duration_s) continue;
      for (const c of [].concat(s.up || [])) out.push({ at, type: 'keyUp', code: c });
      for (const c of [].concat(s.down || [])) out.push({ at, type: 'keyDown', code: c });
      if (s.click) out.push({ at, type: 'click', selector: s.click });
    }
  }
  return out.sort((a, b) => a.at - b.at);
}

/** Start the local host (evani-host) for the networked mode; resolves with its port. */
async function startHost(cfg, gameDir) {
  const port = await freePort();
  const referee = resolve(cfg.referee);
  if (!existsSync(referee)) throw new Error(`networked mode: no referee at ${referee} (build it first; see tools/smoothness/README.md)`);
  const args = [join(here, '..', 'evani-host', 'server.mjs'), '--referee', referee, '--port', String(port), '--static', resolve(gameDir), ...(cfg.host_args || [])];
  const proc = spawn(process.execPath, args, { stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true });
  let err = '';
  proc.stderr.on('data', (d) => (err += d));
  await new Promise((ok, bad) => {
    const t = setTimeout(() => bad(new Error('evani-host did not start: ' + err.slice(0, 400))), 20000);
    proc.stdout.once('data', () => {
      clearTimeout(t);
      ok();
    });
    proc.once('exit', (c) => bad(new Error(`evani-host exited (${c}): ${err.slice(0, 400)}`)));
  });
  return { port, stop: () => proc.kill() };
}

/**
 * Run one build. `opts`: `{ build, label, config, net, seed, log }`.
 * Returns the trace: `{ label, build, width, height, net, probe, mark0, mark1, pres, motion, notes, relay }`.
 */
export async function runOnce({ build, label, config: cfg, net: netSpec = 'none', seed = 1, log = console.log }) {
  const W = cfg.width ?? 1280;
  const H = cfg.height ?? 720;
  const notes = [];
  const site = await serveDir(build);
  let host = null;
  let relay = null;
  let chrome = null;
  const p = profile(netSpec);
  const online = netSpec !== 'none';
  // Online a game may skip its menus (it joins the host): its own start and ready.
  const startSteps = online && cfg.net && cfg.net.start ? cfg.net.start : cfg.start || [];
  const readyCheck = online && cfg.net && cfg.net.ready ? cfg.net.ready : cfg.ready;
  try {
    let query = cfg.query || '';
    if (cfg.net && online) {
      host = await startHost(cfg.net, build);
      relay = await startRelay(host.port, p, seed);
      const param = cfg.net.server_param || 'evani-server';
      query += `${query ? '&' : ''}${param}=${encodeURIComponent(`ws://127.0.0.1:${relay.port}/`)}`;
      log(`  ${label}: host on ${host.port}, relay on ${relay.port} (${JSON.stringify(p)})`);
    } else if (online) {
      throw new Error('--net needs a "net" block in the smoothness config (the referee to host)');
    }
    chrome = await startChrome({ width: W, height: H, prefix: 'evani-smooth-' });
    const { cdp } = chrome;
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    const s = (m, a) => cdp.send(m, a, sessionId);
    const pageErrors = [];
    cdp.on((m) => {
      if (m.sessionId !== sessionId) return;
      if (m.method === 'Runtime.exceptionThrown') pageErrors.push(String(m.params.exceptionDetails?.exception?.description || m.params.exceptionDetails?.text).slice(0, 300));
    });
    await s('Page.enable');
    await s('Runtime.enable');
    await s('Emulation.setDeviceMetricsOverride', { width: W, height: H, deviceScaleFactor: 1, mobile: false });
    await s('Emulation.setFocusEmulationEnabled', { enabled: true });
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS + '\n' + FRAME_HOOK });
    const ev = async (expr, timeoutMs = 60000) => {
      const r = await cdp.send('Runtime.evaluate', { expression: expr, returnByValue: true, awaitPromise: true }, sessionId, timeoutMs);
      if (r.exceptionDetails) throw new Error(`page: ${r.exceptionDetails.exception?.description || r.exceptionDetails.text}`);
      return r.result?.value;
    };
    const url = `http://127.0.0.1:${site.port}/index.html${query ? '?' + query : ''}`;
    await s('Page.navigate', { url });
    // Ready: the engine drew its first frame and the config's own check (if any) holds.
    const readyExpr = `(() => { try { return !!(window.__engine && window.__engine.started) && (${readyCheck || 'true'}); } catch (e) { return false; } })()`;
    let ready = false;
    for (let i = 0; i < 240 && !ready; i++) {
      await sleep(500);
      ready = await ev(readyExpr).catch(() => false);
    }
    if (!ready) throw new Error(`${label}: the game did not start in 120 s (${pageErrors[0] || (await ev('JSON.stringify(window.__engine || null)').catch(() => '?'))})`);
    const key = async (type, code) => {
      const k = keyInfo(code);
      await s('Input.dispatchKeyEvent', { type, code, key: k.key, windowsVirtualKeyCode: k.vk, nativeVirtualKeyCode: k.vk, location: k.location });
    };
    const held = new Set();
    const play = async (events, t0) => {
      for (const e of events) {
        const wait = t0 + e.at * 1000 - Date.now();
        if (wait > 0) await sleep(wait);
        if (e.type === 'click') await ev(`(document.querySelector(${JSON.stringify(e.selector)}) || { click() {} }).click()`);
        else {
          await key(e.type, e.code);
          if (e.type === 'keyDown') held.add(e.code);
          else held.delete(e.code);
        }
      }
    };
    // The start script (menus, countdown), then the settle time.
    const start = schedule(startSteps, Infinity);
    await play(start, Date.now());
    await sleep((cfg.warmup_s ?? 3) * 1000);
    // The motion probe through the live API.
    const hasLive = await ev('!!(window.evani && window.evani.live && window.evani.live.call)');
    let traced = false;
    if (!hasLive) notes.push('no live API in this build (a ship build?): frame times only');
    else {
      const r = await ev(`window.evani.live.call('motion.trace', ${JSON.stringify({ probe: cfg.probe || { kind: 'camera_target' } })}, { telemetry_ms: 0 }).then((r) => ({ ok: true, r }), (e) => ({ ok: false, code: e.code, message: e.message }))`);
      if (r.ok) traced = true;
      else notes.push(`motion.trace refused (${r.code}: ${r.message}): frame times only`);
    }
    const duration = cfg.duration_s ?? 20;
    const mark0 = await ev('performance.now()');
    await play(schedule(cfg.script || [], duration, cfg.repeat_s || 0), Date.now());
    const left = mark0 + duration * 1000 - (await ev('performance.now()'));
    if (left > 0) await sleep(left);
    const mark1 = await ev('performance.now()');
    for (const c of held) await key('keyUp', c);
    const pres = await ev(`(window.__evaniGaps ? window.__evaniGaps.pres.filter((t) => t >= ${mark0 - 1000} && t <= ${mark1 + 100}) : [])`);
    // The page's own work per animation frame ([start, ms]): tells a slow
    // frame of game work from the browser or GPU making the page wait.
    const cpu = await ev(`(window.__evaniGaps && window.__evaniGaps.cpu ? window.__evaniGaps.cpu.filter(([t]) => t >= ${mark0 - 1000} && t <= ${mark1 + 100}).map(([t, d]) => [Math.round(t * 10) / 10, Math.round(d * 10) / 10]) : [])`);
    const motion = [];
    if (traced) {
      await ev(`window.evani.live.call('motion.stop', {}, { telemetry_ms: 0 })`);
      for (let since = 0; ; ) {
        const r = await ev(`window.evani.live.call('motion.read', { since: ${since} }, { telemetry_ms: 0 })`);
        motion.push(...r.samples);
        if (r.next >= r.total || r.samples.length === 0) {
          if (r.dropped) notes.push(`${r.dropped} frames past the trace buffer were not recorded`);
          if (r.missing) notes.push(`the probe was not drawn on ${r.missing} frames`);
          break;
        }
        since = r.next;
      }
    }
    if (cfg.shot !== false) {
      const png = await s('Page.captureScreenshot', { format: 'png' });
      notes.push(`screenshot:${png.data.length}`);
      chrome.lastShot = png.data;
    }
    if (pageErrors.length) notes.push(`page errors: ${pageErrors.slice(0, 3).join(' | ')}`);
    const shot = chrome.lastShot || null;
    return {
      label,
      build: resolve(build),
      width: W,
      height: H,
      net: netSpec === 'none' ? 'none' : typeof netSpec === 'string' ? netSpec : 'custom',
      relay: relay ? { profile: p, chunks: relay.stats.down_chunks, delay_p50_ms: med(relay.stats.delay_ms), delay_max_ms: relay.stats.delay_ms.length ? Math.max(...relay.stats.delay_ms) : 0 } : null,
      probe: cfg.probe || { kind: 'camera_target' },
      mark0,
      mark1,
      pres,
      cpu,
      motion,
      notes: notes.filter((n) => !n.startsWith('screenshot:')),
      shot,
    };
  } finally {
    if (chrome) await chrome.stop();
    if (relay) relay.close();
    if (host) host.stop();
    site.close();
  }
}

function med(a) {
  if (!a.length) return 0;
  const s = a.slice().sort((x, y) => x - y);
  return s[s.length >> 1];
}
