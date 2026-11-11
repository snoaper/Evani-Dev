//! Composition from typed building-block links (docs/PROCEDURAL.md
//! "Composition from typed links"; DECISIONS.md "Typed building-block
//! links").
//!
//! The building-block graph (catalog/procedural/blocks.json, version 2) says
//! what each landscape and block is made of AND how each child feeds its
//! parent. tools/procedural/gen_blocks.mjs writes it as Rust data
//! (evani-procedural-lib's `blocks_table`, feature `compose`); this module
//! assembles an asset from it:
//!
//! * [`Kind::Layer`]: a material layer blended into the parent's surface by
//!   a mask. Drawn through the engine's existing per-surface layering, the
//!   catalogue materials' condition layers ([`Surface::condition`]: snow,
//!   growth for moss, mud...), on the parent's own surfaced parts: a layer
//!   never adds a mesh or a draw. Its mask terms are evaluated on the CPU
//!   ([`LayerUse::coverage`]) and carried in the [`Assembly`].
//! * [`Kind::Scatter`]: instanced copies of the child over the parent's
//!   ground (density per m2, scale range, slope band, height band, water),
//!   seeded and deterministic; [`Assembly::place`] puts them in the
//!   [`ProcScene`]. Only scatter links make instances.
//! * [`Kind::Base`]: the parent's geometry. [`Join::Merge`] joins the
//!   child's meshes into the parent's own parts (no new draw);
//!   [`Join::Part`] is its own draw at the parent's spot (sky, sea, a separate
//!   piece); [`Join::Surface`] dresses the parent's bare parts in the child's
//!   surface (the stone a rock is proven on).
//!
//! A planned block (`planned:<id>`, no generator yet) is a grouping: its own
//! links apply to the parent, inside the mask of the layer that brought it.
//!
//! What has no engine path yet is listed in [`Assembly::missing`] instead of
//! being guessed: a layer with no condition layer (blending a second
//! catalogue material by a mask), a material named as a surface (the
//! catalogue row's block is not compiled in), per-edge height, curvature and
//! noise masks in the shader (the condition shader masks snow by slope only,
//! through the global `material.cond.snow.angle`), and the scatter's LOD and
//! impostor distances (the procedural module draws every placement at full
//! detail).

use evani_render::mesh::Mesh;

use crate::asset::{Output, Part};
use crate::noise;
use crate::scene::ProcScene;
use crate::surface::{CONDITIONS, Surface};

/// What a layer's mask term reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mask {
    /// Degrees from flat (0..90).
    Slope,
    /// A fraction of the parent's height span (0 at its lowest point, 1 at its highest).
    Height,
    /// -1..1, below 0 concave (cracks, hollows, valley floors).
    Curvature,
    /// Seeded value noise (0..1) at [`Term::scale`] patches per metre.
    Noise,
}

impl Mask {
    /// The term's range: a layer's falloff is a fraction of it.
    pub const fn range(self) -> (f32, f32) {
        match self {
            Mask::Slope => (0.0, 90.0),
            Mask::Height | Mask::Noise => (0.0, 1.0),
            Mask::Curvature => (-1.0, 1.0),
        }
    }
}

/// One mask term: where the layer shows (above `min`, below `max`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Term {
    pub by: Mask,
    pub min: Option<f32>,
    pub max: Option<f32>,
    /// Noise patches per metre (noise terms only; 1 otherwise).
    pub scale: f32,
}

/// A material layer blended into the parent's surface.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Layer {
    /// Every term must hold (they multiply).
    pub mask: &'static [Term],
    /// The layer's weight (0..1]: its condition amount.
    pub blend: f32,
    /// The soft edge, as a fraction of each term's range.
    pub falloff: f32,
    /// The engine's condition layer that draws it ([`CONDITIONS`]).
    pub condition: Option<&'static str>,
}

