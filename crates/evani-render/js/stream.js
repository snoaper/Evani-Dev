// Evani.Dev's `stream` shim (feature `models`, S3-4). The wasm
// side (src/model/stream.rs) decides what to fetch, in which order and for
// which tier; this file only does what needs the browser. Cache Storage is
// keyed by content hash under one shared name, so a file two games share is
// stored once. navigator.storage.persist() is asked only on Chromium
// browsers (S3-0: they never show a prompt; Firefox can), once.
let open;
let asked;
const store = () => (open ||= self.caches ? caches.open('evani-stream').catch(() => null) : Promise.resolve(null));
const key = (k) => new Request('/.evani-stream/' + k);
export async function cache_get(k) {
  const c = await store();
  const r = c && (await c.match(key(k)).catch(() => null));
  return r ? new Uint8Array(await r.arrayBuffer()) : undefined;
}
export function cache_put(k, b) {
  b = b.slice();
  store().then((c) => c && c.put(key(k), new Response(b)).catch(() => {}));
  if (!asked && navigator.userAgentData && navigator.storage && navigator.storage.persist) {
    asked = 1;
    navigator.storage.persist().catch(() => {});
  }
}
export async function get(u, o, n, low) {
  const r = await fetch(u, { headers: o < 0 ? {} : { Range: `bytes=${o}-${o + n - 1}` }, priority: low ? 'low' : 'auto' });
  if (!r.ok) throw `${u}: HTTP ${r.status}`;
  const b = new Uint8Array(await r.arrayBuffer());
  return o >= 0 && r.status == 200 ? b.slice(o, o + n) : b;
}
