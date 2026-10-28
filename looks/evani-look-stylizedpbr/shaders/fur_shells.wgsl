
// evani.shader.fur_shells
// evani-look-stylizedpbr feature `fur`: shells and fins for fur materials.
// Shells: the part drawn again `shells` times (the instance buffer holds
// each instance once per shell, so shell = instance_index % shells), each
// pushed out along the normal by length * h (h = 1/shells .. 1) and drooping
// a little under gravity. A strand grid (cells of 1 / sqrt(density) cm in
// the part's own space) keeps a pixel only where a strand still reaches
// height h, and strands thin towards their tips, so each shell has fewer,
// thinner strands than the one below. Alpha-tested and depth-written: no
// sorting. Fins: quads standing on the fur's edges (own vertex buffer, the
// same instance rows), shorter than the shells, tufts of strand columns
// along the edge, faded out where the edge faces the camera so they only
// show on the silhouette, coloured from the material's base map at the edge
// (float texture coordinates).

struct ShellOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(3) uv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(7) obj: vec3<f32>,
    @location(8) obj_n: vec3<f32>,
    @location(9) h: f32,
};

// Edge rules (look-lab review: High's silhouette read ragged next to the
// painted Low coat). Where a shell is seen edge-on its strands are squeezed
// down to FUR_EDGE_REACH of the fur length (they shorten, not cut off), so
// the outline stays close to the body like Low's (up close, shells stand
// several pixels apart edge-on, so each higher shell would show as a loose
// dot), and edge-on heights whose strands cover less than FUR_EDGE_SPARSE
// of the shell are dropped there. Fins stand only FUR_FIN_REACH of the
// length (never more than the squeezed shells) and are drawn as tufts of
// FUR_FIN_TUFT strand columns, each a soft wedge with a pointed tip, never
// as single hairs a pixel or less wide.
const FUR_EDGE_REACH: f32 = 0.3;
const FUR_FIN_REACH: f32 = 0.3;
const FUR_FIN_TUFT: f32 = 3.0;
const FUR_EDGE_SPARSE: f32 = 0.25;

// How edge-on the surface is to the camera: 0 facing it, 1 edge-on or
// turned away (shells on the far side only ever show past the outline).
// The ramp starts well before the outline: strands of a surface still
// half facing the camera stand out sideways too, and at full length their
// sparse tips poked past the squeezed edge as loose dark dots.
fn fur_edge(n: vec3<f32>, world: vec3<f32>) -> f32 {
    let v = normalize(frame.eye.xyz - world);
    return 1.0 - smoothstep(0.1, 0.75, dot(n, v));
}

// Two hash values in 0..1 for an integer cell (own mixing constants).
fn fur_hash(c: vec2<i32>) -> vec2<f32> {
    var h = (bitcast<u32>(c.x) * 0x9e3d5a27u) ^ (bitcast<u32>(c.y) * 0x6c8e9cf5u);
    h = h ^ (h >> 15u);
    h = h * 0x2545f491u;
    h = h ^ (h >> 13u);
    let g = (h * 0x7a3c19e5u) ^ (h >> 16u);
    return vec2<f32>(f32(h & 0xffffu), f32(g & 0xffffu)) / 65535.0;
}

// 1 where a strand of cell grid `q` reaches height h (0 root .. 1 tip).
fn fur_strand(q: vec2<f32>, h: f32, clump_height: f32) -> f32 {
    let cell = floor(q);
    let f = q - cell;
    let r = fur_hash(vec2<i32>(cell));
    let centre = vec2<f32>(0.25) + r * 0.5;
    let len = mix(0.55, 1.0, r.y) * mix(0.6, 1.0, clamp(clump_height, 0.0, 1.0));
    let t = h / max(len, 1e-3);
    let radius = 0.5 * (1.0 - 0.85 * t);
    return select(0.0, 1.0, t <= 1.0 && distance(f, centre) < radius);
}

// Share of a shell at height h that strands cover, on average over many
// cells (the same length and taper rules as fur_strand, in closed form).
fn fur_coverage(h: f32, clump_height: f32) -> f32 {
    let a = mix(0.6, 1.0, clamp(clump_height, 0.0, 1.0));
    // Strand length is a * u with u even in 0.55..1; those with u >= u0 reach h.
    let u0 = h / max(a, 1e-3);
    let reach = clamp((1.0 - u0) / 0.45, 0.0, 1.0);
    let len = a * 0.5 * (max(u0, 0.55) + 1.0);
    let radius = 0.5 * (1.0 - 0.85 * clamp(h / max(len, 1e-3), 0.0, 1.0));
    return reach * min(3.14159265 * radius * radius, 1.0);
}