/// Instanced copies of the child.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Scatter {
    /// Copies per square metre where the rules allow.
    pub density: f32,
    /// Uniform scale range.
    pub scale: [f32; 2],
    /// Metres to the lower detail.
    pub lod: f32,
    /// Metres to the impostor (0: none).
    pub impostor: f32,
    /// Allowed slope, degrees.
    pub slope: [f32; 2],
    /// Allowed height band (fractions of the parent's height span).
    pub height: [f32; 2],
    /// Keep out of water (below [`Options::water_level`]).
    pub avoid_water: bool,
}

/// How a base child joins its parent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Join {
    /// Joined into the parent's own parts: no new draw.
    Merge,
    /// Its own draw at the parent's spot.
    Part,
    /// The parent's bare parts wear its surface.
    Surface,
}

/// How a child feeds its parent.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    Layer(Layer),
    Scatter(Scatter),
    Base(Join),
}

/// One typed link: the child block's id and how it is used.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Edge {
    /// A library asset key, `material:<id>` or `planned:<id>`.
    pub block: &'static str,
    pub kind: Kind,
}

/// A block and its links.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Node {
    pub id: &'static str,
    pub edges: &'static [Edge],
}

/// The typed graph (a DAG: gen_blocks refuses cycles).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Graph {
    pub nodes: &'static [Node],
}

impl Graph {
    /// The links of `id` (none when it has no node).
    pub fn edges(&self, id: &str) -> &'static [Edge] {
        self.nodes.iter().find(|n| n.id == id).map_or(&[], |n| n.edges)
    }
}

/// The world resource that turns composition on in [`crate::ProcModule`]:
/// each placed asset with links is drawn with its layers and base joins
/// applied ([`apply`]). Without it nothing changes.
#[derive(Debug, Clone, Copy)]
pub struct Composition {
    pub graph: &'static Graph,
}

/// How [`assemble`] runs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Options {
    pub seed: u32,
    /// World height of the water surface, for `avoid_water`; `None`: no water.
    pub water_level: Option<f32>,
    /// The most instances in all (0: no scatter at all).
    pub max_instances: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self { seed: 1, water_level: None, max_instances: 4096 }
    }
}

/// A layer as used.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerUse {
    pub block: &'static str,
    pub condition: Option<&'static str>,
    pub amount: f32,
    /// Its own terms with those of the planned layers it sits in.
    pub mask: Vec<Term>,
    pub falloff: f32,
    /// The mean mask weight over the parent's vertices (0..1).
    pub coverage: f32,
    /// Whether a shader layer draws it on the parent's parts.
    pub drawn: bool,
}

/// One scattered copy, relative to the parent's base.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Instance {
    pub block: &'static str,
    pub position: [f32; 3],
    pub yaw: f32,
    pub scale: f32,
    pub lod: f32,
    pub impostor: f32,
}

/// An asset assembled from its typed links.
#[derive(Debug, Clone, PartialEq)]
pub struct Assembly {
    /// The parent's output with its layers and base joins applied.
    pub output: Output,
    pub layers: Vec<LayerUse>,
    /// Scatter links only.
    pub instances: Vec<Instance>,
    /// Base children drawn on their own ([`Join::Part`]).
    pub parts: Vec<&'static str>,
    /// Base children whose surface the parent wears ([`Join::Surface`]).
    pub surfaces: Vec<&'static str>,
    /// What has no engine path yet, one plain line each.
    pub missing: Vec<String>,
}

impl Assembly {
    /// The meshes it draws (layers never add one).
    pub fn meshes(&self) -> usize {
        self.output.meshes().len()
    }

