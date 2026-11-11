// Frame gaps in a live page, for the tier-switch freeze check (capture.mjs
// `--tier-switch`, docs/KIT_PROCESS.md item 30). FRAME_HOOK runs in the page
// before the game loads: it timestamps every presented frame (the canvas's
// getCurrentTexture) and every shader module and render pipeline the page
// creates. A pipeline built at runtime is compiled by the browser's GPU
// process, and while it compiles the page presents nothing: the material
// viewer stopped for 30 to 53 s on each tier it had not shown before
// (2026-10-25). The expressions below read the hook back over CDP.

export const FRAME_HOOK = `(() => {
  if (window.__evaniGaps || typeof GPUDevice === 'undefined') return;
  const L = window.__evaniGaps = { pres: [], ev: [] };
  const now = () => performance.now();
  for (const m of ['createShaderModule', 'createRenderPipeline', 'createRenderPipelineAsync', 'createComputePipeline']) {
    const orig = GPUDevice.prototype[m];
    if (!orig) continue;
    // What was built, so a check can name it: the label, entry points and
    // sample count of a pipeline, the label and source size of a module.
    const what = (d) => (m === 'createShaderModule' ? [d?.label ?? '', d?.code?.length ?? 0] : [d?.label ?? '', d?.vertex?.entryPoint ?? '', d?.fragment?.entryPoint ?? '', d?.multisample?.count ?? 1]).join('|');
    GPUDevice.prototype[m] = function (...a) { L.ev.push([m, now(), what(a[0])]); return orig.apply(this, a); };
  }
  // The main thread's work per frame: each animation-frame callback timed
  // (the game's whole frame runs in one), so a check can tell a heavy
  // frame from a waiting one.
  L.cpu = [];
  const raf = window.requestAnimationFrame.bind(window);
  window.requestAnimationFrame = (cb) => raf((t) => {
    const s = now();
    try { cb(t); } finally {
      L.cpu.push([s, now() - s]);
      if (L.cpu.length > 20000) L.cpu.splice(0, 10000);
    }
  });
  const gct = GPUCanvasContext.prototype.getCurrentTexture;
  GPUCanvasContext.prototype.getCurrentTexture = function (...a) {
    L.pres.push(now());
    if (L.pres.length > 20000) L.pres.splice(0, 10000);
    return gct.apply(this, a);
  };
})();`;

/**
 * Page-side: the main thread's frame work in [from, to], second by second:
 * [second, mean ms, max ms, frames over 20 ms].
 */
export const cpuBySecondExpr = (from, to) => `(() => {
  const L = window.__evaniGaps;
  if (!L || !L.cpu) return null;
  const out = [];
  for (let s = ${from}; s < ${to}; s += 1000) {
    const v = L.cpu.filter(([t]) => t >= s && t < s + 1000).map(([, d]) => d);
    if (!v.length) { out.push([Math.round((s - ${from}) / 1000), null, null, 0]); continue; }
    out.push([Math.round((s - ${from}) / 1000), Math.round(v.reduce((a, b) => a + b, 0) / v.length * 10) / 10, Math.round(Math.max(...v)), v.filter((d) => d > 20).length]);
  }
  return out;
})()`;

/** Page-side: performance.now(). */
export const PAGE_NOW = 'performance.now()';

/**
 * Page-side: whether frames run steadily again after `from` (ms, page time):
 * at least `fps` presented frames in each of the last two whole seconds.
 * Answers { t, steady_at } (steady_at null while not yet).
 */
export const steadyExpr = (from, fps = 50) => `(() => {
  const L = window.__evaniGaps, t = performance.now();
  if (!L) return { t, steady_at: null, hook: false };
  const c = (a, b) => L.pres.filter((x) => x >= a && x < b).length;
  const ok = t - ${from} > 2000 && c(t - 2000, t - 1000) >= ${fps} && c(t - 1000, t) >= ${fps};
  return { t, steady_at: ok ? t - 2000 : null, hook: true };
})()`;

/**
 * Page-side: the longest gap between presented frames in [from, to] (the
 * frame just before `from` counts, so a stop that starts at the switch is
 * measured whole), and what the page created meanwhile.
 */
export const gapExpr = (from, to) => `(() => {
  const L = window.__evaniGaps;
  if (!L) return null;
  const before = L.pres.filter((x) => x < ${from}).pop();
  const a = (before === undefined ? [] : [before]).concat(L.pres.filter((x) => x >= ${from} && x <= ${to}));
  let gap = 0, at = 0, slow = 0;
  for (let i = 1; i < a.length; i++) {
    if (a[i] - a[i - 1] > 25) slow++;
    if (a[i] - a[i - 1] > gap) { gap = a[i] - a[i - 1]; at = a[i - 1] - ${from}; }
  }
  if (a.length && ${to} - a[a.length - 1] > gap) { gap = ${to} - a[a.length - 1]; at = a[a.length - 1] - ${from}; }
  const ev = L.ev.filter(([, t]) => t >= ${from} && t <= ${to});
  return {
    max_gap_ms: Math.round(gap),
    gap_at_ms: Math.round(at),
    frames: a.length,
    // Frames that took over 25 ms (a hitch, short of a stop): how smooth
    // the switch was beyond its longest gap.
    slow_frames: slow,
    pipelines: ev.filter(([k]) => k.startsWith('createRenderPipeline') || k === 'createComputePipeline').length,
    modules: ev.filter(([k]) => k === 'createShaderModule').length,
    built: ev.slice(0, 12).map(([k, t, w]) => k.replace('create', '') + '@' + Math.round(t - ${from}) + 'ms ' + (w ?? '')),
  };
})()`;
