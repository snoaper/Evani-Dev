// Smoothness analysis: pure functions, no browser (tests: smoothness.test.mjs).
//
// Ported from APEX Velocity's stutter harness (its .evani/tmp/stutter/
// analyze.mjs and chart.mjs, 2026-11-11), made genre-agnostic:
//
// - Motion steadiness. For each drawn frame, the tracked thing's speed on that
//   frame (distance moved since the previous drawn frame, divided by the time
//   between them) over the mean speed of its neighbouring frames (+-7 frames,
//   within 140 ms). Steady motion stays at 1; judder shows as frames near 0
//   (held still) and frames well above 1 (catching up). Dividing by the
//   neighbours, not by a fixed speed, means a game that runs slower or faster
//   overall is not counted as judder. Only frames moving faster than
//   `min_speed` count (a parked car has no steadiness to measure).
// - Frame pacing. The time between presented frames (each canvas
//   getCurrentTexture, tools/live-check/frame-gaps.mjs) against the 60 Hz
//   target (16.7 ms; 8.3 ms when the display runs at 120 Hz).

/** Neighbour window: frames each side, and the most time apart they may be. */
export const NEIGHBOURS = 7;
export const NEIGHBOUR_MS = 140;
/** A frame is "uneven" when its ratio is outside this band (APEX's band). */
export const EVEN_BAND = [0.6, 1.4];
/** Frames further apart than this are a stall, not motion: skipped. */
export const MAX_FRAME_GAP_MS = 50;

/**
 * Default pass/fail limits. From the APEX fixed build's four runs (1280 and 2560 px,
 * 0 and 20 earlier visits, relay delay and jitter; .evani/perf/stutter/report.json in
 * APEX, re-analysed with this file: see DECISIONS.md "Smoothness harness") with small
 * headroom. Worst of the four: uneven 0.038 %, ratio deviation p99 0.042, ratio SD 0.024,
 * longest frame 116.6 ms, hitches (frames over 33.3 ms) 1.
 */
export const DEFAULT_THRESHOLDS = Object.freeze({
  uneven_pct_max: 0.1,
  ratio_dev_p99_max: 0.06,
  ratio_sd_max: 0.04,
  frame_max_ms: 150,
  hitches_max: 2,
});

export const THRESHOLD_TEXT = {
  uneven_pct_max: 'uneven frames (ratio outside 0.6..1.4), % of moving frames',
  ratio_dev_p99_max: 'displacement-ratio deviation from 1, 99th percentile',
  ratio_sd_max: 'displacement-ratio standard deviation',
  frame_max_ms: 'longest time between presented frames, ms',
  hitches_max: 'hitches: presented frames more than twice the target apart',
};

/** Quantile of plain numbers (nearest rank, as APEX). NaN when empty. */
export function quantile(values, p) {
  if (!values.length) return NaN;
  const s = values.slice().sort((a, b) => a - b);
  return s[Math.min(s.length - 1, Math.floor(p * (s.length - 1) + 0.5))];
}

const r3 = (v) => (Number.isFinite(v) ? Math.round(v * 1000) / 1000 : null);

/**
 * Per-frame speeds from motion samples `[t_ms, frame, x, y, z, sx, sy]`
 * (motion.read rows). `metric` "world" uses x, y, z; "screen" uses sx, sy.
 * Returns `[[t_ms, speed], ...]` for frames that moved faster than
 * `minSpeed` (units a second) and followed their previous frame within
 * MAX_FRAME_GAP_MS.
 */
export function speeds(samples, { metric = 'world', minSpeed = 0, from = -Infinity, to = Infinity } = {}) {
  const pos = (s) => (metric === 'screen' ? (s[5] == null ? null : [s[5], s[6]]) : s[2] == null ? null : [s[2], s[3], s[4]]);
  const out = [];
  let prev = null;
  for (const s of samples) {
    if (s[0] < from || s[0] > to) continue;
    const p = pos(s);
    if (!p) {
      prev = null;
      continue;
    }
    if (prev) {
      const dt = (s[0] - prev.t) / 1000;
      if (dt > 0 && dt * 1000 <= MAX_FRAME_GAP_MS) {
        const d = Math.hypot(...p.map((v, i) => v - prev.p[i]));
        out.push([s[0], d / dt]);
      }
    }
    prev = { t: s[0], p };
  }
  // The speed filter judges the neighbourhood, not the frame itself: a held
  // frame (speed 0) in the middle of fast motion is exactly the judder to count.
  const smooth = neighbourMeans(out);
  return out.filter((_, k) => smooth[k] !== null && smooth[k] > minSpeed);
}

/** Mean speed of each frame's neighbours (itself included), or null when too few. */
function neighbourMeans(sp) {
  return sp.map((s, k) => {
    let sum = 0;
    let n = 0;
    for (let j = Math.max(0, k - NEIGHBOURS); j <= Math.min(sp.length - 1, k + NEIGHBOURS); j++) {
      if (Math.abs(sp[j][0] - s[0]) < NEIGHBOUR_MS) {
        sum += sp[j][1];
        n++;
      }
    }
    return n >= 9 ? sum / n : null;
  });
}

/** Each frame's speed over its neighbours' mean: `[[t_ms, ratio], ...]`. */
export function ratios(sp) {
  const m = neighbourMeans(sp);
  const out = [];
  for (let k = 0; k < sp.length; k++) if (m[k] !== null && m[k] > 0) out.push([sp[k][0], sp[k][1] / m[k]]);
  return out;
}

/** Intervals between presented frames in [from, to]: `[[t_ms, interval_ms], ...]`. */
export function intervals(pres, from = -Infinity, to = Infinity) {
  const p = pres.filter((t) => t >= from && t <= to);
  return p.slice(1).map((t, i) => [t, t - p[i]]);
}

