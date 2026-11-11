// node --test tools/studio/server-views.test.mjs (also run by `cargo test -p evani-xtask commons`).
// The server views must stay the server's: view0 and the seed-1 draws below were taken from Evani.App's own
// drawPlan (platform/server/src/commons.ts, bundled and run on 2026-11-11: equal for seeds 1, 2, 7, 12345,
// 987654321 and 4294967295).
import { test } from "node:test";
import assert from "node:assert/strict";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { objectPlan, shotsFile, SERVER_BASE, SERVER_SIZE, VIEW_NAMES, DEFAULT_CAMERA, CAMERA_RANGES, readCameraParams, referenceCamera, referenceStudio, readStudio, studioParams, STUDIO_RANGES, referenceShotSize } from "./server-views.mjs";

const P = "kit.proc_field.";
const view = (yaw, pitch, dist, sunYaw, sunPitch) => ({
  [P + "cam_free"]: 1, [P + "sun_free"]: 1, [P + "cam_yaw_deg"]: yaw, [P + "cam_pitch_deg"]: pitch,
  [P + "cam_dist_scale"]: dist, [P + "sun_yaw_deg"]: sunYaw, [P + "sun_pitch_deg"]: sunPitch,
});

test("seed 1 is the server's own plan, view for view", () => {
  assert.deepEqual(objectPlan(1), [
    { name: "view0", params: view(0, 20, 1, 135, 40) },
    { name: "low", params: view(-1.9, 4.3, 1, 135, 40) },
    { name: "behind", params: view(182.8, 20, 1, 135, 40) },
    { name: "close", params: view(16.9, 20, 0.695, 135, 40) },
    { name: "edge", params: view(90, 8, 1, 135, 40) },
    { name: "light", params: view(0, 20, 1, 258, 36.4) },
  ]);
});

test("view0 is the server's default camera for every seed; the hidden views keep its ranges", () => {
  assert.deepEqual(SERVER_BASE, { cam_yaw_deg: 0, cam_pitch_deg: 20, cam_dist_scale: 1, sun_yaw_deg: 135, sun_pitch_deg: 40 });
  assert.equal(SERVER_SIZE, "1536x1024");
  for (let seed = 1; seed < 400; seed += 7) {
    const p = Object.fromEntries(objectPlan(seed).map((v) => [v.name, v.params]));
    assert.deepEqual(Object.keys(p), VIEW_NAMES);
    assert.deepEqual(p.view0, view(0, 20, 1, 135, 40));
    assert.ok(p.low[P + "cam_pitch_deg"] >= 4 && p.low[P + "cam_pitch_deg"] <= 12);
    assert.ok(Math.abs(p.low[P + "cam_yaw_deg"]) <= 30);
    assert.ok(p.behind[P + "cam_yaw_deg"] >= 150 && p.behind[P + "cam_yaw_deg"] <= 210);
    assert.ok(p.close[P + "cam_dist_scale"] >= 0.55 && p.close[P + "cam_dist_scale"] <= 0.7);
    assert.deepEqual(p.edge, view(90, 8, 1, 135, 40));
    assert.ok(p.light[P + "sun_yaw_deg"] >= 225 && p.light[P + "sun_yaw_deg"] <= 285);
    assert.ok(p.light[P + "sun_pitch_deg"] >= 20 && p.light[P + "sun_pitch_deg"] <= 50);
    assert.deepEqual(objectPlan(seed), objectPlan(seed), "the same seed draws the same views");
  }
});

test("a held reference camera moves view0 and the views drawn round it", () => {
  const cam = { cam_yaw_deg: 30, cam_pitch_deg: 14, cam_dist_scale: 0.8, sun_yaw_deg: 100, sun_pitch_deg: 50 };
  const p = Object.fromEntries(objectPlan(1, cam).map((v) => [v.name, v.params]));
  assert.deepEqual(p.view0, view(30, 14, 0.8, 100, 50));
  assert.deepEqual(p.edge, view(120, 8, 0.8, 100, 50));
});

test("the shots file shoots every view with the asset, one PNG each", () => {
  const s = shotsFile(objectPlan(1));
  assert.deepEqual(s.map((x) => x.file), VIEW_NAMES.map((n) => n + ".png"));
  for (const x of s) {
    assert.equal(x.view, 0);
    assert.equal(x.params[P + "subject_off"], 0);
    assert.equal(x.params[P + "cam_free"], 1);
  }
});

// Per-item reference camera (Evani.App commit 7387cee2, platform/server/src/commons.ts: DEFAULT_CAMERA, CAMERA_RANGES,
// readCameraParams, drawPlan with the item's camera as base; run on 2026-11-11 for the seeds below).
const PEBBLES = { cam_yaw_deg: 0, cam_pitch_deg: 14, cam_dist_scale: 0.55 };

