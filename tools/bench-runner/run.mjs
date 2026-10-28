// Evani.Dev bench runner: drives headless Chrome over the DevTools protocol
// (raw CDP over Node's built-in WebSocket; no npm packages) and collects
// `window.__bench` from the bench page, one fresh page per job.
//
// Called by `cargo xtask bench`; usable by hand:
//   node tools/bench-runner/run.mjs --url http://127.0.0.1:8080/ \
//        --jobs '[{"tier":"low","width":1920,"height":1080}]'
//
// Page mode (`cargo xtask netlog`): `--page-ms <ms>` loads the page (no
// `?tier=`, no `window.__bench`), keeps it open that long and ends.
//   --shots <dir> --shots-at 1500,9000   screenshots at those times (ms after
//                                         the navigation), <label>-<ms>ms.png
//   --netlog <dir>                        every request the page sends (CDP
//                                         Network.requestWillBeSent), with its
//                                         status, in <dir>/netlog-<label>.json
//                                         and in the job's `requests`
//   --throttle-kbps 4000                  a slow line (CDP network emulation)
//   --connect http://127.0.0.1:9222       drive a Chrome already running (the
//                                         leased phone over adb) instead of
//                                         starting one; only its own tabs close
// A job may carry `css` (a style sheet added to the page), `label` (file names; default its tier) and `storage`
// ({ key: value } put in localStorage before the page's own scripts run,
// e.g. the saved tier `evani:<game>:render.tier`, so there is no warm-up).
//
// stdout: one JSON document { chrome, adapter, jobs: [{ tier, bench, error, console, requests }] }.
// stderr: progress lines. Exit code 0 when the protocol part worked (the
// caller judges PASS/FAIL), 2 on a runner error (no Chrome, no WebGPU...).
//
// Safety: it only ever kills the Chrome it started itself (by its own PID).

