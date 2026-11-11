// evani.shader.trails_draw
// Ribbons and trails, drawn from the points `trails.sim` wrote. One instanced
// draw per group; an instance is one segment, reading its point and the next
// one (the same buffer bound twice, a point apart, so no vertex-stage storage
// is needed and no mesh is built on the CPU). Each segment is a curve through
// its two points cut into `steps` quads: two segments meeting at a point use
// the same position and direction there, so they share their corners exactly
// and no gap can open. A segment whose first point has another key (a dead
// slot, the end of a strip) is moved off screen. Screen groups draw inside
// the sprites pass, the world group inside the forward pass, depth-tested
// without writing depth. Premultiplied, so blended and additive strips share
// the draw; no sorting.

struct U {
    // World: view-projection and eye. Screen: unused.
    vp: mat4x4<f32>,
    eye: vec3<f32>,
    // The group this draw shows.
    key: u32,
    // Screen: canvas size in CSS pixels.
    viewport: vec2<f32>,
    // Quads per segment (from the tier) and how much of the half width
    // fades to nothing at the strip's edges.
    steps: f32,
    soft: f32,
};

@group(0) @binding(2) var<uniform> u: U;

struct VsIn {
    @builtin(vertex_index) vi: u32,
    // This point: position and width, direction, half span, key, colour.
    @location(0) a_pw: vec4<f32>,
    @location(1) a_tan: vec3<f32>,
    @location(2) a_edge: vec3<f32>,
    @location(3) a_key: u32,
    @location(4) a_color: vec4<f32>,
    // The next point.
    @location(5) b_pw: vec4<f32>,
    @location(6) b_tan: vec3<f32>,
    @location(7) b_edge: vec3<f32>,
    @location(8) b_color: vec4<f32>,
};

struct VsOut {
    @builtin(position) pos: vec4<f32>,
    // Across the strip, -1 to 1.
    @location(0) v: f32,
    @location(1) color: vec4<f32>,
    // The soft edge's share of the half width (less on a widened thin strip,
    // so its few pixels are not all edge).
    @location(2) soft: f32,
};

struct Shape {
    pos: vec3<f32>,
    dir: vec3<f32>,
    w: f32,
    edge: vec3<f32>,
    color: vec4<f32>,
    side: f32,
};

const MIN_PX: f32 = 1.5;

// How much to widen a strip `px` pixels across (1 unless it is thinner than
// `MIN_PX`; a strip with no width, a tapered end, stays a point).
fn thin(px: f32) -> f32 {
    if (px > 0.0 && px < MIN_PX) {
        return MIN_PX / px;
    }
    return 1.0;
}

fn culled() -> VsOut {
    var o: VsOut;
    o.pos = vec4<f32>(2.0, 2.0, 2.0, 1.0);
    o.v = 0.0;
    o.color = vec4<f32>(0.0);
    o.soft = 1.0;
    return o;
}

// The curve at this vertex: a cubic through the two points along their shared
// directions, sized by the segment's length so it never overshoots.
fn shape(in: VsIn) -> Shape {
    var along = array<f32, 6>(0.0, 0.0, 1.0, 1.0, 0.0, 1.0);
    var across = array<f32, 6>(-1.0, 1.0, -1.0, -1.0, 1.0, 1.0);
    let n = u32(max(u.steps, 1.0));
    let q = (in.vi / 6u) % n;
    let c = in.vi % 6u;
    let s = (f32(q) + along[c]) / f32(n);
    let a = in.a_pw.xyz;
    let b = in.b_pw.xyz;
    let l = length(b - a);
    let m0 = in.a_tan * l;
    let m1 = in.b_tan * l;
    let s2 = s * s;
    let s3 = s2 * s;
    var out: Shape;
    out.pos = a * (2.0 * s3 - 3.0 * s2 + 1.0) + m0 * (s3 - 2.0 * s2 + s) + b * (3.0 * s2 - 2.0 * s3) + m1 * (s3 - s2);
    out.dir = (a - b) * (6.0 * s2 - 6.0 * s) + m0 * (3.0 * s2 - 4.0 * s + 1.0) + m1 * (3.0 * s2 - 2.0 * s);
    // At a point the direction is the point's own, not a multiple of it, so
    // both segments compute exactly the same corners there.
    if (s == 0.0) {
        out.dir = in.a_tan;
    } else if (s == 1.0) {
        out.dir = in.b_tan;
    }
    out.w = mix(in.a_pw.w, in.b_pw.w, s);
    out.edge = mix(in.a_edge, in.b_edge, s);
    out.color = mix(in.a_color, in.b_color, s);
    out.side = across[c];
    return out;
}

@vertex
fn vs_world(in: VsIn) -> VsOut {
    if (in.a_key != u.key) {
        return culled();
    }
    let g = shape(in);
    var side = g.edge;
    if (dot(side, side) == 0.0) {
        // Across the strip as the eye sees it.
        var r = cross(g.dir, u.eye - g.pos);
        let l = dot(r, r);
        if (l > 1e-12) {
            r = r / sqrt(l);
        }
        side = r * g.w * 0.5;
    }
    // A strip thinner on screen than `MIN_PX` (a tyre mark seen along the
    // ground, a far trail) is widened to that on screen, in clip space, so it
    // keeps its own depth, and drawn fainter by the square root of the
    // widening: it stays a faint line, and one seen nearly end-on fades out.
    let c0 = u.vp * vec4<f32>(g.pos - side, 1.0);
    let c1 = u.vp * vec4<f32>(g.pos + side, 1.0);
    var c = u.vp * vec4<f32>(g.pos + side * g.side, 1.0);
    var k = 1.0;
    if (c0.w > 0.0 && c1.w > 0.0) {
        // The span in pixels.
        let d = (c1.xy / c1.w - c0.xy / c0.w) * u.viewport * 0.5;
        let px = length(d);
        k = thin(px);
        if (k > 1.0) {
            let grow = d / max(px, 1e-6) * g.side * (MIN_PX - px) / max(u.viewport, vec2<f32>(1.0));
            c = vec4<f32>(c.xy + grow * c.w, c.zw);
        }
    }
    var o: VsOut;
    o.pos = c;
    o.v = g.side;
    o.color = g.color / sqrt(k);
    o.soft = u.soft / k;
    return o;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let e = (1.0 - abs(in.v)) / max(in.soft, 1e-3);
    return in.color * clamp(e, 0.0, 1.0);
}
