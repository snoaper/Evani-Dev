// Tests for the smoothness harness (no browser): node --test tools/smoothness/
import assert from 'node:assert/strict';
import net from 'node:net';
import { test } from 'node:test';
import { DEFAULT_THRESHOLDS, chartSeries, intervals, judge, quantile, ratios, speeds, summarise } from './analyze.mjs';
import { html } from './report.mjs';
import { PROFILES, arrivals, drawDelay, lcg, profile, startRelay } from './relay.mjs';
import { keyInfo, resolveInside, schedule } from './run.mjs';
import { parseArgs } from './smoothness.mjs';

const FRAME = 1000 / 60;

/** A trace of `n` frames at 60 Hz moving `step(k)` units along x per frame. */
function trace(n, step, { label = 'after', pres } = {}) {
  const motion = [];
  let x = 0;
  for (let k = 0; k < n; k++) {
    x += step(k);
    motion.push([1000 + k * FRAME, k, x, 0, 0, 640, 360]);
  }
  return { label, mark0: 1000, mark1: 1000 + (n - 1) * FRAME, pres: pres || motion.map((m) => m[0]), motion, width: 1280 };
}

test('steady motion has ratio 1 everywhere and passes the default limits', () => {
  const t = trace(600, () => 0.5);
  const s = summarise(t, { minSpeed: 1 });
  assert.equal(s.motion.uneven_pct, 0);
  assert.equal(s.motion.ratio_sd, 0);
  assert.equal(s.motion.ratio_dev_p99, 0);
  assert.equal(s.frames.hitches, 0);
  assert.ok(Math.abs(s.frames.p50_ms - FRAME) < 0.01);
  assert.deepEqual(judge(s), { pass: true, failures: [] });
});

test('judder (a tick held then caught up, the APEX live build) is caught', () => {
  // The server's 30 Hz ticks drawn raw: the car moves two frames' worth, then holds.
  const t = trace(600, (k) => (k % 2 ? 0 : 1.0));
  const s = summarise(t, { minSpeed: 1 });
  assert.ok(s.motion.uneven_pct > 90, JSON.stringify(s.motion));
  assert.ok(s.motion.ratio_sd > 0.9);
  const v = judge(s);
  assert.equal(v.pass, false);
  assert.deepEqual(v.failures.map((f) => f.key).sort(), ['ratio_dev_p99_max', 'ratio_sd_max', 'uneven_pct_max']);
});

test('a slower or faster game overall is not judder: ratios are against the neighbours', () => {
  const t = trace(900, (k) => 0.2 + k * 0.001);
  const s = summarise(t, { minSpeed: 1 });
  assert.equal(s.motion.uneven_pct, 0);
  assert.ok(s.motion.ratio_dev_p99 < 0.01, s.motion.ratio_dev_p99);
});

test('a parked probe is not measured, and too little motion fails the run', () => {
  const s = summarise(trace(300, () => 0), { minSpeed: 1 });
  assert.equal(s.motion.frames, 0);
  const v = judge(s);
  assert.equal(v.pass, false);
  assert.equal(v.failures[0].key, 'motion');
  assert.equal(judge(s, DEFAULT_THRESHOLDS, { requireMotion: false }).pass, true);
});

test('a held frame in fast motion counts (the speed filter judges the neighbourhood)', () => {
  const t = trace(300, (k) => (k === 150 ? 0 : 1));
  const sp = speeds(t.motion, { minSpeed: 1 });
  assert.ok(sp.some((s) => s[1] === 0), 'the held frame is kept');
  const r = ratios(sp);
  assert.ok(Math.min(...r.map((x) => x[1])) < 0.1);
});

test('frame pacing: hitches, longest frame, 120 Hz target', () => {
  const pres = [];
  let t = 1000;
  for (let k = 0; k < 600; k++) {
    t += k === 300 ? 120 : k === 400 ? 40 : FRAME;
    pres.push(t);
  }
  // The page worked 3 ms inside the 120 ms gap: the browser held it, not the game.
  const gapStart = pres[299];
  const tr = { label: 'x', mark0: pres[0], mark1: pres[pres.length - 1], pres, motion: [], cpu: [[gapStart + 1, 3], [pres[10], 9]] };
  const s = summarise(tr);
  assert.equal(s.frames.hitches, 2);
  assert.equal(s.frames.max_ms, 120);
  assert.equal(s.frames.longest_page_work_ms, 3);
  assert.ok(Math.abs(s.frames.longest_at_s - (gapStart - pres[0]) / 1000) < 0.002);
  assert.equal(s.motion, null);
  const v = judge(s, { ...DEFAULT_THRESHOLDS, hitches_max: 1 }, { requireMotion: false });
  assert.deepEqual(v.failures.map((f) => f.key), ['hitches_max']);
  const fast = pres.map((_, k) => 1000 + k * (1000 / 120));
  assert.ok(Math.abs(summarise({ ...tr, pres: fast, mark0: fast[0], mark1: fast[fast.length - 1] }).frames.target_ms - 8.333) < 0.01);
  assert.equal(intervals([1, 2, 4], 0, 3).length, 1);
});

test('screen metric uses the canvas position', () => {
  const motion = [];
  for (let k = 0; k < 200; k++) motion.push([k * FRAME, k, 0, 0, 0, 100 + 3 * k, 50]);
  const sp = speeds(motion, { metric: 'screen', minSpeed: 1 });
  assert.ok(Math.abs(sp[5][1] - 180) < 0.01, 'px a second');
  assert.equal(speeds(motion, { metric: 'world', minSpeed: 1 }).length, 0);
});