import { spawn, spawnSync } from 'node:child_process';
import { existsSync, mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

// Chrome finder, CDP client and the pointer/keyboard stubs (shared with tools/convert-runner).
import { Cdp, STUBS, findChrome as findChromeAt, sleep } from './chrome.mjs';

const args = {};
for (let i = 2; i < process.argv.length; i++) {
  const a = process.argv[i];
  if (a.startsWith('--')) args[a.slice(2)] = process.argv[++i];
}
const log = (...m) => console.error('[bench-runner]', ...m);
const findChrome = () => findChromeAt(args.chrome);

/**
 * `--connect http://127.0.0.1:9222`: drive a Chrome that is already running
 * (Chrome on a phone over `adb forward ... localabstract:chrome_devtools_remote`,
 * for `cargo xtask netlog --device phone`). Nothing is started, closed or
 * killed except the tabs this run opens.
 */
async function connectMain(url, jobs, perJobMs) {
  const base = args.connect.replace(/\/$/, '');
  const ver = await (await fetch(`${base}/json/version`)).json();
  // The phone names itself `localhost`; the forwarded port is the one we know.
  const port = new URL(base).port;
  const wsUrl = String(ver.webSocketDebuggerUrl ?? '').replace(/^ws:\/\/[^/]+/, `ws://127.0.0.1:${port}`);
  if (!wsUrl.startsWith('ws://')) throw new Error(`${base}/json/version has no browser socket (is Chrome open on the device?)`);
  const out = { chrome: 'connected: ' + base, version: ver.Browser ?? null, adapter: null, throttle_kbps: args['throttle-kbps'] ? Number(args['throttle-kbps']) : null, jobs: [] };
  log('browser:', out.version);
  const cdp = await Cdp.connect(wsUrl);
  try {
    for (const job of jobs) out.jobs.push(await runJob(cdp, url, job, perJobMs, out));
  } finally {
    cdp.ws.close();
  }
  process.stdout.write(JSON.stringify(out));
}

async function main() {
  const url = args.url;
  const jobs = JSON.parse(args.jobs ?? '[]');
  if (!url || jobs.length === 0) throw new Error('usage: --url <page> --jobs <json array>');
  const perJobMs = Number(args['timeout-ms'] ?? 300000);
  if (args.connect) return connectMain(url, jobs, perJobMs);
  const chromePath = findChrome();
  if (!chromePath) throw new Error('no Chrome found (set EVANI_CHROME or pass --chrome)');
  const isShell = /headless_shell|chrome-headless-shell/.test(chromePath);

  const profile = mkdtempSync(join(tmpdir(), 'evani-bench-'));
  const first = jobs[0];
  const flags = [
    `--user-data-dir=${profile}`,
    '--remote-debugging-port=0',
    '--no-first-run',
    '--no-default-browser-check',
    '--mute-audio',
    '--enable-unsafe-webgpu',
    '--enable-webgpu-developer-features',
    // Measure the renderer, not the display: no 60 Hz cap, no background throttling.
    '--disable-frame-rate-limit',
    '--disable-gpu-vsync',
    '--disable-background-timer-throttling',
    '--disable-renderer-backgrounding',
    '--disable-backgrounding-occluded-windows',
    `--window-size=${first.width},${first.height}`,
    'about:blank',
  ];
  // `--capped 1`: keep Chrome's 60 Hz frame cap (the stress scene). Uncapped,
  // the page floods the GPU process with frames and timestamp readbacks
  // come back only a few times a second.
  if (args.capped) {
    for (const f of ['--disable-frame-rate-limit', '--disable-gpu-vsync']) flags.splice(flags.indexOf(f), 1);
  }
  if (!isShell) flags.unshift('--headless=new');
  log('chrome:', chromePath);
  const chrome = spawn(chromePath, flags, { stdio: 'ignore', windowsHide: true });
  let chromeExited = false;
  chrome.on('exit', () => (chromeExited = true));

  const out = { chrome: chromePath, version: null, adapter: null, throttle_kbps: args['throttle-kbps'] ? Number(args['throttle-kbps']) : null, jobs: [] };
  let cdp = null;
  try {
    // Chrome writes the debugging port into the profile folder.
    let wsUrl = null;
    for (let i = 0; i < 200 && !wsUrl && !chromeExited; i++) {
      const f = join(profile, 'DevToolsActivePort');
      if (existsSync(f)) {
        const [port, path] = readFileSync(f, 'utf8').trim().split(/\r?\n/);
        if (port && path) wsUrl = `ws://127.0.0.1:${port}${path}`;
      }
      if (!wsUrl) await sleep(100);
    }
    if (!wsUrl) throw new Error('Chrome did not open its debugging port');
    cdp = await Cdp.connect(wsUrl);
    const ver = await cdp.send('Browser.getVersion');
    out.version = ver.product;
    log('browser:', ver.product);

    for (const job of jobs) {
      out.jobs.push(await runJob(cdp, url, job, perJobMs, out));
    }
    try {
      await cdp.send('Browser.close');
    } catch {
      // closing may drop the socket before answering
    }
  } finally {
    if (cdp) cdp.ws.close();
    for (let i = 0; i < 50 && !chromeExited; i++) await sleep(100);
    if (!chromeExited) {
      // Only the process we started (and its children).
      spawnSync('taskkill', ['/PID', String(chrome.pid), '/T', '/F'], { stdio: 'ignore' });
    }
    await sleep(300);
    try {
      rmSync(profile, { recursive: true, force: true });
    } catch {
      // the profile is in the temp folder; leftovers are harmless
    }
  }
  process.stdout.write(JSON.stringify(out));
}

async function runJob(cdp, baseUrl, job, timeoutMs, out) {
  const result = { tier: job.tier, bench: null, error: null, console: [] };
  const label = job.label ?? job.tier;
  const pageMs = args['page-ms'] ? Number(args['page-ms']) : 0;
  if (args.netlog) result.requests = [];
  let navAt = 0;
  const { targetId } = await cdp.send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await cdp.send('Target.attachToTarget', { targetId, flatten: true });
  const s = (method, params, ms) => cdp.send(method, params, sessionId, ms);
  let crashed = false;
  cdp.on((m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === 'Inspector.targetCrashed') crashed = true;
    if (m.method === 'Network.requestWillBeSent' && result.requests) {
      const p = m.params;
      result.requests.push({ id: p.requestId, url: p.request.url, method: p.request.method, type: p.type ?? null, ms: navAt ? Date.now() - navAt : 0, status: null });
    }
    if (m.method === 'Network.responseReceived' && result.requests) {
      const r = result.requests.find((x) => x.id === m.params.requestId && x.status === null);
      if (r) r.status = m.params.response.status;
    }
    if (m.method === 'Network.loadingFinished' && result.requests) {
      const r = result.requests.find((x) => x.id === m.params.requestId && x.done_ms === undefined);
      if (r) {
        r.done_ms = navAt ? Date.now() - navAt : 0;
        // Bytes received for it (body and headers), for `cargo xtask bytes`.
        r.bytes = m.params.encodedDataLength ?? null;
      }
    }
    if (m.method === 'Network.loadingFailed' && result.requests) {
      const r = result.requests.find((x) => x.id === m.params.requestId && x.status === null);
      if (r) r.status = 'failed: ' + m.params.errorText;
    }
    if (m.method === 'Runtime.exceptionThrown' && result.console.length < 20) {
      result.console.push('exception: ' + (m.params.exceptionDetails?.exception?.description ?? m.params.exceptionDetails?.text));
    }
    if (m.method === 'Runtime.consoleAPICalled' && m.params.type === 'error' && result.console.length < 20) {
      result.console.push('console.error: ' + m.params.args.map((a) => a.value ?? a.description ?? '').join(' ').slice(0, 300));
    }
  });
  try {
    await s('Page.enable');
    await s('Runtime.enable');
    await s('Inspector.enable');
    // A real device keeps its own screen.
    if (!args.connect) {
      await s('Emulation.setDeviceMetricsOverride', {
        width: job.width,
        height: job.height,
        deviceScaleFactor: job.dpr ?? 1,
        mobile: false,
      });
    }
    await s('Page.addScriptToEvaluateOnNewDocument', { source: STUBS });
    if (job.storage) {
      // Before the page's own scripts, on the page's origin only.
      const seed = `(() => { if (location.protocol.startsWith('http')) { try { const s = ${JSON.stringify(job.storage)}; for (const k in s) localStorage.setItem(k, s[k]); } catch (e) {} } })();`;
      await s('Page.addScriptToEvaluateOnNewDocument', { source: seed });
    }
    if (job.css) {
      // Extra page CSS (e.g. hide the DOM overlays so a canvas check sees only the canvas).
      const css = `(() => { const add = () => { const st = document.createElement('style'); st.textContent = ${JSON.stringify(job.css)}; document.head.appendChild(st); }; if (document.head) add(); else document.addEventListener('DOMContentLoaded', add); })();`;
      await s('Page.addScriptToEvaluateOnNewDocument', { source: css });
    }
    if (result.requests || args['throttle-kbps']) await s('Network.enable');
    if (args['throttle-kbps']) {
      // A slow line, so a streamed file arrives after the first frames.
      const bps = (Number(args['throttle-kbps']) * 1000) / 8;
      await s('Network.emulateNetworkConditions', { offline: false, latency: 40, downloadThroughput: bps, uploadThroughput: bps });
    }
    const q = pageMs ? job.query ?? '' : `tier=${encodeURIComponent(job.tier)}${job.query ? '&' + job.query : ''}`;
    const target = q ? `${baseUrl}${baseUrl.includes('?') ? '&' : '?'}${q}` : baseUrl;
    log(`${label}: ${job.width}x${job.height} -> ${target}`);
    navAt = Date.now();
    await s('Page.navigate', { url: target });
    if (pageMs) {
      await pageRun(s, job, label, pageMs, () => crashed, result);
      return result;
    }

    if (!out.adapter) {
      // Wait for the document, then ask which GPU WebGPU gives us.
      for (let i = 0; i < 40; i++) {
        const r = await s('Runtime.evaluate', { expression: 'typeof navigator.gpu', returnByValue: true });
        if (r.result?.value === 'object') break;
        await sleep(250);
      }
      const r = await s('Runtime.evaluate', {
        expression: `navigator.gpu ? navigator.gpu.requestAdapter({powerPreference:'high-performance'}).then(a => a ? { vendor: a.info.vendor, architecture: a.info.architecture, device: a.info.device, description: a.info.description, features: [...a.features].includes('timestamp-query') } : null) : null`,
        awaitPromise: true,
        returnByValue: true,
      });
      out.adapter = r.result?.value ?? null;
      log('adapter:', JSON.stringify(out.adapter));
    }

    const start = Date.now();
    let lastProgress = '';
    let shotsTaken = 0;
    for (;;) {
      if (crashed) throw new Error('the page crashed');
      if (Date.now() - start > timeoutMs) throw new Error(`timed out after ${Math.round(timeoutMs / 1000)} s`);
      const r = await s('Runtime.evaluate', {
        expression: `(() => { const b = window.__bench; const e = window.__engine; return JSON.stringify({ has: !!b, done: !!(b && b.done), progress: b ? b.progress : null, engineError: e ? e.error : null }); })()`,
        returnByValue: true,
      });
      const st = JSON.parse(r.result?.value ?? '{}');
      if (st.progress && st.progress !== lastProgress && /measuring 0|done|waiting/.test(st.progress)) {
        lastProgress = st.progress;
        log(`${job.tier}: ${st.progress}`);
      }
      // Optional picture of each phase mid-run (--shots <folder>), to see that something was drawn.
      if (args.shots && st.progress && st.progress.includes(' measuring ') && shotsTaken < 2) {
        const phase = st.progress.includes(' 3D ') ? '3D' : '2D';
        const file = join(args.shots, `bench-${job.tier}-${phase}.png`);
        if (!existsSync(file)) {
          mkdirSync(args.shots, { recursive: true });
          shotsTaken++; // one try per phase: a busy page may not answer
          try {
            const shot = await s('Page.captureScreenshot', { format: 'png' }, 15000);
            writeFileSync(file, Buffer.from(shot.data, 'base64'));
          } catch (e) {
            log('screenshot skipped: ' + e.message);
          }
        }
      }
      if (st.done) break;
      if (st.engineError && !st.has && Date.now() - start > 5000) throw new Error('engine error: ' + st.engineError);
      await sleep(500);
    }
    const r = await s('Runtime.evaluate', { expression: 'JSON.stringify(window.__bench)', returnByValue: true });
    result.bench = JSON.parse(r.result.value);
  } catch (e) {
    result.error = String(e.message ?? e);
  } finally {
    try {
      await cdp.send('Target.closeTarget', { targetId });
    } catch {
      // already gone
    }
  }
  return result;
}

