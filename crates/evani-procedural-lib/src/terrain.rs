//! 2. Terrain and landforms.
//!
//! Built: `terrain-rolling-hills` (the reference seeded CPU asset), and
//! the basic generators of batch A ([`register`]).
//! Inspired by THREE.Terrain (MIT): a height grid filled by layered value
//! noise (fBm); written fresh here on evani-procedural's noise and grid.

#[cfg(feature = "terrain-rolling-hills")]
pub use hills::{Hills, RollingHills, ground as hills_ground};
#[cfg(feature = "hills-grass")]
pub use hills::meadow_grass as hills_meadow_grass;

#[cfg(feature = "terrain-rolling-hills")]
mod hills {
    use evani_procedural::land::{Layer, cover, patchy, rolling, soil_sides};
    use evani_procedural::surface::presets as sp;
    use evani_procedural::{AssetInfo, Output, ParamSpec, Part, ProcAsset, Values, geo, noise};

    /// Rolling hills: a square of seeded fractal noise, its lowest point at
    /// the ground, falling off to flat at the edge so it sits in a world.
    pub struct RollingHills;

    const INFO: AssetInfo = AssetInfo::cpu("terrain", "rolling-hills", "Rolling hills");
    /// The colour param's default: the meadow grass's own green (linear).
    pub const GRASS: [f32; 3] = [0.07, 0.13, 0.03];
    /// Ground-cover albedos as the library's low sun shows them (linear).
    const LIMESTONE: [f32; 3] = [0.36, 0.35, 0.31];
    const LOAM: [f32; 3] = [0.13, 0.09, 0.06];
    /// Dry, sun-bleached turf on the crests (linear).
    const DRY: [f32; 3] = [0.12, 0.13, 0.04];
    /// Asset Studio column 28 (player-scale hills): the bare, stony soil of
    /// the worn scars on the steeper banks (linear).
    const SCAR: [f32; 3] = [0.075, 0.062, 0.048];
    const PARAMS: [ParamSpec; 11] = [
        ParamSpec::num("size", 24.0, 4.0, 400.0).unit("m").describe("Width of the square (m)"),
        ParamSpec::num("height", 3.0, 0.0, 60.0).unit("m").describe("Tallest hill over the lowest ground (m)"),
        ParamSpec::num("feature_size", 10.0, 1.0, 200.0).unit("m").describe("Distance between hilltops (m)"),
        ParamSpec::num("roughness", 0.45, 0.0, 0.9).describe("How strong each finer layer of bumps is"),
        ParamSpec::num("octaves", 4.0, 1.0, 8.0).step(1.0).describe("Layers of detail"),
        ParamSpec::num("resolution", 64.0, 4.0, 200.0).step(1.0).describe("Grid cells per side"),
        ParamSpec::color("color", GRASS).describe("Grass colour"),
        ParamSpec::num("edge", 0.3, 0.0, 3.0).unit("m").describe("Depth of the soil sides (m; 0: none, for a terrain set into a world)"),
        ParamSpec::num("outcrops", 0.5, 0.0, 1.0).describe("Limestone outcrops on the steepest slopes: how many clusters (0: none)"),
        // Asset Studio column 28: hills a player walks in. Both off by
        // default, so the plot is as it was.
        ParamSpec::num("horizon", 0.0, 0.0, 20000.0).unit("m").describe("The land past the square: the hills run on round it out to this far (m from the middle; 0: none, the plot alone)"),
        ParamSpec::num("scars", 0.0, 0.0, 1.0).describe("Worn soil scars on the steeper banks, with the outcrops in them (0: none)"),
    ];

    impl ProcAsset for RollingHills {
        fn info(&self) -> &AssetInfo {
            &INFO
        }

        fn params(&self) -> &[ParamSpec] {
            &PARAMS
        }

