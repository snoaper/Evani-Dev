// Evani.Dev launcher for the Evani.dev tab (docs/LIVE_API.md 2.1).
//
// The tab runs:  <node> .vibedeck/dev-harness/live.mjs --port <P> --quiet
// with cwd = this folder, and waits up to 60 s for GET /live/status.
//
// Node built-ins only (the tab runs this with Electron's Node).
//
// 1. Engine root = ../.. from this file.
// 2. In an engine pack, run <pack>/tools/bin/evani-live-bridge as is.
//    Otherwise, if target/debug/evani-live-bridge(.exe) is missing or older than the
//    bridge sources or Cargo.lock, answer /live/status (phase "building")
//    and / (a holding page) on the port while `cargo build` compiles the
//    bridge, so a first-time compile never trips the tab's 60 s wait. Then
//    free the port.
// 3. Run the bridge: serve --root <root> --port <P> [--game <g> | --app-dir <app>]
//    [--session <file>] [--quiet], stdio inherited (so Evani.App's one look-guard
//    key line on stdin reaches the bridge untouched), and exit with its exit code.
//    Stopping this process stops the bridge.
//
// Options (docs/ENGINE_PACK.md 4.3):
//   --port <P>         the port the tab waits on
//   --game <name>      one of the engine's own example games (default: sandbox)
//   --app-dir <folder> a game kept outside the engine: its app folder (the one with
//                      its evani.toml). Not with --game. Relative paths are taken
//                      from this launcher's working folder, then passed absolute.
//   --session <file>   where the bridge writes its session file (a pack's
//                      <state>/live/session.json); same path rule
//   --quiet
//   --print-command    print {"exe","args","cwd"} as one JSON line and exit, without
//                      compiling or starting anything (used by the launcher test,
//                      tools/live-check/launcher-args.mjs)

import { spawn } from "node:child_process";
import fs from "node:fs";
import http from "node:http";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const root = path.resolve(here, "..", "..");
const bridgeName = process.platform === "win32" ? "evani-live-bridge.exe" : "evani-live-bridge";
// In an engine pack (Evani.App's bundled, read-only engine: <pack>/engine is
// this root, <pack>/manifest.json beside it) the prebuilt bridge in
// <pack>/tools/bin runs as is; nothing is compiled or written into the pack
// (docs/ENGINE_PACK.md).
const packBridge = path.join(root, "..", "tools", "bin", bridgeName);
const inPack = fs.existsSync(path.join(root, "..", "manifest.json")) && fs.existsSync(packBridge);
const exe = inPack ? packBridge : path.join(root, "target", "debug", bridgeName);

// ---- arguments (unknown flags are ignored) ---------------------------------------
let port = 8096;
let quiet = false;
let game = null;
let appDir = null;
let session = null;
let printCommand = false;
const argv = process.argv.slice(2);
for (let i = 0; i < argv.length; i++) {
  if (argv[i] === "--port" && argv[i + 1]) port = Number(argv[++i]) || 8096;
  else if (argv[i].startsWith("--port=")) port = Number(argv[i].slice(7)) || 8096;
  else if (argv[i] === "--game" && argv[i + 1]) game = argv[++i];
  else if (argv[i].startsWith("--game=")) game = argv[i].slice(7) || null;
  else if (argv[i] === "--app-dir" && argv[i + 1]) appDir = path.resolve(argv[++i]);
  else if (argv[i].startsWith("--app-dir=")) appDir = argv[i].slice(10) ? path.resolve(argv[i].slice(10)) : null;
  else if (argv[i] === "--session" && argv[i + 1]) session = path.resolve(argv[++i]);
  else if (argv[i].startsWith("--session=")) session = argv[i].slice(10) ? path.resolve(argv[i].slice(10)) : null;
  else if (argv[i] === "--print-command") printCommand = true;
  else if (argv[i] === "--quiet") quiet = true;
}
if (game && appDir) {
  console.error("[evani-live] give --game or --app-dir, not both (--app-dir is the game's own folder)");
  process.exit(2);
}
const log = (msg) => { if (!quiet) console.log(`[evani-live] ${msg}`); };

// ---- is the prebuilt bridge current? ---------------------------------------------
function newestMtime(p) {
  let st;
  try { st = fs.statSync(p); } catch { return 0; }
  if (!st.isDirectory()) return st.mtimeMs;
  let newest = 0;
  for (const name of fs.readdirSync(p)) newest = Math.max(newest, newestMtime(path.join(p, name)));
  return newest;
}

function bridgeIsCurrent() {
  if (inPack) return true;
  let built;
  try { built = fs.statSync(exe).mtimeMs; } catch { return false; }
  const sources = Math.max(
    newestMtime(path.join(root, "tools", "evani-live-bridge")),
    // The bridge links evani-live (bake planner) and through it evani-core.
    newestMtime(path.join(root, "crates", "evani-live")),
    newestMtime(path.join(root, "crates", "evani-core")),
    newestMtime(path.join(root, "Cargo.lock")),
  );
  return built >= sources;
}

