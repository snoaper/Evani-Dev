// Cross-process GPU capture lock for every script that starts a rendering
// headless Chrome (startChrome in ./chrome.mjs takes it for you). One capture
// uses the graphics card at a time on this PC; the rest wait their turn.
//
//   Lock file: <main checkout>/.evani/tmp/gpu-capture.lock  (JSON {pid, started, script, hb})
//   - created with exclusive create (flag "wx"), so two callers never both win;
//   - a lease: while it holds the lock, the holder refreshes the file's modified time every 30 s (heartbeat);
//   - a stale lock is taken over: its PID is dead, or its heartbeat is older than 2 minutes (a hung or frozen
//     holder). A lock written by an older copy of this script (no "hb" field, no heartbeat) keeps the old rule:
//     PID dead, or older than 15 minutes, so a live old-style holder is never pushed out mid-capture;
//   - a waiting caller prints "waiting for GPU capture lock held by PID ..." at
//     least every 30 s (so idle watchers do not kill it) and gives up after 20 min;
//   - released by release(), and on process exit or SIGINT/SIGTERM/SIGHUP/SIGBREAK.
// Node built-ins only. Override the path with EVANI_GPU_LOCK_FILE (tests only).
import { openSync, writeSync, closeSync, readFileSync, unlinkSync, statSync, mkdirSync, utimesSync } from 'node:fs';
import { dirname, basename, resolve } from 'node:path';
import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';