// Keep this shell pixel? `footprint` is strand cells per pixel. Up close it
// is the exact strand test; once strands shrink below about a pixel that
// test aliases into rings, so it hands over (per cell, between 0.5 and 1
// cells per pixel) to the average coverage tested against the hash of a
// coarser cell about a pixel wide. `axes` is the footprint along each grid
// axis: the coarse cell is sized per axis, so where the surface turns away
// (strands squeezed along one axis only) it stays about a pixel each way
// instead of a long block of many pixels (the blocky patch by the ear). The
// coarse hash ignores h, so a cell kept high up is kept on every shell below
// it and still reads as a strand; it lives in the part's own space, so it
// holds still while the camera moves.
fn fur_keep(q: vec2<f32>, axes: vec2<f32>, h: f32, clump_height: f32) -> bool {
    let footprint = max(axes.x, axes.y);
    let exact = fur_strand(q, h, clump_height) > 0.5;
    let level = max(ceil(log2(max(axes, vec2<f32>(1e-3)))), vec2<f32>(0.0));
    let coarse = vec2<i32>(floor(q / exp2(level)));
    let r = fur_hash(coarse + vec2<i32>(i32(level.x) * 7919, 104729 + i32(level.y) * 613));
    let averaged = r.x < fur_coverage(h, clump_height);
    let handover = clamp(footprint * 2.0 - 1.0, 0.0, 1.0);
    return select(exact, averaged, r.y < handover);
}

// Strand cells per metre for the material's density (strands per cm^2).
fn fur_cells_per_m() -> f32 {
    return sqrt(max(surface.fur.y, 0.01)) * 100.0;
}

// Fur colour at height h. Levels matched by eye (look-lab, v2 chimp arm
// and the plush) so the coat seen through the shells keeps the tone of
// the painted fur Low and Lite draw: about a quarter of the coat is the
// dark root surface showing between the strands (surface_fur), so the
// strands sit a little above the base colour to bring the mix back to the
// painted coat's level (arm luma within about 2 points of Low and Lite).
fn fur_colour(base: vec3<f32>, clump: vec4<f32>, h: f32) -> vec3<f32> {
    let rooted = base * mix(1.2, 1.3, h) * mix(0.9, 1.1, clump.g);
    return mix(rooted, surface.tip.rgb, 0.45 * h * h);
}

// A strand's own occlusion at height h (its neighbours hide the root). It
// stands in for GTAO on the strands: with feature `gtao` they write alpha
// 1.95, so GTAO leaves them alone.
fn fur_occ(h: f32) -> f32 {
    return mix(0.9, 1.0, h);
}

// Light on a strand: smooth wrapped diffuse, shadowed, darker towards the
// roots (self-occlusion), and a soft rim sheen near the tips. The sky's
// reflection, the specular term and the shadow tint follow model_lit_fs
// (rough, non-metal), so the strands keep the tone and hue of the painted
// coat Low and Lite draw: without them High's arm read about 10 luma
// darker and redder (look-lab review).
fn fur_light(albedo: vec3<f32>, world: vec3<f32>, n: vec3<f32>, h: f32) -> vec3<f32> {
    let dist = length(world - frame.eye.xyz);
    let v = (frame.eye.xyz - world) / max(dist, 1e-4);
    let l = frame.light_dir.xyz;
    let ndl = dot(n, l);
    let ndv = max(dot(n, v), 1e-3);
    // Shadowed as the coat at the strand's root (the shell's offset and
    // droop undone), so strands standing off the arm never pick up shadow
    // the painted coat would not have.
    let len = surface.fur.x;
    let root = world - n * (len * h) + vec3<f32>(0.0, len * 0.35 * h * h, 0.0);
    let vis = shadow_vis(root, n) * blob_vis(root, n) * self_shadow(ndl);
    let ao = fur_occ(h);
    let diffuse = slot_toon_ramp(ndl) * vis * mix(0.9, 1.0, h);
    let rough = clamp(surface.pbr.y, 0.04, 1.0);
    let f0 = vec3<f32>(0.04);
    let spec = slot_specular(max(dot(n, normalize(l + v)), 0.0), max(ndl, 0.0), ndv, rough, 0.0, albedo) * vis;
    let sp0 = slot_surface_params(0u);
    let r2 = rough * rough;
    let env_y = mix(reflect(-v, n).y, n.y, r2);
    let env = surface_hemi(env_y) * surface_env_fresnel(ndv, f0, rough) * (sp0.y * mix(max(sp0.z, 1.0), 1.0, r2) * ao);
    var c = albedo * (surface_hemi(n.y) * ao + frame.light_color.rgb * diffuse) + frame.light_color.rgb * spec + env
        + slot_rim(ndv, ndl, dist, albedo) * h;
    let k = clamp(1.0 - vis, 0.0, 1.0) * clamp(frame.shadow_tint.w, 0.0, 1.0);
    let w = vec3<f32>(0.2126, 0.7152, 0.0722);
    let ts = sqrt(max(frame.shadow_tint.rgb, vec3<f32>(0.0)));
    let tt = ts / max(dot(ts, w), 1e-3) * dot(sqrt(max(c, vec3<f32>(0.0))), w);
    let tc = tt / max(1.0, max(tt.r, max(tt.g, tt.b)));
    c = mix(c, tc * tc, k);
    let f = clamp(slot_fog(dist, world.y), 0.0, 1.0);
    return mix(c, frame.fog_color.rgb, f);
}