/** Page mode: keep the page open `pageMs`, screenshots at `--shots-at`, then the request log. */
async function pageRun(s, job, label, pageMs, crashed, result) {
  const start = Date.now();
  const times = String(args['shots-at'] ?? '')
    .split(',')
    .map(Number)
    .filter((t) => t > 0 && t <= pageMs)
    .sort((a, b) => a - b);
  for (const t of times) {
    if (crashed()) throw new Error('the page crashed');
    const wait = t - (Date.now() - start);
    if (wait > 0) await sleep(wait);
    if (!args.shots) continue;
    mkdirSync(args.shots, { recursive: true });
    try {
      const shot = await s('Page.captureScreenshot', { format: 'png' }, 15000);
      writeFileSync(join(args.shots, `${label}-${t}ms.png`), Buffer.from(shot.data, 'base64'));
    } catch (e) {
      log('screenshot skipped: ' + e.message);
    }
  }
  const rest = pageMs - (Date.now() - start);
  if (rest > 0) await sleep(rest);
  if (crashed()) throw new Error('the page crashed');
  const r = await s('Runtime.evaluate', { expression: 'JSON.stringify({ error: window.__engine ? window.__engine.error ?? null : null })', returnByValue: true });
  result.page = JSON.parse(r.result?.value ?? '{}');
  if (args.netlog && result.requests) {
    mkdirSync(args.netlog, { recursive: true });
    writeFileSync(join(args.netlog, `netlog-${label}.json`), JSON.stringify({ tier: job.tier, label, storage: job.storage ?? null, throttle_kbps: args['throttle-kbps'] ? Number(args['throttle-kbps']) : null, requests: result.requests }, null, 2));
  }
  log(`${label}: ${result.requests ? result.requests.length + ' requests' : 'done'}`);
}

main().catch((e) => {
  console.error('[bench-runner] error:', e.message ?? e);
  process.exit(2);
});
