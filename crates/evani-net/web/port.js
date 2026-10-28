// evani-net/1, the page side (evani-net feature `port`). Bytes between the
// game and its host over one MessagePort; the game opens no network itself.
//
// Two ways in, both platform-neutral:
//  * A host frame around the game posts, once, on the game's window:
//      {t: "hello", v: 1, role: "player" | "spectator", follow?: n}
//    with a MessagePort in the transfer list (or as `port`). Only a message
//    whose source is the real parent window counts (never this window).
//  * An in-page adapter (a bridge script in the same page) calls
//      globalThis.evaniNet.attachPort(port, {role, follow})
//    or, if it runs before the game loaded, pushes [port, opts] onto
//      globalThis.evaniNetPorts (an array; it is drained at load).
// Only the first port is taken. Then, on the port:
//   host -> game: {t:"data", b} (a datagram: Uint8Array or ArrayBuffer),
//                 {t:"rdata", b} (reliable, ordered), {t:"status", s},
//                 {t:"pause"}, {t:"resume"}, {t:"caps", maxSendPerSec, maxBytes},
//                 {t:"follow", k} (a spectator follows seat k)
//   game -> host: {t:"ready", v:1, protocol}, {t:"data", b}, {t:"rdata", b},
//                 {t:"leave"}
// Payloads are only ever copied as bytes: never parsed as HTML, evaluated or
// used to navigate. Over-size or wrong-type messages are dropped.

const MAX_BYTES = 131072;
const MAX_QUEUE = 512;
const S = {
  port: null,
  by: "",
  role: "",
  follow: -1,
  status: "waiting",
  paused: false,
  maxSend: 0,
  maxBytes: MAX_BYTES,
  inbox: [],
  lastReliable: false,
  dropped: 0,
};

function bytesOf(b) {
  if (b instanceof Uint8Array) return b;
  if (b instanceof ArrayBuffer) return new Uint8Array(b);
  return null;
}

function attach(port, opts, by) {
  if (S.port || !port || typeof port.postMessage !== "function") return false;
  const o = opts && typeof opts === "object" ? opts : {};
  S.port = port;
  S.by = by;
  S.role = o.role === "spectator" ? "spectator" : "player";
  S.follow = Number.isInteger(o.follow) ? o.follow : -1;
  S.status = "live";
  port.onmessage = (e) => {
    const d = e.data;
    if (!d || typeof d !== "object") return;
    switch (d.t) {
      case "data":
      case "rdata": {
        const b = bytesOf(d.b);
        if (!b || b.byteLength > S.maxBytes || S.inbox.length >= MAX_QUEUE) {
          S.dropped++;
          return;
        }
        S.inbox.push({ b: b.slice(), r: d.t === "rdata" });
        return;
      }
      case "status":
        if (d.s === "live" || d.s === "reconnecting" || d.s === "closed") S.status = d.s;
        return;
      case "pause":
        S.paused = true;
        return;
      case "resume":
        S.paused = false;
        return;
      case "caps":
        if (Number.isFinite(d.maxSendPerSec) && d.maxSendPerSec >= 0) S.maxSend = d.maxSendPerSec;
        if (Number.isFinite(d.maxBytes) && d.maxBytes > 0) S.maxBytes = Math.min(d.maxBytes, MAX_BYTES);
        return;
      case "follow":
        if (Number.isInteger(d.k) && d.k >= -1 && d.k < 64) S.follow = d.k;
        return;
      default:
    }
  };
  if (typeof port.start === "function") port.start();
  return true;
}

if (typeof window !== "undefined") {
  window.addEventListener("message", (e) => {
    if (window.parent === window || e.source !== window.parent) return;
    const d = e.data;
    if (!d || typeof d !== "object" || d.t !== "hello" || d.v !== 1) return;
    attach((e.ports && e.ports[0]) || d.port, d, "parent");
  });
}