// ---- holding server while cargo compiles the bridge ------------------------------
const holdingPage = `<!doctype html><html><head><meta charset="utf-8"><title>Evani.Dev: starting</title>
<style>body{font:16px system-ui,sans-serif;background:#111;color:#ddd;display:grid;place-items:center;height:100vh;margin:0}
pre{max-width:90vw;white-space:pre-wrap;color:#f99;font-size:12px}</style></head>
<body><div><p id="m">Preparing the live server (first start compiles it, this can take a few minutes)...</p><pre id="e"></pre></div>
<script>
async function poll(){
  try{
    const s=await (await fetch('/live/status',{cache:'no-store'})).json();
    if(!s.launcher){location.reload();return;}
    if(s.phase==='error'){document.getElementById('m').textContent='The live server did not build.';document.getElementById('e').textContent=s.error||'';}
  }catch(_){}
  setTimeout(poll,1000);
}
poll();
</script></body></html>`;

function holdingStatus(phase, error) {
  return JSON.stringify({
    type: "live_hello", build: 0, url: null, phase, error, build_ms: 0,
    relay: `ws://localhost:${port}`, engine: "evani.dev", protocol: "evani-live/1",
    root, game: "sandbox", engine_connected: false, tier: null,
    launcher: "building-bridge",
  });
}

function startHolding() {
  const state = { phase: "building", error: null };
  const server = http.createServer((req, res) => {
    const url = (req.url || "/").split("?")[0];
    if (url === "/live/status") {
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      res.end(holdingStatus(state.phase, state.error));
    } else if (req.method === "GET" && url === "/") {
      res.writeHead(200, { "content-type": "text/html; charset=utf-8", "cache-control": "no-store" });
      res.end(holdingPage);
    } else {
      res.writeHead(503, { "content-type": "text/plain" });
      res.end("The live server is still starting.");
    }
  });
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => resolve({ server, state }));
  });
}

function closeHolding(server) {
  return new Promise((resolve) => {
    server.close(() => resolve());
    server.closeAllConnections?.();
  });
}

// ---- child processes --------------------------------------------------------------
let child = null;
function stopChild() {
  if (child && child.exitCode === null) {
    try { child.kill(); } catch { /* already gone */ }
  }
}
for (const sig of ["SIGINT", "SIGTERM", "SIGBREAK", "SIGHUP"]) {
  try { process.on(sig, () => { stopChild(); process.exit(130); }); } catch { /* not on this OS */ }
}
process.on("exit", stopChild);

function belowNormal(pid) {
  try { os.setPriority(pid, os.constants.priority.PRIORITY_BELOW_NORMAL); } catch { /* best effort */ }
}

function buildBridge() {
  return new Promise((resolve) => {
    let tail = "";
    child = spawn("cargo", ["build", "-q", "-j", "4", "-p", "evani-live-bridge"], {
      cwd: root,
      stdio: ["ignore", "inherit", "pipe"],
      windowsHide: true,
    });
    belowNormal(child.pid);
    child.stderr.on("data", (d) => {
      if (!quiet) process.stderr.write(d);
      tail = (tail + d.toString()).slice(-4000);
    });
    child.on("error", (e) => resolve({ code: 1, tail: `cannot run cargo: ${e.message}` }));
    child.on("exit", (code) => resolve({ code: code ?? 1, tail }));
  });
}

// The bridge's command line: serve --root <root> --port <P>, then --game or --app-dir,
// then --session and --quiet (the order the tab and docs/ENGINE_PACK.md use).
function bridgeArgs() {
  const args = ["serve", "--root", root, "--port", String(port)];
  if (game) args.push("--game", game);
  if (appDir) args.push("--app-dir", appDir);
  if (session) args.push("--session", session);
  if (quiet) args.push("--quiet");
  return args;
}

async function main() {
  if (printCommand) {
    console.log(JSON.stringify({ exe, args: bridgeArgs(), cwd: root }));
    return;
  }
  if (!bridgeIsCurrent()) {
    let holding;
    try {
      holding = await startHolding();
    } catch (e) {
      console.error(`[evani-live] port ${port} is busy (${e.code || e.message})`);
      process.exit(1);
    }
    log(`compiling the live bridge (holding page on http://127.0.0.1:${port}/)`);
    const { code, tail } = await buildBridge();
    child = null;
    if (code !== 0) {
      // Show the error on the holding page for a while, then exit so the
      // tab's "Try again" starts a fresh attempt.
      holding.state.phase = "error";
      holding.state.error = tail.trim() || `cargo build exited with ${code}`;
      console.error(`[evani-live] the bridge did not build:\n${holding.state.error}`);
      await new Promise((r) => setTimeout(r, 30_000));
      await closeHolding(holding.server);
      process.exit(code);
    }
    await closeHolding(holding.server);
  }

  const args = bridgeArgs();
  log(`starting ${path.relative(root, exe)} ${args.join(" ")}`);
  child = spawn(exe, args, { cwd: root, stdio: "inherit", windowsHide: true });
  child.on("error", (e) => {
    console.error(`[evani-live] cannot start the bridge: ${e.message}`);
    process.exit(1);
  });
  child.on("exit", (code, signal) => process.exit(code ?? (signal ? 1 : 0)));
}

main();