// Default: .evani/tmp/gpu-capture.lock in the MAIN checkout (git's common dir), so every worktree of this repo
// shares one lock; outside a git checkout, in this checkout. No fixed folder.
function defaultLockFile() {
  const repo = resolve(dirname(fileURLToPath(import.meta.url)), '..', '..');
  let main = repo;
  try {
    const common = execFileSync('git', ['rev-parse', '--git-common-dir'], { cwd: repo, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    const dir = resolve(repo, common);
    if (basename(dir) === '.git') main = dirname(dir);
  } catch { /* not a git checkout: use this folder */ }
  return resolve(main, '.evani', 'tmp', 'gpu-capture.lock');
}
export const LOCK_FILE = process.env.EVANI_GPU_LOCK_FILE || defaultLockFile();
const LEGACY_STALE_MS = 15 * 60 * 1000; // a lock from an older copy of this script (no heartbeat)
const BEAT_MS = Number(process.env.EVANI_GPU_LOCK_BEAT_MS) || 30 * 1000; // env override: tests only
const LEASE_MS = Number(process.env.EVANI_GPU_LOCK_LEASE_MS) || 2 * 60 * 1000; // heartbeat older than this: taken over
const WAIT_MS = 20 * 60 * 1000;
const NOTE_MS = 30 * 1000;
const POLL_MS = 1000;

let held = 0; // re-entrant inside one process
let hooked = false;
let beatTimer = null;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

function alive(pid) {
  if (!Number.isInteger(pid) || pid <= 0) return false;
  try {
    process.kill(pid, 0);
    return true;
  } catch (e) {
    return e.code === 'EPERM';
  }
}

function readLock() {
  let raw = '';
  let mtime = Date.now();
  try {
    raw = readFileSync(LOCK_FILE, 'utf8');
    mtime = statSync(LOCK_FILE).mtimeMs;
  } catch {
    return null; // gone
  }
  try {
    const j = JSON.parse(raw);
    return { raw, pid: Number(j.pid), started: Number(j.started) || mtime, mtime, hb: j.hb === 1 };
  } catch {
    return { raw, pid: 0, started: mtime, mtime, partial: true }; // being written, or junk
  }
}

function isStale(l) {
  if (l.partial) return Date.now() - l.mtime > 10000; // give a writer 10 s to finish
  if (!alive(l.pid)) return true;
  if (l.hb) return Date.now() - l.mtime > LEASE_MS; // its heartbeat stopped: hung, frozen or a reused PID
  return Date.now() - l.started > LEGACY_STALE_MS;
}

// The holder's heartbeat: touch the file while it is still ours. Losing it (taken over after a long freeze) is said once.
function beat() {
  const l = readLock();
  if (!l || l.pid !== process.pid) {
    stopBeat();
    if (held) console.error(`gpu-lock: PID ${process.pid} lost the GPU capture lock (its heartbeat was late; another job took it over)`);
    return;
  }
  try {
    const now = new Date();
    utimesSync(LOCK_FILE, now, now);
  } catch {
    // gone meanwhile: the next beat says so
  }
}
function startBeat() {
  stopBeat();
  beatTimer = setInterval(beat, BEAT_MS);
  if (beatTimer.unref) beatTimer.unref(); // never keeps the process alive
}
function stopBeat() {
  if (beatTimer) clearInterval(beatTimer);
  beatTimer = null;
}

function tryCreate() {
  mkdirSync(dirname(LOCK_FILE), { recursive: true });
  let fd;
  try {
    fd = openSync(LOCK_FILE, 'wx');
  } catch (e) {
    if (e.code === 'EEXIST') return false;
    throw e;
  }
  try {
    writeSync(fd, JSON.stringify({ pid: process.pid, started: Date.now(), script: process.argv[1] || '', hb: 1 }));
  } finally {
    closeSync(fd);
  }
  return true;
}

function releaseSync() {
  if (!held) return;
  held = 0;
  stopBeat();
  const l = readLock();
  if (l && l.pid === process.pid) {
    try {
      unlinkSync(LOCK_FILE);
    } catch {
      // already gone
    }
  }
}

function hook() {
  if (hooked) return;
  hooked = true;
  process.on('exit', releaseSync);
  for (const sig of ['SIGINT', 'SIGTERM', 'SIGHUP', 'SIGBREAK']) {
    try {
      process.on(sig, () => {
        releaseSync();
        process.exit(130);
      });
    } catch {
      // signal not supported on this platform
    }
  }
}

/** Wait for and take the GPU capture lock. Resolves with a release function. */
export async function acquireGpuLock(label = '') {
  if (held) {
    held++;
    return release;
  }
  const t0 = Date.now();
  let lastNote = 0;
  for (;;) {
    if (tryCreate()) break;
    const l = readLock();
    if (l && isStale(l)) {
      // take over, but only remove the very lock we judged stale
      const again = readLock();
      if (again && again.raw === l.raw && again.mtime === l.mtime) {
        const why = !l.partial && !alive(l.pid) ? 'its PID is gone' : l.hb ? `no heartbeat for ${Math.round((Date.now() - l.mtime) / 1000)} s` : `${Math.round((Date.now() - l.started) / 1000)} s old`;
        console.error(`gpu-lock: taking over stale lock (PID ${l.pid || '?'}: ${why})`);
        try {
          unlinkSync(LOCK_FILE);
        } catch {
          // someone else took it over first
        }
      }
      continue;
    }
    const waited = Date.now() - t0;
    if (waited > WAIT_MS) {
      throw new Error(`gpu-lock: gave up after ${Math.round(waited / 60000)} min waiting for the GPU capture lock held by PID ${l ? l.pid : '?'} (${LOCK_FILE})`);
    }
    if (Date.now() - lastNote >= NOTE_MS) {
      lastNote = Date.now();
      console.error(`gpu-lock: waiting for GPU capture lock held by PID ${l ? l.pid : '?'} (waited ${Math.round(waited / 1000)} s${label ? ', ' + label : ''})`);
    }
    await sleep(POLL_MS + Math.floor(Math.random() * 500));
  }
  held = 1;
  hook();
  startBeat();
  const waited = Math.round((Date.now() - t0) / 1000);
  console.error(`gpu-lock: acquired by PID ${process.pid}${waited ? ` after ${waited} s` : ''}`);
  return release;
}

/** Release one hold (the lock file goes when the last hold is released). */
export function release() {
  if (held > 1) {
    held--;
    return;
  }
  releaseSync();
}