// The general web mode: a host that runs its own server (any host; nothing
// partner-specific) gives the page a server address at start-up, as
// `?evani-server=wss://host/path` (and `&evani-role=spectator`) or
// `globalThis.EVANI_SERVER` (and `EVANI_ROLE`) set before the game loads.
// The page then opens one WebSocket and talks JSON, every message under
// 4 KB: {t:"hello", v:1, role} first, then {t:"data", id, part, of, b} with
// `b` base64 (a payload over 3000 characters is split into parts). With no
// address, or if the socket never opens, nothing is attached and the game
// is the single-player game. Never used when a host frame or an adapter has
// handed a port over.
const WS_CHUNK = 3000;
function b64(u8) {
  let s = "";
  for (let i = 0; i < u8.length; i += 0x8000) s += String.fromCharCode.apply(null, u8.subarray(i, i + 0x8000));
  return btoa(s);
}
function unb64(s) {
  const bin = atob(s);
  const u8 = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) u8[i] = bin.charCodeAt(i);
  return u8;
}
function wsStart() {
  if (typeof location === "undefined" || typeof WebSocket === "undefined") return;
  const q = new URLSearchParams(location.search);
  const given = q.get("evani-server") || globalThis.EVANI_SERVER;
  if (!given || S.port) return;
  let url;
  try {
    url = new URL(String(given), location.href);
  } catch {
    return;
  }
  if (url.protocol !== "ws:" && url.protocol !== "wss:") return;
  const role = (q.get("evani-role") || globalThis.EVANI_ROLE) === "spectator" ? "spectator" : "player";
  let ws;
  try {
    ws = new WebSocket(url.href);
  } catch {
    return;
  }
  const ch = new MessageChannel();
  const parts = new Map();
  let nextId = 1;
  const send = (o) => {
    if (ws.readyState === 1) ws.send(JSON.stringify(o));
  };
  ws.onopen = () => {
    send({ t: "hello", v: 1, role });
    if (!attach(ch.port2, { role }, "ws")) ws.close();
  };
  ws.onmessage = (e) => {
    if (typeof e.data !== "string" || e.data.length > 4096) return;
    let d;
    try {
      d = JSON.parse(e.data);
    } catch {
      return;
    }
    if (!d || typeof d !== "object") return;
    if (d.t === "status" || d.t === "caps" || d.t === "follow" || d.t === "pause" || d.t === "resume") {
      ch.port1.postMessage(d);
      return;
    }
    if (d.t !== "data" || typeof d.b !== "string") return;
    const of = Number.isInteger(d.of) && d.of > 0 && d.of <= 64 ? d.of : 1;
    let text = d.b;
    if (of > 1) {
      const key = String(d.id);
      const p = parts.get(key) || [];
      p[d.part | 0] = d.b;
      parts.set(key, p);
      if (p.filter((x) => typeof x === "string").length < of) return;
      parts.delete(key);
      text = p.join("");
      if (parts.size > 32) parts.clear();
    }
    try {
      ch.port1.postMessage({ t: "data", b: unb64(text) });
    } catch {
      // not base64: dropped
    }
  };
  ws.onclose = () => {
    if (S.by === "ws") ch.port1.postMessage({ t: "status", s: "closed" });
  };
  ch.port1.onmessage = (e) => {
    const d = e.data;
    if (!d || typeof d !== "object") return;
    if (d.t === "data" || d.t === "rdata") {
      const s = b64(d.b);
      const id = nextId++;
      const of = Math.max(1, Math.ceil(s.length / WS_CHUNK));
      for (let k = 0; k < of; k++) send({ t: "data", id, part: k, of, b: s.slice(k * WS_CHUNK, (k + 1) * WS_CHUNK) });
    } else if (d.t === "ready" || d.t === "leave") {
      send(d);
    }
  };
  ch.port1.start();
}

const api = Object.freeze({
  v: 1,
  attachPort(port, opts) {
    return attach(port, opts, "adapter");
  },
});
if (!globalThis.evaniNet) globalThis.evaniNet = api;
const queued = Array.isArray(globalThis.evaniNetPorts) ? globalThis.evaniNetPorts : [];
globalThis.evaniNetPorts = {
  push(q) {
    const [port, opts] = Array.isArray(q) ? q : [q && q.port, q];
    return attach(port, opts, "adapter");
  },
};
for (const q of queued) globalThis.evaniNetPorts.push(q);
wsStart();

export function port_status() {
  return S.status;
}
export function port_role() {
  return S.role;
}
export function port_follow() {
  return S.follow;
}
export function port_paused() {
  return S.paused;
}
export function port_max_send() {
  return S.maxSend;
}
export function port_recv() {
  const m = S.inbox.shift();
  if (!m) return undefined;
  S.lastReliable = m.r;
  return m.b;
}
export function port_last_reliable() {
  return S.lastReliable;
}
export function port_send(bytes, reliable) {
  if (!S.port || S.status === "closed" || bytes.byteLength > S.maxBytes) return false;
  // `bytes` is a view into wasm memory: copy it before it leaves.
  S.port.postMessage({ t: reliable ? "rdata" : "data", b: bytes.slice() });
  return true;
}
export function port_ready(protocol) {
  if (S.port) S.port.postMessage({ t: "ready", v: 1, protocol: protocol >>> 0 });
}
export function port_leave() {
  if (S.port) S.port.postMessage({ t: "leave" });
}
