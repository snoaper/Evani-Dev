// Tests for the GPU capture lock lease (gpu-lock.mjs): run with `node tools/bench-runner/gpu-lock.test.mjs`.
// Every holder and waiter is its own child process (the lock is per process); the lock file is a temp one
// (EVANI_GPU_LOCK_FILE) and the heartbeat and lease are shortened through their test-only env overrides.
// No GPU and no browser: only the lock itself.
import { spawn } from 'node:child_process';
import { mkdtempSync, writeFileSync, existsSync, utimesSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const SELF = fileURLToPath(import.meta.url);

if (process.argv[2] === 'child') {
  // child <mode> <ms>: hold | freeze | wait
  const { acquireGpuLock, release } = await import(pathToFileURL(join(SELF, '..', 'gpu-lock.mjs')).href);
  const mode = process.argv[3], ms = Number(process.argv[4]) || 0;
  const say = (w) => console.log(`${w} ${Date.now()}`);
  await acquireGpuLock('test ' + mode);
  say('acquired');
  if (mode === 'freeze') {
    // a hung holder: its event loop blocks, so no heartbeat goes out, but the process stays alive
    Atomics.wait(new Int32Array(new SharedArrayBuffer(4)), 0, 0, ms);
    say('woke');
    await new Promise((r) => setTimeout(r, 400));
  } else if (mode === 'hold') {
    await new Promise((r) => setTimeout(r, ms));
  } else if (mode === 'forever') {
    await new Promise(() => setInterval(() => {}, 1000));
  }
  say('releasing');
  release();
  process.exit(0);
}

const dir = mkdtempSync(join(tmpdir(), 'gpu-lock-test-'));
let fails = 0, n = 0;
const ok = (cond, what) => { n++; if (cond) console.log(`ok ${n} - ${what}`); else { fails++; console.log(`not ok ${n} - ${what}`); } };

function child(lock, mode, ms, env = {}) {
  const p = spawn(process.execPath, [SELF, 'child', mode, String(ms || 0)], {
    env: { ...process.env, EVANI_GPU_LOCK_FILE: lock, EVANI_GPU_LOCK_BEAT_MS: '200', EVANI_GPU_LOCK_LEASE_MS: '1500', ...env },
    stdio: ['ignore', 'pipe', 'pipe'], windowsHide: true
  });
  const ev = {}; let err = '';
  const waiters = [];
  p.stdout.on('data', (d) => { for (const line of String(d).split(/\r?\n/)) { const m = /^(\w+) (\d+)$/.exec(line.trim()); if (m) { ev[m[1]] = Number(m[2]); waiters.forEach((w) => w()); } } });
  p.stderr.on('data', (d) => { err += d; });
  const exited = new Promise((r) => p.on('exit', (code) => { ev.exit = Date.now(); r(code); }));
  const when = (name, t = 10000) => new Promise((res, rej) => {
    const check = () => { if (ev[name]) { res(ev[name]); return true; } return false; };
    if (check()) return;
    const to = setTimeout(() => rej(new Error(`${mode}: no "${name}" within ${t} ms; stderr: ${err.slice(-400)}`)), t);
    waiters.push(() => { if (check()) clearTimeout(to); });
  });
  return { p, ev, when, exited, err: () => err };
}
const kill = (c) => { try { process.kill(c.p.pid, 'SIGKILL'); } catch { /* already gone */ } }; // only our own child, by its exact PID

try {
  // 1. A crash while holding the lock: the holder is killed outright (no exit handlers run), so the lock file stays behind.
  {
    const lock = join(dir, 'crash.lock');
    const a = child(lock, 'forever');
    await a.when('acquired');
    kill(a); await a.exited;
    ok(existsSync(lock), 'crash: the dead holder left its lock file behind');
    const t0 = Date.now();
    const b = child(lock, 'hold', 50);
    const got = await b.when('acquired');
    ok(got - t0 < 5000, `crash: the next job ran ${got - t0} ms after the holder died (within the lease)`);
    ok(/PID .*its PID is gone/.test(b.err()), 'crash: the takeover says why (PID gone)');
    await b.exited;
    ok(!existsSync(lock), 'crash: the next job released the lock when it finished');
  }
  // 2. A hung holder (alive, but no heartbeat): taken over once its heartbeat is older than the lease.
  {
    const lock = join(dir, 'hung.lock');
    const a = child(lock, 'freeze', 4500);
    await a.when('acquired');
    const t0 = Date.now();
    const b = child(lock, 'hold', 1500);
    const got = await b.when('acquired');
    ok(!a.ev.woke || got < a.ev.woke, 'hung: the waiter took over while the hung holder was still frozen');
    ok(got - t0 < 4000, `hung: taken over ${got - t0} ms after the waiter came (lease 1.5 s)`);
    ok(/no heartbeat for/.test(b.err()), 'hung: the takeover says why (no heartbeat)');
    await a.exited; await b.exited;
    ok(/lost the GPU capture lock/.test(a.err()), 'hung: the old holder notices it lost the lock');
    ok(!existsSync(lock), 'hung: the old holder did not remove the new holder\'s lock; the lock is free at the end');
  }
  // 3. A healthy holder working longer than the lease is never pushed out: its heartbeat keeps the lease.
  {
    const lock = join(dir, 'busy.lock');
    const a = child(lock, 'hold', 4000);
    await a.when('acquired');
    const b = child(lock, 'hold', 50);
    const got = await b.when('acquired', 15000);
    const rel = await a.when('releasing');
    ok(got >= rel, `busy: the waiter got the lock only after the live holder released it (${got - rel} ms after)`);
    ok(!/taking over/.test(b.err()), 'busy: no takeover of a live holder with a fresh heartbeat');
    ok(/waiting for GPU capture lock/.test(b.err()) || got - rel < 3000, 'busy: the waiter waited its turn');
    await a.exited; await b.exited;
  }
  // 4. A lock from an older copy of the script (no heartbeat field) held by a live process is not taken over early.
  {
    const lock = join(dir, 'legacy.lock');
    const live = spawn(process.execPath, ['-e', 'setTimeout(()=>{}, 8000)'], { stdio: 'ignore', windowsHide: true });
    writeFileSync(lock, JSON.stringify({ pid: live.pid, started: Date.now() - 3 * 60 * 1000, script: 'old' }));
    const old = new Date(Date.now() - 3 * 60 * 1000); utimesSync(lock, old, old);
    const b = child(lock, 'hold', 50);
    let early = false; try { await b.when('acquired', 3000); early = true; } catch { /* expected: still waiting */ }
    ok(!early, 'legacy: a live old-style holder (3 min, no heartbeat) is not taken over by the 2-minute lease');
    try { process.kill(live.pid, 'SIGKILL'); } catch { /* gone */ }
    const got = await b.when('acquired', 6000).catch(() => 0);
    ok(got > 0, 'legacy: once that holder is gone, the waiter takes the lock');
    kill(b); await b.exited;
  }
  // 5. A normal finish releases the lock (exit handler), with no heartbeat timer keeping the process alive.
  {
    const lock = join(dir, 'exit.lock');
    const a = child(lock, 'hold', 300);
    await a.when('acquired');
    const code = await a.exited;
    ok(code === 0 && !existsSync(lock), 'exit: released on a normal finish, and the heartbeat did not keep the process alive');
  }
} catch (e) {
  fails++; console.log(`not ok - ${e.message}`);
} finally {
  try { rmSync(dir, { recursive: true, force: true }); } catch { /* temp */ }
}
console.log(fails ? `gpu-lock tests: ${fails} failed of ${n}` : `gpu-lock tests: all ${n} passed`);
process.exit(fails ? 1 : 0);
