// Picture notes (visual-review.json) for Evani.App's visual review, written by a tool, never by hand.
//
// Why (2026-11-11, Distance fog column 74): helpers hand-wrote the notes and Evani.App refused them three times
// with '"bytes_same_as_earlier_round" ... has no value' and '"earlier_round_file" ... has no value'. Evani.App's
// checker (claude-code-desktop src/main/qqVisualPacketCore.ts, numbersOf) takes a picture's numbers only as a
// finite number or non-empty text: true/false, null and "" all read as "no value". This tool computes every
// field from the files and checks the whole file with the same rules before request_review.
// Since Evani.App ef6ebfd0 (QQ-17, live after its next Restart & update) Evani.App pairs each picture with the last
// round's kept copy itself (SHA-256) and drops earlier-round fields a helper writes, so they are written only with
// --legacy-fields (for an Evani.App that has not updated yet), never by default.
//
//   node tools/studio/visual-review.mjs write --spec <spec.json> [--out <folder>/visual-review.json]
//        [--legacy-fields [--earlier <old visual-review.json | folder>]]
//   node tools/studio/visual-review.mjs check <visual-review.json>
//   node tools/studio/visual-review.mjs usable <picture.png> [--size WxH]
//
// spec.json (paths relative to the spec's folder):
//   { "title": "...", "build": "...", "shared": {"camera": "..."},
//     "numbers_from": "how the numbers were measured"   (or "no_numbers_because": "...")
//     "earlier": "<last round's visual-review.json or its picture folder>"   (--legacy-fields only; --earlier wins)
//     "views": [ { "name": "Fog at 1280", "picture": "a.png", "numbers": {...} },
//                { "name": "Against the reference", "parts": [
//                    { "label": "Stored reference", "role": "reference", "picture": "ref.png" },
//                    { "label": "Ours", "picture": "ours.png", "numbers": {...} } ] } ] }
//
// Filled in for every picture (computed, never guessed): width_px, height_px, png_bytes, sha256, capture_path
// ("browser" | "offscreen" from the capture tool's capture-paths.json beside the picture, "stored" for a
// reference, else "not recorded"). With --legacy-fields also earlier_round_file / earlier_round_sha256 (the last
// round's file for the same view and label, or "none") and bytes_same_as_earlier_round ("yes"/"no"; "no" with no
// earlier round). Your own numbers: a yes/no becomes "yes"/"no", an empty or null one "none"; earlier-round
// fields in a spec are dropped.
// The notes are written next to the pictures by default and their own time is after the pictures'.

import { createHash } from "node:crypto";
import { existsSync, readFileSync, statSync, writeFileSync, mkdirSync } from "node:fs";
import { basename, dirname, extname, isAbsolute, join, resolve } from "node:path";
import { deflateSync, inflateSync } from "node:zlib";
import { fileURLToPath } from "node:url";

// Evani.App's limits (claude-code-desktop src/shared/qqVisual.ts).
export const MAX_BYTES = 3_700_000;
export const MAX_EDGE = 2600;
const PICTURE = /\.(png|jpe?g|webp)$/i;
const isText = (v) => typeof v === "string" && !!v.trim();

// ---------------------------------------------------------------- PNG facts

function chunks(buf) {
  if (buf.length < 33 || buf.readUInt32BE(0) !== 0x89504e47 || buf.toString("latin1", 12, 16) !== "IHDR") return null;
  const ihdr = { width: buf.readUInt32BE(16), height: buf.readUInt32BE(20), depth: buf[24], color: buf[25], interlace: buf[28] };
  const idat = [];
  for (let at = 8; at + 8 <= buf.length; ) {
    const len = buf.readUInt32BE(at);
    const type = buf.toString("latin1", at + 4, at + 8);
    if (type === "IDAT") idat.push(buf.subarray(at + 8, at + 8 + len));
    if (type === "IEND") break;
    at += 12 + len;
  }
  return idat.length ? { ...ihdr, idat: Buffer.concat(idat) } : null;
}

/** Evani.App's own blank test (qqVisualPacketCore.ts pngFacts): a handful of byte values in the raw rows. */
export function pngFacts(buf) {
  const c = chunks(buf);
  if (!c) return null;
  try {
    const raw = inflateSync(c.idat);
    const seen = new Set();
    const step = Math.max(1, Math.floor(raw.length / 200_000));
    for (let i = 0; i < raw.length && seen.size <= 8; i += step) seen.add(raw[i]);
    return { width: c.width, height: c.height, blank: seen.size <= 8 };
  } catch {
    return null;
  }
}