/** Summary of one run (`trace` from run.mjs). */
export function summarise(trace, { metric = 'world', minSpeed = 0 } = {}) {
  const from = trace.mark0;
  const to = trace.mark1;
  const iv = intervals(trace.pres || [], from, to);
  const ft = iv.map((v) => v[1]);
  const target = quantile(ft, 0.5) < 12 ? 1000 / 120 : 1000 / 60;
  const motion = trace.motion && trace.motion.length ? trace.motion : null;
  const sp = motion ? speeds(motion, { metric, minSpeed, from, to }) : [];
  const ra = ratios(sp);
  const rv = ra.map((r) => r[1]);
  const mean = rv.reduce((s, v) => s + v, 0) / Math.max(1, rv.length);
  const sd = Math.sqrt(rv.reduce((s, v) => s + (v - mean) ** 2, 0) / Math.max(1, rv.length));
  const dev = rv.map((v) => Math.abs(v - 1));
  // The longest gap, and how much of it was the page's own work (an
  // animation-frame callback running inside it): little work means the
  // browser, the GPU or the machine held the page, not the game.
  let longest = null;
  for (const [t, d] of iv) if (!longest || d > longest.ms) longest = { ms: d, end: t };
  let pageMs = null;
  if (longest && trace.cpu) {
    const start = longest.end - longest.ms;
    pageMs = trace.cpu.filter(([t]) => t >= start - 1 && t < longest.end).reduce((m, [, d]) => Math.max(m, d), 0);
  }
  return {
    label: trace.label,
    build: trace.build,
    width: trace.width,
    net: trace.net || 'none',
    seconds: r3((to - from) / 1000),
    frames: {
      count: ft.length,
      target_ms: r3(target),
      p50_ms: r3(quantile(ft, 0.5)),
      p95_ms: r3(quantile(ft, 0.95)),
      p99_ms: r3(quantile(ft, 0.99)),
      max_ms: r3(ft.length ? Math.max(...ft) : NaN),
      longest_at_s: longest ? r3((longest.end - longest.ms - from) / 1000) : null,
      longest_page_work_ms: pageMs === null ? null : r3(pageMs),
      hitches: ft.filter((v) => v > 2 * target).length,
      over_50ms: ft.filter((v) => v > 50).length,
    },
    motion: motion
      ? {
          probe: trace.probe || null,
          metric,
          min_speed: minSpeed,
          samples: motion.length,
          frames: rv.length,
          uneven_pct: r3((100 * rv.filter((v) => v < EVEN_BAND[0] || v > EVEN_BAND[1]).length) / Math.max(1, rv.length)),
          ratio_sd: r3(rv.length ? sd : NaN),
          ratio_p5: r3(quantile(rv, 0.05)),
          ratio_p95: r3(quantile(rv, 0.95)),
          ratio_dev_p99: r3(quantile(dev, 0.99)),
          ratio_dev_max: r3(dev.length ? Math.max(...dev) : NaN),
          mean_speed: r3(sp.length ? sp.reduce((s, v) => s + v[1], 0) / sp.length : NaN),
        }
      : null,
  };
}

/**
 * Judge a summary against limits. Returns `{ pass, failures: [{ key, value, limit, text }] }`.
 * A run with too few moving frames fails (nothing was measured), unless
 * `requireMotion` is false.
 */
export function judge(summary, thresholds = DEFAULT_THRESHOLDS, { requireMotion = true, minFrames = 60 } = {}) {
  const t = { ...DEFAULT_THRESHOLDS, ...thresholds };
  const failures = [];
  const over = (key, value) => {
    if (value === null || value === undefined || Number.isNaN(value)) return;
    if (value > t[key]) failures.push({ key, value, limit: t[key], text: THRESHOLD_TEXT[key] });
  };
  const m = summary.motion;
  if (m && m.frames >= minFrames) {
    over('uneven_pct_max', m.uneven_pct);
    over('ratio_dev_p99_max', m.ratio_dev_p99);
    over('ratio_sd_max', m.ratio_sd);
  } else if (requireMotion) {
    failures.push({
      key: 'motion',
      value: m ? m.frames : 0,
      limit: minFrames,
      text: m ? `too few moving frames to judge (min_speed ${m.min_speed}; is the probe moving?)` : 'no motion trace (the build has no live module or no motion.trace command)',
    });
  }
  over('frame_max_ms', summary.frames.max_ms);
  over('hitches_max', summary.frames.hitches);
  return { pass: failures.length === 0, failures };
}

/**
 * Chart series for the report: the displacement ratio over `window_s`
 * seconds starting `start_s` into the run (or at the first data after it),
 * and every frame interval. Times are seconds from the start of the run.
 */
export function chartSeries(trace, { metric = 'world', minSpeed = 0, start_s = 2, window_s = 6 } = {}) {
  const a = trace.mark0;
  const motion = trace.motion && trace.motion.length ? trace.motion : [];
  const ra = ratios(speeds(motion, { metric, minSpeed, from: trace.mark0, to: trace.mark1 })).map((r) => [(r[0] - a) / 1000, r[1]]);
  const first = ra.find((r) => r[0] >= start_s) || ra[0];
  const t0 = first ? first[0] : 0;
  return {
    label: trace.label,
    ratio: ra.filter((r) => r[0] >= t0 && r[0] < t0 + window_s).map((r) => [r3(r[0] - t0), r3(r[1])]),
    ft: intervals(trace.pres || [], trace.mark0, trace.mark1).map((r) => [r3((r[0] - a) / 1000), Math.round(r[1] * 100) / 100]),
  };
}
