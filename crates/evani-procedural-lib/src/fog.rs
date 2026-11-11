//! Distance fog (`sky-distance-fog`, Asset Studio column 77): the air's own
//! fog, worked out for every pixel from how far away and how high the thing
//! behind it is. One full-screen game pass after the opaque 3D; no meshes.
//!
//! What it does, in the order the picture shows it:
//! - **Clear air round the eye.** The fog's density rises smoothly from
//!   nothing at half `start` to full at one and a half, so what stands a few
//!   metres away stays crisp and dark ([`reach`]).
//! - **Thickens with distance.** Past that it is an exponential fog whose
//!   density along the ground loses 95% of a thing at `visibility`
//!   ([`density`]), with a long faint tail (`tail`, `reach`): the far things
//!   never snap out, the last stay as faint shapes, and the ground runs into
//!   the sky with no line.
//! - **Thicker near the ground.** The density is a ground layer `height`
//!   metres deep (it falls by e every `height`) over a share that stays at
//!   every height (`aloft`), taken as the mean along the ray, so the foot of
//!   a far pillar is more lost than its top ([`layer_mean`]).
//! - **The sky is the fog.** A pixel with nothing behind it looks through
//!   the whole layer and the air aloft: with any `aloft` the sky is the
//!   fog's colour (`sky` lets some of the scene's own sky through).
//! - **Its colour.** `color` is the fog high up; it is a little darker
//!   along the horizon, where the eye looks through the dense ground layer
//!   (`horizon`, `rise`), and a little darker again looking down at the
//!   ground (`ground`, `dip`). A fine `grain` keeps its smooth gradients
//!   from showing as bands.
//! - **It softens what it covers.** The light of a far thing is scattered
//!   on its way, so its edges blur a little into what lies beside and behind
//!   it, more the more fog there is (`soft`); nearer things stay sharp in
//!   front.
//!
//! [`DistanceFog`] is hosted by the procedural module whenever the asset is
//! compiled in (as the volumetric clouds' volume is), so a game that lists
//! the asset and places it once gets the fog; the placement's height is the
//! fog's floor. The generator's CPU mesh ([`bank`]) is only the fog's
//! bounds, for tests and a host without the pass.
//!
//! Written fresh; the ideas are the usual ones for height fog in games (an
//! exponential density integrated in closed form along the view ray).

use evani_core::ecs::World;
use evani_core::params::{ParamId, ParamRegistry};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};
use evani_procedural::geo::{self, Mesh};
use evani_procedural::{ProcScene, Registry};
use evani_render::game_pass::{self, GamePass, GamePasses, PassParam, Stage};

/// The asset's feature.
pub const FEATURE: &str = "sky-distance-fog";
/// The fog pass; its live params are `procedural.distance_fog.*`.
pub const PASS: &str = "procedural.distance_fog";

/// The asset's own defaults (crate::sky registers them; the staging's look).
pub const VISIBILITY: f32 = 55.0;
pub const START: f32 = 8.0;
pub const HEIGHT: f32 = 6.0;
pub const COLOR: [f32; 3] = [0.643, 0.665, 0.727];
/// The pass's defaults.
pub const ALOFT: f32 = 0.5;
pub const TAIL: f32 = 0.09;
pub const REACH: f32 = 8.0;

/// The data records the pass reads (colour and switch; density, start,
/// layer height and floor).
const HEADER: usize = 4;

/// The fog's density along the ground (1/m): 95% of a thing is lost at
/// `visibility`, counted from where the fog starts.
pub fn density(visibility: f32, start: f32) -> f32 {
    3.0 / (visibility - start).max(1.0)
}

/// The metres of fog along `l` metres of ray from the eye: the density
/// rises smoothly (a smoothstep) from 0 at half `start` to full at one and a
/// half, so it is `l - start` far away.
pub fn reach(l: f32, start: f32) -> f32 {
    if start < 0.01 {
        return l.max(0.0);
    }
    let t = ((l - 0.5 * start) / start).clamp(0.0, 1.0);
    start * (t * t * t - 0.5 * t * t * t * t) + (l - 1.5 * start).max(0.0)
}