/** Decoded 8-bit RGB/RGBA pixels (non-interlaced), or null for other kinds of PNG. */
export function decodePng(buf) {
  const c = chunks(buf);
  if (!c || c.depth !== 8 || c.interlace !== 0 || (c.color !== 2 && c.color !== 6)) return null;
  const bpp = c.color === 6 ? 4 : 3;
  const stride = c.width * bpp;
  let raw;
  try { raw = inflateSync(c.idat); } catch { return null; }
  if (raw.length < (stride + 1) * c.height) return null;
  const out = Buffer.alloc(stride * c.height);
  for (let y = 0; y < c.height; y++) {
    const f = raw[y * (stride + 1)];
    const row = raw.subarray(y * (stride + 1) + 1, (y + 1) * (stride + 1));
    const o = y * stride;
    for (let x = 0; x < stride; x++) {
      const a = x >= bpp ? out[o + x - bpp] : 0;
      const b = y > 0 ? out[o - stride + x] : 0;
      const cc = x >= bpp && y > 0 ? out[o - stride + x - bpp] : 0;
      let v = row[x];
      if (f === 1) v += a;
      else if (f === 2) v += b;
      else if (f === 3) v += (a + b) >> 1;
      else if (f === 4) { const p = a + b - cc, pa = Math.abs(p - a), pb = Math.abs(p - b), pc = Math.abs(p - cc); v += pa <= pb && pa <= pc ? a : pb <= pc ? b : cc; }
      out[o + x] = v & 255;
    }
  }
  return { width: c.width, height: c.height, bpp, data: out };
}

/** Write an 8-bit RGBA PNG (tests and synthetic pictures). */
export function encodePng(width, height, rgba) {
  const row = width * 4;
  const raw = Buffer.alloc((row + 1) * height);
  for (let y = 0; y < height; y++) rgba.copy(raw, y * (row + 1) + 1, y * row, (y + 1) * row);
  const chunk = (type, data) => {
    const t = Buffer.from(type, "latin1");
    const len = Buffer.alloc(4); len.writeUInt32BE(data.length);
    let c = 0xffffffff;
    for (const b of Buffer.concat([t, data])) c = CRC[(c ^ b) & 255] ^ (c >>> 8);
    const crc = Buffer.alloc(4); crc.writeUInt32BE((c ^ 0xffffffff) >>> 0);
    return Buffer.concat([len, t, data, crc]);
  };
  const ihdr = Buffer.alloc(13);
  ihdr.writeUInt32BE(width, 0); ihdr.writeUInt32BE(height, 4); ihdr[8] = 8; ihdr[9] = 6;
  return Buffer.concat([Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]), chunk("IHDR", ihdr), chunk("IDAT", deflateSync(raw)), chunk("IEND", Buffer.alloc(0))]);
}
const CRC = (() => { const t = new Uint32Array(256); for (let n = 0; n < 256; n++) { let c = n; for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1; t[n] = c >>> 0; } return t; })();

/**
 * Is a captured frame usable? { ok, why?, width, height, mean, spread }.
 * Not usable: unreadable, the wrong size, Evani.App's "blank", black (mean luma under 4/255), or uniform
 * (luma spread under 1.5/255 over a sample grid: one flat colour, such as a cleared canvas).
 */
export function usable(buf, want) {
  const facts = pngFacts(buf);
  if (!facts) return { ok: false, why: "not a readable PNG" };
  const base = { width: facts.width, height: facts.height };
  if (want && (facts.width !== want.width || facts.height !== want.height)) return { ...base, ok: false, why: `the wrong size: ${facts.width}x${facts.height}, not ${want.width}x${want.height}` };
  if (facts.blank) return { ...base, ok: false, why: "blank (one flat colour)" };
  const img = decodePng(buf);
  if (!img) return { ...base, ok: true };
  let n = 0, sum = 0, sq = 0;
  const sx = Math.max(1, Math.floor(img.width / 64)), sy = Math.max(1, Math.floor(img.height / 64));
  for (let y = 0; y < img.height; y += sy) for (let x = 0; x < img.width; x += sx) {
    const i = (y * img.width + x) * img.bpp;
    const l = 0.2126 * img.data[i] + 0.7152 * img.data[i + 1] + 0.0722 * img.data[i + 2];
    n++; sum += l; sq += l * l;
  }
  const mean = sum / n, spread = Math.sqrt(Math.max(0, sq / n - mean * mean));
  const r = { ...base, mean: Math.round(mean * 10) / 10, spread: Math.round(spread * 10) / 10 };
  if (mean < 4) return { ...r, ok: false, why: `black (mean brightness ${r.mean} of 255)` };
  if (spread < 1.5) return { ...r, ok: false, why: `uniform (brightness spread ${r.spread} of 255)` };
  return { ...r, ok: true };
}