@vertex
fn fur_shell_vs(in: ModelIn, @builtin(instance_index) ii: u32) -> ShellOut {
    let shells = max(u32(surface.fur.z + 0.5), 1u);
    let h = (f32(ii % shells) + 1.0) / f32(shells);
    let local = in.pos_offset.xyz + in.pos_scale.xyz * in.pos.xyz;
    let l4 = vec4<f32>(local, 1.0);
    let root = vec3<f32>(dot(in.row0, l4), dot(in.row1, l4), dot(in.row2, l4));
    let c0 = vec3<f32>(in.row0.x, in.row1.x, in.row2.x);
    let c1 = vec3<f32>(in.row0.y, in.row1.y, in.row2.y);
    let c2 = vec3<f32>(in.row0.z, in.row1.z, in.row2.z);
    let x12 = cross(c1, c2);
    let s = select(1.0, -1.0, dot(c0, x12) < 0.0);
    let n = normalize(mat3x3<f32>(x12, cross(c2, c0), cross(c0, c1)) * in.normal.xyz * s);
    let len = surface.fur.x;
    var world = root + n * (len * h);
    world.y = world.y - len * 0.35 * h * h;
    var o: ShellOut;
    o.pos = frame.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = n;
    o.uv = in.uv_dq.zw + in.uv_dq.xy * in.uv;
    o.color = in.tint;
    o.obj = local * length(c0);
    o.obj_n = in.normal.xyz;
    o.h = h;
    return o;
}

@fragment
fn fur_shell_fs(in: ShellOut) -> @location(0) vec4<f32> {
    let uv3 = vec3<f32>(in.uv, 1.0);
    let uv = vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3));
    let tex = textureSample(base_map, map_sampler, uv);
    let clump = fur_sample(in.obj, in.obj_n, surface.fur.w);
    let q = fur_plane(in.obj, fur_axis(in.obj_n)) * fur_cells_per_m();
    // Derivatives before any discard (uniform control flow).
    let axes = max(abs(dpdx(q)), abs(dpdy(q)));
    let n = normalize(in.normal);
    // Edge-on, the strands are squeezed shorter (h read against a shorter
    // reach), and heights where strands would only be stray dots (coverage
    // under FUR_EDGE_SPARSE) are left to the fins' tufts.
    let edge = fur_edge(n, in.world);
    let reach = mix(1.0, FUR_EDGE_REACH, edge);
    let h = in.h / reach;
    // Squeezed strands also thin out, so the lowest shell fills in towards
    // the edge (share = edge): the dark roots would otherwise show through
    // where the painted coat reads as a full, lighter edge.
    let fill = in.h * surface.fur.z < 1.5 && fur_hash(vec2<i32>(floor(q)) + vec2<i32>(31, 7)).x < edge;
    if (!fill && (fur_coverage(h, clump.r) < FUR_EDGE_SPARSE * sqrt(edge) || !fur_keep(q, axes, h, clump.r))) {
        discard;
    }
    let base = tex.rgb * model_mat.base.rgb * in.color.rgb;
    return vec4<f32>(fur_light(fur_colour(base, clump, in.h), in.world, n, in.h), 1.0);
}