/// The ground layer's mean density (as a share of its value at the floor)
/// along a ray between two heights: 1 under the floor, falling by e every
/// `height` above it.
pub fn layer_mean(ya: f32, yb: f32, floor: f32, height: f32) -> f32 {
    let lo = ya.min(yb) - floor;
    let hi = ya.max(yb) - floor;
    let span = hi - lo;
    let (a, b) = (lo.max(0.0), hi.max(0.0));
    if span < 0.01 {
        return (-a / height).exp();
    }
    let below = (-lo).clamp(0.0, span);
    (below + height * ((-a / height).exp() - (-b / height).exp())) / span
}

/// How much of a thing is left through an optical depth: the fog proper and
/// its long faint tail (`tail` of it thins `tail_reach` times slower).
pub fn left(tau: f32, tail: f32, tail_reach: f32) -> f32 {
    (1.0 - tail) * (-tau).exp() + tail * (-tau / tail_reach.max(1.0)).exp()
}

/// How much of a thing `l` metres away at height `y` the fog covers, seen
/// from `eye` (its height), with the pass's default `aloft`, `tail` and
/// `reach`: the pass's own sum, for tests and for kits that want the number.
pub fn cover(l: f32, eye: f32, y: f32, floor: f32, visibility: f32, start: f32, height: f32) -> f32 {
    let mean = ALOFT + (1.0 - ALOFT) * layer_mean(eye, y, floor, height.max(0.05));
    1.0 - left(density(visibility, start) * reach(l, start) * mean, TAIL, REACH)
}

/// The generator's CPU mesh: the fog bank's bounds, a low ring from where
/// the fog starts out to its visibility, as deep as its ground layer.
pub fn bank(visibility: f32, start: f32, height: f32) -> Mesh {
    let inner = start.min(0.9 * visibility).max(0.0);
    geo::lathe(&[[inner, 0.0], [visibility, 0.0], [visibility, height], [inner, height], [inner, 0.0]], 48)
}

const FOG_WGSL: &str = "
// Above this the sky ray's length through the air aloft is cut off (m).
const FOG_SKY_FAR: f32 = 20000.0;

// The metres of fog along `l` metres of ray: clear round the eye, the density
// rising smoothly from half `s` to one and a half.
fn fog_reach(l: f32, s: f32) -> f32 {
    if (s < 0.01) { return max(l, 0.0); }
    let t = clamp((l - 0.5 * s) / s, 0.0, 1.0);
    return s * (t * t * t - 0.5 * t * t * t * t) + max(l - 1.5 * s, 0.0);
}

// The ground layer's mean density between two heights (1 under the floor,
// falling by e every `h` above it).
fn fog_layer_mean(ya: f32, yb: f32, base: f32, h: f32) -> f32 {
    let lo = min(ya, yb) - base;
    let hi = max(ya, yb) - base;
    let span = hi - lo;
    let a = max(lo, 0.0);
    let b = max(hi, 0.0);
    if (span < 0.01) { return exp(-a / h); }
    let below = clamp(-lo, 0.0, span);
    return (below + h * (exp(-a / h) - exp(-b / h))) / span;
}

// White noise per pixel, 0 to 1.
fn fog_noise(p: vec2<f32>) -> f32 {
    return fract(sin(dot(p, vec2<f32>(12.9898, 78.233))) * 43758.5453);
}