        fn generate(&self, v: &Values, seed: u64, _style: &str) -> Output {
            let land = Hills::new(v, seed);
            let (h, depth) = (land.h, land.depth);
            let m = &land.mesh;
            let size = land.size;
            let seed = seed as u32;
            // Realism (round 6): one patchy turf, its dry-grass and worn-soil
            // patches and the bare banks drawn smoothly by the turf shader
            // (no patch edge follows the faces), bare chalky rock only where
            // it is steepest, and soil sides so the plot reads as a cut block
            // of ground. Round 9: drier turf on the crests, the greener grass
            // in the hollows, and limestone outcrops (real rock geometry).
            // Their own patch layout (round 9: the field and the hills shared
            // one, seed for seed).
            let grass = patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(v.color("color")).seed(seed ^ 0x6b1d)), 0.12);
            let dry = patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(DRY).seed(seed ^ 0x6b1d)), 0.15);
            let limestone = sp::ROCK_LIMESTONE.tint(LIMESTONE).seed(seed).scale(3.0);
            let layers = [Layer::all(limestone).slope(0.55, 2.0), Layer::all(dry).height(depth + h * 0.72, f32::MAX), Layer::all(grass)];
            let res = v.f("resolution").round().max(2.0) as u32;
            let mut parts = Vec::new();
            // Column 28: with a horizon the land runs on round the square (no
            // soil sides: it is set into the world, not cut out of it).
            if depth > 0.0 && land.horizon <= 0.0 {
                parts.push(Part::surfaced(soil_sides(m, res, 0.0), sp::GROUND_SOIL_LOAM.tint(LOAM).seed(seed).scale(0.8)));
            }
            let mut drawn = if land.horizon > 0.0 { land.far_mesh() } else { m.clone() };
            let clusters = (v.f("outcrops").clamp(0.0, 1.0) * 6.0 * (size / 48.0).clamp(1.0, 7.0)).round() as u32;
            let rocks = if land.scars > 0.0 {
                // Column 28: worn scars of bare, stony soil on the steeper
                // banks (where the turf has slipped), the limestone breaking
                // through inside them, the turf's own covers everywhere else.
                // A scar is the land's own: where it lies depends only on the
                // ground's place and slope (not the faces), so the grass, a
                // game and the mesh agree; the faces along its edge are cut
                // finer (to about a third of a cell), so it has a ragged
                // outline, not the grid's saw teeth.
                let is_scar = |c: [f32; 3], _n: [f32; 3]| usize::from(!land.scar_at(c[0], c[2]));
                let cell = size / res.max(1) as f32;
                evani_procedural::detail::refine_near(&mut drawn, cell * 0.35, 3, |c, len| {
                    let r = len * 0.6;
                    if !land.on_square(c[0], c[2], r) {
                        return false;
                    }
                    let here = land.scar_at(c[0], c[2]);
                    [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)].iter().any(|(dx, dz)| land.scar_at(c[0] + dx, c[2] + dz) != here)
                });
                let mut split = evani_procedural::detail::split_by(&drawn, 2, is_scar).into_iter();
                let (scar, rest) = (split.next().unwrap_or_default(), split.next().unwrap_or_default());
                if !scar.indices.is_empty() {
                    parts.push(Part::surfaced(scar, sp::GROUND_GRAVEL_SCREE.tint(SCAR).seed(seed ^ 0x5ca7).scale(0.8)));
                }
                parts.extend(cover(&rest, &layers, 0.08, seed));
                // The rocks only in the square's own scars (not out on the
                // far land), each cluster sunk into its bank; on a walker's
                // scale (over 48 m) at most 16 clusters of plain rocks, seen
                // from tens of metres.
                let scar_in = evani_procedural::detail::split_by(m, 2, is_scar).into_iter().next().unwrap_or_default();
                let (k, rock, dens) = if size > 48.0 { (clusters.min(16), 1.4, v.density().min(0.4)) } else { (clusters, (size * 0.075).clamp(0.3, 3.0), v.density()) };
                evani_procedural::land::outcrops(&scar_in, k, rock, dens, seed)
            } else {
                parts.extend(cover(&drawn, &layers, 0.08, seed));
                evani_procedural::land::outcrops(m, clusters, (size * 0.075).clamp(0.3, 3.0), v.density(), seed)
            };
            if !rocks.indices.is_empty() {
                parts.push(Part::surfaced(rocks, limestone));
            }
            Output::Parts(parts)
        }
    }

    /// The rolling hills' land (Asset Studio column 28: one place for the
    /// drawn mesh, the grass's ground and a game's player): the square's
    /// height grid as drawn, the height function behind it (so the land can
    /// run on past the square, `horizon`, and a game can stand its player on
    /// it), and the worn scars (`scars`).
    pub struct Hills {
        pub size: f32,
        /// Tallest hill over the lowest ground of the square, and the soil
        /// depth it stands on (m).
        pub h: f32,
        pub depth: f32,
        /// The land past the square (m from the middle; 0: none).
        pub horizon: f32,
        pub scars: f32,
        /// The square's top surface as drawn.
        pub mesh: geo::Mesh,
        /// Grid points a side.
        pub n: usize,
        half: f32,
        f: f32,
        gain: f32,
        oct: u32,
        seed: u32,
        lo: f32,
        k: f32,
        /// How steep a bank must be to wear a scar (0 flat, 1 vertical).
        scar_steep: f32,
        scar_size: f32,
    }

    impl Hills {
        pub fn new(v: &Values, seed: u64) -> Self {
            let size = v.f("size").max(1.0);
            let (h, f) = (v.f("height"), 1.0 / v.f("feature_size").max(0.1));
            let (gain, oct) = (v.f("roughness"), v.f("octaves").round().max(1.0) as u32);
            let depth = v.f("edge").max(0.0);
            let horizon = v.f("horizon").max(0.0);
            let horizon = if horizon > size * 0.5 + 1.0 { horizon } else { 0.0 };
            let res = v.f("resolution").round().max(2.0) as u32;
            let mut l = Self {
                size,
                h,
                depth,
                horizon,
                scars: v.f("scars").clamp(0.0, 1.0),
                mesh: geo::Mesh::default(),
                n: res as usize + 1,
                half: size * 0.5,
                f,
                gain,
                oct,
                seed: seed as u32,
                lo: 0.0,
                k: 1.0,
                scar_steep: 1.0,
                scar_size: v.f("feature_size").max(0.1) * 0.07,
            };
            // Round 9: a plot with soil sides is a cut block of hillside, so its
            // hills run to the cut edge (the hill profile shows on the sides, as
            // the reference); set into a world (no sides) they still settle to
            // flat at the edge, unless the land runs on past it (`horizon`).
            let mut m = geo::height_grid([size, size], res, |x, z| l.raw(x, z));
            // Round 9: the tallest hill is `height` over the lowest ground, as
            // the param says (r8's noise reached a fraction of it, and the
            // relief read as a flat slab).
            let (lo, hi) = geo::bounds(&m);
            let k = h / (hi[1] - lo[1]).max(1e-4);
            for p in &mut m.vertices {
                p[1] = (p[1] - lo[1]) * k + depth;
            }
            geo::smooth_normals(&mut m);
            (l.lo, l.k) = (lo[1], k);
            if l.scars > 0.0 {
                // The steepest share of the square's ground may wear a scar
                // (a quarter of it at full `scars`), whatever the hills' scale.
                let mut st: Vec<f32> = m.vertices.iter().map(|p| 1.0 - p[4].clamp(-1.0, 1.0).abs()).collect();
                st.sort_by(f32::total_cmp);
                let q = ((1.0 - 0.25 * l.scars) * (st.len().max(1) - 1) as f32) as usize;
                l.scar_steep = st.get(q).copied().unwrap_or(1.0).max(1e-4);
            }
            l.mesh = m;
            l
        }

        /// The height function before scaling: weathered (warped, the
        /// hollows filled, the tops round) plus a fine lumpy turf a few
        /// centimetres high, settling to flat at the square's edge when it
        /// stands alone.
        fn raw(&self, x: f32, z: f32) -> f32 {
            let half = self.half;
            // 0 at the edge, 1 over the middle half: the hills settle to flat.
            let edge = |d: f32| ((half - d.abs()) / (half * 0.5)).clamp(0.0, 1.0);
            let fall = if self.depth > 0.0 || self.horizon > 0.0 { 1.0 } else { edge(x) * edge(z) };
            let n = rolling(x * self.f, z * self.f, self.seed, self.oct, self.gain);
            let turf = 0.04 * noise::fbm(x * 1.7, z * 1.7, self.seed ^ 0x77, 3, 2.0, 0.5);
            n * fall * fall * (3.0 - 2.0 * fall) + turf * fall / self.h.max(0.05)
        }

        /// The land's height at (x, z) (m; the asset's own frame), on the
        /// square or past it.
        pub fn height(&self, x: f32, z: f32) -> f32 {
            (self.raw(x, z) - self.lo) * self.k + self.depth
        }

        /// The ground as drawn at (x, z): the square's grid, bilinear
        /// between its points (what the grass stands on), the height
        /// function past it.
        pub fn ground(&self, x: f32, z: f32) -> f32 {
            let res = (self.n - 1) as f32;
            let (u, w) = ((x / self.size + 0.5) * res, (z / self.size + 0.5) * res);
            if !(0.0..=res).contains(&u) || !(0.0..=res).contains(&w) {
                return self.height(x, z);
            }
            let (i, j) = ((u.floor() as usize).min(self.n - 2), (w.floor() as usize).min(self.n - 2));
            let (fu, fw) = (u - i as f32, w - j as f32);
            let at = |i: usize, j: usize| self.mesh.vertices.get(j * self.n + i).map_or(0.0, |p| p[1]);
            let a = at(i, j) + (at(i + 1, j) - at(i, j)) * fu;
            let b = at(i, j + 1) + (at(i + 1, j + 1) - at(i, j + 1)) * fu;
            a + (b - a) * fw
        }

        /// How steep the ground is at (x, z) (0 flat, 1 vertical).
        pub fn steep(&self, x: f32, z: f32) -> f32 {
            let e = (self.size / (self.n - 1) as f32).max(0.05);
            let dx = (self.ground(x + e, z) - self.ground(x - e, z)) / (2.0 * e);
            let dz = (self.ground(x, z + e) - self.ground(x, z - e)) / (2.0 * e);
            1.0 - 1.0 / (1.0 + dx * dx + dz * dz).sqrt()
        }

        /// Whether (x, z) lies on the square, `margin` m in from its edge.
        pub fn on_square(&self, x: f32, z: f32, margin: f32) -> bool {
            x.abs() < self.half - margin && z.abs() < self.half - margin
        }

        /// Whether a worn scar lies at (x, z) (the ground's own slope there).
        pub fn scar_at(&self, x: f32, z: f32) -> bool {
            self.scars > 0.0 && self.on_square(x, z, 0.0) && self.scar(x, z, self.steep(x, z))
        }

        /// Whether a worn scar lies at (x, z) on ground `steep` (soft,
        /// ragged patches on the steeper banks; never with `scars` 0).
        pub fn scar(&self, x: f32, z: f32, steep: f32) -> bool {
            // Only on the square itself (the land past it is seen from afar,
            // its cells too coarse for a scar's outline).
            let inner = self.half - self.size / (self.n - 1) as f32;
            if self.scars <= 0.0 || steep < self.scar_steep || x.abs() > inner || z.abs() > inner {
                return false;
            }
            let s = self.scar_size;
            let p = noise::fbm(x / s, z / s, self.seed ^ 0x5ca7, 4, 2.0, 0.55) + 0.3 * noise::fbm(x / (s * 0.12), z / (s * 0.12), self.seed ^ 0x5ca8, 3, 2.0, 0.5);
            // Steeper banks scar more readily.
            p > 0.32 - 0.6 * (steep / self.scar_steep - 1.0).min(0.4)
        }

        /// The land round the square out to `horizon` together with the
        /// square (one grid, so no seam): the square's own grid lines in the
        /// middle, the cells growing outwards (one and a half of the
        /// square's, 10% more each step).
        pub fn far_mesh(&self) -> geo::Mesh {
            let res = self.n - 1;
            let cell = self.size / res as f32;
            let mut out = Vec::new();
            let (mut x, mut step) = (self.half, cell * 1.5);
            while x < self.horizon - 1.0 {
                x = (x + step).min(self.horizon);
                out.push(x);
                step *= 1.1;
            }
            let mut lines: Vec<f32> = out.iter().rev().map(|x| -x).collect();
            lines.extend((0..=res).map(|i| (i as f32 / res as f32 - 0.5) * self.size));
            lines.extend(out.iter().copied());
            let l = lines.len();
            let mut b = geo::MeshBuilder::new();
            for z in &lines {
                for x in &lines {
                    b.vertex([*x, self.height(*x, *z), *z], [0.0, 1.0, 0.0]);
                }
            }
            let row = l as u32;
            for j in 0..row - 1 {
                for i in 0..row - 1 {
                    let a = j * row + i;
                    b.quad(a, a + row, a + row + 1, a + 1);
                }
            }
            let mut m = b.finish();
            geo::smooth_normals(&mut m);
            m
        }
    }

    /// The hills' ground for evani-grass (round 9; CORE_REQUESTS
    /// `grass-heightfield`): the plot's width and its top surface's
    /// heights, `n` x `n` row by row from the -x, -z corner (the grid's own
    /// vertices, so the blades stand on the drawn ground at each).
    pub fn ground(v: &Values, seed: u64) -> (f32, usize, Vec<f32>) {
        let l = Hills::new(v, seed);
        let n = l.n;
        (l.size, n, l.mesh.vertices.iter().take(n * n).map(|p| p[1]).collect())
    }

    /// Meadow grass on the hills between `min` and `max` (plan m, the
    /// asset's own frame) for evani-grass (feature `hills-grass`, Asset
    /// Studio column 28): a long-meadow patch on the hills' own ground, the
    /// worn scars left bare (their blades sunk out of sight). A game hands it
    /// over round its player: `Grass::add_patch_on(patch, n, &heights)`, the
    /// placement's offset in `at`.
    #[cfg(feature = "hills-grass")]
    pub fn meadow_grass(v: &Values, seed: u64, min: [f32; 2], max: [f32; 2], at: [f32; 3]) -> (evani_grass::Patch, usize, Vec<f32>) {
        let l = Hills::new(v, seed);
        let n = 113usize;
        let mut h = Vec::with_capacity(n * n);
        for j in 0..n {
            for i in 0..n {
                let x = min[0] + (max[0] - min[0]) * i as f32 / (n - 1) as f32;
                let z = min[1] + (max[1] - min[1]) * j as f32 / (n - 1) as f32;
                let g = l.ground(x, z);
                h.push(if l.scar_at(x, z) { g + at[1] - 3.0 } else { g + at[1] + 0.02 });
            }
        }
        let patch = evani_grass::Patch::new(evani_grass::Kind::Meadow, [at[0] + min[0], at[2] + min[1]], [at[0] + max[0], at[2] + max[1]]).feather(0.05).density(0.8);
        (patch, n, h)
    }
}