// ---------------------------------------------------------------- the checker (Evani.App's rules)

/** Why a value can't stand as a number in Evani.App's notes, or null when it can. */
export function valueProblem(v) {
  if (typeof v === "number") return Number.isFinite(v) ? null : "is not a finite number";
  if (isText(v)) return null;
  if (typeof v === "boolean") return "is true/false: Evani.App reads only numbers or text (write \"yes\"/\"no\" or 1/0)";
  if (v === null || v === undefined) return "is empty (null): write \"none\" or a number";
  if (typeof v === "string") return "is empty text: write \"none\"";
  return "is a list or object: write it as text";
}

function numbersProblems(nums, where) {
  if (nums === undefined || nums === null) return [];
  if (typeof nums !== "object" || Array.isArray(nums)) return [`the numbers for ${where} aren't a list of labels and values`];
  const out = [];
  for (const [k, v] of Object.entries(nums)) {
    if (!k.trim()) out.push(`a number for ${where} has no label`);
    const p = valueProblem(v);
    if (p) out.push(`"${k}" for ${where} ${p}`);
  }
  return out;
}

/**
 * Check a visual-review.json the way Evani.App's collectVisualPacket does (every problem, not just the first).
 * `file`: its path (pictures resolve against its folder). Returns { ok, problems: string[], pictures }.
 * Video frames ("video" + "at") are Evani.App's to take; they are only checked for an "at".
 */
export function checkNotes(json, file) {
  const problems = [];
  const base = dirname(resolve(file));
  let pictures = 0, anyNumbers = false;
  if (!json || !Array.isArray(json.views) || !json.views.length) return { ok: false, problems: [`${basename(file)} lists no views: a review with no pictures never counts`], pictures: 0 };
  const names = new Set();
  for (const raw of json.views) {
    if (!isText(raw?.name)) { problems.push("a view has no name"); continue; }
    const name = raw.name.trim();
    if (names.has(name.toLowerCase())) problems.push(`two views are called "${name}"`);
    names.add(name.toLowerCase());
    const parts = Array.isArray(raw.parts) && raw.parts.length ? raw.parts
      : isText(raw.picture) || isText(raw.video) ? [{ label: name, picture: raw.picture, video: raw.video, at: raw.at, numbers: raw.numbers }] : null;
    if (!parts) { problems.push(`the view "${name}" has no picture`); continue; }
    for (const p of parts) {
      const label = isText(p?.label) ? p.label.trim() : isText(p?.picture) ? basename(p.picture) : "(no label)";
      const where = `"${name} · ${label}"`;
      if (isText(p?.video)) {
        if (!(typeof p.at === "number" && Number.isFinite(p.at) && p.at >= 0)) problems.push(`a frame in the view "${name}" names a video but no "at" (seconds)`);
        pictures++;
      } else if (!isText(p?.picture)) { problems.push(`a picture in the view "${name}" has no file`); continue; }
      else {
        const pic = isAbsolute(p.picture) ? p.picture : resolve(base, p.picture);
        if (!PICTURE.test(pic)) problems.push(`${where} is ${extname(pic) || "not a picture"}: use a PNG`);
        let st = null;
        try { st = statSync(pic); } catch { problems.push(`the picture for ${where} wasn't found (${pic})`); }
        if (st) {
          if (!st.isFile() || st.size < 100) problems.push(`the picture for ${where} is empty`);
          else if (st.size > MAX_BYTES) problems.push(`the picture for ${where} is ${(st.size / 1e6).toFixed(1)} MB; it must be under ${(MAX_BYTES / 1e6).toFixed(1)} MB`);
          else if (/\.png$/i.test(pic)) {
            const f = pngFacts(readFileSync(pic));
            if (!f) problems.push(`the picture for ${where} isn't a readable PNG`);
            else if (Math.max(f.width, f.height) > MAX_EDGE) problems.push(`the picture for ${where} is ${f.width}x${f.height}; keep the long edge to about 2,500 px`);
            else if (f.blank) problems.push(`the picture for ${where} is blank`);
          }
          pictures++;
        }
      }
      problems.push(...numbersProblems(p.numbers, where));
      if (p.numbers && typeof p.numbers === "object" && Object.keys(p.numbers).length) anyNumbers = true;
    }
  }
  problems.push(...numbersProblems(json.shared, "the shared settings"));
  if (!anyNumbers && !isText(json.no_numbers_because)) problems.push("no measured numbers were given: add numbers, or say why not with no_numbers_because");
  if (anyNumbers && !isText(json.numbers_from)) problems.push("the numbers don't say how they were measured: add numbers_from");
  if (!pictures) problems.push("no pictures at all: a review with no pictures never counts");
  return { ok: problems.length === 0, problems, pictures };
}