fn pass_main(uv: vec2<f32>, px: vec2<f32>) -> vec4<f32> {
    let here = textureSampleLevel(scene, samp, uv, 0.0);
    let head = data[0];
    if (head.w < 0.5) { return here; }
    // density along the ground (1/m), start (m), layer height (m), floor (m)
    let k = data[1];
    let ro = frame.camera.xyz;
    let rd = view_ray(uv);
    let dz = textureLoad(depth, vec2<i32>(px), 0);
    let aloft = param_aloft();
    var tau = 0.0;
    var most = 1.0;
    if (dz > 0.0) {
        let p = world_at(uv, dz);
        let mean = aloft + (1.0 - aloft) * fog_layer_mean(ro.y, p.y, k.w, k.z);
        tau = k.x * fog_reach(length(p - ro), k.y) * mean;
    } else {
        // The sky: through the whole ground layer and the air aloft.
        let e = ro.y - k.w;
        let through = (max(-e, 0.0) + exp(-max(e, 0.0) / k.z) * k.z) / max(rd.y, 1e-4);
        tau = k.x * (aloft * FOG_SKY_FAR + (1.0 - aloft) * min(through, FOG_SKY_FAR));
        most = param_sky();
    }
    let tail = param_tail();
    let left = (1.0 - tail) * exp(-tau) + tail * exp(-tau / max(param_reach(), 1.0));
    // Its colour: darker along the horizon than high up, darker again
    // looking down at the ground.
    let up = smoothstep(0.0, max(param_rise(), 0.001), rd.y);
    let down = smoothstep(0.0, max(param_dip(), 0.001), -rd.y);
    // A fine grain (two noises: a triangular spread) so the smooth
    // gradients never show as bands on an 8-bit screen.
    let grain = (fog_noise(px) + fog_noise(px + vec2<f32>(17.3, 5.7)) - 1.0) * param_grain();
    let col = max(head.rgb * (1.0 - param_horizon() * (1.0 - up)) * (1.0 - param_ground() * down) + vec3<f32>(grain), vec3<f32>(0.0));
    let cover = clamp((1.0 - left) * most, 0.0, 1.0);
    // The fog also softens what it covers: the light of a far thing is
    // scattered on its way, so its edges blur into what lies beside and
    // behind it (never into nearer things, which stay sharp in front).
    // Twelve taps on a turned spiral, wider the more fog there is.
    var seen = here.rgb;
    let r = param_soft() * cover * cover;
    if (dz > 0.0 && r * frame.size.y > 0.25) {
        var sum = here.rgb;
        var n = 1.0;
        let turn = fog_noise(px + vec2<f32>(3.1, 9.2)) * 6.2831853;
        let last = vec2<i32>(frame.size.xy) - vec2<i32>(1);
        for (var i = 0; i < 12; i++) {
            let a = turn + f32(i) * 2.3999632;
            let d = r * sqrt((f32(i) + 0.5) / 12.0);
            let q = uv + vec2<f32>(cos(a) * d * frame.size.y * frame.size.z, sin(a) * d);
            let dn = textureLoad(depth, clamp(vec2<i32>(q * frame.size.xy), vec2<i32>(0), last), 0);
            if (dn <= dz * 1.05) {
                sum += textureSampleLevel(scene, samp, q, 0.0).rgb;
                n += 1.0;
            }
        }
        seen = sum / n;
    }
    return vec4<f32>(mix(seen, col, cover), here.a);
}
";

/// Draws the placed fog (hosted by the procedural module).
#[derive(Debug, Default)]
pub struct DistanceFog {
    /// Whether the pass is in (a build or app without the render module has
    /// none, and nothing is drawn).
    ready: bool,
    /// The param revision and the placed fog's floor the data was made for.
    made_at: Option<(u64, Option<f32>)>,
    /// The asset's own params (only their changes remake the data).
    ids: Vec<ParamId>,
}

impl DistanceFog {
    pub fn new() -> Self {
        Self::default()
    }

    /// The pass's data for a fog placed with its floor at `floor` (none
    /// placed, or switched off: the pass draws nothing).
    fn data(world: &World, floor: Option<f32>) -> Vec<[f32; 4]> {
        let mut d = vec![[0.0; 4]; HEADER];
        if let (Some(floor), Some(registry), Some(reg)) = (floor, world.try_res::<Registry>(), world.try_res::<ParamRegistry>())
            && registry.is_on(&reg, FEATURE)
            && let Some((v, _)) = registry.values(&reg, FEATURE)
        {
            let c = v.color("color");
            let (vis, start) = (v.f("visibility"), v.f("start"));
            d[0] = [c[0], c[1], c[2], 1.0];
            d[1] = [density(vis, start), start.max(0.0), v.f("height").max(0.05), floor];
        }
        d
    }
}

