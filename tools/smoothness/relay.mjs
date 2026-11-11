// The networked mode's relay: sits between the game page and a local
// authoritative host (tools/evani-host/server.mjs) and holds back what the
// host sends by a relay delay plus jitter, the way a real relay delivers
// server snapshots unevenly. Ported from APEX Velocity's host-sim.mjs
// "wan" model (2026-11-11): a base delay, exponential jitter and an
// occasional hiccup; in order, so a late chunk holds the ones behind it
// (a TCP relay never reorders).
//
// It is a plain TCP relay: it understands nothing of WebSocket or the game's
// messages, so it works for any host. Page-to-host traffic passes straight
// through (`up_ms` adds a fixed delay if wanted).

import net from 'node:net';

/** The relay profiles (`--net`). `wan` is APEX's. Times in ms. */
export const PROFILES = Object.freeze({
  none: { base_ms: 0, jitter_ms: 0, hiccup_ms: 0, hiccup_every: 0, up_ms: 0 },
  lan: { base_ms: 2, jitter_ms: 1, hiccup_ms: 0, hiccup_every: 0, up_ms: 0 },
  wan: { base_ms: 22, jitter_ms: 6, hiccup_ms: 70, hiccup_every: 90, up_ms: 0 },
  bad: { base_ms: 60, jitter_ms: 25, hiccup_ms: 200, hiccup_every: 40, up_ms: 30 },
});

/** Seeded generator (a 32-bit LCG, as APEX): the same seed gives the same delays. */
export function lcg(seed = 1) {
  let s = (seed >>> 0) % 2147483648;
  return () => {
    s = (s * 1103515245 + 12345) % 2147483648;
    return s / 2147483648;
  };
}

/** One delay draw: base + exponential jitter (mean `jitter_ms`) + a hiccup once in `hiccup_every`. */
export function drawDelay(p, rnd) {
  if (!p.base_ms && !p.jitter_ms && !p.hiccup_ms) return 0;
  const jitter = p.jitter_ms > 0 ? -p.jitter_ms * Math.log(1 - rnd()) : 0;
  const hiccup = p.hiccup_every > 0 && rnd() < 1 / p.hiccup_every ? p.hiccup_ms : 0;
  return p.base_ms + jitter + hiccup;
}

/**
 * In-order delivery times: each chunk at `max(previous, now + delay)`.
 * Pure (tests): `arrivals(sendTimes, delays)`.
 */
export function arrivals(sendTimes, delays) {
  let last = -Infinity;
  return sendTimes.map((t, i) => (last = Math.max(last, t + delays[i])));
}

/** Resolve `--net` (a profile name or a JSON object over `wan`). */
export function profile(spec) {
  if (!spec || spec === 'none') return { ...PROFILES.none };
  if (typeof spec === 'object') return { ...PROFILES.wan, ...spec };
  if (PROFILES[spec]) return { ...PROFILES[spec] };
  throw new Error(`unknown --net profile ${spec} (${Object.keys(PROFILES).join(', ')})`);
}

/**
 * Start a relay on a free localhost port forwarding to `target` (port).
 * Returns `{ port, stats, close() }`. `stats` counts chunks and the delays drawn.
 */
export async function startRelay(targetPort, p, seed = 1) {
  const rnd = lcg(seed);
  const stats = { connections: 0, down_chunks: 0, down_bytes: 0, delay_ms: [] };
  const sockets = new Set();
  const server = net.createServer((client) => {
    stats.connections++;
    const upstream = net.connect(targetPort, '127.0.0.1');
    client.setNoDelay(true);
    upstream.setNoDelay(true);
    sockets.add(client);
    sockets.add(upstream);
    let last = 0;
    const end = () => {
      client.destroy();
      upstream.destroy();
    };
    client.on('error', end);
    upstream.on('error', end);
    client.on('close', end);
    upstream.on('close', end);
    upstream.on('data', (chunk) => {
      const now = performance.now();
      const d = drawDelay(p, rnd);
      const at = Math.max(last, now + d);
      last = at;
      stats.down_chunks++;
      stats.down_bytes += chunk.length;
      if (stats.delay_ms.length < 100000) stats.delay_ms.push(Math.round((at - now) * 10) / 10);
      if (at <= now + 0.5) client.write(chunk);
      else setTimeout(() => client.write(chunk), at - now);
    });
    client.on('data', (chunk) => {
      if (p.up_ms > 0) setTimeout(() => upstream.write(chunk), p.up_ms);
      else upstream.write(chunk);
    });
  });
  await new Promise((r) => server.listen(0, '127.0.0.1', r));
  return {
    port: server.address().port,
    stats,
    close: () => {
      for (const s of sockets) s.destroy();
      server.close();
    },
  };
}