test('quantile is nearest-rank and NaN when empty', () => {
  assert.equal(quantile([3, 1, 2], 0.5), 2);
  assert.ok(Number.isNaN(quantile([], 0.5)));
});

test('the relay delays in order with the seeded wan model', () => {
  assert.deepEqual(arrivals([0, 10, 20], [50, 5, 5]), [50, 50, 50]);
  const a = lcg(7);
  const b = lcg(7);
  for (let i = 0; i < 5; i++) assert.equal(a(), b());
  const rnd = lcg(1);
  const d = Array.from({ length: 9000 }, () => drawDelay(PROFILES.wan, rnd));
  assert.ok(Math.min(...d) >= 22);
  const mean = d.reduce((s, v) => s + v, 0) / d.length;
  // 22 base + 6 mean jitter + 70/90 hiccup share
  assert.ok(Math.abs(mean - (22 + 6 + 70 / 90)) < 0.6, mean);
  assert.equal(drawDelay(PROFILES.none, rnd), 0);
  assert.equal(profile({ base_ms: 5 }).jitter_ms, PROFILES.wan.jitter_ms);
  assert.throws(() => profile('moon'));
});

test('the relay forwards both ways and holds the host side back', async () => {
  const echo = net.createServer((s) => s.on('data', (d) => s.write(d)));
  await new Promise((r) => echo.listen(0, '127.0.0.1', r));
  const relay = await startRelay(echo.address().port, { base_ms: 40, jitter_ms: 0, hiccup_ms: 0, hiccup_every: 0, up_ms: 0 }, 1);
  try {
    const c = net.connect(relay.port, '127.0.0.1');
    await new Promise((r) => c.once('connect', r));
    const t0 = performance.now();
    c.write('ping');
    const got = await new Promise((r) => c.once('data', (d) => r(String(d))));
    const dt = performance.now() - t0;
    assert.equal(got, 'ping');
    assert.ok(dt >= 38, `held back ${dt} ms`);
    assert.equal(relay.stats.connections, 1);
    c.destroy();
  } finally {
    relay.close();
    echo.close();
  }
});

test('the input script loops and orders key releases before presses', () => {
  const ev = schedule([{ at: 0, down: ['KeyW'] }, { at: 1, down: ['KeyA'] }, { at: 1.5, up: ['KeyA'], down: ['KeyD'] }], 5, 2);
  assert.equal(ev.filter((e) => e.code === 'KeyW').length, 3);
  const at15 = ev.filter((e) => e.at === 1.5).map((e) => e.type);
  assert.deepEqual(at15, ['keyUp', 'keyDown']);
  assert.ok(ev.every((e, i) => i === 0 || ev[i - 1].at <= e.at));
  assert.deepEqual(keyInfo('KeyW'), { key: 'w', vk: 87, location: 0 });
  assert.equal(keyInfo('ShiftLeft').location, 1);
  assert.equal(keyInfo('ArrowLeft').location, 0);
  assert.throws(() => keyInfo('Kart'));
});

test('the static server never leaves its folder', () => {
  const root = process.platform === 'win32' ? 'C:\\game\\dist' : '/game/dist';
  assert.ok(resolveInside(root, '/index.html').endsWith('index.html'));
  assert.ok(resolveInside(root, '/').endsWith('index.html'));
  assert.equal(resolveInside(root, '/../secret.txt'), null);
  assert.equal(resolveInside(root, '/%2e%2e/secret.txt'), null);
});

test('the report overlays Before and After with both charts and a table', () => {
  const before = trace(600, (k) => (k % 2 ? 0 : 1), { label: 'before' });
  const after = trace(600, () => 0.5);
  const runs = [
    ['before', 'Before', before],
    ['after', 'After', after],
  ].map(([key, name, t]) => {
    const summary = summarise(t, { minSpeed: 1 });
    return { key, name, legend: `${name}: test`, summary, verdict: judge(summary), series: chartSeries(t, { minSpeed: 1 }) };
  });
  const page = html(runs, { title: 'Test <game>: before and after', subtitle: 's', window_s: 6, target_ms: 16.7 });
  assert.match(page, /<svg id="m"/);
  assert.match(page, /<svg id="f"/);
  assert.match(page, /Table view/);
  assert.match(page, /Test &lt;game&gt;/, 'titles are escaped');
  assert.match(page, /<td>Before<\/td>.*<td>fail<\/td>/s);
  assert.match(page, /<td>After<\/td>.*<td>pass<\/td>/s);
  const series = runs[1].series.ratio;
  assert.ok(series.length > 300 && series.every((p) => p[0] >= 0 && p[0] < 6));
});

test('command-line options', () => {
  const o = parseArgs(['--config', 'c.json', '--baseline', 'a', '--candidate', 'b', '--net', 'wan', '--limit', 'hitches_max=3']);
  assert.equal(o.baseline, 'a');
  assert.equal(o.candidate, 'b');
  assert.equal(o.limits.hitches_max, 3);
  assert.throws(() => parseArgs(['--config', 'c.json']), /--build/);
  assert.throws(() => parseArgs(['--config', 'c.json', '--build', 'x', '--limit', 'speed=3']), /known limits/);
  assert.throws(() => parseArgs(['--build', 'x']), /--config/);
});
