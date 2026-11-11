//! The asset trait: what every library entry is.

use evani_render::mesh::Mesh;

use crate::spec::{ParamSpec, Values};

/// How an asset is made.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Path {
    /// A mesh made on the CPU from the params and the seed, uploaded to the
    /// engine's mesh library and drawn by [`crate::ProcModule`] wherever the
    /// game places it ([`crate::ProcScene`]).
    Cpu,
    /// An engine module that makes and draws it on the GPU itself (the
    /// spectral sea, the grass). The library entry is a wrapper: it names
    /// the module, its params and the evani.toml line, and the module is
    /// unchanged.
    Gpu {
        /// The wrapped module's key (`nature`, `grass`, `sky`): wrapped
        /// entries sharing a key share one hosted module.
        host: &'static str,
        /// The prefix of the wrapped module's own live params
        /// (`material.nature.water`, `material.grass`).
        params: &'static str,
    },
}

/// What identifies an asset.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetInfo {
    /// The category's id (`terrain`).
    pub category: &'static str,
    /// The asset's id inside its category (`rolling-hills`).
    pub id: &'static str,
    /// Its display name ("Rolling hills").
    pub name: &'static str,
    pub path: Path,
    /// The look material it draws with (`ground`, `accent`...; the look's
    /// first material when the look has no such name).
    pub material: &'static str,
}

impl AssetInfo {
    pub const fn cpu(category: &'static str, id: &'static str, name: &'static str) -> Self {
        Self { category, id, name, path: Path::Cpu, material: "ground" }
    }

    pub const fn gpu(category: &'static str, id: &'static str, name: &'static str, host: &'static str, params: &'static str) -> Self {
        Self { category, id, name, path: Path::Gpu { host, params }, material: "ground" }
    }

    pub const fn material(mut self, material: &'static str) -> Self {
        self.material = material;
        self
    }

    /// Its Cargo feature and evani.toml part: `<category>-<id>`.
    pub fn feature(&self) -> String {
        format!("{}-{}", self.category, self.id)
    }

    /// Its live params' prefix: `procedural.<category>.<id>` with `_` for `-`.
    pub fn prefix(&self) -> String {
        format!("procedural.{}.{}", self.category.replace('-', "_"), self.id.replace('-', "_"))
    }
}

/// One coloured piece of a multi-part asset (a tree's wood and leaves).
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub mesh: Mesh,
    /// Linear RGB and alpha (under 1: the renderer's screen-door fade, for
    /// mist, spray and glassy parts).
    pub color: [f32; 4],
    /// The look material it draws with; empty: the asset's
    /// ([`AssetInfo::material`]). A name the look lacks falls back to the
    /// look's first material.
    pub material: &'static str,
    /// A catalogue material (realism): with the base's `surface` feature
    /// the module draws the part through the textured-model path with this
    /// block, so the recipe stage paints its grain, pores or pebbles; else
    /// it draws flat in `color` as before (crate::surface).
    pub surface: Option<crate::surface::Surface>,
    /// The surfaced part's own uvs, one per vertex in metres (a leaf card's
    /// corners, bark laid along a branch); empty: the surface's layout
    /// ([`crate::surface::layout_given`]).
    pub uvs: Vec<[f32; 2]>,
}

impl Part {
    pub fn new(mesh: Mesh, rgb: [f32; 3]) -> Self {
        Self { mesh, color: [rgb[0], rgb[1], rgb[2], 1.0], material: "", surface: None, uvs: Vec::new() }
    }

    /// A part in a catalogue material; its flat colour (no recipe stage) is
    /// the material's colour A.
    pub fn surfaced(mesh: Mesh, s: crate::surface::Surface) -> Self {
        let c = s.base_rgb();
        Self { mesh, color: [c[0], c[1], c[2], 1.0], material: "", surface: Some(s), uvs: Vec::new() }
    }

    /// A surfaced part with its own uvs (one per vertex, metres).
    pub fn surfaced_uv(mesh: Mesh, s: crate::surface::Surface, uvs: Vec<[f32; 2]>) -> Self {
        Self { uvs, ..Self::surfaced(mesh, s) }
    }

    pub fn alpha(mut self, a: f32) -> Self {
        self.color[3] = a.clamp(0.0, 1.0);
        self
    }

    pub fn material(mut self, m: &'static str) -> Self {
        self.material = m;
        self
    }
}