test("the server's default camera and camera ranges", () => {
  assert.equal(SERVER_BASE, DEFAULT_CAMERA);
  assert.deepEqual(CAMERA_RANGES, { cam_yaw_deg: [-360, 360], cam_pitch_deg: [-10, 85], cam_dist_scale: [0.3, 3], sun_yaw_deg: [-360, 360], sun_pitch_deg: [5, 89], cam_aim_y: [-1, 1], cam_lens_deg: [10, 90] });
});

test("a camera's aim and lens go on every view only when it sets them", () => {
  const cam = { cam_pitch_deg: 14, cam_aim_y: -0.2, cam_lens_deg: 24 };
  assert.deepEqual(readCameraParams(cam), { ...DEFAULT_CAMERA, cam_pitch_deg: 14, cam_aim_y: -0.2, cam_lens_deg: 24 });
  for (const bad of [{ cam_aim_y: 1.2 }, { cam_aim_y: -1.01 }, { cam_lens_deg: 9 }, { cam_lens_deg: 91 }, { cam_lens_deg: "24" }]) assert.equal(readCameraParams(bad), null, JSON.stringify(bad));
  for (const v of objectPlan(7, cam)) {
    assert.equal(v.params[P + "cam_aim_y"], -0.2, v.name);
    assert.equal(v.params[P + "cam_lens_deg"], 24, v.name);
  }
  // Without them the plan is the one it was (the server's, pinned above).
  for (const v of objectPlan(7, { cam_pitch_deg: 14 })) assert.ok(!(P + "cam_aim_y" in v.params) && !(P + "cam_lens_deg" in v.params));
});

test("an object's studio is read in range and goes on every view as the render app's params", () => {
  assert.deepEqual(STUDIO_RANGES, { backdrop_grey: [40, 230], floor_grey: [40, 230], warmth: [-40, 40] });
  assert.deepEqual(readStudio({ backdrop_grey: 153.5, floor_grey: 149 }), { backdrop_grey: 153.5, floor_grey: 149, warmth: 0 });
  assert.deepEqual(readStudio({ backdrop_grey: 40, floor_grey: 230, warmth: -40 }), { backdrop_grey: 40, floor_grey: 230, warmth: -40 });
  for (const bad of [null, [], {}, { backdrop_grey: 150 }, { floor_grey: 150 }, { backdrop_grey: 39, floor_grey: 150 }, { backdrop_grey: 150, floor_grey: 231 }, { backdrop_grey: 150, floor_grey: 150, warmth: 41 }, { backdrop_grey: "150", floor_grey: 150 }, { backdrop_grey: 150, floor_grey: 150, key: 1 }])
    assert.equal(readStudio(bad), null, JSON.stringify(bad));
  assert.deepEqual(studioParams({ backdrop_grey: 170.84, floor_grey: 145.4, warmth: 9.7 }), { [P + "studio_backdrop"]: 170.8, [P + "studio_floor"]: 145.4, [P + "studio_warmth"]: 9.7 });
  assert.deepEqual(studioParams(null), {});
  const st = { backdrop_grey: 134.5, floor_grey: 142.5, warmth: 1 };
  for (const v of objectPlan(3, null, st)) {
    assert.equal(v.params[P + "studio_backdrop"], 134.5, v.name);
    assert.equal(v.params[P + "studio_floor"], 142.5, v.name);
    assert.equal(v.params[P + "studio_warmth"], 1, v.name);
  }
  // No studio: the plan is the server's, pinned above.
  assert.deepEqual(objectPlan(1, null, null), objectPlan(1));
  assert.deepEqual(objectPlan(1, null, { backdrop_grey: 300, floor_grey: 1 }), objectPlan(1), "an unusable studio is left out");
});

test("a camera is read as the server reads it: missing values the default's, one out of range refused", () => {
  assert.deepEqual(readCameraParams(PEBBLES), { ...PEBBLES, sun_yaw_deg: 135, sun_pitch_deg: 40 });
  for (const bad of [null, undefined, [], {}, { cam_fov: 30 }, { cam_pitch_deg: "14" }, { cam_pitch_deg: 86 }, { cam_pitch_deg: -10.5 }, { cam_dist_scale: 0.29 }, { cam_dist_scale: 3.1 }, { cam_yaw_deg: 361 }, { sun_pitch_deg: 4 }, { sun_pitch_deg: 90 }, { sun_yaw_deg: -400 }, { ...PEBBLES, cam_dist_scale: Infinity }])
    assert.equal(readCameraParams(bad), null, JSON.stringify(bad));
  assert.deepEqual(readCameraParams({ cam_pitch_deg: -10, cam_dist_scale: 3, sun_pitch_deg: 89, sun_yaw_deg: -360 }), { cam_yaw_deg: 0, cam_pitch_deg: -10, cam_dist_scale: 3, sun_yaw_deg: -360, sun_pitch_deg: 89 });
  // An unusable camera shoots the server's default plan, as the server does.
  assert.deepEqual(objectPlan(1, { cam_pitch_deg: 99 }), objectPlan(1));
});

