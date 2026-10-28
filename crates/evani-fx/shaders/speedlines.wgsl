// evani.shader.speedlines
// Speed lines (one pass, `speedlines`): radial streaks from the centre of
// each view, added to `hdr` (dst + src) after the outlines and before bloom
// and tonemap. Uniform only. Split-aware: with `split` = 1 the top half is
// player 1's view (drive_a) and the bottom half player 2's (drive_b), each
// with its own centre. The game drives `drive_*` from speed and boosts and
// advances `phase` (0..1, wraps) to move the dashes outwards.

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> VsOut {
    let p = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: VsOut;
    o.pos = vec4<f32>(p * 2.0 - 1.0, 0.0, 1.0);
    o.uv = vec2<f32>(p.x, 1.0 - p.y);
    return o;
}

struct Speedlines {
    // fx.speedlines.intensity (brightness of a full-drive streak).
    intensity: f32,
    // fx.speedlines.density (streak cells around the circle).
    density: f32,
    // fx.speedlines.inner_radius (clear centre, 1 = half the view height).
    inner_radius: f32,
    // fx.speedlines.drive_a (player 1, or the only view), 0..1.
    drive_a: f32,
    // fx.speedlines.drive_b (player 2), 0..1.
    drive_b: f32,
    // fx.speedlines.split: 0 = one full view, 1 = two stacked views.
    split: f32,
    // Render target width / height (set by the renderer).
    aspect: f32,
    // fx.speedlines.phase: animation phase 0..1 (wraps seamlessly).
    phase: f32,
};

@group(0) @binding(0) var<uniform> u: Speedlines;

fn hash(x: f32) -> f32 {
    return fract(sin(x * 127.1 + 311.7) * 43758.5453);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    var centre = vec2<f32>(0.5, 0.5);
    var view_h = 1.0;
    var drive = u.drive_a;
    if (u.split > 0.5) {
        view_h = 0.5;
        if (in.uv.y < 0.5) {
            centre = vec2<f32>(0.5, 0.25);
        } else {
            centre = vec2<f32>(0.5, 0.75);
            drive = u.drive_b;
        }
    }
    drive = clamp(drive, 0.0, 1.0);
    // View-space offset, 1 = half the view's height.
    let d = in.uv - centre;
    let q = vec2<f32>(d.x * u.aspect, d.y) / (0.5 * view_h);
    let r = length(q);
    let density = max(round(u.density), 1.0);
    let turn = atan2(q.y, q.x) / 6.2831853 + 0.5;
    let cell = floor(turn * density);
    let across = fract(turn * density) - 0.5;
    let pick = hash(cell);
    let speed_pick = hash(cell * 1.7 + 3.1);
    // More cells carry a streak as the drive rises.
    let shown = step(pick, drive * 0.85 + 0.05);
    // Thin streaks, a little wider for some cells.
    let width = 0.12 + 0.18 * speed_pick;
    let line = 1.0 - smoothstep(0.0, width, abs(across));
    // Clear centre, fading in past inner_radius.
    let radial = smoothstep(u.inner_radius, u.inner_radius + 0.35, r);
    // Dashes moving outwards; whole-number speeds keep the phase wrap seamless.
    let speed = 1.0 + floor(speed_pick * 3.0);
    let s = fract(r * (0.6 + 0.5 * pick) - u.phase * speed + pick * 7.0);
    let dash = smoothstep(0.0, 0.08, s) * (1.0 - smoothstep(0.4, 0.65, s));
    let v = line * radial * dash * shown * drive * u.intensity;
    // Alpha 0: added to `hdr`, it leaves the forward pass's fade marks alone.
    return vec4<f32>(vec3<f32>(v), 0.0);
}