/// What a generation gives.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// A CPU mesh (`[px, py, pz, nx, ny, nz]` vertices, u32 indices), its
    /// base at y = 0, drawn in the asset's `color` param.
    Mesh(Mesh),
    /// Several CPU meshes, each in its own colour and material.
    Parts(Vec<Part>),
    /// The wrapped engine module draws it (a [`Path::Gpu`] asset).
    Module,
}

impl Output {
    /// The single mesh of a [`Output::Mesh`].
    pub fn mesh(&self) -> Option<&Mesh> {
        match self {
            Output::Mesh(m) => Some(m),
            Output::Parts(_) | Output::Module => None,
        }
    }

    /// Every mesh it draws (none for a module).
    pub fn meshes(&self) -> Vec<&Mesh> {
        match self {
            Output::Mesh(m) => vec![m],
            Output::Parts(p) => p.iter().map(|p| &p.mesh).collect(),
            Output::Module => Vec::new(),
        }
    }

    /// The bounds of everything it draws; `None` for a module or no
    /// vertices.
    pub fn bounds(&self) -> Option<([f32; 3], [f32; 3])> {
        let ms: Vec<&Mesh> = self.meshes().into_iter().filter(|m| !m.vertices.is_empty()).collect();
        let mut it = ms.iter().map(|m| crate::geo::bounds(m));
        let first = it.next()?;
        Some(it.fold(first, |(lo, hi), (l, h)| (std::array::from_fn(|k| lo[k].min(l[k])), std::array::from_fn(|k| hi[k].max(h[k])))))
    }

    /// Its triangles.
    pub fn triangles(&self) -> u32 {
        self.meshes().iter().map(|m| m.triangles()).sum()
    }

    /// Whether every mesh is valid: triangles, indices in range, finite.
    pub fn is_valid(&self) -> bool {
        let ms = self.meshes();
        !ms.is_empty()
            && ms.iter().all(|m| !m.indices.is_empty() && m.indices.len() % 3 == 0 && m.indices.iter().all(|&i| (i as usize) < m.vertices.len()) && m.vertices.iter().all(|v| v.iter().all(|x| x.is_finite())))
    }
}

/// One procedural asset. Implement it once per asset, register it with
/// [`crate::Registry::add`] behind its own Cargo feature, and record it in
/// catalog/procedural/library.json (docs/PROCEDURAL.md "Adding an asset").
pub trait ProcAsset: 'static {
    fn info(&self) -> &AssetInfo;
    /// Its params' schema (the seed and the on/off switch are added by the
    /// registry; a wrapped module's own params are not listed here).
    fn params(&self) -> &[ParamSpec];
    /// The art styles it builds natively (`WK01`..`WK24`); any other style
    /// falls back to Realism (crate::style). Realism only by default.
    fn styles(&self) -> &[&'static str] {
        &[crate::style::REALISM]
    }
    /// Make it in `style` (one of [`ProcAsset::styles`]). Deterministic: the
    /// same values, seed and style give the same output on every run and
    /// device (no wall clock, no `rand`).
    fn generate(&self, values: &Values, seed: u64, style: &str) -> Output;
}

/// An asset made by a function: its info, its params and `make(values,
/// seed)`. The library's basic generators are these (one per asset, each
/// behind its own Cargo feature); Realism is their only style.
#[derive(Clone, Copy)]
pub struct FnAsset {
    pub info: AssetInfo,
    pub params: &'static [ParamSpec],
    pub make: fn(&Values, u32) -> Output,
}

impl ProcAsset for FnAsset {
    fn info(&self) -> &AssetInfo {
        &self.info
    }

    fn params(&self) -> &[ParamSpec] {
        self.params
    }

    /// What `make` gives, standing on the ground: anything under y = 0 (a
    /// trunk's or a stem's foot, a tube's lower edge) is pressed up to it,
    /// where it is hidden anyway; empty parts are dropped.
    fn generate(&self, values: &Values, seed: u64, _style: &str) -> Output {
        let mut out = (self.make)(values, seed as u32);
        let ground = |m: &mut Mesh| {
            for v in &mut m.vertices {
                v[1] = v[1].max(0.0);
            }
        };
        match &mut out {
            Output::Mesh(m) => ground(m),
            Output::Parts(ps) => {
                // A part a setting empties (no stools, no stripes) is left out.
                ps.retain(|p| !p.mesh.indices.is_empty());
                ps.iter_mut().for_each(|p| ground(&mut p.mesh));
            }
            Output::Module => {}
        }
        out
    }
}
