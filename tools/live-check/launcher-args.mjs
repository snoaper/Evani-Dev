// Launcher argument test (no server, no build, no network):
//   node tools/live-check/launcher-args.mjs
//
// Copies .vibedeck/dev-harness/live.mjs into a pack-shaped temp folder
// (<pack>/manifest.json, <pack>/tools/bin/evani-live-bridge(.exe), <pack>/engine/...)
// and asks it, with --print-command, which bridge command line it would run.
// Checks the --game form, the --app-dir form (+ --session) and the refusal.
// Evani.App starts live through this launcher, so bridge flag changes stay ours
// (docs/ENGINE_PACK.md 4.3).
import { spawnSync } from "node:child_process";
import assert from "node:assert/strict";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const launcher = path.resolve(here, "..", "..", ".vibedeck", "dev-harness", "live.mjs");
const tmp = fs.mkdtempSync(path.join(os.tmpdir(), "evani-launcher-args-"));
try {
  const pack = path.join(tmp, "pack");
  const harness = path.join(pack, "engine", ".vibedeck", "dev-harness");
  const bridgeName = process.platform === "win32" ? "evani-live-bridge.exe" : "evani-live-bridge";
  fs.mkdirSync(harness, { recursive: true });
  fs.mkdirSync(path.join(pack, "tools", "bin"), { recursive: true });
  fs.writeFileSync(path.join(pack, "manifest.json"), "{}");
  fs.writeFileSync(path.join(pack, "tools", "bin", bridgeName), "");
  fs.copyFileSync(launcher, path.join(harness, "live.mjs"));
  const engine = path.join(pack, "engine");
  const bridge = path.join(pack, "tools", "bin", bridgeName);
  const run = (args, cwd = tmp) =>
    spawnSync(process.execPath, [path.join(harness, "live.mjs"), "--print-command", ...args], { cwd, encoding: "utf8" });
  const plan = (args, cwd) => {
    const r = run(args, cwd);
    assert.equal(r.status, 0, r.stderr);
    return JSON.parse(r.stdout.trim());
  };

  // --game form (the engine's own example)
  let p = plan(["--port", "8123", "--game", "sandbox", "--quiet"]);
  assert.equal(path.resolve(p.exe), bridge);
  assert.deepEqual(p.args, ["serve", "--root", engine, "--port", "8123", "--game", "sandbox", "--quiet"]);

  // no game: the bridge's own default (sandbox) applies
  p = plan(["--port", "8123"]);
  assert.deepEqual(p.args, ["serve", "--root", engine, "--port", "8123"]);

  // --app-dir form with --session, absolute paths pass through unchanged
  const app = path.join(tmp, "Games", "mine", "apps", "mine");
  const sess = path.join(tmp, "state", "live", "session.json");
  p = plan(["--port", "8124", "--app-dir", app, "--session", sess, "--quiet"]);
  assert.deepEqual(p.args, ["serve", "--root", engine, "--port", "8124", "--app-dir", app, "--session", sess, "--quiet"]);

  // `=` forms, and relative paths become absolute from the launcher's working folder
  p = plan(["--port=8125", "--app-dir=games/x", "--session=s/session.json"], tmp);
  assert.deepEqual(p.args, ["serve", "--root", engine, "--port", "8125",
    "--app-dir", path.join(tmp, "games", "x"), "--session", path.join(tmp, "s", "session.json")]);

  // --game with --session (the example game, session in the state folder)
  p = plan(["--port", "8126", "--game", "sandbox", "--session", sess]);
  assert.deepEqual(p.args, ["serve", "--root", engine, "--port", "8126", "--game", "sandbox", "--session", sess]);

  // both --game and --app-dir: refused before anything starts
  const both = run(["--game", "sandbox", "--app-dir", app]);
  assert.equal(both.status, 2);
  assert.match(both.stderr, /not both/);

  console.log("launcher-args: ok (6 checks)");
} finally {
  fs.rmSync(tmp, { recursive: true, force: true });
}
