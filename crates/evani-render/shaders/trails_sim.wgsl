// evani.shader.trails_sim
// Ribbons and trails, compute pass `trails.sim` (feature `trails`). Every
// point of every emitter's ring is aged in closed form (ticks since its
// fixed-step stamp plus the frame's alpha), so nothing is integrated frame by
// frame and a stepped capture repeats. One output per ring slot, newest first
// per emitter, which the draw reads as neighbouring instances. Same formula
// as `trails/tests.rs` `eval` (the CPU reference). One shader module with
// `trails_draw.wgsl` (and `trails_screen.wgsl` where screen strips are built),
// so the compute bindings start at 4, clear of the draw's uniform at 2.

struct Point {
    pos: vec3<f32>,
    tick: u32,
    // Half span of a ribbon; zero faces the camera.
    edge: vec3<f32>,
    // Scale at this point; below zero the strip stops at its older neighbour.
    width: f32,
};

struct Head {
    // Where the newest point stood a tick ago (the emitter's drawn position
    // moves from there to the newest point over the tick).
    prev: vec3<f32>,
    // Ring slot of the newest point.
    head: u32,
    color: vec4<f32>,
    color_end: vec4<f32>,
    count: u32,
    life: f32,
    width: f32,
    // The draw group; bit 31 says additive.
    key: u32,
};

struct Vert {
    pos: vec3<f32>,
    width: f32,
    tan: vec3<f32>,
    pad: f32,
    edge: vec3<f32>,
    // Draws a segment to the next output when it matches a group.
    key: u32,
    color: vec4<f32>,
};

struct Sim {
    now: u32,
    frac: f32,
    dt: f32,
    trails: u32,
    fade: f32,
    width: f32,
    _p0: f32,
    _p1: f32,
    // The width curve's 16 samples, then the alpha curve's.
    luts: array<vec4<f32>, 8>,
};

const POINTS: u32 = 64u;
const KEY_NONE: u32 = 0xffffffffu;
const KEY_MASK: u32 = 0x7fffffffu;
const WIDTH: u32 = 0u;
const ALPHA: u32 = 16u;

@group(0) @binding(4) var<storage, read> points: array<Point>;
@group(0) @binding(5) var<storage, read> heads: array<Head>;
@group(0) @binding(6) var<storage, read_write> outs: array<Vert>;
@group(0) @binding(7) var<uniform> sim: Sim;

// A curve at u in 0..1, from its 16 evenly spaced samples.
fn lut(curve: u32, u: f32) -> f32 {
    let x = clamp(u, 0.0, 1.0) * 15.0;
    let i = min(u32(x), 14u);
    let n = curve + i;
    let m = n + 1u;
    return mix(sim.luts[n / 4u][n % 4u], sim.luts[m / 4u][m % 4u], x - f32(i));
}

// Unit vector, or zero for a zero vector.
fn unit(v: vec3<f32>) -> vec3<f32> {
    let l = dot(v, v);
    if (l < 1e-12) {
        return vec3<f32>(0.0);
    }
    return v / sqrt(l);
}

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= sim.trails * POINTS || i >= arrayLength(&outs)) {
        return;
    }
    let t = i / POINTS;
    let k = i % POINTS;
    let h = heads[t];
    var o: Vert;
    o.key = KEY_NONE;
    if (k < h.count) {
        let at = t * POINTS;
        let p = points[at + (h.head + POINTS - k) % POINTS];
        let has_older = k + 1u < h.count && p.width >= 0.0;
        var has_newer = false;
        var to_newer = vec3<f32>(0.0);
        if (k > 0u) {
            let n = points[at + (h.head + POINTS - (k - 1u)) % POINTS];
            has_newer = n.width >= 0.0;
            if (has_newer) {
                to_newer = unit(p.pos - n.pos);
            }
        }
        var to_older = vec3<f32>(0.0);
        if (has_older) {
            to_older = unit(points[at + (h.head + POINTS - (k + 1u)) % POINTS].pos - p.pos);
        }
        // The strip's direction here, shared by the two segments that meet at
        // this point; a full reversal keeps the older side.
        var tan = unit(to_newer + to_older);
        if (dot(tan, tan) == 0.0) {
            tan = to_older;
        }
        let ticks = max(f32(sim.now - p.tick) + sim.frac - 1.0, 0.0);
        let u = ticks * sim.dt / max(h.life, 1e-4);
        var pos = p.pos;
        var scale = abs(p.width) * sim.width * lut(WIDTH, u);
        if (k == 0u) {
            // The emitter's drawn position: a tick behind, as a kit's
            // interpolated draw is. An emitter gone quiet tapers to a point.
            pos = mix(h.prev, p.pos, clamp(ticks, 0.0, 1.0));
            scale *= clamp(2.0 - ticks, 0.0, 1.0);
        } else if (!has_newer) {
            scale = 0.0;
        }
        // The first point of a strip comes to a point too.
        if (!has_older) {
            scale = 0.0;
        }
        let a = lut(ALPHA, u) * clamp((1.0 - u) / max(sim.fade, 1e-3), 0.0, 1.0);
        let c = mix(h.color, h.color_end, clamp(u, 0.0, 1.0));
        o.pos = pos;
        o.width = h.width * scale;
        o.tan = tan;
        o.edge = p.edge * scale;
        // Premultiplied: additive strips cover nothing (alpha 0).
        o.color = vec4<f32>(c.rgb * c.a * a, select(c.a * a, 0.0, (h.key >> 31u) != 0u));
        if (has_older && u < 1.0) {
            o.key = h.key & KEY_MASK;
        }
    }
    outs[i] = o;
}
