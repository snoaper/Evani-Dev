// Clustered forward light binning (evani-render feature `clustered`, S5-R3).
// One thread per cluster: 16 x 9 screen tiles x 16 depth slices per view
// (exponential from `info.w`; slice 0 starts at the eye and the last runs
// on), up to 4 views. Each cluster's view-space box is tested against every
// light's sphere and keeps up to 31 of them: word 0's low byte is the count,
// bytes 1..31 of the cluster's 8 words are light indices. The light record
// layout matches gpu/lights.rs.

struct PointLight { pos_range: vec4<f32>, color: vec4<f32> }
struct LightView { rect: vec4<f32>, eye: vec4<f32>, right: vec4<f32>, up: vec4<f32>, fwd: vec4<f32> }
struct Lights { info: vec4<f32>, views: array<LightView, 4>, list: array<PointLight, 256> }

@group(0) @binding(0) var<uniform> lights: Lights;
@group(0) @binding(1) var<storage, read_write> clusters: array<u32>;

const TILES_X: u32 = 16u;
const TILES_Y: u32 = 9u;
const SLICES: u32 = 16u;
const PER_VIEW: u32 = 2304u; // TILES_X * TILES_Y * SLICES
const MAX_IN: u32 = 31u;

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let c = id.x;
    if (c >= min(u32(lights.info.y), 4u) * PER_VIEW) {
        return;
    }
    let vw = lights.views[c / PER_VIEW];
    let t = c % PER_VIEW;
    let tx = t % TILES_X;
    let ty = (t / TILES_X) % TILES_Y;
    let tz = t / (TILES_X * TILES_Y);
    // Tile edges in NDC (y up; tile row 0 is the top) scaled by the half
    // field of view, so a point at view depth z spans `edge * z`.
    let a = vec2<f32>(-1.0 + 2.0 * f32(tx) / f32(TILES_X), 1.0 - 2.0 * f32(ty + 1u) / f32(TILES_Y)) * vec2<f32>(vw.right.w, vw.up.w);
    let b = a + vec2<f32>(2.0 / f32(TILES_X), 2.0 / f32(TILES_Y)) * vec2<f32>(vw.right.w, vw.up.w);
    let z0 = select(lights.info.w * exp2(f32(tz) / lights.info.z), 0.0, tz == 0u);
    let z1 = select(lights.info.w * exp2(f32(tz + 1u) / lights.info.z), 1e5, tz == SLICES - 1u);
    let lo = vec3<f32>(min(a * z0, a * z1), z0);
    let hi = vec3<f32>(max(b * z0, b * z1), z1);
    var words: array<u32, 8>;
    var count = 0u;
    let n = min(u32(lights.info.x), 256u);
    for (var i = 0u; i < n; i++) {
        let l = lights.list[i];
        let d = l.pos_range.xyz - vw.eye.xyz;
        let lv = vec3<f32>(dot(d, vw.right.xyz), dot(d, vw.up.xyz), dot(d, vw.fwd.xyz));
        let e = lv - clamp(lv, lo, hi);
        if (count < MAX_IN && dot(e, e) <= l.pos_range.w * l.pos_range.w) {
            count += 1u;
            words[count / 4u] |= i << ((count % 4u) * 8u);
        }
    }
    words[0] |= count;
    for (var k = 0u; k < 8u; k++) {
        clusters[c * 8u + k] = words[k];
    }
}