    /// The condition layers on its parts: (part, layer names).
    pub fn material_layers(&self) -> Vec<Vec<&'static str>> {
        match &self.output {
            Output::Parts(ps) => ps.iter().map(|p| p.surface.map_or_else(Vec::new, |s| conditions_of(&s))).collect(),
            Output::Mesh(_) => vec![Vec::new()],
            Output::Module => Vec::new(),
        }
    }

    /// Place its own parts at `at` and every instance round it; the parent
    /// itself is placed by the caller. Returns how many placements it added.
    pub fn place(&self, scene: &mut ProcScene, at: [f32; 3]) -> usize {
        let n0 = scene.placements.len();
        for f in &self.parts {
            scene.place(f, at);
        }
        for i in &self.instances {
            let k = scene.place(i.block, [at[0] + i.position[0], at[1] + i.position[1], at[2] + i.position[2]]);
            if let Some(p) = scene.placements.get_mut(k) {
                p.yaw = i.yaw;
                p.scale = i.scale;
            }
        }
        scene.placements.len() - n0
    }
}

/// The condition layers a surface carries: on in its mask (byte 3, the
/// layers the material reacts to) with a default amount above 0 (bytes
/// 24..32), so they show without a global amount.
pub fn conditions_of(s: &Surface) -> Vec<&'static str> {
    CONDITIONS.iter().enumerate().filter(|(i, _)| s.block[3] & (1 << i) != 0 && s.block.get(24 + i).is_some_and(|a| *a > 0)).map(|(_, c)| *c).collect()
}

/// [`assemble`] without scatter: the parent's output with its layers and
/// base joins (what [`crate::ProcModule`] draws under a [`Composition`]).
pub fn apply(graph: &Graph, id: &str, parent: Output, child: &mut dyn FnMut(&str) -> Option<Output>) -> Output {
    if graph.edges(id).is_empty() {
        return parent;
    }
    assemble(graph, id, parent, child, &Options { max_instances: 0, ..Options::default() }).output
}

/// Assemble `id` from its links: `parent` is its own generated output,
/// `child` generates a child asset by key (`None`: not compiled in).
/// Deterministic for the same inputs and seed.
pub fn assemble(graph: &Graph, id: &str, parent: Output, child: &mut dyn FnMut(&str) -> Option<Output>, opts: &Options) -> Assembly {
    let mut c = Ctx { graph, opts: *opts, left: opts.max_instances, ground: None, a: Assembly { output: parent, layers: Vec::new(), instances: Vec::new(), parts: Vec::new(), surfaces: Vec::new(), missing: Vec::new() } };
    if matches!(c.a.output, Output::Module) && !graph.edges(id).is_empty() {
        c.a.missing.push(format!("{id}: drawn by its engine module, so its links can't change its meshes"));
    }
    c.shape(id, child, 0);
    c.ground = Some(Ground::of(&c.a.output));
    c.dress(id, &[], 0);
    c.a
}

const MAX_DEPTH: usize = 8;
const PLANNED: &str = "planned:";
const MATERIAL: &str = "material:";

struct Ctx<'g> {
    graph: &'g Graph,
    opts: Options,
    left: usize,
    ground: Option<Ground>,
    a: Assembly,
}

