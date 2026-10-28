// evani-host: a reference game server for Evani.Dev multiplayer (Node, no
// dependencies; a dev and test tool, never shipped in a game). Any host can
// run its own the same way; nothing here is specific to one platform.
//
// It loads a game's headless referee (`referee.wasm`, the neutral
// `evani-referee/1` ABI), steps it every 50 ms, and talks to the game pages
// over WebSocket in the general web mode of `evani-net/1` (port.js): JSON,
// every message under 4 KB, bytes as base64, split into parts when needed.
//   page -> server: {t:"hello", v:1, role}, {t:"data", id, part, of, b},
//                   {t:"ready", protocol}, {t:"leave"}
//   server -> page: {t:"data", id, part, of, b}, {t:"status", s}
// A closed socket is a leave; a new socket is a new join (no ids, no
// tokens, no idle kick). Spectators get the spectator view.
//
//   node server.mjs --referee referee.wasm [--port 8111] [--static <game folder>] [--laps 3] [--seed n]
// The game page: http://127.0.0.1:8111/index.html?evani-server=ws://127.0.0.1:8111/
// Stats: GET /stats (JSON).

import { createServer } from "node:http";
import { createHash } from "node:crypto";
import { readFileSync } from "node:fs";
import { readFile, stat } from "node:fs/promises";
import { extname, join, normalize, resolve } from "node:path";

const args = Object.fromEntries(
  process.argv.slice(2).reduce((a, v, i, all) => (v.startsWith("--") ? [...a, [v.slice(2), all[i + 1]]] : a), []),
);
const PORT = Number(args.port || 8111);
const STATIC = args.static ? resolve(args.static) : null;
const MAX_MSG = 4096;
const CHUNK = 3000;

// ---- the referee ----------------------------------------------------------------
const { instance } = await WebAssembly.instantiate(readFileSync(args.referee), {});
const x = instance.exports;
const mem = () => new Uint8Array(x.memory.buffer);
const put = (b) => {
  const p = x.evani_ref_alloc(b.length);
  mem().set(b, p);
  return p;
};
const bytes = (r) => {
  const v = BigInt.asUintN(64, r);
  const p = Number(v >> 32n);
  return mem().slice(p, p + Number(v & 0xffffffffn));
};
if (x.evani_ref_abi() !== 1) throw new Error("not an evani-referee/1 module");
const cfg = new Uint8Array([Number(args.laps || 3)]);
if (x.evani_ref_init(Number(args.seed || 1) >>> 0, 0, put(cfg), cfg.length) !== 0) throw new Error("referee init failed");

// ---- WebSocket (RFC 6455, text frames only, as much as the game needs) ----------
function frame(text) {
  const p = Buffer.from(text);
  const head = p.length < 126 ? Buffer.from([0x81, p.length]) : Buffer.from([0x81, 126, p.length >> 8, p.length & 255]);
  return Buffer.concat([head, p]);
}
function parser(onText, onClose) {
  let buf = Buffer.alloc(0);
  return (chunk) => {
    buf = Buffer.concat([buf, chunk]);
    for (;;) {
      if (buf.length < 2) return;
      const op = buf[0] & 0x0f;
      let len = buf[1] & 0x7f;
      let off = 2;
      if (len === 126) {
        if (buf.length < 4) return;
        len = buf.readUInt16BE(2);
        off = 4;
      } else if (len === 127) return onClose("frame too big");
      const masked = buf[1] & 0x80;
      if (!masked) return onClose("unmasked client frame");
      if (buf.length < off + 4 + len) return;
      const mask = buf.subarray(off, off + 4);
      const data = Buffer.from(buf.subarray(off + 4, off + 4 + len));
      for (let i = 0; i < data.length; i++) data[i] ^= mask[i & 3];
      buf = buf.subarray(off + 4 + len);
      if (op === 8) return onClose("closed");
      if (op === 1) {
        if (data.length > MAX_MSG) return onClose("message over 4 KB");
        onText(data.toString("utf8"));
      }
    }
  };
}

// ---- sessions -------------------------------------------------------------------
const peers = new Map(); // peer number -> {sock, spectator, inbox parts, stats}
let nextPeer = 1;
const stats = { started: Date.now(), steps: 0, maxMsgOut: 0, maxMsgIn: 0, peers: {} };

function sendBytes(p, u8) {
  const s = Buffer.from(u8).toString("base64");
  const of = Math.max(1, Math.ceil(s.length / CHUNK));
  const id = p.nextId++;
  for (let k = 0; k < of; k++) {
    const msg = JSON.stringify({ t: "data", id, part: k, of, b: s.slice(k * CHUNK, (k + 1) * CHUNK) });
    stats.maxMsgOut = Math.max(stats.maxMsgOut, msg.length);
    p.st.down += msg.length;
    p.st.msgsDown++;
    p.sock.write(frame(msg));
  }
}

