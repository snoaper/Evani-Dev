// Evani.Dev model converter (tools/evani-convert): resize one model's
// textures for one tier and write the core image (PNG or JPEG) plus a PNG
// for `ktx create`, which does not read JPEG. Uses sharp from the pinned
// glTF-Transform npm tree (tools/evani-convert/npm). Never upscales: sizes
// are powers of two at or below the source and at or below the tier's size.
//
// Usage: node images.mjs <node_modules> <jobs.json>
// jobs.json: [{ "src", "max", "core", "core_format": "png"|"jpg", "png" }]
// Writes <jobs.json>.out.json: [{ "core", "w", "h", "src_w", "src_h", "alpha" }]
import { createRequire } from 'node:module';
import fs from 'node:fs';
import path from 'node:path';

const [nodeModules, jobFile] = process.argv.slice(2);
const require = createRequire(path.join(nodeModules, 'evani-convert.cjs'));
const sharp = require('sharp');
sharp.concurrency(1);
sharp.cache(false);

const pow2Floor = (n) => 2 ** Math.floor(Math.log2(Math.max(1, n)));
const jobs = JSON.parse(fs.readFileSync(jobFile, 'utf8'));
const out = [];
for (const j of jobs) {
  const meta = await sharp(j.src).metadata();
  let w = pow2Floor(meta.width);
  let h = pow2Floor(meta.height);
  const scale = Math.min(1, j.max / Math.max(w, h));
  w = Math.max(1, Math.round(w * scale));
  h = Math.max(1, Math.round(h * scale));
  const resized = () => sharp(j.src).resize(w, h, { fit: 'fill', kernel: 'lanczos3' });
  if (j.core_format === 'png') {
    await resized().png({ compressionLevel: 9, adaptiveFiltering: false, palette: false }).toFile(j.core);
  } else {
    await resized().removeAlpha().jpeg({ quality: 90, chromaSubsampling: '4:4:4', mozjpeg: false }).toFile(j.core);
  }
  if (j.png && j.png !== j.core) {
    await resized().png({ compressionLevel: 1, adaptiveFiltering: false, palette: false }).toFile(j.png);
  }
  out.push({ core: j.core, w, h, src_w: meta.width, src_h: meta.height, alpha: !!meta.hasAlpha });
}
fs.writeFileSync(jobFile + '.out.json', JSON.stringify(out));