impl EngineModule for DistanceFog {
    fn id(&self) -> &'static str {
        "evani.module.procedural.fog"
    }

    fn label(&self) -> &'static str {
        "procedural-fog"
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        if ctx.world.try_res::<GamePasses>().is_none() {
            return Ok(());
        }
        game_pass::add(
            ctx,
            GamePass::fullscreen(PASS, FOG_WGSL)
                .at(Stage::AfterOpaque)
                .reads("scene", "hdr")
                .reads_depth("depth")
                .param(PassParam::f32("aloft", ALOFT, 0.0, 1.0).describe("How much of the ground's fog is left high up (1: the same at every height, 0: a layer with clear sky over it)"))
                .param(PassParam::f32("tail", TAIL, 0.0, 0.5).describe("The share of far things that stays faintly visible (the long soft tail)"))
                .param(PassParam::f32("reach", REACH, 1.0, 30.0).describe("How many times farther the faint tail carries"))
                .param(PassParam::f32("sky", 1.0, 0.0, 1.0).describe("How much of the sky the fog covers"))
                .param(PassParam::f32("horizon", 0.13, 0.0, 0.8).describe("How much darker the fog is along the horizon than high up"))
                .param(PassParam::f32("rise", 0.22, 0.02, 1.0).describe("How high (the sine of the angle up) the fog reaches its full brightness"))
                .param(PassParam::f32("ground", 0.1, 0.0, 0.8).describe("How much darker again the fog is looking down at the ground"))
                .param(PassParam::f32("dip", 0.05, 0.005, 0.5).describe("How far below the horizon (the sine of the angle down) that darkening is complete"))
                .param(PassParam::f32("soft", 0.003, 0.0, 0.02).describe("How far the fog blurs the edges of far things (a share of the picture's height at full fog; 0: none)"))
                .param(PassParam::f32("grain", 0.009, 0.0, 0.05).describe("A fine grain that keeps the fog's smooth gradients from showing as bands (0: none)")),
        )?;
        self.ready = true;
        Ok(())
    }

    fn frame(&mut self, world: &mut World) {
        if !self.ready {
            return;
        }
        if self.ids.is_empty()
            && let (Some(registry), Some(reg)) = (world.try_res::<Registry>(), world.try_res::<ParamRegistry>())
        {
            self.ids = registry.param_names(FEATURE).iter().filter_map(|n| reg.id(n)).collect();
        }
        let (rev, changed, floor) = {
            let Some(scene) = world.try_res::<ProcScene>() else {
                return;
            };
            let reg = world.res::<ParamRegistry>();
            let floor = scene.placements.iter().find(|p| p.feature == FEATURE).map(|p| p.position[1]);
            let changed = self.made_at.is_none_or(|(r, _)| reg.any_changed_since(&self.ids, r));
            (reg.revision(), changed, floor)
        };
        if changed || self.made_at.is_none_or(|(_, f)| f != floor) {
            let data = Self::data(world, floor);
            if let Some(mut gp) = world.try_res_mut::<GamePasses>()
                && let Some(d) = gp.data_mut(PASS)
            {
                d.clone_from(&data);
            }
            self.made_at = Some((rev, floor));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const EYE: f32 = 1.7;

    fn at(l: f32, y: f32) -> f32 {
        cover(l, EYE, y, 0.0, VISIBILITY, START, HEIGHT)
    }

    /// Near things stay crisp, middle ones are half lost, far ones fade to
    /// faint shapes without ever snapping out: the staging's three rows.
    #[test]
    fn the_fog_thickens_from_near_to_far_with_a_long_tail() {
        assert!(at(3.0, 0.5) < 1e-4, "clear at the feet: {}", at(3.0, 0.5));
        assert!(at(7.7, 0.5) < 0.06, "the near cubes stay crisp: {}", at(7.7, 0.5));
        assert!((0.4..0.65).contains(&at(22.5, 0.6)), "the middle row is half lost: {}", at(22.5, 0.6));
        let far: Vec<f32> = [45.0, 70.0, 100.0, 140.0].iter().map(|&l| at(l, 2.5)).collect();
        assert!(far.windows(2).all(|w| w[1] > w[0]), "{far:?}");
        assert!(far[0] > 0.7 && (0.93..0.995).contains(&far[3]), "far pillars fade to faint shapes: {far:?}");
        // No kink where the fog starts, and never less with distance.
        let mut last = 0.0;
        for i in 0..4000 {
            let f = at(i as f32 * 0.1, 0.0);
            assert!(f >= last - 1e-6 && f - last < 0.01, "at {} m: {last} -> {f}", i as f32 * 0.1);
            last = f;
        }
        assert!(at(5000.0, 0.0) > 0.999, "the ground runs into the fog");
    }

    #[test]
    fn the_fog_is_thicker_near_the_ground() {
        // The foot of a far pillar is more lost than its top.
        assert!(at(70.0, 0.3) > at(70.0, 5.0) + 0.01);
        let low = layer_mean(1.7, 0.0, 0.0, HEIGHT);
        let high = layer_mean(1.7, 6.0, 0.0, HEIGHT);
        assert!(low > high && high > 0.4 && low < 1.0, "{low} {high}");
        // Under its floor the layer is whole; level rays take the height's own value.
        assert!((layer_mean(-3.0, -1.0, 0.0, HEIGHT) - 1.0).abs() < 1e-6);
        assert!((layer_mean(6.0, 6.0, 0.0, 6.0) - (-1.0f32).exp()).abs() < 1e-5);
        // A deeper layer thins less over the same climb.
        assert!(layer_mean(1.7, 6.0, 0.0, 30.0) > high);
    }

    #[test]
    fn the_start_and_the_visibility_mean_what_they_say() {
        assert!((reach(100.0, 8.0) - 92.0).abs() < 1e-3);
        assert_eq!(reach(4.0, 8.0), 0.0);
        assert_eq!(reach(50.0, 0.0), 50.0);
        // 95% of the fog proper at the visibility, along the ground.
        let tau = density(60.0, 8.0) * reach(60.0, 8.0);
        assert!((tau - 3.0).abs() < 1e-4, "{tau}");
        assert!(left(3.0, 0.0, 8.0) < 0.05 && left(3.0, TAIL, REACH) > left(3.0, 0.0, REACH));
        // A longer visibility is a thinner fog.
        assert!(cover(30.0, EYE, 1.0, 0.0, 200.0, START, HEIGHT) < at(30.0, 1.0));
    }

    #[test]
    fn the_bank_is_the_fogs_bounds() {
        let m = bank(VISIBILITY, START, HEIGHT);
        let (lo, hi) = geo::bounds(&m);
        assert!(lo[1].abs() < 1e-5 && (hi[1] - HEIGHT).abs() < 1e-5);
        assert!((hi[0] - VISIBILITY).abs() < 0.01 && m.triangles() < 1000);
        assert_ne!(m, bank(VISIBILITY, 0.0, HEIGHT));
    }

    /// The gate for game passes: the fog pass is accepted and its whole WGSL
    /// module compiles.
    #[test]
    fn the_fog_pass_checks_clean() {
        let mut a = evani_core::App::new("t").with_module(evani_render::RenderModule::new()).with_module(crate::module());
        a.start();
        let n = game_pass::check(&a.world);
        assert!(matches!(n, Ok(k) if k >= 1), "{n:?}");
        assert!(a.world.res::<GamePasses>().pass(PASS).is_some());
    }

    /// Placed and on, the pass gets the fog's numbers; not placed, nothing.
    #[test]
    fn the_pass_draws_only_a_placed_fog() {
        let mut a = evani_core::App::new("t").with_module(evani_render::RenderModule::new()).with_module(crate::module());
        a.start();
        assert!(DistanceFog::data(&a.world, None).iter().all(|r| *r == [0.0; 4]));
        let d = DistanceFog::data(&a.world, Some(2.0));
        assert_eq!(d[0], [COLOR[0], COLOR[1], COLOR[2], 1.0]);
        assert_eq!(d[1], [density(VISIBILITY, START), START, HEIGHT, 2.0]);
    }
}
