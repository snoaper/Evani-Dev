// tools/nature/sea-foam-view.mjs <png...>: counts the field sea's foam view (dev builds, `foam_debug`,
// sea-shots.mjs --foam-debug): pixels painted red (new foam from an event: a crest breaking, the shore),
// blue (carried from a break), white-ish (fading) and yellow (foam with no cause; there should be none).
// Prints per picture and the total; exits 1 if any picture has yellow.
import { readFileSync } from "node:fs";
import { readPng } from "./png-small.mjs";
let bad = 0;
const tot = { red: 0, blue: 0, white: 0, yellow: 0, px: 0 };
for (const f of process.argv.slice(2)) {
  const { width: W, height: H, data } = readPng(readFileSync(f));
  const c = { red: 0, blue: 0, white: 0, yellow: 0 };
  let hz = 0, best = 0, prev = null;
  for (let y = Math.round(H * 0.2); y < H * 0.8; y++) { let m = 0; for (let x = 0; x < W; x += 8) m += data[4 * (y * W + x) + 1]; m /= W / 8; if (prev !== null && prev - m > best) { best = prev - m; hz = y + 2; } prev = m; }
  for (let i = 0; i < W * H; i++) {
    const r = data[4 * i], g = data[4 * i + 1], b = data[4 * i + 2];
    // By hue against the grey water: yellow (red and green well over blue), red, blue; white only
    // below the sky (rows under the brightest row band are water; the sky is bright too, so white
    // counts pixels brighter than any grey water, under the horizon's row of largest drop).
    if (r > 150 && g > 130 && b < 0.55 * g && r - b > 90) c.yellow++;
    else if (r > g + 70 && r > b + 70) c.red++;
    else if (b > r + 50 && b > g + 10) c.blue++;
    else if (i > hz * W && r > 225 && g > 225 && b > 225) c.white++;
  }
  for (const k of Object.keys(c)) tot[k] += c[k];
  tot.px += W * H;
  if (c.yellow > 0) bad++;
  console.log(f.split(/[\/]/).slice(-1)[0].padEnd(28), "red", c.red, "blue", c.blue, "white", c.white, "yellow", c.yellow);
}
console.log("total", JSON.stringify(tot), "pictures with yellow:", bad);
process.exit(bad ? 1 : 0);
