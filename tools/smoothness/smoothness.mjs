#!/usr/bin/env node
// Evani.Dev smoothness harness: frame pacing and motion steadiness of a built
// game, optionally before and after a change. docs/SMOOTHNESS.md; usually run
// through `cargo xtask smoothness <id>`.
//
//   node tools/smoothness/smoothness.mjs --config <smoothness.json>
//        (--build <dir> | --candidate <dir> [--baseline <dir>])
//        [--out <dir>] [--net none|lan|wan|bad] [--seed n] [--duration s] [--width px]
//        [--title text] [--baseline-label text] [--candidate-label text]
//        [--limit key=value ...] [--no-fail]
//
// Writes <out>/result.json, report.html, trace-<before|after>.json and
// shot-<before|after>.png. Exit code: 0 the candidate (or the one build) is
// within the limits, 1 it is over a limit, 2 the run could not be made.

import { mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { dirname, join, resolve } from 'node:path';
import { pathToFileURL } from 'node:url';
import { DEFAULT_THRESHOLDS, chartSeries, judge, summarise } from './analyze.mjs';
import { html } from './report.mjs';
import { runOnce } from './run.mjs';

export function parseArgs(argv) {
  const o = { limits: {}, fail: true };
  const need = (i, k) => {
    if (i + 1 >= argv.length) throw new Error(`${k} needs a value`);
    return argv[i + 1];
  };
  for (let i = 0; i < argv.length; i++) {
    const a = argv[i];
    switch (a) {
      case '--config': o.config = need(i++, a); break;
      case '--build': o.candidate = need(i++, a); break;
      case '--candidate': o.candidate = need(i++, a); break;
      case '--baseline': o.baseline = need(i++, a); break;
      case '--out': o.out = need(i++, a); break;
      case '--net': o.net = need(i++, a); break;
      case '--seed': o.seed = Number(need(i++, a)); break;
      case '--duration': o.duration = Number(need(i++, a)); break;
      case '--width': o.width = Number(need(i++, a)); break;
      case '--height': o.height = Number(need(i++, a)); break;
      case '--title': o.title = need(i++, a); break;
      case '--baseline-label': o.baselineLabel = need(i++, a); break;
      case '--candidate-label': o.candidateLabel = need(i++, a); break;
      case '--referee': o.referee = need(i++, a); break;
      case '--no-fail': o.fail = false; break;
      case '--limit': {
        const [k, v] = need(i++, a).split('=');
        if (!(k in DEFAULT_THRESHOLDS) || !Number.isFinite(Number(v))) throw new Error(`--limit ${k}=${v}: known limits are ${Object.keys(DEFAULT_THRESHOLDS).join(', ')}`);
        o.limits[k] = Number(v);
        break;
      }
      default:
        throw new Error(`unknown option ${a}`);
    }
  }
  if (!o.config) throw new Error('--config <smoothness.json> is required');
  if (!o.candidate) throw new Error('--build <dir> (or --candidate <dir>) is required');
  return o;
}

/** The config file plus command-line overrides. */
export function loadConfig(o) {
  const cfg = JSON.parse(readFileSync(o.config, 'utf8'));
  const base = dirname(resolve(o.config));
  if (cfg.net && cfg.net.referee) cfg.net.referee = resolve(base, cfg.net.referee);
  if (cfg.net && o.referee) cfg.net.referee = resolve(o.referee);
  if (o.duration) cfg.duration_s = o.duration;
  if (o.width) cfg.width = o.width;
  if (o.height) cfg.height = o.height;
  cfg.thresholds = { ...DEFAULT_THRESHOLDS, ...(cfg.thresholds || {}), ...o.limits };
  return cfg;
}

export async function main(argv) {
  const o = parseArgs(argv);
  const cfg = loadConfig(o);
  const out = resolve(o.out || join('.evani', 'smoothness', cfg.game || 'game'));
  mkdirSync(out, { recursive: true });
  const net = o.net || 'none';
  const seed = o.seed ?? 1;
  const metric = cfg.metric || 'world';
  const minSpeed = cfg.min_speed ?? 0;
  const builds = [];
  if (o.baseline) builds.push({ key: 'before', dir: o.baseline, name: 'Before', legend: o.baselineLabel || `Before: ${o.baseline}` });
  builds.push({ key: 'after', dir: o.candidate, name: o.baseline ? 'After' : 'This build', legend: o.candidateLabel || `${o.baseline ? 'After' : 'This build'}: ${o.candidate}` });
  const runs = [];
  for (const b of builds) {
    console.log(`smoothness: ${b.name} (${b.dir}), ${cfg.duration_s ?? 20} s at ${cfg.width ?? 1280} px, net ${net}`);
    const trace = await runOnce({ build: b.dir, label: b.key, config: cfg, net, seed });
    if (trace.shot) writeFileSync(join(out, `shot-${b.key}.png`), Buffer.from(trace.shot, 'base64'));
    delete trace.shot;
    writeFileSync(join(out, `trace-${b.key}.json`), JSON.stringify(trace));
    const summary = summarise(trace, { metric, minSpeed });
    const verdict = judge(summary, cfg.thresholds);
    for (const n of trace.notes) console.log(`  note: ${n}`);
    const m = summary.motion;
    const lw = summary.frames.longest_page_work_ms;
    const longestText = lw === null ? '' : ` at ${summary.frames.longest_at_s} s (the game's own work in it: ${lw} ms${lw < summary.frames.max_ms / 4 ? ', so the browser, GPU or machine held the page' : ''})`;
    console.log(`  frames ${summary.frames.count}, p50 ${summary.frames.p50_ms} ms, longest ${summary.frames.max_ms} ms${longestText}, hitches ${summary.frames.hitches}` + (m ? `; moving frames ${m.frames}, uneven ${m.uneven_pct} %, ratio SD ${m.ratio_sd}, deviation p99 ${m.ratio_dev_p99}` : '; no motion trace'));
    console.log(`  ${verdict.pass ? 'PASS' : 'FAIL'}${verdict.failures.map((f) => `\n    over: ${f.text} ${f.value} (limit ${f.limit})`).join('')}`);
    runs.push({ ...b, summary, verdict, trace, series: chartSeries(trace, { metric, minSpeed, start_s: cfg.chart_start_s ?? 2, window_s: cfg.chart_window_s ?? 6 }), relay: trace.relay });
  }
  const netText = net === 'none' ? 'no network' : `a local authoritative host behind a relay adding delay and jitter (${net})`;
  const title = o.title || cfg.title || `${cfg.game || 'Game'}: ${o.baseline ? 'before and after' : 'smoothness'}`;
  const subtitle = `Headless Chrome on the graphics card, ${cfg.width ?? 1280} px wide, driven by synthetic keys for ${cfg.duration_s ?? 20} s; ${netText}. Tracked: ${JSON.stringify(cfg.probe || { kind: 'camera_target' })} (${metric} space).`;
  const target = runs[runs.length - 1].summary.frames.target_ms || 16.7;
  writeFileSync(join(out, 'report.html'), html(runs, { title, subtitle, window_s: cfg.chart_window_s ?? 6, target_ms: target }));
  const candidate = runs[runs.length - 1];
  const result = {
    format: 'evani-smoothness/1',
    game: cfg.game || null,
    config: resolve(o.config),
    net,
    seed,
    thresholds: cfg.thresholds,
    pass: candidate.verdict.pass,
    runs: runs.map((r) => ({ key: r.key, name: r.name, build: resolve(r.dir), summary: r.summary, verdict: r.verdict, notes: r.trace.notes, relay: r.relay })),
    report: join(out, 'report.html'),
  };
  writeFileSync(join(out, 'result.json'), JSON.stringify(result, null, 2));
  console.log(`smoothness: ${candidate.verdict.pass ? 'PASS' : 'FAIL'} (${candidate.name}); report ${join(out, 'report.html')}`);
  return result.pass || !o.fail ? 0 : 1;
}

if (process.argv[1] && pathToFileURL(resolve(process.argv[1])).href === import.meta.url) {
  main(process.argv.slice(2)).then(
    (code) => process.exit(code),
    (e) => {
      console.error(`smoothness: ${e.message}`);
      process.exit(2);
    },
  );
}
