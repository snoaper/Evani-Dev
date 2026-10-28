// Evani.Dev live capture check: starts the live bridge (which builds and
// serves the sandbox with the `live` module), opens it in headless Chrome
// over the DevTools protocol (no npm packages), then calls `capture.frame`
// and `compare.ab` (bloom on vs off) through `POST /live/v1/cmd` and saves
// the screenshots.
//
//   node tools/live-check/capture.mjs [--port 8096] [--out .vibedeck/screenshots/stage6]
//
// Writes capture.png, compare-a.png, compare-b.png and summary.json into
// --out; prints the summary on stdout. Exit 0 on success, 2 on failure.
//
// Review mode (used by `cargo xtask review`):
//   node tools/live-check/capture.mjs --game sandbox --times 0,20,40 --width 640 --out <dir>
//   node tools/live-check/capture.mjs --app-dir ../my-game/app --times 0,20,40 --width 640 --out <dir>
// (--app-dir: a game kept outside the engine folder, its app folder; the bridge
// builds and serves it with `serve --app-dir`. Its id is the `name` in its
// evani.toml. Not with --game.)
// takes one `capture.frame` per time on the game's bench camera path
// (camera "path", sim clock held at that time), saves capture-1.png,
// capture-2.png, ... and captures.json, and skips the compare.
//
// Moments mode (used by `cargo xtask review` when evani.toml lists
// `[review] moments`):
//   node tools/live-check/capture.mjs --game kart-racer --moments drift@high,item_hit@low --width 640 --out <dir>
// turns adaptive quality off (`aq.enabled` false, so the tier can't shed
// effects between shots), then per moment switches to its tier (when one is
// given after @) and calls `capture.frame { moment }`, saving
// moment-<name>.png. It writes captures.json with each capture's moment,
// label, tier and the engine commit, then puts the tier and `aq.enabled`
// back as they were. A moment listed on more than one tier is saved as
// moment-<name>-<tier>.png.
//
// --viewport WxH (default 1280x720) sets the page size in CSS pixels at
// device scale 1. A pixel-art game captured at its own width (for example
// --viewport 1920x1080 --width 1920) comes out exact: `render.pixel_art`
// renders at full device pixels on every tier.
//
// Process-check probes (both review modes; used by `cargo xtask review`, see
// docs/KIT_PROCESS.md items 11 to 15): a style rule hides the debug overlay
// and panel for the whole run, and each card capture is followed by a page
// screenshot (page-<name>.png) and a DOM probe of the overlay and of the HUD
// selectors given with `--hud "#hud0,.score"`, plus the live entity count
// (`session.status`). A moment's phone shot (page-<name>-phone.png) stages the
// moment again once the page is at the phone size, so it shows the same
// instant as the desktop one, not the game most of a second later (a drift
// let go and boosting). Every moment capture uses `hold_after` and every
// moment page shot settles the page's CSS animations first (SETTLE below), so
// two runs of the same build give the same pictures. `--phone-dpr 3` (evani.toml `[review] phone_dpr`)
// takes the phone shots at that device pixel ratio (default 1), so a pixel-art
// game picks the whole-pixel scale a real phone gets; the shot is then saved
// at device pixels. The registered params are read once (`schema.get`).
// For the world-edge check, every scripted camera (the boot view, each
// moment, eight views on the bench camera path) is rendered twice with
// `render.clear_color` black and white (geo-<name>-black.png and -white.png,
// 480 px wide) and the reply's camera pose is kept; the clear colour, which
// is also the fog colour, is put back afterwards. All of it goes into the
// summary's `probe` object.
//
// Showcase phase (both review modes, docs/KIT_PROCESS.md item 18): with
//   --showcase low,ultra --showcase-moments drift,boost --showcase-size 1920x1080
//   --engine abc1234 [--video drift] [--video-seconds 10]
// the page is resized to the showcase size and, per tier (`tier.set`, adaptive
// quality off), each moment is staged with `capture.frame { moment }` and the
// whole page (canvas and HUD) is screenshotted as shot-<moment>-<tier>.png
// with a burned-in label "Low · engine abc1234 · drift" (a fixed div in the
// bottom-right corner, removed again; its rectangle is reported so the visual
// regression can ignore it). Then the video moment is staged and the page is
// recorded with the CDP screencast for --video-seconds (jpeg, 1280x720 at
// most, or --video-size WxH: the review passes a whole multiple of a pixel-art
// game's internal resolution, e.g. 960x540, so art pixels stay crisp), encoded to video.webm with ffmpeg (when it is on PATH) and six evenly
// spaced frames are kept as video-frame-1.jpg .. 6. Results go to the
// summary's `showcase` list and `video` object (`showcase_error` and
// `video_error` say what failed). The tier and adaptive quality are put back.
//
// Plan mode (used by the geometry check, `cargo run -p evani-geocheck`):
//   node tools/live-check/capture.mjs --game kart-racer --port 8095 --plan plan.json --out <dir>
// plan.json is { "tier": "high" | null, "steps": [{ "name", "cmd", "params",
// "save": "file.png", "note", "timeout_ms" }] }. Adaptive quality goes off
// and the tier is set, then each step's live command runs in order (a failed
// step is recorded, the rest still run); a reply's png_base64 is saved under
// the step's `save` name. An optional "size": "1920x1080" sets the page size.
// Writes plan-result.json ({ steps: [{ name, ok,
// file, error, result }] }, base64 stripped), then puts the tier and adaptive
// quality back. The debug overlay is hidden as in the review modes.
//
// Safety: the page's pointer lock, fullscreen and keyboard lock are stubbed;
// the real mouse and keyboard are never used. Chrome runs at the display's
// normal frame cap. It only ever kills the processes it started itself.