test("Pebbles' camera, seed 1: the server's own plan view for view", () => {
  assert.deepEqual(objectPlan(1, PEBBLES), [
    { name: "view0", params: view(0, 14, 0.55, 135, 40) },
    { name: "low", params: view(-1.9, 4.3, 0.55, 135, 40) },
    { name: "behind", params: view(182.8, 14, 0.55, 135, 40) },
    { name: "close", params: view(16.9, 14, 0.382, 135, 40) },
    { name: "edge", params: view(90, 8, 0.55, 135, 40) },
    { name: "light", params: view(0, 14, 0.55, 258, 36.4) },
  ]);
});

test("a held camera: view0 is it for every seed and the hidden views keep the server's ranges round it", () => {
  for (const cam of [PEBBLES, { cam_yaw_deg: 30, cam_pitch_deg: 6, cam_dist_scale: 2, sun_yaw_deg: 100, sun_pitch_deg: 50 }]) {
    const b = readCameraParams(cam);
    for (let seed = 1; seed < 400; seed += 7) {
      const p = Object.fromEntries(objectPlan(seed, cam).map((v) => [v.name, v.params]));
      const near = (a, lo, hi) => a >= lo - 0.051 && a <= hi + 0.051;
      assert.deepEqual(p.view0, view(b.cam_yaw_deg, b.cam_pitch_deg, b.cam_dist_scale, b.sun_yaw_deg, b.sun_pitch_deg));
      assert.ok(near(p.low[P + "cam_pitch_deg"], 4, 12));
      assert.ok(near(p.low[P + "cam_yaw_deg"], b.cam_yaw_deg - 30, b.cam_yaw_deg + 30));
      assert.equal(p.low[P + "cam_dist_scale"], b.cam_dist_scale);
      assert.ok(near(p.behind[P + "cam_yaw_deg"], b.cam_yaw_deg + 150, b.cam_yaw_deg + 210));
      assert.equal(p.behind[P + "cam_pitch_deg"], b.cam_pitch_deg);
      assert.ok(p.close[P + "cam_dist_scale"] >= b.cam_dist_scale * 0.55 - 0.001 && p.close[P + "cam_dist_scale"] <= b.cam_dist_scale * 0.7 + 0.001);
      assert.ok(near(p.close[P + "cam_yaw_deg"], b.cam_yaw_deg - 20, b.cam_yaw_deg + 20));
      assert.deepEqual(p.edge, view(b.cam_yaw_deg + 90, Math.min(b.cam_pitch_deg, 8), b.cam_dist_scale, b.sun_yaw_deg, b.sun_pitch_deg));
      assert.ok(near(p.light[P + "sun_yaw_deg"], b.sun_yaw_deg + 90, b.sun_yaw_deg + 150));
      assert.ok(near(p.light[P + "sun_pitch_deg"], 20, 50));
      assert.equal(p.light[P + "cam_dist_scale"], b.cam_dist_scale);
    }
  }
});

test("shoot and plan take the asset's camera, studio and shape from catalog/procedural/references.json", () => {
  const root = join(dirname(fileURLToPath(import.meta.url)), "..", "..");
  // Measured against each reference picture (2026-11-11, docs/EVANI_COMMONS.md "Per-object studio").
  const pebbles = { cam_yaw_deg: 0, cam_pitch_deg: 14, cam_dist_scale: 0.57, cam_aim_y: -0.09 };
  assert.deepEqual(referenceCamera(root, "rocks-pebbles@photoreal"), readCameraParams(pebbles));
  assert.deepEqual(referenceCamera(root, "rocks-pebbles"), readCameraParams(pebbles));
  assert.deepEqual(referenceStudio(root, "rocks-pebbles"), { backdrop_grey: 134.5, floor_grey: 142.5, warmth: 1 });
  assert.deepEqual(referenceStudio(root, "geometry-cube-cuboid@photoreal"), { backdrop_grey: 170.8, floor_grey: 145.4, warmth: 9.7 });
  assert.equal(referenceCamera(root, "geometry-cube-cuboid").sun_yaw_deg, 33);
  assert.equal(referenceShotSize(root, "geometry-cube-cuboid@photoreal"), "1536x864", "the cube's reference is 16:9");
  assert.equal(referenceShotSize(root, "rocks-pebbles"), "1536x1024");
  assert.equal(referenceShotSize(root, "no-such-asset"), "1536x1024");
  assert.equal(referenceCamera(root, "geometry-cone@photoreal"), null, "no camera held: the server's default");
  assert.equal(referenceStudio(root, "terrain-mountains"), null, "a scene's reference has no studio");
  assert.equal(referenceCamera(root, "no-such-asset"), null);
  assert.equal(referenceCamera(root, undefined), null);
});
