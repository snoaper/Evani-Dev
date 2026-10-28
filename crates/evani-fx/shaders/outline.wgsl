// evani.shader.outline
// Ink outlines (one pass, `outline`): depth-edge silhouettes and creases.
// Reads the scene depth (layout BD: uniform at binding 0, depth at 1) and
// multiplies ink into `hdr` (dst * src) before bloom and tonemap, so it never
// reads the colour it writes.
//
// Silhouettes: the second difference of inverse linear depth (1 / distance),
// which is zero across any plane seen in perspective, so long grazing floors
// stay clean. Relative, so a step inks the same at any range.
//
// Creases: the same five depth samples rebuilt into view-space points. On a
// plane the three points along a row (or column) are collinear at any
// viewing angle; at a crease they bend. 1 + cos(angle at the centre point)
// is 0 when flat and about 1 at a right-angle edge, so box edges ink while
// the facets of a round mesh (22 degrees or less) don't. This is what inks
// small, close objects (a kart's body, a driver's helmet) whose depth steps
// are only a few percent of their distance. Fades out with distance, where
// depth precision can't tell a facet from noise.
//
// Only the nearer side of an edge is inked (the sign of the second
// difference: convex creases and the near side of silhouettes), so lines
// stay `thickness` pixels wide. The cleared sky (the depth clear value)
// reads as the far plane: sky next to geometry inks the geometry's edge, sky next to sky gives
// nothing.

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

struct Outline {
    // fx.outline.color (linear RGB ink).
    color: vec3<f32>,
    // fx.outline.strength (0..1).
    strength: f32,
    // fx.outline.thickness (pixels; below 1 fades the line out).
    thickness: f32,
    // fx.outline.depth_sensitivity (higher = more, finer edges).
    depth_sensitivity: f32,
    // 1 / distance = inv_dist.x + inv_dist.y * depth, from the renderer's
    // depth convention (evani-render depth::inverse_distance), so this
    // shader works whichever way depth runs.
    inv_dist: vec2<f32>,
    // fx.outline.crease (0 = silhouettes only, 1 = creases of about 45
    // degrees and sharper ink, higher = gentler creases too).
    crease: f32,
    // View-space size of one pixel at 1 m (2 tan(fov / 2) / height).
    pixel: f32,
    // Distance (m) by which the crease lines have faded out.
    crease_far: f32,
    pad: f32,
};

@group(0) @binding(0) var<uniform> u: Outline;
@group(0) @binding(1) var depth_tex: texture_depth_2d;

// Linear distance (m) from a stored depth value.
fn dist(p: vec2<i32>, size: vec2<i32>) -> f32 {
    let z = textureLoad(depth_tex, clamp(p, vec2<i32>(0), size - vec2<i32>(1)), 0);
    return 1.0 / max(u.inv_dist.x + u.inv_dist.y * z, 1e-12);
}

// View-space point of pixel `p` at distance `d` (any fixed pixel scale keeps
// planes flat, so split-screen views need no per-view projection).
fn view_point(p: vec2<i32>, d: f32) -> vec3<f32> {
    return vec3<f32>(vec2<f32>(p) * (u.pixel * d), d);
}

// How sharply the surface bends at `c` between `a` and `b` (0 = flat).
fn bend(a: vec3<f32>, c: vec3<f32>, b: vec3<f32>) -> f32 {
    let da = a - c;
    let db = b - c;
    let la = length(da);
    let lb = length(db);
    if (la < 1e-6 || lb < 1e-6) {
        return 0.0;
    }
    return 1.0 + dot(da, db) / (la * lb);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let size = vec2<i32>(textureDimensions(depth_tex));
    let p = vec2<i32>(in.pos.xy);
    let t = max(i32(round(u.thickness)), 1);
    let pl = p - vec2<i32>(t, 0);
    let pr = p + vec2<i32>(t, 0);
    let pu = p - vec2<i32>(0, t);
    let pd = p + vec2<i32>(0, t);
    let dc = dist(p, size);
    let dl = dist(pl, size);
    let dr = dist(pr, size);
    let du = dist(pu, size);
    let dd = dist(pd, size);
    let wc = 1.0 / dc;
    // Positive when the neighbours are further away (this pixel is the
    // near side of the edge); relative, so edges read the same at any range.
    let lap_x = (2.0 * wc - 1.0 / dl - 1.0 / dr) / wc;
    let lap_y = (2.0 * wc - 1.0 / du - 1.0 / dd) / wc;
    let gap = max(max(lap_x, lap_y), 0.0) * u.depth_sensitivity;
    var edge = smoothstep(0.04, 0.12, gap);
    // Convex creases: bent rows or columns whose centre is the near side.
    let c = view_point(p, dc);
    let bx = select(0.0, bend(view_point(pl, dl), c, view_point(pr, dr)), lap_x > 0.0);
    let by = select(0.0, bend(view_point(pu, du), c, view_point(pd, dd)), lap_y > 0.0);
    let fade = 1.0 - smoothstep(u.crease_far * 0.5, u.crease_far, dc);
    let crease = smoothstep(0.2, 0.45, max(bx, by) * u.crease) * fade;
    edge = max(edge, crease);
    let k = edge * clamp(u.strength, 0.0, 1.0) * clamp(u.thickness, 0.0, 1.0);
    return vec4<f32>(mix(vec3<f32>(1.0), u.color, k), 1.0);
}