import { spawn, spawnSync } from 'node:child_process';
import { copyFileSync, existsSync, mkdirSync, mkdtempSync, readFileSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, resolve } from 'node:path';
import { fileURLToPath } from 'node:url';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i];
}
const ROOT = resolve(fileURLToPath(new URL('../..', import.meta.url)));
const OUT = resolve(ROOT, args.out ?? '.vibedeck/screenshots/stage6');
// `--app-dir <folder>`: a game outside the engine folder (relative to the
// current folder). The bridge resolves and guards it; the id is its name.
const APP_DIR = args['app-dir'] ? resolve(process.cwd(), args['app-dir']) : null;
if (APP_DIR && args.game) {
  console.error("[live-check] --game and --app-dir: give one (--app-dir is the game's folder)");
  process.exit(2);
}
const gameOf = (dir) => {
  let text = '';
  try {
    text = readFileSync(join(dir, 'evani.toml'), 'utf8');
  } catch (e) {
    console.error(`[live-check] --app-dir ${dir}: no evani.toml (${e.message}); give the game's app folder`);
    process.exit(2);
  }
  const m = /^\s*name\s*=\s*"([a-z0-9-]+)"/m.exec(text);
  if (!m) {
    console.error(`[live-check] --app-dir ${dir}: evani.toml has no name = "<game id>"`);
    process.exit(2);
  }
  return m[1];
};
const GAME = APP_DIR ? gameOf(APP_DIR) : (args.game ?? 'sandbox');
const TIMES = args.times ? args.times.split(',').filter((t) => t !== '').map(Number) : null;
const WIDTH = Number(args.width ?? 480);
const HUD = args.hud ? args.hud.split(',').map((x) => x.trim()).filter(Boolean) : [];
// `--canvas-hud 1`: the kit draws its HUD inside the canvas (evani-render's
// `RenderWorld::hud_layers`); each review capture's `canvas_hud` (the HUD
// sprites of the very frame captured) is added to the HUD probe as `canvas`.
const CANVAS_HUD = args['canvas-hud'] === '1';
const GEO_WIDTH = 480;
// Bench-path times for the world-edge views (the orbit repeats every 60 s).
const GEO_PATH_TIMES = [0, 7.5, 15, 22.5, 30, 37.5, 45, 52.5];
const [VIEW_W, VIEW_H] = (args.viewport ?? '1280x720').split('x').map(Number);
if (!(VIEW_W >= 64 && VIEW_H >= 64 && VIEW_W <= 4096 && VIEW_H <= 4096)) {
  console.error('[live-check] --viewport must be WxH, 64-4096 each');
  process.exit(2);
}
// The engine commit the captures were taken from (labels every capture).
const COMMIT = (() => {
  const r = spawnSync('git', ['rev-parse', '--short', 'HEAD'], { cwd: resolve(fileURLToPath(new URL('../..', import.meta.url))), encoding: 'utf8' });
  const dirty = spawnSync('git', ['status', '--porcelain', '--untracked-files=no'], { cwd: resolve(fileURLToPath(new URL('../..', import.meta.url))), encoding: 'utf8' });
  const h = (r.stdout ?? '').trim();
  return h ? h + ((dirty.stdout ?? '').trim() ? '+dirty' : '') : '';
})();
// "drift@high,item_hit@low,finish" -> [{ name, tier }]
const MOMENTS = args.moments
  ? args.moments.split(',').filter((m) => m !== '').map((m) => {
      const [name, tier] = m.split('@');
      return { name, tier: tier || null };
    })
  : null;
const list = (v) => (v ? v.split(',').map((x) => x.trim()).filter(Boolean) : []);
const SHOWCASE_TIERS = list(args.showcase);
const SHOWCASE_MOMENTS = list(args['showcase-moments']);
const [SHOW_W, SHOW_H] = (args['showcase-size'] ?? '1920x1080').split('x').map(Number);
const ENGINE = args.engine ?? 'unknown';
const VIDEO_MOMENT = args.video ?? SHOWCASE_MOMENTS[0] ?? null;
const VIDEO_SECONDS = Number(args['video-seconds'] ?? 10);
const [VIDEO_W, VIDEO_H] = (args['video-size'] ?? '1280x720').split('x').map(Number);
if (!(VIDEO_W >= 64 && VIDEO_H >= 64 && VIDEO_W <= 4096 && VIDEO_H <= 4096)) {
  console.error('[live-check] --video-size must be WxH, 64-4096 each');
  process.exit(2);
}
const PLAN = args.plan ? JSON.parse(readFileSync(resolve(args.plan), 'utf8')) : null;
const log =(...m) => console.error('[live-check]', ...m);
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const norm = (p) => resolve(p).replace(/\\/g, '/').replace(/\/$/, '').toLowerCase();