function onMessage(peer, text) {
  const p = peers.get(peer);
  if (!p) return;
  stats.maxMsgIn = Math.max(stats.maxMsgIn, text.length);
  p.st.up += text.length;
  p.st.msgsUp++;
  let d;
  try {
    d = JSON.parse(text);
  } catch {
    return;
  }
  if (!d || typeof d !== "object") return;
  if (d.t === "hello" && !p.joined) {
    p.spectator = d.role === "spectator";
    p.seat = x.evani_ref_join(peer, p.spectator ? 1 : 0);
    p.joined = true;
    p.st.seat = p.seat;
    return;
  }
  if (d.t === "ready") {
    p.st.protocol = (d.protocol >>> 0).toString(16);
    return;
  }
  if (d.t === "leave") {
    x.evani_ref_leave(peer);
    return;
  }
  if (d.t !== "data" || typeof d.b !== "string" || !p.joined) return;
  const of = Number.isInteger(d.of) && d.of > 0 && d.of <= 8 ? d.of : 1;
  let s = d.b;
  if (of > 1) {
    const got = p.parts.get(d.id) || [];
    got[d.part | 0] = d.b;
    p.parts.set(d.id, got);
    if (got.filter((v) => typeof v === "string").length < of) return;
    p.parts.delete(d.id);
    s = got.join("");
  }
  const b = new Uint8Array(Buffer.from(s, "base64"));
  if (x.evani_ref_input(peer, put(b), b.length) !== 0) p.st.refused++;
}

setInterval(() => {
  x.evani_ref_step(3);
  stats.steps++;
  let spec = null;
  for (const [peer, p] of peers) {
    if (!p.joined) continue;
    if (p.spectator || p.seat === 255) {
      spec = spec || bytes(x.evani_ref_view(0xffffffff));
      sendBytes(p, spec);
    } else {
      sendBytes(p, bytes(x.evani_ref_view(peer)));
    }
  }
}, 50);

// ---- HTTP: static files, stats, upgrade ---------------------------------------------
const types = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".css": "text/css", ".png": "image/png", ".glb": "model/gltf-binary", ".txt": "text/plain" };
const server = createServer(async (req, res) => {
  const url = new URL(req.url, "http://x");
  if (url.pathname === "/stats") {
    const secs = (Date.now() - stats.started) / 1000;
    res.writeHead(200, { "content-type": "application/json" });
    res.end(JSON.stringify({ ...stats, secs, tick: Number(x.evani_ref_tick()), hash: Number(x.evani_ref_hash()) }));
    return;
  }
  if (STATIC) {
    try {
      const p = resolve(join(STATIC, normalize(decodeURIComponent(url.pathname)) || "index.html"));
      const f = p.endsWith("\\") || p.endsWith("/") || p === STATIC ? join(p, "index.html") : p;
      if (f.startsWith(STATIC) && (await stat(f)).isFile()) {
        res.writeHead(200, { "content-type": types[extname(f)] || "application/octet-stream", "cache-control": "no-store" });
        res.end(await readFile(f));
        return;
      }
    } catch {
      // fall through
    }
  }
  res.writeHead(404);
  res.end("not found");
});
server.on("upgrade", (req, sock) => {
  const key = req.headers["sec-websocket-key"];
  if (!key || req.headers.upgrade?.toLowerCase() !== "websocket") return sock.destroy();
  const accept = createHash("sha1").update(key + "258EAFA5-E914-47DA-95CA-C5AB0DC85B11").digest("base64");
  sock.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`);
  sock.setNoDelay(true);
  const peer = nextPeer++;
  const st = { up: 0, down: 0, msgsUp: 0, msgsDown: 0, refused: 0, seat: null, since: Date.now() };
  stats.peers[peer] = st;
  peers.set(peer, { sock, spectator: false, joined: false, seat: null, parts: new Map(), nextId: 1, st });
  const close = () => {
    if (peers.delete(peer)) x.evani_ref_leave(peer);
    st.closed = Date.now();
    sock.destroy();
  };
  sock.on("data", parser((t) => onMessage(peer, t), close));
  sock.on("close", close);
  sock.on("error", close);
});
server.listen(PORT, "127.0.0.1", () => console.log(`evani-host on ws://127.0.0.1:${PORT}/ (static: ${STATIC ?? "none"})`));