impl Ctx<'_> {
    /// Pass 1: base links (merge, part, surface), through planned groupings.
    fn shape(&mut self, id: &str, child: &mut dyn FnMut(&str) -> Option<Output>, depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for e in self.graph.edges(id) {
            let planned = e.block.starts_with(PLANNED);
            let material = e.block.starts_with(MATERIAL);
            match e.kind {
                Kind::Base(Join::Merge | Join::Part) | Kind::Layer(_) if planned => self.shape(e.block, child, depth + 1),
                Kind::Base(Join::Merge) => match (material, child(e.block)) {
                    (true, _) => self.a.missing.push(format!("{}: a material has no geometry to merge", e.block)),
                    (false, Some(o)) => {
                        if let Some(why) = merge_into(&mut self.a.output, o) {
                            self.a.missing.push(format!("{}: not merged ({why})", e.block));
                        }
                    }
                    (false, None) => self.a.missing.push(format!("{}: not compiled into this build, so it can't be merged", e.block)),
                },
                Kind::Base(Join::Part) => self.a.parts.push(e.block),
                Kind::Base(Join::Surface) => {
                    self.a.surfaces.push(e.block);
                    if material {
                        if has_bare(&self.a.output) {
                            self.a.missing.push(format!("{}: the catalogue row's block isn't compiled into the engine, so the bare parts stay flat", e.block));
                        }
                    } else if !planned && let Some(s) = child(e.block).as_ref().and_then(first_surface) {
                        dress_bare(&mut self.a.output, s);
                    }
                }
                Kind::Layer(_) | Kind::Scatter(_) => {}
            }
        }
    }

    /// Pass 2: layers and scatters, inside the masks of the planned layers
    /// they sit in.
    fn dress(&mut self, id: &str, mask: &[Term], depth: usize) {
        if depth > MAX_DEPTH {
            return;
        }
        for (k, e) in self.graph.edges(id).iter().enumerate() {
            let planned = e.block.starts_with(PLANNED);
            match e.kind {
                Kind::Layer(l) => {
                    let mut m = mask.to_vec();
                    m.extend_from_slice(l.mask);
                    let coverage = self.ground.as_ref().map_or(0.0, |g| g.coverage(&m, l.falloff, self.opts.seed));
                    let drawn = l.condition.is_some_and(|c| layer_parts(&mut self.a.output, c, l.blend));
                    match l.condition {
                        Some(c) if !drawn => self.a.missing.push(format!("{}: its {c} layer needs catalogue-material parts; this parent has none", e.block)),
                        None if !planned => self.a.missing.push(format!("{}: no shader layer draws it yet (blending a second catalogue material by a mask has no engine path)", e.block)),
                        _ => {}
                    }
                    if drawn && l.mask.iter().any(|t| t.by != Mask::Slope) {
                        self.note_once("the condition shader masks by slope only (global material.cond.snow.angle); height, curvature and noise terms are carried in the assembly but not drawn per link yet");
                    }
                    self.a.layers.push(LayerUse { block: e.block, condition: l.condition, amount: l.blend, mask: m.clone(), falloff: l.falloff, coverage, drawn });
                    if planned {
                        self.dress(e.block, &m, depth + 1);
                    }
                }
                Kind::Base(Join::Merge | Join::Part) if planned => self.dress(e.block, mask, depth + 1),
                Kind::Base(_) => {}
                Kind::Scatter(s) => {
                    if planned || e.block.starts_with(MATERIAL) {
                        self.a.missing.push(format!("{}: has no generator to scatter yet", e.block));
                    } else if self.left > 0 {
                        let salt = noise::hash((depth as u32).wrapping_mul(0x9e37_79b9) ^ (k as u32).wrapping_mul(0x85eb_ca6b) ^ fnv(e.block));
                        self.scatter(e.block, &s, mask, salt);
                    }
                }
            }
        }
    }

    fn note_once(&mut self, line: &str) {
        if !self.a.missing.iter().any(|m| m == line) {
            self.a.missing.push(line.to_string());
        }
    }

    fn scatter(&mut self, block: &'static str, s: &Scatter, mask: &[Term], salt: u32) {
        let Some(g) = self.ground.take() else { return };
        self.scatter_on(&g, block, s, mask, salt);
        self.ground = Some(g);
    }

    fn scatter_on(&mut self, g: &Ground, block: &'static str, s: &Scatter, mask: &[Term], salt: u32) {
        let area = g.cum.last().copied().unwrap_or(0.0);
        if area <= 0.0 {
            return;
        }
        let want = (s.density.max(0.0) * area).round();
        let n = (want as usize).min(self.left);
        let mut rng = Rng(self.opts.seed ^ salt);
        let mut out = Vec::new();
        for _ in 0..n {
            let r = rng.next() * area;
            let t = g.cum.partition_point(|&c| c < r).min(g.tris.len().saturating_sub(1));
            let Some(tri) = g.tris.get(t) else { continue };
            let (u, v) = (rng.next(), rng.next());
            let su = u.sqrt();
            let w = [1.0 - su, su * (1.0 - v), su * v];
            let site = g.site_in(tri, w);
            let keep = site.slope >= s.slope[0] && site.slope <= s.slope[1] && site.height >= s.height[0] && site.height <= s.height[1] && !(s.avoid_water && self.opts.water_level.is_some_and(|wl| site.p[1] < wl));
            let weight = mask_weight(mask, &site, 0.0, self.opts.seed);
            let (yaw, sc) = (rng.next() * std::f32::consts::TAU, rng.next());
            if keep && weight > 0.5 {
                out.push(Instance { block, position: site.p, yaw, scale: s.scale[0] + (s.scale[1] - s.scale[0]) * sc, lod: s.lod, impostor: s.impostor });
            }
        }
        if want as usize > n {
            self.a.missing.push(format!("{block}: capped at {n} of {want} copies (Options::max_instances)"));
        }
        self.left -= n.min(self.left);
        self.a.instances.extend(out);
        if s.lod > 0.0 || s.impostor > 0.0 {
            self.note_once("scatter LOD and impostor distances are carried on each instance; the procedural module draws every placement at full detail (no LOD or impostor path yet)");
        }
    }
}