function findChrome() {
  const candidates = [args.chrome, process.env.EVANI_CHROME];
  const pf = [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA];
  for (const base of pf) {
    if (base) candidates.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  }
  for (const base of pf) {
    if (base) candidates.push(join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  }
  const pw = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (existsSync(pw)) {
    const dirs = readdirSync(pw).filter((d) => d.startsWith('chromium_headless_shell-')).sort().reverse();
    for (const d of dirs) candidates.push(join(pw, d, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe'));
  }
  return candidates.find((c) => c && existsSync(c));
}

/** Minimal CDP client over one WebSocket (same as tools/bench-runner). */
class Cdp {
  constructor(ws) {
    this.ws = ws;
    this.id = 0;
    this.pending = new Map();
    this.handlers = [];
    ws.addEventListener('message', (ev) => {
      const msg = JSON.parse(ev.data);
      if (msg.id !== undefined) {
        const p = this.pending.get(msg.id);
        if (p) {
          this.pending.delete(msg.id);
          if (msg.error) p.reject(new Error(`${p.method}: ${msg.error.message}`));
          else p.resolve(msg.result);
        }
      } else {
        for (const h of this.handlers) h(msg);
      }
    });
    ws.addEventListener('close', () => {
      for (const p of this.pending.values()) p.reject(new Error('browser connection closed'));
      this.pending.clear();
    });
  }
  static async connect(url) {
    const ws = new WebSocket(url);
    await new Promise((res, rej) => {
      ws.addEventListener('open', res, { once: true });
      ws.addEventListener('error', () => rej(new Error('cannot connect to ' + url)), { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}, sessionId, timeoutMs = 60000) {
    const id = ++this.id;
    return new Promise((res, rej) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        rej(new Error(`${method}: no answer from the browser in ${Math.round(timeoutMs / 1000)} s`));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (v) => { clearTimeout(timer); res(v); },
        reject: (e) => { clearTimeout(timer); rej(e); },
        method,
      });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  on(handler) {
    this.handlers.push(handler);
  }
}

// The page must never grab the real pointer or keyboard in a test run.
const STUBS = `(() => {
  const noop = () => Promise.resolve();
  try { Element.prototype.requestPointerLock = noop; } catch (e) {}
  try { Element.prototype.requestFullscreen = noop; } catch (e) {}
  try { Document.prototype.exitPointerLock = function () {}; } catch (e) {}
  try { if (typeof Keyboard !== 'undefined') Keyboard.prototype.lock = noop; } catch (e) {}
  try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
})();`;

async function status(port) {
  try {
    const r = await fetch(`http://127.0.0.1:${port}/live/status`, { signal: AbortSignal.timeout(2000) });
    return await r.json();
  } catch {
    return null;
  }
}

/** Free (null), this project's bridge ('ours') or something else ('other'). */
async function portState(port) {
  const s = await status(port);
  // The same folder too: an --app-dir bridge only for that folder, an apps/ one only without.
  const sameDir = APP_DIR ? Boolean(s?.app_dir) && norm(s.app_dir) === norm(APP_DIR) : !s?.app_dir;
  if (s && s.type === 'live_hello' && s.root && norm(s.root) === norm(ROOT) && s.game === GAME && sameDir) return 'ours';
  if (s) return 'other';
  // Not answering HTTP: free only if nothing listens.
  const r = spawnSync('netstat', ['-ano', '-p', 'TCP'], { encoding: 'utf8' });
  const busy = (r.stdout ?? '').split(/\r?\n/).some((l) => /LISTENING/.test(l) && new RegExp(`:${port}\\s`).test(l));
  return busy ? 'other' : null;
}

// The debug overlay, the sandbox's debug panel and the live build's model
// drop bar (crates/evani-live/src/convert_ui.rs) never show in a review run.
const HIDE_OVERLAY = `(() => {
  const add = () => {
    const st = document.createElement('style');
    st.id = 'lc-hide-debug';
    st.textContent = '#evani-overlay, #evani-panel, #evani-convert-zone, #evani-convert-report { display: none !important; }';
    (document.head || document.documentElement).appendChild(st);
  };
  if (document.documentElement) add();
  else document.addEventListener('DOMContentLoaded', add, { once: true });
})();`;

// Page screenshots of a staged moment must not depend on how long they took:
// the moment is shot with `capture.frame { hold_after: true }` (the sim clock
// stays held on the captured frame after the reply), and the page's CSS
// animations and transitions, which run on the wall clock, are settled first:
// finite ones jump to their end, endless ones (a pulsing banner) are stopped at
// their start and played again after the shot (UNSETTLE). Then two frames are
// drawn so the canvas shows the held frame.
const SETTLE = `(async () => {
  const stopped = [];
  for (const a of document.getAnimations()) {
    try {
      const end = a.effect?.getComputedTiming?.().endTime;
      if (end === Infinity) { a.cancel(); stopped.push(a); } else { a.finish(); }
    } catch {}
  }
  window.__evaniSettled = stopped;
  await new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r)));
  return stopped.length;
})()`;
const UNSETTLE = `(() => {
  for (const a of window.__evaniSettled ?? []) { try { a.play(); } catch {} }
  window.__evaniSettled = [];
})()`;

// Phone page shots (landscape).
const PHONE_W = 844;
const PHONE_H = 390;
const PHONE_DPR = Number(args['phone-dpr'] ?? 1);
if (!(PHONE_DPR >= 1 && PHONE_DPR <= 4)) {
  console.error('[live-check] --phone-dpr must be a number from 1 to 4');
  process.exit(2);
}
/** Smallest visible text (px) and shortest visible button (px) in the game's UI at the phone size. */
const PHONE_PROBE = `(() => {
  const ui = document.getElementById('ui');
  if (!ui) return {};
  let minFont = Infinity, minFontEl = '', minButton = Infinity;
  const seen = (el) => {
    for (let n = el; n && n !== document.body; n = n.parentElement) {
      const cs = getComputedStyle(n);
      if (cs.display === 'none' || cs.visibility === 'hidden' || parseFloat(cs.opacity) <= 0.05) return false;
    }
    const r = el.getBoundingClientRect();
    return r.width > 0 && r.height > 0;
  };
  for (const el of ui.querySelectorAll('*')) {
    if (!seen(el)) continue;
    const own = [...el.childNodes].some((n) => n.nodeType === 3 && n.textContent.trim());
    if (own) {
      const f = parseFloat(getComputedStyle(el).fontSize);
      if (f < minFont) { minFont = f; minFontEl = (el.className || el.tagName) + ': ' + el.textContent.trim().slice(0, 20); }
    }
    if (el.tagName === 'BUTTON') minButton = Math.min(minButton, el.getBoundingClientRect().height);
  }
  return {
    min_font_px: Number.isFinite(minFont) ? minFont : null,
    min_font_el: minFontEl,
    min_button_px: Number.isFinite(minButton) ? minButton : null,
    width: innerWidth, height: innerHeight,
  };
})()`;

/** The DOM probe: is the debug overlay shown, and is each HUD selector visible and filled in? */
function domProbeExpr(selectors) {
  return `(() => {
    const sels = ${JSON.stringify(selectors)};
    const shown = (el) => {
      for (let n = el; n; n = n.parentElement) {
        const cs = getComputedStyle(n);
        if (cs.display === 'none' || cs.visibility === 'hidden' || parseFloat(cs.opacity) <= 0.05) return false;
      }
      const r = el.getBoundingClientRect();
      return r.width >= 4 && r.height >= 4 && r.right > 0 && r.bottom > 0 && r.left < innerWidth && r.top < innerHeight;
    };
    const text = (el) => (el.innerText || '').trim().length;
    const filled = (el) => text(el) > 0 || !!el.querySelector('svg, img, canvas');
    const dbg = ['evani-overlay', 'evani-panel'].map((id) => document.getElementById(id)).filter(Boolean);
    return {
      overlay: { present: dbg.length > 0, visible: dbg.some(shown) },
      hud: sels.map((selector) => {
        let els = [];
        try { els = [...document.querySelectorAll(selector)]; } catch (e) {}
        return {
          selector,
          matched: els.length,
          visible: els.some((el) => shown(el) && filled(el)),
          text_len: els.reduce((m, el) => Math.max(m, text(el)), 0),
        };
      }),
    };
  })()`;
}

/**
 * The session file of a bridge this script starts on `port`: its own, never
 * the Evani.dev tab's `.evani/live/session.json`, which a review bridge must
 * not overwrite or delete.
 */
const ownSession = (port) => join(ROOT, '.evani', 'live', 'review', String(port), 'session.json');

/** The token of the bridge on `port`: from its own session file, else the tab's if it is that port's. */
function sessionToken(port) {
  for (const f of [ownSession(port), join(ROOT, '.evani', 'live', 'session.json')]) {
    try {
      const s = JSON.parse(readFileSync(f, 'utf8'));
      if (s.http === `http://127.0.0.1:${port}` && s.token) return s.token;
    } catch {
      // not there
    }
  }
  throw new Error(`no session file for the bridge on ${port}`);
}

let cmdN = 0;
async function cmd(port, token, name, params, timeoutMs = 120000, note = undefined, opts = undefined) {
  const body = { type: 'cmd', v: 1, id: `lc-${++cmdN}`, cmd: name, params, ...(note ? { note } : {}), ...(opts ? { opts } : {}) };
  const r = await fetch(`http://127.0.0.1:${port}/live/v1/cmd`, {
    method: 'POST',
    headers: { 'content-type': 'application/json', authorization: `Bearer ${token}` },
    body: JSON.stringify(body),
    signal: AbortSignal.timeout(timeoutMs),
  });
  const v = await r.json();
  if (!v.ok) throw new Error(`${name}: ${v.error?.code}: ${v.error?.message}`);
  return v.result;
}

function savePng(name, b64) {
  const file = join(OUT, name);
  writeFileSync(file, Buffer.from(b64, 'base64'));
  return file;
}

async function main() {
  let port = Number(args.port ?? 8096);
  const started = [];
  let chrome = null;
  let chromeExited = true;
  let cdp = null;
  let profile = null;
  try {
    // 1. The bridge: reuse this project's, else start one on a free port.
    let st = await portState(port);
    while (st === 'other') {
      log(`port ${port} is used by something else; trying ${port + 1}`);
      port += 1;
      st = await portState(port);
    }
    if (st === 'ours') {
      log(`reusing this project's bridge on ${port}`);
    } else {
      const jobs = process.env.CARGO_BUILD_JOBS || '4';
      log(`building the bridge (cargo build -j ${jobs})...`);
      // Cargo builds where CARGO_TARGET_DIR points (relative to the engine root), else target/.
      const targetDir = process.env.CARGO_TARGET_DIR ? resolve(ROOT, process.env.CARGO_TARGET_DIR) : join(ROOT, 'target');
      let built = join(targetDir, 'debug', process.platform === 'win32' ? 'evani-live-bridge.exe' : 'evani-live-bridge');
      const b = spawnSync('cargo', ['build', '-j', jobs, '-p', 'evani-live-bridge'], { cwd: ROOT, stdio: ['ignore', 'ignore', 'pipe'], encoding: 'utf8' });
      if (b.stderr) process.stderr.write(b.stderr);
      if (b.status !== 0) {
        // On Windows a running bridge locks its exe, so cargo can't replace
        // it. Bridges started here run from a copy (below) and never lock
        // it; one started some other way still can: then use the exe as built.
        const locked = /os error (5|32)\b|Access is denied|being used by another process/i.test(b.stderr ?? '');
        if (!(locked && existsSync(built))) throw new Error('the bridge did not build');
        // Build it in a target dir of its own instead (the first time takes a
        // few minutes), so it is this source's bridge, not a stale one.
        log('the bridge exe is in use by a running copy; building a fresh one in target/bridge-alt');
        const alt = join(ROOT, 'target', 'bridge-alt');
        const b2 = spawnSync('cargo', ['build', '-j', jobs, '-p', 'evani-live-bridge', '--target-dir', alt], { cwd: ROOT, stdio: ['ignore', 'ignore', 'pipe'], encoding: 'utf8' });
        if (b2.stderr) process.stderr.write(b2.stderr);
        const altExe = join(alt, 'debug', process.platform === 'win32' ? 'evani-live-bridge.exe' : 'evani-live-bridge');
        if (b2.status === 0 && existsSync(altExe)) {
          built = altExe;
        } else {
          log('that did not build either; using the exe as last built');
        }
      }
      // Run from a per-port copy so the next build can always replace the exe.
      const runs = join(ROOT, 'target', 'debug', 'bridge-runs');
      mkdirSync(runs, { recursive: true });
      const exe = join(runs, process.platform === 'win32' ? `evani-live-bridge-${port}.exe` : `evani-live-bridge-${port}`);
      copyFileSync(built, exe);
      log(`starting the bridge on ${port} (it builds ${GAME} with live if needed)...`);
      // Trunk reads NO_COLOR as a boolean flag and refuses "1". (The bridge now strips it
      // itself when it spawns trunk; this stays as a harmless belt-and-braces.)
      const env = { ...process.env, CARGO_BUILD_JOBS: jobs };
      delete env.NO_COLOR;
      const which = APP_DIR ? ['--app-dir', APP_DIR] : ['--game', GAME];
      const bridge = spawn(exe, ['serve', '--root', ROOT, '--port', String(port), ...which, '--session', ownSession(port), '--quiet'], {
        cwd: ROOT,
        stdio: ['ignore', 'ignore', 'inherit'],
        windowsHide: true,
        env,
      });
      started.push(bridge);
    }
    // Wait for the game build.
    const t0 = Date.now();
    let lastPhase = '';
    for (;;) {
      const s = await status(port);
      if (s && s.phase !== lastPhase) {
        lastPhase = s.phase;
        log(`bridge phase: ${s.phase}`);
      }
      if (s && s.phase === 'idle') break;
      if (s && s.phase === 'error') throw new Error('the game did not build:\n' + s.error);
      if (Date.now() - t0 > 20 * 60000) throw new Error('the game build took over 20 minutes');
      await sleep(1000);
    }
    const token = sessionToken(port);

    // 2. Headless Chrome with the live page.
    const chromePath = findChrome();
    if (!chromePath) throw new Error('no Chrome found (set EVANI_CHROME or pass --chrome)');
    const isShell = /headless_shell|chrome-headless-shell/.test(chromePath);
    profile = mkdtempSync(join(tmpdir(), 'evani-live-check-'));
    const flags = [
      `--user-data-dir=${profile}`,
      '--remote-debugging-port=0',
      '--no-first-run',
      '--no-default-browser-check',
      '--mute-audio',
      '--enable-unsafe-webgpu',
      '--enable-webgpu-developer-features',
      '--disable-background-timer-throttling',
      '--disable-renderer-backgrounding',
      '--disable-backgrounding-occluded-windows',
      `--window-size=${VIEW_W},${VIEW_H}`,
      'about:blank',
    ];
    if (!isShell) flags.unshift('--headless=new');
    log('chrome:', chromePath);
    chrome = spawn(chromePath, flags, { stdio: 'ignore', windowsHide: true });
    chromeExited = false;
    chrome.on('exit', () => (chromeExited = true));
    let wsUrl = null;
    for (let i = 0; i < 200 && !wsUrl && !chromeExited; i++) {
      const f = join(profile, 'DevToolsActivePort');
      if (existsSync(f)) {
        const [p, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
        if (p && path) wsUrl = `ws://127.0.0.1:${p}${path}`;
      }
      if (!wsUrl) await sleep(100);
    }
    if (!wsUrl) throw new Error('Chrome did not open its debugging port');
    cdp = await Cdp.connect(wsUrl);
    const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
    const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
    const s = (method, params, ms) => cdp.send(method, params, sessionId, ms);
    const consoleLines = [];
    // Screencast frames while the showcase video records (null otherwise).
    let castFrames = null;
    cdp.on((m) => {
      if (m.sessionId !== sessionId) return;
      if (m.method === 'Page.screencastFrame') {
        if (castFrames) castFrames.push({ data: m.params.data, t: m.params.metadata?.timestamp ?? Date.now() / 1000 });
        cdp.send('Page.screencastFrameAck', { sessionId: m.params.sessionId }, sessionId).catch(() => {});
        return;
      }
      if (m.method === 'Runtime.exceptionThrown' && consoleLines.length < 20) {
        consoleLines.push('exception: ' + (m.params.exceptionDetails?.exception?.description ?? m.params.exceptionDetails?.text));
      }
      if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error' && consoleLines.length < 20) {
        consoleLines.push('console.error: ' + m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
      }
    });
    await s('Page.enable');
    await s('Runtime.enable');
    // A plan may ask for a bigger page ("size": "1920x1080") so its wide
    // capture.frame shots are not capped by the canvas; otherwise --viewport.
    const [PLAN_W, PLAN_H] = PLAN?.size ? String(PLAN.size).split('x').map(Number) : [0, 0];
    const PAGE_W = PLAN_W || VIEW_W;
    const PAGE_H = PLAN_H || VIEW_H;
    await s('Emulation.setDeviceMetricsOverride', { width: PAGE_W, height: PAGE_H, deviceScaleFactor: 1, mobile: false });
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
    const reviewing = Boolean(MOMENTS || TIMES);
    if (reviewing || PLAN) await s('Page.addScriptToEvaluateOnNewDocument', { source: HIDE_OVERLAY });
    const url = `http://127.0.0.1:${port}/?live=1`;
    log('page:', url);
    await s('Page.navigate', { url });

    // 3. Wait for the page to join the bridge, then let the scene settle.
    const t1 = Date.now();
    for (;;) {
      const st2 = await status(port);
      if (st2?.engine_connected) break;
      if (Date.now() - t1 > 60000) throw new Error('the page did not connect to the bridge in 60 s\n' + consoleLines.join('\n'));
      await sleep(500);
    }
    log('page connected');
    await sleep(3000);

    mkdirSync(OUT, { recursive: true });

    // Process-check probes (see the header).
    const probe = { entities: [], params: [], captures: [], geo: [], errors: [] };
    const entityCount = async (at) => {
      const st = await cmd(port, token, 'session.status', {});
      probe.entities.push({ at, count: Number.isFinite(st.entities) ? st.entities : null });
    };
    // The canvas HUD of one capture.frame reply as a HUD probe entry: shown
    // when at least one HUD sprite was drawn on the canvas, over at least
    // 4 x 4 CSS px; `text_len` is the number of HUD sprites.
    const canvasHudProbe = (cap) => {
      const h = cap?.canvas_hud;
      const [, , w, ht] = h?.rect_css ?? [0, 0, 0, 0];
      return { selector: 'canvas', matched: h ? 1 : 0, visible: !!h && h.sprites > 0 && w >= 4 && ht >= 4, text_len: h?.sprites ?? 0, rect_css: h?.rect_css ?? null };
    };
    // A page screenshot with the CSS animations settled (see SETTLE).
    const stillShot = async (opts) => {
      await s('Runtime.evaluate', { expression: SETTLE, awaitPromise: true, returnByValue: true });
      try {
        return await s('Page.captureScreenshot', opts);
      } finally {
        await s('Runtime.evaluate', { expression: UNSETTLE }).catch(() => {});
      }
    };
    const pageProbe = async (name, moment = null, cap = null) => {
      // A moment's page shots show the held moment (`hold_after` on its captures).
      const pageShot = (opts) => (moment ? stillShot(opts) : s('Page.captureScreenshot', opts));
      let dom = {};
      try {
        const r = await s('Runtime.evaluate', { expression: domProbeExpr(HUD), returnByValue: true });
        dom = r.result?.value ?? {};
      } catch (e) {
        probe.errors.push(`dom probe ${name}: ${e.message}`);
      }
      if (CANVAS_HUD) dom.hud = [...(dom.hud ?? []), canvasHudProbe(cap)];
      let page_png = null;
      try {
        const shot = await pageShot({ format: 'png', clip: { x: 0, y: 0, width: 1280, height: 720, scale: 0.5 } });
        page_png = `page-${name}.png`;
        savePng(page_png, shot.data);
      } catch (e) {
        probe.errors.push(`page screenshot ${name}: ${e.message}`);
      }
      // The same page at a phone's size (844x390, landscape): the HUD must
      // stay readable there (text 12 px and up, buttons 44 px tall and up).
      let phone = {};
      try {
        // As a touch screen (`pointer: coarse` matches), so a kit's touch
        // controls show as they would on the phone.
        await s('Emulation.setTouchEmulationEnabled', { enabled: true, maxTouchPoints: 5 });
        await s('Emulation.setDeviceMetricsOverride', { width: PHONE_W, height: PHONE_H, deviceScaleFactor: PHONE_DPR, mobile: true });
        // The engine shows its touch controls when the page starts on a touch
        // screen or at the first touch; this page started on a desktop, so it
        // gets a first touch, at the top edge where no control is (the moment
        // is staged again below, so nothing the tap did stays in the picture).
        for (const type of ['touchStart', 'touchEnd']) {
          await s('Input.dispatchTouchEvent', { type, touchPoints: type === 'touchStart' ? [{ x: PHONE_W / 2, y: 2 }] : [] });
        }
        await sleep(500);
        // The game ran on while the page resized: stage the moment again
        // (it is the same every time) so the phone shows it, not what the
        // race did next.
        if (moment) await cmd(port, token, 'capture.frame', { width: 64, moment, hold_after: true }, 60000, `Phone shot of ${moment}`);
        const r = await s('Runtime.evaluate', { expression: PHONE_PROBE, returnByValue: true });
        phone = r.result?.value ?? {};
        const shot = await pageShot({ format: 'png', clip: { x: 0, y: 0, width: PHONE_W, height: PHONE_H, scale: 1 } });
        phone.png = `page-${name}-phone.png`;
        savePng(phone.png, shot.data);
      } catch (e) {
        probe.errors.push(`phone page screenshot ${name}: ${e.message}`);
      }
      // And held upright (390x844, portrait), for the critic's phone check.
      let portrait = {};
      try {
        await s('Emulation.setDeviceMetricsOverride', { width: PHONE_H, height: PHONE_W, deviceScaleFactor: PHONE_DPR, mobile: true });
        await sleep(500);
        // Stage the moment again here too, as for the phone shot: the
        // particles of the moment (sparkles, confetti) were emitted for the
        // layout of the screen it was staged on and stay where they were
        // emitted, so a shot taken after the resize would show them away
        // from their pieces (critic round 3: sparkles at a portrait
        // screen's edge).
        if (moment) await cmd(port, token, 'capture.frame', { width: 64, moment, hold_after: true }, 60000, `Portrait shot of ${moment}`);
        const r = await s('Runtime.evaluate', { expression: PHONE_PROBE, returnByValue: true });
        portrait = r.result?.value ?? {};
        const shot = await pageShot({ format: 'png', clip: { x: 0, y: 0, width: PHONE_H, height: PHONE_W, scale: 1 } });
        portrait.png = `page-${name}-portrait.png`;
        savePng(portrait.png, shot.data);
      } catch (e) {
        probe.errors.push(`portrait page screenshot ${name}: ${e.message}`);
      } finally {
        await s('Emulation.setTouchEmulationEnabled', { enabled: false }).catch(() => {});
        // A key hides the touch controls the first touch showed (the engine's
        // touch-only rule), so the desktop shots after these have none.
        for (const type of ['keyDown', 'keyUp']) {
          await s('Input.dispatchKeyEvent', { type, key: 'Shift', code: 'ShiftLeft', windowsVirtualKeyCode: 16 }).catch(() => {});
        }
        await s('Emulation.setDeviceMetricsOverride', { width: PAGE_W, height: PAGE_H, deviceScaleFactor: 1, mobile: false }).catch(() => {});
        await sleep(300);
      }
      probe.captures.push({ name, page_png, phone, portrait, ...dom });
    };
    const readParams = async () => {
      try {
        const sc = await cmd(port, token, 'schema.get', { include: ['params'] });
        probe.params = (sc.params ?? []).map((p) => ({ name: p.name, owner: p.owner }));
      } catch (e) {
        probe.errors.push(`schema.get: ${e.message}`);
      }
    };
    // World-edge views: the same camera with a black and then a white clear colour.
    const CLEAR = 'render.clear_color';
    let clear0 = null;
    const setClear = (rgb, why) =>
      cmd(port, token, 'param.set', { values: { [CLEAR]: rgb } }, 30000, `Review geocheck: ${why}`, { telemetry_ms: 0 });
    const geoShot = async (name, side, camParams) => {
      const c = await cmd(port, token, 'capture.frame', { width: GEO_WIDTH, ...camParams });
      savePng(`geo-${name}-${side}.png`, c.png_base64);
      return c.camera ?? null;
    };
    const geoViews = async (views) => {
      // views: [{ name, params }]; all black, then all white.
      try {
        const cams = {};
        await setClear([0, 0, 0], 'black clear colour');
        for (const v of views) cams[v.name] = await geoShot(v.name, 'black', v.params);
        await setClear([1, 1, 1], 'white clear colour');
        for (const v of views) await geoShot(v.name, 'white', v.params);
        for (const v of views) {
          probe.geo.push({ name: v.name, black: `geo-${v.name}-black.png`, white: `geo-${v.name}-white.png`, camera: cams[v.name] });
        }
      } catch (e) {
        probe.errors.push(`geocheck ${views.map((v) => v.name).join(',')}: ${e.message}`);
        log(`geocheck: ${e.message}`);
      } finally {
        // The next card capture must not be drawn on a white or black sky.
        await restoreClear();
      }
    };
    const pathViews = () => GEO_PATH_TIMES.map((t) => ({ name: `path_${String(t).replace('.', '_')}s`, params: { sim_time: t, camera: 'path' } }));
    const restoreClear = async () => {
      if (!clear0) return;
      await setClear(clear0, 'put the clear colour back').catch((e) => log(`clear colour back: ${e.message}`));
    };
    // The showcase phase (see the header): labelled full-page shots per tier
    // and moment, then the run video.
    const runShowcase = async () => {
      const out = { showcase: [], video: null };
      if (!SHOWCASE_TIERS.length || !SHOWCASE_MOMENTS.length) return out;
      const errors = [];
      const st0 = await cmd(port, token, 'session.status', {});
      const tier0 = st0.tier;
      const aq0 = (await cmd(port, token, 'param.get', { names: ['aq.enabled'] }).catch(() => null))?.values?.['aq.enabled'];
      await cmd(port, token, 'param.set', { values: { 'aq.enabled': false } }, 30000, 'Showcase: hold the tier steady').catch((e) => errors.push(`aq.enabled: ${e.message}`));
      await s('Emulation.setDeviceMetricsOverride', { width: SHOW_W, height: SHOW_H, deviceScaleFactor: 1, mobile: false });
      await sleep(1500);
      const cap = (t) => t.charAt(0).toUpperCase() + t.slice(1);
      const engine = ENGINE.slice(0, 7);
      try {
        for (const tier of SHOWCASE_TIERS) {
          try {
            await cmd(port, token, 'tier.set', { tier }, 60000, `Showcase on ${tier}`);
            await sleep(1500);
          } catch (e) {
            errors.push(`tier ${tier}: ${e.message}`);
            continue;
          }
          for (const moment of SHOWCASE_MOMENTS) {
            try {
              // Held after the reply: the page shot shows the staged frame.
              await cmd(port, token, 'capture.frame', { width: 64, moment, hold_after: true });
              const label = `${cap(tier)} · engine ${engine} · ${moment}`;
              const r = await s('Runtime.evaluate', {
                returnByValue: true,
                expression: `(() => {
                  document.getElementById('evani-review-label')?.remove();
                  const d = document.createElement('div');
                  d.id = 'evani-review-label';
                  d.textContent = ${JSON.stringify(label)};
                  d.style.cssText = 'position:fixed;right:12px;bottom:12px;z-index:2147483647;padding:4px 10px;font:600 18px/1.2 system-ui,sans-serif;color:#fff;background:rgba(0,0,0,0.65);border-radius:4px;pointer-events:none';
                  document.body.appendChild(d);
                  const b = d.getBoundingClientRect();
                  return [b.x, b.y, b.width, b.height];
                })()`,
              });
              const shot = await stillShot({ format: 'png', clip: { x: 0, y: 0, width: SHOW_W, height: SHOW_H, scale: 1 } });
              await s('Runtime.evaluate', { expression: `document.getElementById('evani-review-label')?.remove()` });
              const file = savePng(`shot-${moment}-${tier}.png`, shot.data);
              const rect = (r.result?.value ?? []).map((x) => Math.round(x));
              // Pad the label rectangle a little for anti-aliasing.
              const label_rect = rect.length === 4 ? [rect[0] - 4, rect[1] - 4, rect[2] + 8, rect[3] + 8] : null;
              out.showcase.push({ moment, tier, file, width: SHOW_W, height: SHOW_H, label, label_rect });
              log(`showcase ${moment} on ${tier} -> ${file}`);
            } catch (e) {
              errors.push(`${moment} on ${tier}: ${e.message}`);
            }
          }
        }
        if (VIDEO_MOMENT) {
          try {
            await cmd(port, token, 'capture.frame', { width: 64, moment: VIDEO_MOMENT });
            castFrames = [];
            const v0 = Date.now();
            await s('Page.startScreencast', { format: 'jpeg', quality: 80, maxWidth: VIDEO_W, maxHeight: VIDEO_H, everyNthFrame: 1 });
            await sleep(VIDEO_SECONDS * 1000);
            await s('Page.stopScreencast');
            const seconds = (Date.now() - v0) / 1000;
            const frames = castFrames;
            castFrames = null;
            if (!frames.length) throw new Error('the screencast sent no frames');
            const dir = join(OUT, 'video-frames');
            rmSync(dir, { recursive: true, force: true });
            mkdirSync(dir, { recursive: true });
            frames.forEach((f, k) => writeFileSync(join(dir, `f${String(k).padStart(5, '0')}.jpg`), Buffer.from(f.data, 'base64')));
            const keep = [];
            for (let k = 0; k < 6; k++) {
              const i = Math.min(frames.length - 1, Math.round((k * (frames.length - 1)) / 5));
              const name = `video-frame-${k + 1}.jpg`;
              writeFileSync(join(OUT, name), Buffer.from(frames[i].data, 'base64'));
              keep.push(name);
            }
            const fps = Math.max(1, Math.round(frames.length / seconds));
            const webm = join(OUT, 'video.webm');
            const ff = spawnSync('ffmpeg', ['-y', '-loglevel', 'error', '-framerate', String(fps), '-i', join(dir, 'f%05d.jpg'), '-c:v', 'libvpx-vp9', '-b:v', '1500k', '-pix_fmt', 'yuv420p', webm], { stdio: ['ignore', 'ignore', 'inherit'] });
            const ok = ff.status === 0 && existsSync(webm);
            if (!ok) out.video_error = `ffmpeg did not make video.webm (${ff.error?.message ?? `exit ${ff.status}`})`;
            rmSync(dir, { recursive: true, force: true });
            out.video = { moment: VIDEO_MOMENT, file: ok ? 'video.webm' : null, seconds, frame_count: frames.length, fps, frames: keep };
            log(`video ${VIDEO_MOMENT}: ${frames.length} frames over ${seconds.toFixed(1)} s, webm ${ok}`);
          } catch (e) {
            castFrames = null;
            out.video_error = `video ${VIDEO_MOMENT}: ${e.message}`;
            await s('Page.stopScreencast').catch(() => {});
          }
        }
      } finally {
        await s('Emulation.setDeviceMetricsOverride', { width: PAGE_W, height: PAGE_H, deviceScaleFactor: 1, mobile: false }).catch(() => {});
        if (tier0) await cmd(port, token, 'tier.set', { tier: tier0 }, 60000, 'Back to the tier before the showcase').catch((e) => log(`tier back: ${e.message}`));
        if (aq0 !== undefined && aq0 !== false) await cmd(port, token, 'param.set', { values: { 'aq.enabled': aq0 } }, 30000, 'Adaptive quality back on after the showcase').catch(() => {});
      }
      if (errors.length) out.showcase_error = errors.join('; ');
      return out;
    };

    if (reviewing) {
      await readParams();
      clear0 = (await cmd(port, token, 'param.get', { names: [CLEAR] }).catch(() => null))?.values?.[CLEAR] ?? null;
      if (!clear0) probe.errors.push(`param.get ${CLEAR}: no value`);
    }

    if (PLAN) {
      // Plan mode (see the header): run the plan's commands in order, adaptive
      // quality off and the plan's tier on, then put both back.
      const st0 = await cmd(port, token, 'session.status', {});
      const tier0 = st0.tier;
      const aq0 = (await cmd(port, token, 'param.get', { names: ['aq.enabled'] }).catch(() => null))?.values?.['aq.enabled'];
      const results = [];
      try {
        await cmd(port, token, 'param.set', { values: { 'aq.enabled': false } }, 30000, 'Geometry check: hold the tier steady (adaptive quality off)').catch((e) => log(`aq.enabled: ${e.message}`));
        if (PLAN.tier) {
          await cmd(port, token, 'tier.set', { tier: PLAN.tier }, 60000, `Geometry check on the ${PLAN.tier} tier`);
          await sleep(1500);
        }
        for (const [i, step] of (PLAN.steps ?? []).entries()) {
          const name = step.name ?? `step-${i + 1}`;
          const r = { name, ok: false };
          const t0 = Date.now();
          try {
            const res = await cmd(port, token, step.cmd, step.params ?? {}, step.timeout_ms ?? 120000, step.note, step.opts);
            if (step.save && res?.png_base64) {
              if (!/^[A-Za-z0-9_.-]+\.png$/.test(step.save)) throw new Error(`bad file name ${step.save}`);
              savePng(step.save, res.png_base64);
              r.file = step.save;
            }
            r.ok = true;
            r.result = res && typeof res === 'object' ? Object.fromEntries(Object.entries(res).filter(([k]) => !k.endsWith('png_base64'))) : res;
          } catch (e) {
            r.error = e.message;
            log(`plan ${name}: ${e.message}`);
          }
          r.ms = Date.now() - t0;
          results.push(r);
          if ((i + 1) % 10 === 0) log(`plan: ${i + 1}/${PLAN.steps.length} steps`);
        }
      } finally {
        if (PLAN.tier && tier0) await cmd(port, token, 'tier.set', { tier: tier0 }, 60000, 'Back to the tier before the geometry check').catch((e) => log(`tier back: ${e.message}`));
        if (aq0 !== undefined && aq0 !== false) await cmd(port, token, 'param.set', { values: { 'aq.enabled': aq0 } }, 30000, 'Adaptive quality back on after the geometry check').catch(() => {});
      }
      const summary = { mode: 'plan', port, game: GAME, tier: PLAN.tier ?? tier0 ?? null, steps: results, console: consoleLines };
      writeFileSync(join(OUT, 'plan-result.json'), JSON.stringify(summary, null, 2));
      log(`plan: ${results.filter((r) => r.ok).length}/${results.length} steps ok -> ${join(OUT, 'plan-result.json')}`);
      try {
        await cdp.send('Target.closeTarget', { targetId });
      } catch {
        // already gone
      }
      return;
    }

    if (MOMENTS) {
      const bad = MOMENTS.find((m) => !/^[a-z0-9_]+$/.test(m.name) || (m.tier && !/^[a-z]+$/.test(m.tier)));
      if (bad) throw new Error(`--moments: bad entry ${bad.name}@${bad.tier}`);
      const st0 = await cmd(port, token, 'session.status', {});
      const tier0 = st0.tier;
      const aq0 = (await cmd(port, token, 'param.get', { names: ['aq.enabled'] }).catch(() => null))?.values?.['aq.enabled'];
      const setAq = (on, note) =>
        cmd(port, token, 'param.set', { values: { 'aq.enabled': on } }, 30000, note).then(() => true, (e) => (log(`aq.enabled: ${e.message}`), false));
      await setAq(false, 'Review captures: hold the tier steady (adaptive quality off)');
      log('adaptive quality off for the captures');
      const captures = [];
      try {
        // The view the game boots into (its title or menu), then the bench camera path.
        await entityCount('boot');
        await geoViews([{ name: 'boot', params: {} }]);
        await geoViews(pathViews());
        for (const m of MOMENTS) {
          if (m.tier) {
            await cmd(port, token, 'tier.set', { tier: m.tier }, 60000, `Review capture ${m.name} on ${m.tier}`);
            // Pipelines and targets rebuild; give the page a moment.
            await sleep(1500);
          }
          // Held after the reply, so the page shots below show this frame.
          const c = await cmd(port, token, 'capture.frame', { width: WIDTH, moment: m.name, hold_after: true });
          const repeats = MOMENTS.filter((o) => o.name === m.name).length > 1;
          const file = savePng(repeats ? `moment-${m.name}-${m.tier ?? 'current'}.png` : `moment-${m.name}.png`, c.png_base64);
          log(`capture.frame moment=${m.name} tier=${c.stats?.tier}: ${c.width}x${c.height} -> ${file}`);
          captures.push({
            file,
            moment: m.name,
            label: c.moment?.label ?? m.name,
            tier: c.stats?.tier ?? m.tier ?? '',
            commit: COMMIT,
            t: c.sim_time,
            width: c.width,
            height: c.height,
            frame: c.frame,
            sim_time: c.sim_time,
            stats: c.stats,
          });
          await entityCount(m.name);
          await pageProbe(m.name, m.name, c);
          await geoViews([{ name: m.name, params: { moment: m.name } }]);
        }
      } finally {
        await restoreClear();
        if (tier0) await cmd(port, token, 'tier.set', { tier: tier0 }, 60000, 'Back to the tier before the review captures').catch((e) => log(`tier back: ${e.message}`));
        if (aq0 !== undefined && aq0 !== false) await setAq(aq0, 'Adaptive quality back on after the review captures');
      }
      const show = await runShowcase();
      const summary = { mode: 'moments', port, game: GAME, commit: COMMIT, viewport: [VIEW_W, VIEW_H], aq_enabled: false, captures, probe, ...show, console: consoleLines };
      writeFileSync(join(OUT, 'captures.json'), JSON.stringify(summary, null, 2));
      process.stdout.write(JSON.stringify(summary, null, 2) + '\n');
      try {
        await cdp.send('Target.closeTarget', { targetId });
      } catch {
        // already gone
      }
      return;
    }
    if (TIMES) {
      // Review mode: fixed times on the bench camera path, no compare.
      if (TIMES.some((t) => !Number.isFinite(t) || t < 0)) throw new Error('--times must be seconds >= 0');
      const captures = [];
      await entityCount('boot');
      for (const [i, t] of TIMES.entries()) {
        const c = await cmd(port, token, 'capture.frame', { width: WIDTH, sim_time: t, camera: 'path' });
        const file = savePng(`capture-${i + 1}.png`, c.png_base64);
        log(`capture.frame t=${t} s: ${c.width}x${c.height} frame ${c.frame} -> ${file}`);
        captures.push({ file, t, tier: c.stats?.tier ?? '', commit: COMMIT, width: c.width, height: c.height, frame: c.frame, sim_time: c.sim_time, stats: c.stats });
        await entityCount(`t${t}`);
        await pageProbe(`t${t}`, null, c);
      }
      try {
        await geoViews(pathViews());
      } finally {
        await restoreClear();
      }
      const show = await runShowcase();
      const summary = { mode: 'review', port, game: GAME, commit: COMMIT, viewport: [VIEW_W, VIEW_H], captures, probe, ...show, console: consoleLines };
      writeFileSync(join(OUT, 'captures.json'), JSON.stringify(summary, null, 2));
      process.stdout.write(JSON.stringify(summary, null, 2) + '\n');
      try {
        await cdp.send('Target.closeTarget', { targetId });
      } catch {
        // already gone
      }
      return;
    }

    // 4. capture.frame, then compare.ab with bloom off as B.
    const cap = await cmd(port, token, 'capture.frame', { width: WIDTH });
    const capFile = savePng('capture.png', cap.png_base64);
    log(`capture.frame: ${cap.width}x${cap.height} frame ${cap.frame} -> ${capFile}`);

    const cmp = await cmd(port, token, 'compare.ab', {
      b: { values: { 'fx.bloom.enabled': false } },
      capture_width: 480,
      order: 'AB',
      seconds: 2,
    });
    const pair = cmp.captures?.[0];
    if (!pair) throw new Error('compare.ab returned no captures');
    const aFile = savePng('compare-a.png', pair.a_png_base64);
    const bFile = savePng('compare-b.png', pair.b_png_base64);
    log(`compare.ab: A -> ${aFile}, B -> ${bFile}`);

    // Restored afterwards? Read the param back.
    const after = await cmd(port, token, 'param.get', { names: ['fx.bloom.enabled'] }).catch((e) => ({ error: e.message }));

    const strip = (side) => side && Object.fromEntries(Object.entries(side).filter(([k]) => !k.endsWith('png_base64')));
    const summary = {
      port,
      capture: { width: cap.width, height: cap.height, frame: cap.frame, sim_time: cap.sim_time, stats: cap.stats },
      compare: {
        run: cmp.run,
        tier: cmp.tier,
        order: cmp.order,
        seconds: cmp.seconds,
        gpu_timing: cmp.gpu_timing,
        a: strip(cmp.a),
        b: strip(cmp.b),
        delta: cmp.delta,
        diff: cmp.diff,
        capture: { t: pair.t, width: pair.width, height: pair.height, diff: pair.diff },
        kept: cmp.kept,
      },
      bloom_after: after,
      console: consoleLines,
      files: [capFile, aFile, bFile],
    };
    writeFileSync(join(OUT, 'summary.json'), JSON.stringify(summary, null, 2));
    process.stdout.write(JSON.stringify(summary, null, 2) + '\n');
    try {
      await cdp.send('Target.closeTarget', { targetId });
    } catch {
      // already gone
    }
  } finally {
    if (cdp) {
      try {
        await cdp.send('Browser.close', {}, undefined, 5000);
      } catch {
        // closing may drop the socket before answering
      }
      cdp.ws.close();
    }
    for (let i = 0; i < 50 && !chromeExited; i++) await sleep(100);
    if (chrome && !chromeExited) spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    // Only the bridge we started (and its build, if one is running).
    for (const p of started) {
      if (p.exitCode === null) spawnSync('taskkill', ['/PID', String(p.pid), '/T', '/F'], { stdio: 'ignore' });
      // A killed bridge can't remove its session file (its own, never the
      // tab's); remove it if it is still that bridge's.
      try {
        const f = ownSession(port);
        if (JSON.parse(readFileSync(f, 'utf8')).pid === p.pid) rmSync(f, { force: true });
      } catch {
        // no session file
      }
    }
    await sleep(300);
    if (profile) {
      try {
        rmSync(profile, { recursive: true, force: true });
      } catch {
        // temp folder; leftovers are harmless
      }
    }
  }
}

main().catch((e) => {
  console.error('[live-check] error:', e.message ?? e);
  process.exit(2);
});
