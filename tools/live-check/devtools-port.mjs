// Chrome's debugging address, shared by every script that starts a headless
// Chrome with `--remote-debugging-port=0`: Chrome writes the port and path
// into `<profile>/DevToolsActivePort` once it listens. On Windows Chrome may
// still hold that file open while it writes it, so reading it can fail with
// EBUSY (a geocheck run failed on it, 2026-10-25) or find it half written:
// both are read again on the next try, never thrown. Any other read error is
// a real fault and throws.

import { existsSync, readFileSync } from 'node:fs';
import { join } from 'node:path';

const RETRY_CODES = ['EBUSY', 'EPERM', 'EACCES'];

/** The `ws://` address in the profile's port file, or null if it is not (fully) there yet. */
export function readDevToolsUrl(profile) {
  const f = join(profile, 'DevToolsActivePort');
  let text = '';
  try {
    if (existsSync(f)) text = readFileSync(f, 'utf8');
  } catch (e) {
    if (!RETRY_CODES.includes(e.code)) throw e;
  }
  const [port, path] = text.trim().split(/\r?\n/);
  return port && path ? `ws://127.0.0.1:${port}${path}` : null;
}

/**
 * Wait for Chrome's debugging address: `tries` reads `delayMs` apart (20 s by
 * default), stopping early when `exited()` says Chrome is gone. Null if it
 * never came.
 */
export async function waitDevToolsUrl(profile, { tries = 200, delayMs = 100, exited = () => false } = {}) {
  for (let i = 0; i < tries && !exited(); i++) {
    const url = readDevToolsUrl(profile);
    if (url) return url;
    await new Promise((r) => setTimeout(r, delayMs));
  }
  return null;
}
