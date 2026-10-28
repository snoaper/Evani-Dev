// Shared headless-Chrome helpers for the Evani.Dev runners (the bench runner
// and the converter's quality-gate runner, tools/convert-runner): find a
// Chrome, start it muted and headless on a throwaway profile, and talk raw
// CDP over Node's built-in WebSocket. Node built-ins only, no npm packages.
//
// Safety: `startChrome` hands back a `stop()` that only ever kills the Chrome
// it started itself (by its own PID, with its children).

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

/** Chrome (or Edge, or Playwright's headless shell): `explicit`, EVANI_CHROME, then the usual places. */
export function findChrome(explicit) {
  const candidates = [explicit, process.env.EVANI_CHROME];
  const pf = [process.env.ProgramFiles, process.env['ProgramFiles(x86)'], process.env.LOCALAPPDATA];
  for (const base of pf) {
    if (base) candidates.push(join(base, 'Google', 'Chrome', 'Application', 'chrome.exe'));
  }
  for (const base of pf) {
    if (base) candidates.push(join(base, 'Microsoft', 'Edge', 'Application', 'msedge.exe'));
  }
  const pw = join(process.env.LOCALAPPDATA ?? '', 'ms-playwright');
  if (existsSync(pw)) {
    const dirs = readdirSync(pw)
      .filter((d) => d.startsWith('chromium_headless_shell-'))
      .sort()
      .reverse();
    for (const d of dirs) {
      candidates.push(join(pw, d, 'chrome-headless-shell-win64', 'chrome-headless-shell.exe'));
    }
  }
  return candidates.find((c) => c && existsSync(c));
}

/** Minimal CDP client over one WebSocket. */
export class Cdp {
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
    await new Promise((resolve, reject) => {
      ws.addEventListener('open', resolve, { once: true });
      ws.addEventListener('error', () => reject(new Error('cannot connect to ' + url)), { once: true });
    });
    return new Cdp(ws);
  }
  send(method, params = {}, sessionId, timeoutMs = 60000) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(method + ": no answer from the browser in " + Math.round(timeoutMs / 1000) + " s"));
      }, timeoutMs);
      this.pending.set(id, {
        resolve: (v) => { clearTimeout(timer); resolve(v); },
        reject: (e) => { clearTimeout(timer); reject(e); },
        method,
      });
      this.ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
    });
  }
  on(handler) {
    this.handlers.push(handler);
  }
  off(handler) {
    this.handlers = this.handlers.filter((h) => h !== handler);
  }
}

// The page must never grab the real pointer or keyboard in a test run.
export const STUBS = `(() => {
  const noop = () => Promise.resolve();
  try { Element.prototype.requestPointerLock = noop; } catch (e) {}
  try { Element.prototype.requestFullscreen = noop; } catch (e) {}
  try { Document.prototype.exitPointerLock = function () {}; } catch (e) {}
  try { if (typeof Keyboard !== 'undefined') Keyboard.prototype.lock = noop; } catch (e) {}
  try { if (navigator.keyboard) navigator.keyboard.lock = noop; } catch (e) {}
})();`;

/**
 * Start a headless, muted Chrome with WebGPU on a throwaway profile and
 * connect to it. `flags` are added to the common ones. Returns
 * `{ cdp, path, version, stop }`; `stop()` closes it (by its own PID only)
 * and removes the profile.
 */
export async function startChrome({ chrome, width = 1920, height = 1080, flags = [], prefix = 'evani-chrome-' } = {}) {
  const path = findChrome(chrome);
  if (!path) throw new Error('no Chrome found (set EVANI_CHROME or pass --chrome)');
  const isShell = /headless_shell|chrome-headless-shell/.test(path);
  const profile = mkdtempSync(join(tmpdir(), prefix));
  const all = [
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
    `--window-size=${width},${height}`,
    ...flags,
    'about:blank',
  ];
  if (!isShell) all.unshift('--headless=new');
  const proc = spawn(path, all, { stdio: 'ignore', windowsHide: true });
  let exited = false;
  proc.on('exit', () => (exited = true));
  let cdp = null;
  const stop = async () => {
    if (cdp) {
      try {
        await cdp.send('Browser.close', {}, undefined, 5000);
      } catch {
        // closing may drop the socket before answering
      }
      cdp.ws.close();
    }
    for (let i = 0; i < 50 && !exited; i++) await sleep(100);
    if (!exited) spawnSync('taskkill', ['/PID', String(proc.pid), '/T', '/F'], { stdio: 'ignore' });
    await sleep(300);
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // the profile is in the temp folder; leftovers are harmless
    }
  };
  try {
    let wsUrl = null;
    for (let i = 0; i < 200 && !wsUrl && !exited; i++) {
      const f = join(profile, 'DevToolsActivePort');
      if (existsSync(f)) {
        const [port, p] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
        if (port && p) wsUrl = `ws://127.0.0.1:${port}${p}`;
      }
      if (!wsUrl) await sleep(100);
    }
    if (!wsUrl) throw new Error('Chrome did not open its debugging port');
    cdp = await Cdp.connect(wsUrl);
    const ver = await cdp.send('Browser.getVersion');
    return { cdp, path, version: ver.product, pid: proc.pid, stop };
  } catch (e) {
    await stop();
    throw e;
  }
}