/// The basic generators (batch A): each a height grid shaped by one idea,
/// its lowest point on the ground. Inspired by THREE.Terrain (MIT):
/// height maps from layered noise and shaping curves; written fresh here.
#[allow(unused_imports, dead_code)]
pub fn register(_r: &mut evani_procedural::Registry) {
    use evani_procedural::{Output, ParamSpec as P, Part, Values, geo, noise, shapes as sh};
    const SIZE: P = P::num("size", 30.0, 4.0, 400.0).unit("m").describe("Width of the square (m)");
    const RES: u32 = 96;

    // Realism: a level field of short grass over gently uneven ground, with
    // worn patches of bare soil and a fine lumpy turf.
    const FLAT_GRASS: [f32; 3] = [0.07, 0.13, 0.03];
    /// The landforms' grass where their colour param is the rock's.
    const GRASS_GREEN: [f32; 3] = [0.07, 0.13, 0.03];
    basic!(_r, "terrain-flat-terrain", "terrain", "flat-terrain", "Flat terrain", "ground",
        [SIZE, P::num("bumps", 0.05, 0.0, 2.0).unit("m").describe("Height of the gentle bumps (m)"), P::color("color", FLAT_GRASS).describe("Grass colour"),
         P::num("bare", 0.25, 0.0, 1.0).describe("Bare, worn soil patches (0: all grass)"),
         P::num("edge", 0.3, 0.0, 3.0).unit("m").describe("Depth of the soil sides (m; 0: none, for a terrain set into a world)")],
        |v: &Values, s: u32| {
            use evani_procedural::surface::presets as sp;
            let b = v.f("bumps");
            let size = v.f("size");
            let cells = ((size / 0.3).round() as u32).clamp(RES, 200);
            let mut m = geo::height_grid([size; 2], cells, |x, z| {
                b * (noise::fbm(x * 0.12, z * 0.12, s, 4, 2.0, 0.5) * 0.5 + 0.5) + 0.035 * noise::fbm(x * 1.9, z * 1.9, s ^ 0x77, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut m);
            // Realism (round 6): one patchy turf (its dry-grass and worn-soil
            // patches drawn smoothly by the turf shader, the bare share from
            // the slider) and soil sides, so the plot reads as a cut block of
            // ground on the floor.
            let grass = evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(v.color("color")).seed(s)), v.f("bare").clamp(0.0, 1.0));
            let depth = v.f("edge").max(0.0);
            let mut parts = Vec::new();
            if depth > 0.0 {
                for p in &mut m.vertices {
                    p[1] += depth;
                }
                parts.push(Part::surfaced(evani_procedural::land::soil_sides(&m, cells, 0.0), sp::GROUND_SOIL_LOAM.tint([0.13, 0.09, 0.06]).seed(s).scale(0.8)));
            }
            parts.push(Part::surfaced(m, grass));
            Output::Parts(parts)
        });

    // The mountains (Asset Studio column 19): their own file, crate::mountain.
    #[cfg(feature = "terrain-mountains")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("terrain", "mountains", "Mountains").material("ground"),
        params: &crate::mountain::PARAMS,
        make: crate::mountain::generate,
    }));

    basic!(_r, "terrain-mountain-ranges", "terrain", "mountain-ranges", "Mountain ranges", "ground",
        [P::num("size", 80.0, 10.0, 400.0).unit("m").describe("Width of the square (m)"), P::num("height", 18.0, 2.0, 200.0).unit("m").describe("Tallest peaks (m)"), P::num("peak_spacing", 18.0, 3.0, 200.0).unit("m").describe("Distance between peaks along the range (m)"), P::color("color", [0.25, 0.24, 0.21]).describe("Rock colour"), P::num("rock", 0.55, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the ground (1)"), P::num("snow", 0.3, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)")],
        |v: &Values, s: u32| {
            let (half, h, f) = (v.f("size") * 0.5, v.f("height"), 1.0 / v.f("peak_spacing").max(0.5));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                // A ridge line along x, wandering in z, falling off across it.
                let line = z - half * 0.25 * (noise::value2(x * f * 0.4, 3.0, s) - 0.5) * 2.0;
                let across = (1.0 - (line / (half * 0.6)).abs()).max(0.0);
                let edge = (1.0 - (x / half).abs()).clamp(0.0, 0.25) * 4.0;
                h * across * across * edge * noise::ridged2(x * f, z * f, s, 5)
            });
            sh::to_ground(&mut m);
            {
                // Realism (round 7): rock, grass and snow by slope and
                // height, the mix live (rock, snow; presets Rocky, Grassy,
                // Snow-capped).
                use evani_procedural::surface::presets as sp;
                Output::Parts(evani_procedural::land::landform(&m, sp::ROCK_GRANITE_GREY.tint(v.color("color")).seed(s).scale(3.0), evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(GRASS_GREEN).seed(s)), 0.2), v.f("rock"), v.f("snow"), s))
            }
        });

    basic!(_r, "terrain-valleys", "terrain", "valleys", "Valleys", "ground",
        [SIZE, P::num("depth", 6.0, 0.5, 80.0).unit("m").describe("How far the sides rise over the floor (m)"), P::num("floor_width", 0.25, 0.05, 0.8).describe("Share of the width that is flat floor"), P::color("color", [0.16, 0.25, 0.09]).describe("Ground colour"), P::num("rock", 0.3, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the ground (1)"), P::num("snow", 0.0, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)")],
        |v: &Values, s: u32| {
            let (half, d, w) = (v.f("size") * 0.5, v.f("depth"), v.f("floor_width"));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                let bend = half * 0.15 * (noise::value2(x * 0.05, 1.0, s) - 0.5) * 2.0;
                let t = (((z - bend).abs() / half - w) / (1.0 - w)).max(0.0);
                d * t.powf(1.5) + 0.4 * noise::fbm2(x * 0.2, z * 0.2, s, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut m);
            {
                // Realism (round 7): rock, grass and snow by slope and
                // height, the mix live (rock, snow; presets Rocky, Grassy,
                // Snow-capped).
                use evani_procedural::surface::presets as sp;
                Output::Parts(evani_procedural::land::landform(&m, sp::ROCK_LIMESTONE.seed(s).scale(3.0), evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(v.color("color")).seed(s)), 0.2), v.f("rock"), v.f("snow"), s))
            }
        });

    // The canyons (Asset Studio column 12): their own file, crate::canyon.
    #[cfg(feature = "terrain-canyons")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("terrain", "canyons", "Canyons").material("ground"),
        params: &crate::canyon::PARAMS,
        make: crate::canyon::generate,
    }));

    basic!(_r, "terrain-cliffs", "terrain", "cliffs", "Cliffs", "ground",
        [SIZE, P::num("height", 8.0, 1.0, 120.0).unit("m").describe("Cliff height (m)"), P::num("steepness", 0.8, 0.1, 1.0).describe("How sheer the face is"), P::color("color", [0.3, 0.28, 0.25]).describe("Rock colour"), P::num("rock", 0.6, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the ground (1)"), P::num("snow", 0.0, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)")],
        |v: &Values, s: u32| {
            let (h, st) = (v.f("height"), v.f("steepness"));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                let edge = x + 2.0 * noise::fbm2(z * 0.15, 0.0, s, 3, 2.0, 0.5);
                let w = 0.3 + (1.0 - st) * 6.0;
                let t = (edge / w).clamp(-1.0, 1.0) * 0.5 + 0.5;
                h * t * t * (3.0 - 2.0 * t) + 0.35 * noise::fbm2(x * 0.4, z * 0.4, s ^ 7, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut m);
            {
                // Realism (round 7): rock, grass and snow by slope and
                // height, the mix live (rock, snow; presets Rocky, Grassy,
                // Snow-capped).
                use evani_procedural::surface::presets as sp;
                Output::Parts(evani_procedural::land::landform(&m, sp::ROCK_LIMESTONE.tint(v.color("color")).seed(s).scale(3.0), evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(GRASS_GREEN).seed(s)), 0.2), v.f("rock"), v.f("snow"), s))
            }
        });

    basic!(_r, "terrain-plateaus", "terrain", "plateaus", "Plateaus", "ground",
        [SIZE, P::num("height", 6.0, 1.0, 100.0).unit("m").describe("Plateau height (m)"), P::num("top_size", 0.5, 0.1, 0.9).describe("Share of the width the flat top covers"), P::color("color", [0.42, 0.26, 0.14]).describe("Rock colour"), P::num("rock", 0.6, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the ground (1)"), P::num("snow", 0.0, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)")],
        |v: &Values, s: u32| {
            let (half, h, top) = (v.f("size") * 0.5, v.f("height"), v.f("top_size"));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                let r = (x * x + z * z).sqrt() / half + 0.12 * noise::fbm2(x * 0.12, z * 0.12, s, 3, 2.0, 0.5);
                let t = ((top + 0.12 - r) / 0.12).clamp(0.0, 1.0);
                h * t * t * (3.0 - 2.0 * t)
            });
            sh::to_ground(&mut m);
            {
                // Realism (round 7): rock, grass and snow by slope and
                // height, the mix live (rock, snow; presets Rocky, Grassy,
                // Snow-capped).
                use evani_procedural::surface::presets as sp;
                Output::Parts(evani_procedural::land::landform(&m, sp::ROCK_SANDSTONE_RED.tint(v.color("color")).seed(s).scale(3.0), evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(GRASS_GREEN).seed(s)), 0.2), v.f("rock"), v.f("snow"), s))
            }
        });

    basic!(_r, "terrain-craters", "terrain", "craters", "Craters", "ground",
        [SIZE, P::num("radius", 8.0, 1.0, 150.0).unit("m").describe("Crater radius (m)"), P::num("depth", 2.5, 0.2, 40.0).unit("m").describe("Bowl depth (m)"), P::color("color", [0.3, 0.28, 0.25]).describe("Ground colour"), P::num("rock", 0.4, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the ground (1)"), P::num("snow", 0.0, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)")],
        |v: &Values, s: u32| {
            let (rad, d) = (v.f("radius"), v.f("depth"));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                let r = (x * x + z * z).sqrt() / rad;
                let bowl = if r < 1.0 { -d * (1.0 - r * r) } else { 0.0 };
                let rim = d * 0.45 * (-((r - 1.0) / 0.22).powi(2)).exp();
                bowl + rim + 0.15 * noise::fbm2(x * 0.3, z * 0.3, s, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut m);
            {
                // Realism (round 7): rock, grass and snow by slope and
                // height, the mix live (rock, snow; presets Rocky, Grassy,
                // Snow-capped).
                use evani_procedural::surface::presets as sp;
                Output::Parts(evani_procedural::land::landform(&m, sp::ROCK_BASALT.tint(v.color("color")).seed(s).scale(3.0), evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(GRASS_GREEN).seed(s)), 0.2), v.f("rock"), v.f("snow"), s))
            }
        });

    basic!(_r, "terrain-sand-dunes", "terrain", "sand-dunes", "Sand dunes", "ground",
        [SIZE, P::num("height", 2.5, 0.2, 40.0).unit("m").describe("Dune height (m)"), P::num("spacing", 9.0, 1.0, 100.0).unit("m").describe("Distance between dune crests (m)"), P::num("wind", 30.0, 0.0, 360.0).unit("°").describe("Wind heading the dunes face (degrees)"), P::color("color", [0.62, 0.46, 0.25]).describe("Sand colour")],
        |v: &Values, s: u32| {
            let (h, sp, dir) = (v.f("height"), v.f("spacing"), v.f("wind").to_radians());
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| sh::ripples(x, z, dir, sp, h, s) + sh::ripples(x, z, dir + 0.3, sp * 0.06, h * 0.03, s ^ 3));
            sh::to_ground(&mut m);
            Output::Mesh(m)
        });

    basic!(_r, "terrain-beaches", "terrain", "beaches", "Beaches", "ground",
        [SIZE, P::num("slope", 0.06, 0.01, 0.4).describe("Rise of the sand per metre"), P::color("color", [0.6, 0.48, 0.3]).describe("Sand colour"), P::color("water", [0.05, 0.22, 0.28]).describe("Water colour")],
        |v: &Values, s: u32| {
            let (size, sl) = (v.f("size"), v.f("slope"));
            let half = size * 0.5;
            let mut sand = geo::height_grid([size; 2], RES, |x, z| (x + half) * sl + 0.08 * sh::ripples(x, z, 0.2, 1.2, 1.0, s));
            sh::to_ground(&mut sand);
            // The sea covers the lower third.
            let level = size * 0.33 * sl;
            let sea = sh::water_sheet([size * 0.5, size], level, 24, |_, _| 0.0);
            let sea = sh::place(&sea, 1.0, 0.0, 0.0, [-size * 0.25, 0.0, 0.0]);
            Output::Parts(vec![Part::new(sand, v.color("color")), Part::new(sea, v.color("water")).alpha(0.8).material("glass")])
        });

    basic!(_r, "terrain-islands", "terrain", "islands", "Islands", "ground",
        [SIZE, P::num("height", 4.0, 0.5, 60.0).unit("m").describe("Island height over the sea (m)"), P::color("color", [0.2, 0.3, 0.1]).describe("Land colour"), P::color("water", [0.04, 0.2, 0.3]).describe("Sea colour")],
        |v: &Values, s: u32| {
            let (size, h) = (v.f("size"), v.f("height"));
            let half = size * 0.5;
            let sea_level = 0.6;
            let mut land = geo::height_grid([size; 2], RES, |x, z| {
                let r = (x * x + z * z).sqrt() / (half * 0.7) + 0.25 * noise::fbm2(x * 0.1, z * 0.1, s, 3, 2.0, 0.5);
                (sea_level + h) * (1.0 - r).max(-0.3) + 0.3 * noise::fbm2(x * 0.4, z * 0.4, s ^ 5, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut land);
            let sea = sh::water_sheet([size; 2], sea_level + 0.3 * h * 0.3, 24, |_, _| 0.0);
            Output::Parts(vec![Part::new(land, v.color("color")), Part::new(sea, v.color("water")).alpha(0.8).material("glass")])
        });

    // Realism (the Asset Studio caves session, column 29, 2026-11-08): a
    // limestone cave passage a player walks into, its walls bedded and
    // jointed into blocks as real geometry, rubble heaped along the walls'
    // feet, lit by the daylight at its mouth (crate::caves::entrance_lights);
    // in the stone and ground recipes.
    #[cfg(feature = "terrain-caves")]
    _r.add(Box::new(evani_procedural::FnAsset {
        info: evani_procedural::AssetInfo::cpu("terrain", "caves", "Caves").material("ground"),
        params: &crate::caves::PARAMS,
        make: crate::caves::generate,
    }));

    basic!(_r, "terrain-ravines", "terrain", "ravines", "Ravines", "ground",
        [SIZE, P::num("depth", 6.0, 1.0, 80.0).unit("m").describe("Ravine depth (m)"), P::num("width", 2.5, 0.5, 30.0).unit("m").describe("Ravine width (m)"), P::color("color", [0.2, 0.24, 0.12]).describe("Ground colour"), P::num("rock", 0.35, 0.0, 1.0).describe("Bare rock: from only the steepest faces (0) to most of the ground (1)"), P::num("snow", 0.0, 0.0, 1.0).describe("Snow: how far down from the top it lies (0: none)")],
        |v: &Values, s: u32| {
            let (half, d, w) = (v.f("size") * 0.5, v.f("depth"), v.f("width"));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                let meander = half * 0.25 * (noise::value2(x * 0.06, 4.0, s) - 0.5) * 2.0;
                let k = ((z - meander) / w).powi(2);
                d - d * (-k * k).exp() + 0.3 * noise::fbm2(x * 0.3, z * 0.3, s, 3, 2.0, 0.5)
            });
            sh::to_ground(&mut m);
            {
                // Realism (round 7): rock, grass and snow by slope and
                // height, the mix live (rock, snow; presets Rocky, Grassy,
                // Snow-capped).
                use evani_procedural::surface::presets as sp;
                Output::Parts(evani_procedural::land::landform(&m, sp::ROCK_SLATE.seed(s).scale(3.0), evani_procedural::land::patchy(evani_procedural::land::turf(sp::GROUND_GRASS_LAWN.tint(v.color("color")).seed(s)), 0.2), v.f("rock"), v.f("snow"), s))
            }
        });

    basic!(_r, "terrain-terraced-landscapes", "terrain", "terraced-landscapes", "Terraced landscapes", "ground",
        [SIZE, P::num("height", 8.0, 1.0, 100.0).unit("m").describe("Hill height (m)"), P::num("steps", 6.0, 2.0, 30.0).step(1.0).describe("Number of terraces"), P::color("color", [0.2, 0.3, 0.1]).describe("Ground colour")],
        |v: &Values, s: u32| {
            let (half, h, n) = (v.f("size") * 0.5, v.f("height"), v.f("steps").round().max(1.0));
            let mut m = geo::height_grid([v.f("size"); 2], RES, |x, z| {
                let r = (x * x + z * z).sqrt() / half + 0.2 * noise::fbm2(x * 0.08, z * 0.08, s, 3, 2.0, 0.5);
                let t = (1.0 - r).clamp(0.0, 1.0) * n;
                let stepped = t.floor() + ((t.fract() - 0.8) / 0.2).clamp(0.0, 1.0);
                h * stepped / n
            });
            sh::to_ground(&mut m);
            Output::Mesh(m)
        });
}