/// A seeded counter hash: 0..1.
struct Rng(u32);

impl Rng {
    fn next(&mut self) -> f32 {
        self.0 = noise::hash(self.0.wrapping_add(0x9e37_79b9));
        (self.0 >> 8) as f32 / (1u32 << 24) as f32
    }
}

fn fnv(s: &str) -> u32 {
    s.bytes().fold(0x811c_9dc5u32, |h, b| (h ^ u32::from(b)).wrapping_mul(0x0100_0193))
}

/// What a mask reads at one point.
#[derive(Debug, Clone, Copy)]
struct Site {
    p: [f32; 3],
    slope: f32,
    height: f32,
    curvature: f32,
}

/// The parent's ground: every vertex of every mesh, with its slope, height
/// fraction and curvature, and its triangles by area.
struct Ground {
    sites: Vec<Site>,
    tris: Vec<[usize; 3]>,
    /// Running triangle area (m2).
    cum: Vec<f32>,
}

fn slope_of(ny: f32) -> f32 {
    ny.clamp(-1.0, 1.0).acos().to_degrees()
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn len(a: [f32; 3]) -> f32 {
    (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt()
}

impl Ground {
    fn of(out: &Output) -> Self {
        let meshes = out.meshes();
        let (mut lo, mut hi) = (f32::MAX, f32::MIN);
        for m in &meshes {
            for v in &m.vertices {
                lo = lo.min(v[1]);
                hi = hi.max(v[1]);
            }
        }
        let span = (hi - lo).max(1e-6);
        let mut sites = Vec::new();
        let mut tris = Vec::new();
        let mut cum = Vec::new();
        let mut total = 0.0f32;
        for m in &meshes {
            let base = sites.len();
            let (curv, count) = curvature(m);
            for (i, v) in m.vertices.iter().enumerate() {
                let c = curv.get(i).copied().unwrap_or(0.0) / count.get(i).copied().unwrap_or(1.0).max(1.0);
                sites.push(Site { p: [v[0], v[1], v[2]], slope: slope_of(v[4]), height: (v[1] - lo) / span, curvature: (-4.0 * c).clamp(-1.0, 1.0) });
            }
            for t in m.indices.as_chunks::<3>().0 {
                let ix = [t[0] as usize + base, t[1] as usize + base, t[2] as usize + base];
                let (Some(a), Some(b), Some(c)) = (sites.get(ix[0]), sites.get(ix[1]), sites.get(ix[2])) else { continue };
                let area = 0.5 * len(cross(sub(b.p, a.p), sub(c.p, a.p)));
                if area > 0.0 && area.is_finite() {
                    total += area;
                    tris.push(ix);
                    cum.push(total);
                }
            }
        }
        Self { sites, tris, cum }
    }

    /// The site at barycentric `w` in a triangle (slope from its face).
    fn site_in(&self, t: &[usize; 3], w: [f32; 3]) -> Site {
        let s = |i: usize| self.sites.get(t[i]).copied().unwrap_or(Site { p: [0.0; 3], slope: 0.0, height: 0.0, curvature: 0.0 });
        let (a, b, c) = (s(0), s(1), s(2));
        let p = std::array::from_fn(|k| a.p[k] * w[0] + b.p[k] * w[1] + c.p[k] * w[2]);
        let n = cross(sub(b.p, a.p), sub(c.p, a.p));
        let l = len(n).max(1e-9);
        Site { p, slope: slope_of((n[1] / l).abs()), height: a.height * w[0] + b.height * w[1] + c.height * w[2], curvature: a.curvature * w[0] + b.curvature * w[1] + c.curvature * w[2] }
    }

    /// The mean mask weight over the vertices.
    fn coverage(&self, mask: &[Term], falloff: f32, seed: u32) -> f32 {
        if self.sites.is_empty() {
            return 0.0;
        }
        self.sites.iter().map(|s| mask_weight(mask, s, falloff, seed)).sum::<f32>() / self.sites.len() as f32
    }
}

/// Per vertex: the summed sine of the angle each neighbour makes above the
/// vertex's tangent plane, and the neighbour count (positive: the ground
/// rises round it, a hollow).
fn curvature(m: &Mesh) -> (Vec<f32>, Vec<f32>) {
    let mut sum = vec![0.0f32; m.vertices.len()];
    let mut n = vec![0.0f32; m.vertices.len()];
    for t in m.indices.as_chunks::<3>().0 {
        for (a, b) in [(t[0], t[1]), (t[1], t[2]), (t[2], t[0]), (t[1], t[0]), (t[2], t[1]), (t[0], t[2])] {
            let (Some(va), Some(vb)) = (m.vertices.get(a as usize), m.vertices.get(b as usize)) else { continue };
            let d = [vb[0] - va[0], vb[1] - va[1], vb[2] - va[2]];
            let l = len(d);
            if l > 1e-9
                && let (Some(s), Some(c)) = (sum.get_mut(a as usize), n.get_mut(a as usize))
            {
                *s += (va[3] * d[0] + va[4] * d[1] + va[5] * d[2]) / l;
                *c += 1.0;
            }
        }
    }
    (sum, n)
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// One term's weight at a value: 1 inside, 0 outside, a soft edge `falloff`
/// of the term's range wide.
fn term_weight(t: &Term, v: f32, falloff: f32) -> f32 {
    let (lo, hi) = t.by.range();
    let w = (hi - lo) * falloff.clamp(0.0, 1.0) * 0.5;
    let above = |m: f32| if w > 0.0 { smoothstep(m - w, m + w, v) } else if v >= m { 1.0 } else { 0.0 };
    t.min.map_or(1.0, above) * t.max.map_or(1.0, |m| 1.0 - above(m))
}

fn mask_weight(mask: &[Term], s: &Site, falloff: f32, seed: u32) -> f32 {
    mask.iter()
        .map(|t| {
            let v = match t.by {
                Mask::Slope => s.slope,
                Mask::Height => s.height,
                Mask::Curvature => s.curvature,
                Mask::Noise => noise::value2(s.p[0] * t.scale, s.p[2] * t.scale, seed ^ 0x6d61_736b),
            };
            term_weight(t, v, falloff)
        })
        .product()
}

/// Turn condition `layer` on every surfaced, non-card part at `amount` (or
/// the part's own amount if larger). Whether any part took it.
fn layer_parts(out: &mut Output, layer: &str, amount: f32) -> bool {
    let Some(i) = CONDITIONS.iter().position(|c| *c == layer) else { return false };
    let Output::Parts(ps) = out else { return false };
    let mut any = false;
    for p in ps.iter_mut() {
        if let Some(s) = p.surface.as_mut().filter(|s| !s.card) {
            let cur = s.block.get(24 + i).map_or(0.0, |b| f32::from(*b) / 255.0);
            *s = s.condition(layer, amount.max(cur));
            any = true;
        }
    }
    any
}

fn has_bare(out: &Output) -> bool {
    match out {
        Output::Mesh(_) => true,
        Output::Parts(ps) => ps.iter().any(|p| p.surface.is_none()),
        Output::Module => false,
    }
}

fn first_surface(o: &Output) -> Option<Surface> {
    match o {
        Output::Parts(ps) => ps.iter().find_map(|p| p.surface),
        _ => None,
    }
}

/// The parent's bare parts wear `s` (a single mesh becomes one surfaced part).
fn dress_bare(out: &mut Output, s: Surface) {
    match out {
        Output::Mesh(m) => *out = Output::Parts(vec![Part::surfaced(std::mem::take(m), s)]),
        Output::Parts(ps) => {
            for p in ps.iter_mut().filter(|p| p.surface.is_none()) {
                let c = s.base_rgb();
                p.surface = Some(s);
                p.color = [c[0], c[1], c[2], p.color[3]];
            }
        }
        Output::Module => {}
    }
}

/// Join the child's meshes into the parent's parts: a child part goes into
/// the parent part with the same surface, else into the parent's first part
/// (taking its look). Never adds a mesh. `Some(why)` when it can't.
fn merge_into(out: &mut Output, child: Output) -> Option<&'static str> {
    let pieces: Vec<(Mesh, Option<Surface>)> = match child {
        Output::Mesh(m) => vec![(m, None)],
        Output::Parts(ps) => ps.into_iter().map(|p| (p.mesh, p.surface)).collect(),
        Output::Module => return Some("it draws through its engine module: use join part"),
    };
    match out {
        Output::Mesh(m) => pieces.iter().for_each(|(c, _)| crate::geo::merge(m, c)),
        Output::Parts(ps) => {
            if ps.is_empty() {
                return Some("the parent has no parts");
            }
            for (c, s) in &pieces {
                let at = s.and_then(|s| ps.iter().position(|p| p.surface == Some(s))).unwrap_or(0);
                if let Some(p) = ps.get_mut(at) {
                    crate::geo::merge(&mut p.mesh, c);
                }
            }
        }
        Output::Module => return Some("the parent draws through its engine module"),
    }
    None
}

/// Generate `feature` from its live values, assemble it from `graph` and
/// place it at `at` with its own parts and scattered copies (the scatter
/// path: each copy is a [`ProcScene`] placement of the child asset).
/// Returns the assembly, or `None` when `feature` is not compiled in.
pub fn place(graph: &Graph, registry: &crate::Registry, reg: &evani_core::params::ParamRegistry, feature: &str, at: [f32; 3], opts: &Options, scene: &mut ProcScene) -> Option<Assembly> {
    let g = registry.generate(reg, feature, &scene.style)?;
    let style = scene.style.clone();
    let a = assemble(graph, feature, g.output, &mut |f| registry.generate(reg, f, &style).map(|g| g.output), opts);
    scene.place(feature, at);
    a.place(scene, at);
    Some(a)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geo;
    use crate::surface::Surface;

    const GREY: Surface = Surface::hex("060003dd9c9c9a5c5c5ee8e6e099001a980099000000005d0000000000000000", 0.6, 0.0);

    /// A 40 m square hill, 10 m high in the middle, as one surfaced part.
    fn hill() -> Output {
        let m = geo::height_grid([40.0, 40.0], 24, |x, z| 10.0 * (1.0 - ((x * x + z * z).sqrt() / 20.0).min(1.0)));
        Output::Parts(vec![Part::surfaced(m, GREY)])
    }

    static SNOW: [Term; 2] = [Term { by: Mask::Height, min: Some(0.6), max: None, scale: 1.0 }, Term { by: Mask::Slope, min: None, max: Some(35.0), scale: 1.0 }];
    static G: Graph = Graph {
        nodes: &[
            Node {
                id: "land",
                edges: &[
                    Edge { block: "material:snow", kind: Kind::Layer(Layer { mask: &SNOW, blend: 0.9, falloff: 0.15, condition: Some("snow") }) },
                    Edge { block: "rocks", kind: Kind::Scatter(Scatter { density: 0.05, scale: [0.5, 1.5], lod: 30.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: true }) },
                    Edge { block: "sky", kind: Kind::Base(Join::Part) },
                    Edge { block: "boulder", kind: Kind::Base(Join::Merge) },
                    Edge { block: "planned:nothing", kind: Kind::Scatter(Scatter { density: 1.0, scale: [1.0, 1.0], lod: 1.0, impostor: 0.0, slope: [0.0, 90.0], height: [0.0, 1.0], avoid_water: false }) },
                ],
            },
            Node { id: "bare", edges: &[Edge { block: "stone", kind: Kind::Base(Join::Surface) }] },
        ],
    };

    fn kid(f: &str) -> Option<Output> {
        match f {
            "boulder" => Some(Output::Parts(vec![Part::surfaced(geo::segmented_box([1.0; 3], 1), GREY)])),
            "stone" => Some(Output::Parts(vec![Part::surfaced(geo::segmented_box([1.0; 3], 1), GREY)])),
            _ => None,
        }
    }

    #[test]
    fn layers_add_no_mesh_merges_add_no_draw_and_only_scatters_make_instances() {
        let a = assemble(&G, "land", hill(), &mut kid, &Options::default());
        assert_eq!(a.meshes(), 1, "{a:?}");
        assert_eq!(a.material_layers(), vec![vec!["snow"]]);
        let snow = &a.layers[0];
        assert!(snow.drawn && snow.coverage > 0.0 && snow.coverage < 0.5, "{snow:?}");
        assert!(!a.instances.is_empty() && a.instances.iter().all(|i| i.block == "rocks"));
        assert_eq!(a.parts, vec!["sky"]);
        assert!(a.missing.iter().any(|m| m.starts_with("planned:nothing: has no generator")), "{:?}", a.missing);
        let b = assemble(&G, "land", hill(), &mut kid, &Options::default());
        assert_eq!(a, b, "deterministic");
        let none = assemble(&G, "land", hill(), &mut kid, &Options { max_instances: 0, ..Options::default() });
        assert!(none.instances.is_empty());
        let mut scene = ProcScene::default();
        assert_eq!(a.place(&mut scene, [5.0, 0.0, 0.0]), 1 + a.instances.len());
    }

    #[test]
    fn a_surface_link_dresses_bare_parts() {
        let m = geo::segmented_box([2.0; 3], 2);
        let out = apply(&G, "bare", Output::Mesh(m), &mut kid);
        let Output::Parts(ps) = out else { panic!("parts") };
        assert_eq!(ps.len(), 1);
        assert_eq!(ps[0].surface, Some(GREY));
        let same = apply(&G, "no-links", Output::Mesh(geo::segmented_box([2.0; 3], 2)), &mut kid);
        assert!(matches!(same, Output::Mesh(_)));
    }

    #[test]
    fn masks_weigh_terms_with_a_soft_edge() {
        let t = Term { by: Mask::Slope, min: Some(30.0), max: None, scale: 1.0 };
        assert_eq!(term_weight(&t, 10.0, 0.1), 0.0);
        assert_eq!(term_weight(&t, 60.0, 0.1), 1.0);
        assert!((term_weight(&t, 30.0, 0.1) - 0.5).abs() < 1e-6);
        assert_eq!(term_weight(&t, 29.9, 0.0), 0.0);
        let band = Term { by: Mask::Height, min: Some(0.2), max: Some(0.6), scale: 1.0 };
        assert_eq!(term_weight(&band, 0.4, 0.0), 1.0);
        assert_eq!(term_weight(&band, 0.7, 0.0), 0.0);
    }
}
