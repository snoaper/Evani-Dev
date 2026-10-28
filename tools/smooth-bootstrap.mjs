// Packaging step for a published playground (docs/PUBLISHING.md): rewrites the Trunk bootstrap in
// <folder>/index.html so the engine starts after the loader's logo has dropped in.
//   node smooth-bootstrap.mjs publish/kart-racer publish/match-3 ...
// Safe to run twice. Only the inline `import init ... await init(...)` module script changes.
import fs from 'node:fs';

const MARK = '// evani-smooth-start';
for (const dir of process.argv.slice(2)) {
  const file = dir.replace(/[\/]+$/, '') + '/index.html';
  const html = fs.readFileSync(file, 'utf8');
  if (html.includes(MARK)) { console.log(file + ': already done'); continue; }
  const m = html.match(/import init, \* as bindings from '(\.\/[^']+\.js)';\r?\nconst wasm = await init\(\{ module_or_path: '(\.\/[^']+\.wasm)' \}\);/);
  if (!m) throw new Error(file + ': the Trunk bootstrap was not found (is it a ship build with relative ./ paths?)');
  const boot = `import init, * as bindings from '${m[1]}';
${MARK}
// Fetch and compile the wasm at once (off the main thread), but start the engine only after the loader's logo
// has landed: engine start-up on the main thread and GPU used to stall the logo's entrance. Then hold the
// loader until frames flow again, so it fades to the game and not to an empty canvas. Without the loader's
// landed and hold (an older loader) the game starts at once, as before.
const wasmUrl = '${m[2]}';
const compiled = WebAssembly.compileStreaming(fetch(wasmUrl))
  .catch(() => fetch(wasmUrl).then((r) => r.arrayBuffer()).then((b) => WebAssembly.compile(b)));
const loader = window.EvaniLoader;
const release = loader && loader.landed && loader.hold ? loader.hold() : null;
if (release) await loader.landed;
let wasm;
try {
  wasm = await init({ module_or_path: await compiled });
  window.wasmBindings = bindings;
  dispatchEvent(new CustomEvent("TrunkApplicationStarted", {detail: {wasm}}));
  if (release) {
    await new Promise((done) => {
      const began = performance.now();
      let last = began;
      let smooth = 0;
      const step = (t) => {
        smooth = t - last < 34 ? smooth + 1 : 0;
        last = t;
        if ((smooth >= 5 && t - began > 400) || t - began > 4000) done();
        else requestAnimationFrame(step);
      };
      requestAnimationFrame(step);
      setTimeout(done, 4500);
    });
  }
} finally {
  if (release) release();
}`;
  // keep the rest of the original script (window.wasmBindings / TrunkApplicationStarted are now in boot)
  const rest = html.slice(m.index + m[0].length).replace(/^\s*\r?\n\s*\r?\n\s*window\.wasmBindings = bindings;\s*\r?\n\s*\r?\n\s*\r?\n\s*dispatchEvent\(new CustomEvent\("TrunkApplicationStarted", \{detail: \{wasm\}\}\)\);\s*\r?\n/, '\n');
  if (rest === html.slice(m.index + m[0].length)) throw new Error(file + ': unexpected text after the bootstrap');
  fs.writeFileSync(file, html.slice(0, m.index) + boot + rest);
  console.log(file + ': bootstrap rewritten');
}