struct FinIn {
    // Edge point in the part's local units, its normal, and (u along the
    // edge in metres, v 0 at the root .. 1 at the tip).
    @location(0) pos: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(2) fuv: vec2<f32>,
    @location(3) tex: vec2<f32>,
    @location(4) row0: vec4<f32>,
    @location(5) row1: vec4<f32>,
    @location(6) row2: vec4<f32>,
    @location(7) pos_scale: vec4<f32>,
    @location(8) pos_offset: vec4<f32>,
    @location(9) uv_dq: vec4<f32>,
    @location(10) tint: vec4<f32>,
};

struct FinOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) world: vec3<f32>,
    @location(1) normal: vec3<f32>,
    @location(3) fuv: vec2<f32>,
    @location(4) color: vec4<f32>,
    @location(5) tex: vec2<f32>,
    @location(9) fade: f32,
};

@vertex
fn fur_fin_vs(in: FinIn) -> FinOut {
    let l4 = vec4<f32>(in.pos, 1.0);
    let root = vec3<f32>(dot(in.row0, l4), dot(in.row1, l4), dot(in.row2, l4));
    let c0 = vec3<f32>(in.row0.x, in.row1.x, in.row2.x);
    let c1 = vec3<f32>(in.row0.y, in.row1.y, in.row2.y);
    let c2 = vec3<f32>(in.row0.z, in.row1.z, in.row2.z);
    let x12 = cross(c1, c2);
    let s = select(1.0, -1.0, dot(c0, x12) < 0.0);
    let n = normalize(mat3x3<f32>(x12, cross(c2, c0), cross(c0, c1)) * in.normal * s);
    let len = surface.fur.x;
    // The fin stands FUR_FIN_REACH of the fur length (h = height in the fur).
    let h = clamp(in.fuv.y, 0.0, 1.0) * FUR_FIN_REACH;
    var world = root + n * (len * h);
    world.y = world.y - len * 0.35 * h * h;
    let v = normalize(frame.eye.xyz - root);
    var o: FinOut;
    o.pos = frame.view_proj * vec4<f32>(world, 1.0);
    o.world = world;
    o.normal = n;
    o.fuv = in.fuv;
    o.tex = in.tex;
    o.color = in.tint;
    // Edge-on to the camera (on the silhouette) = 1, facing it = 0.
    o.fade = 1.0 - smoothstep(0.2, 0.5, abs(dot(n, v)));
    return o;
}

@fragment
fn fur_fin_fs(in: FinOut) -> @location(0) vec4<f32> {
    // Sampled before any discard (uniform control flow).
    let uv3 = vec3<f32>(in.tex, 1.0);
    let tex = textureSample(base_map, map_sampler, vec2<f32>(dot(model_mat.uv0.xyz, uv3), dot(model_mat.uv1.xyz, uv3)));
    // v: 0 root .. 1 top of the fin; h: the same point as a height in the fur.
    let v = clamp(in.fuv.y, 0.0, 1.0);
    let h = v * FUR_FIN_REACH;
    let u = in.fuv.x * fur_cells_per_m();
    // Tufts at least about two pixels wide: FUR_FIN_TUFT columns up close,
    // doubling in steps (whole levels, so they do not swim) further away.
    let per_px = max(length(vec2<f32>(dpdx(u), dpdy(u))), 1e-4);
    let level = max(ceil(log2(per_px * 2.0 / FUR_FIN_TUFT)), 0.0);
    let size = FUR_FIN_TUFT * exp2(level);
    let x = u / size;
    let tuft = floor(x);
    let r = fur_hash(vec2<i32>(i32(tuft), 7919 + i32(level) * 31));
    // A tuft's length (shorter where the edge starts to face the camera, so
    // the fringe tapers in instead of popping), and a wedge that is a full
    // tuft wide at the root and closes to a point at its tip, leaning a
    // little to one side.
    let len = mix(0.6, 1.0, r.y) * mix(0.5, 1.0, in.fade);
    let t = v / len;
    let lean = (r.x - 0.5) * 0.35 * t;
    let width = 0.55 * (1.0 - t) * (1.0 - 0.3 * t);
    // Fade whole tufts (a hash per tuft), so the silhouette thins out tuft
    // by tuft instead of breaking into dots.
    let keep = fur_hash(vec2<i32>(i32(tuft), 104729));
    if (t > 1.0 || abs(x - tuft - 0.5 - lean) > width || in.fade <= keep.x * 0.8) {
        discard;
    }
    let base = tex.rgb * model_mat.base.rgb * in.color.rgb;
    let clump = vec4<f32>(len, 0.5 + 0.5 * r.x, r.y, 1.0);
    return vec4<f32>(fur_light(fur_colour(base, clump, h), in.world, normalize(in.normal), h), 1.0);
}