// ---------------------------------------------------------------- the writer

const sha256 = (buf) => createHash("sha256").update(buf).digest("hex");

/** A value Evani.App can read: yes/no for true/false, "none" for empty, text for lists. */
export function plainValue(v) {
  if (typeof v === "boolean") return v ? "yes" : "no";
  if (v === null || v === undefined || (typeof v === "string" && !v.trim())) return "none";
  if (typeof v === "number") return Number.isFinite(v) ? v : String(v);
  if (typeof v === "object") return JSON.stringify(v).slice(0, 400);
  return v;
}

const keyOf = (view, label) => `${String(view).trim().toLowerCase()}\u0000${String(label).trim().toLowerCase()}`;

/** The last round's pictures: by view+label (from its notes) and by file name (notes or a folder). */
export function earlierIndex(earlier) {
  const byKey = new Map(), byName = new Map();
  if (!earlier || !existsSync(earlier)) return { byKey, byName };
  if (statSync(earlier).isDirectory()) {
    const notes = join(earlier, "visual-review.json");
    if (existsSync(notes)) return earlierIndex(notes);
    return { byKey, byName, folder: earlier };
  }
  let json;
  try { json = JSON.parse(readFileSync(earlier, "utf8").replace(/^﻿/, "")); } catch { return { byKey, byName }; }
  const base = dirname(resolve(earlier));
  for (const v of json?.views ?? []) {
    const parts = Array.isArray(v.parts) && v.parts.length ? v.parts : isText(v.picture) ? [{ label: v.name, picture: v.picture }] : [];
    for (const p of parts) {
      if (!isText(p.picture)) continue;
      const f = isAbsolute(p.picture) ? p.picture : resolve(base, p.picture);
      const label = isText(p.label) ? p.label : basename(p.picture);
      byKey.set(keyOf(v.name, label), f);
      byName.set(basename(f).toLowerCase(), f);
    }
  }
  return { byKey, byName, folder: base };
}

function capturePathOf(pic, role, given) {
  if (isText(given)) return given.trim();
  const side = join(dirname(pic), "capture-paths.json");
  if (existsSync(side)) {
    try {
      const m = JSON.parse(readFileSync(side, "utf8"));
      const v = m[basename(pic)];
      if (isText(v?.path)) return v.path;
      if (isText(v)) return v;
    } catch { /* fall through */ }
  }
  return role === "reference" ? "stored" : "not recorded";
}

/**
 * Build complete notes from a spec (see the top of this file). `specDir`: where the spec's relative paths start.
 * `outFile`: where the notes go (pictures are written relative to it when they sit under its folder).
 * Returns { json, check }.
 */
export const LEGACY_FIELDS = ["earlier_round_file", "earlier_round_sha256", "bytes_same_as_earlier_round"];
export function buildNotes(spec, { specDir, outFile, earlier, legacyFields = false }) {
  const idx = earlierIndex(earlier ?? (spec.earlier ? resolve(specDir, spec.earlier) : undefined));
  const outDir = dirname(resolve(outFile));
  const rel = (f) => (resolve(f).toLowerCase().startsWith(outDir.toLowerCase() + "\\") || resolve(f).toLowerCase().startsWith(outDir.toLowerCase() + "/") ? resolve(f).slice(outDir.length + 1).replace(/\\/g, "/") : resolve(f));
  const views = [];
  for (const v of spec.views ?? []) {
    const parts = Array.isArray(v.parts) && v.parts.length ? v.parts : [{ label: v.name, picture: v.picture, numbers: v.numbers, role: v.role, capture_path: v.capture_path }];
    const outParts = [];
    for (const p of parts) {
      if (!isText(p.picture)) throw new Error(`the view "${v.name}" has a part with no picture`);
      const pic = isAbsolute(p.picture) ? p.picture : resolve(specDir, p.picture);
      if (!existsSync(pic)) throw new Error(`the picture for "${v.name}" is missing: ${pic}`);
      const buf = readFileSync(pic);
      const facts = pngFacts(buf);
      const label = isText(p.label) ? p.label.trim() : basename(pic);
      const role = p.role === "reference" ? "reference" : "ours";
      const old = idx.byKey.get(keyOf(v.name, label)) ?? idx.byName.get(basename(pic).toLowerCase()) ?? (idx.folder && existsSync(join(idx.folder, basename(pic))) ? join(idx.folder, basename(pic)) : undefined);
      const oldBuf = old && resolve(old) !== resolve(pic) && existsSync(old) ? readFileSync(old) : null;
      const hash = sha256(buf), oldHash = oldBuf ? sha256(oldBuf) : null;
      const own = {};
      // Evani.App (since ef6ebfd0, QQ-17) works out the earlier-round facts itself and drops these: never copied from a spec.
      for (const [k, x] of Object.entries(p.numbers ?? {})) if (!LEGACY_FIELDS.includes(k)) own[k] = plainValue(x);
      const numbers = {
        ...own,
        ...(facts ? { width_px: facts.width, height_px: facts.height } : {}),
        png_bytes: buf.length,
        sha256: hash,
        ...(legacyFields ? {
          earlier_round_file: oldBuf ? resolve(old) : "none",
          earlier_round_sha256: oldHash ?? "none",
          bytes_same_as_earlier_round: oldHash && oldHash === hash ? "yes" : "no",
        } : {}),
        capture_path: capturePathOf(pic, role, p.capture_path),
      };
      outParts.push({ label, role, picture: rel(pic), numbers });
    }
    views.push({ name: String(v.name).trim(), parts: outParts, ...(isText(v.url) ? { url: v.url } : {}), ...(typeof v.width === "number" ? { width: v.width } : {}) });
  }
  const shared = {};
  for (const [k, x] of Object.entries(spec.shared ?? {})) shared[k] = plainValue(x);
  const json = {
    ...(isText(spec.title) ? { title: spec.title } : {}),
    ...(isText(spec.build) ? { build: spec.build } : {}),
    shared,
    views,
    numbers_from: isText(spec.numbers_from) ? spec.numbers_from : "tools/studio/visual-review.mjs (file facts: size, bytes, SHA-256, capture path) plus the capture tool's own numbers",
    ...(isText(spec.no_numbers_because) ? { no_numbers_because: spec.no_numbers_because } : {}),
  };
  return { json, check: checkNotes(json, outFile) };
}

// ---------------------------------------------------------------- CLI

const argOf = (args, k, d) => { const i = args.indexOf("--" + k); return i >= 0 && args[i + 1] !== undefined && !args[i + 1].startsWith("--") ? args[i + 1] : d; };

export function main(args) {
  const cmd = args[0];
  if (cmd === "write") {
    const specFile = argOf(args, "spec");
    if (!specFile) return usage();
    const spec = JSON.parse(readFileSync(specFile, "utf8").replace(/^﻿/, ""));
    const specDir = dirname(resolve(specFile));
    const outFile = resolve(argOf(args, "out", join(specDir, "visual-review.json")));
    let built;
    try { built = buildNotes(spec, { specDir, outFile, earlier: argOf(args, "earlier"), legacyFields: args.includes("--legacy-fields") }); } catch (e) { console.error(`visual-review: ${e.message}`); return 1; }
    if (!built.check.ok) {
      console.error(`visual-review: NOT written; Evani.App would refuse it:\n  - ${built.check.problems.join("\n  - ")}`);
      return 1;
    }
    mkdirSync(dirname(outFile), { recursive: true });
    writeFileSync(outFile, JSON.stringify(built.json, null, 1) + "\n");
    console.log(`visual-review: wrote ${outFile} (${built.check.pictures} pictures, checked: ok)`);
    return 0;
  }
  if (cmd === "check") {
    const f = args[1];
    if (!f) return usage();
    let json;
    try { json = JSON.parse(readFileSync(f, "utf8").replace(/^﻿/, "")); } catch (e) { console.error(`visual-review: ${basename(f)} couldn't be read as JSON (${e.message})`); return 1; }
    const r = checkNotes(json, f);
    if (r.ok) { console.log(`visual-review: ${f}: ok (${r.pictures} pictures)`); return 0; }
    console.error(`visual-review: ${f}: Evani.App would refuse it:\n  - ${r.problems.join("\n  - ")}`);
    return 1;
  }
  if (cmd === "usable") {
    const f = args[1];
    if (!f) return usage();
    const size = argOf(args, "size");
    const want = size ? { width: Number(size.split("x")[0]), height: Number(size.split("x")[1]) } : undefined;
    const r = usable(readFileSync(f), want);
    console.log(JSON.stringify(r));
    return r.ok ? 0 : 1;
  }
  return usage();
}
function usage() {
  console.error("usage: node tools/studio/visual-review.mjs write --spec <spec.json> [--out <visual-review.json>] [--legacy-fields [--earlier <old notes|folder>]] | check <visual-review.json> | usable <png> [--size WxH]");
  return 2;
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) process.exit(main(process.argv.slice(2)));
