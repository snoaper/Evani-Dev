//! The procedural module (`evani.module.procedural`): registers every
//! compiled-in asset's live params, keeps the [`Registry`] and the
//! [`ProcScene`] as world resources, draws the placed CPU assets, and hosts
//! the engine modules that wrapped (GPU) assets stand for, unchanged.

use evani_core::ecs::World;
use evani_core::params::{ParamId, ParamRegistry, ParamValue};
use evani_core::plugin::{EngineModule, LookData, PluginContext, PluginError};
use evani_render::mesh::MeshId;
use evani_render::{MeshInstance, MeshLibrary, RenderWorld};

use crate::asset::{Output, Path};
use crate::registry::Registry;
use crate::scene::ProcScene;

/// A wrapped engine module the library hosts (one per key).
pub struct Host {
    /// The key wrapped assets name ([`Path::Gpu`]'s `host`).
    pub key: &'static str,
    pub module: Box<dyn EngineModule>,
}

struct Cached {
    feature: String,
    /// Each part's mesh, colour (`None`: the asset's `color` param, read
    /// live) and material name.
    parts: Vec<(MeshId, Option<[f32; 4]>, &'static str)>,
    ids: Vec<ParamId>,
    /// The param revision it was made at, and its style.
    at: u64,
    style: String,
    /// The mesh density it was made at (the tier's; round 9).
    density: f32,
    triangles: u32,
    /// The surfaced parts' in-memory model (feature `surface`).
    #[cfg(feature = "surface")]
    model: Option<evani_render::model::ModelId>,
}

/// The module. Add it after the render module.
pub struct ProcModule {
    registry: Option<Registry>,
    hosts: Vec<Host>,
    /// Assets a hosted module draws itself ([`ProcModule::host_drawing`]):
    /// their placements are skipped here.
    drawn_by_host: Vec<&'static str>,
    cache: Vec<Cached>,
    error: Option<String>,
    /// Models of assets that left the scene, reused by the next one (the
    /// model table has no removal; replacing keeps it from growing).
    #[cfg(feature = "surface")]
    spare: Vec<evani_render::model::ModelId>,
}

impl ProcModule {
    pub fn new(registry: Registry) -> Self {
        Self {
            registry: Some(registry),
            hosts: Vec::new(),
            drawn_by_host: Vec::new(),
            cache: Vec::new(),
            error: None,
            #[cfg(feature = "surface")]
            spare: Vec::new(),
        }
    }

    /// Host a wrapped module (only when a compiled-in asset names `key`).
    pub fn host(mut self, key: &'static str, module: Box<dyn EngineModule>) -> Self {
        let wanted = self.registry.as_ref().is_some_and(|r| r.assets().any(|a| matches!(a.info().path, Path::Gpu { host, .. } if host == key)));
        if wanted && !self.hosts.iter().any(|h| h.key == key) {
            self.hosts.push(Host { key, module });
        }
        self
    }

    /// Host a wrapped module a CPU asset needs for part of itself (the
    /// shoreline's water is the nature pass's), whether or not a GPU asset
    /// names its key; once per key.
    pub fn host_always(mut self, key: &'static str, module: Box<dyn EngineModule>) -> Self {
        if !self.hosts.iter().any(|h| h.key == key) {
            self.hosts.push(Host { key, module });
        }
        self
    }

    /// Host `module`, which draws the placed `features` itself (a CPU asset
    /// whose look is a GPU pass: the volumetric clouds' ray-marched volume).
    /// Their placements stay in the [`ProcScene`] and their generators still
    /// make the CPU mesh (bounds, tests, a module-less host), but this
    /// module no longer draws it. Only features of compiled-in assets count.
    pub fn host_drawing(mut self, key: &'static str, features: &[&'static str], module: Box<dyn EngineModule>) -> Self {
        let have: Vec<&'static str> = features.iter().copied().filter(|f| self.registry.as_ref().is_some_and(|r| r.get(f).is_some())).collect();
        if !have.is_empty() && !self.hosts.iter().any(|h| h.key == key) {
            self.hosts.push(Host { key, module });
            self.drawn_by_host.extend(have);
        }
        self
    }

    /// Don't host these keys: the app adds those modules itself (the
    /// material viewer has the nature module already).
    pub fn hosted_elsewhere(mut self, keys: &[&str]) -> Self {
        self.hosts.retain(|h| !keys.contains(&h.key));
        self
    }

    /// The keys this module hosts.
    pub fn hosted(&self) -> Vec<&'static str> {
        self.hosts.iter().map(|h| h.key).collect()
    }
}

impl EngineModule for ProcModule {
    fn id(&self) -> &'static str {
        "evani.module.procedural"
    }

    fn label(&self) -> &'static str {
        "procedural"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let registry = self.registry.take().unwrap_or_default();
        registry.register_params(&mut ctx.world.res_mut::<ParamRegistry>()).map_err(|e| PluginError(format!("procedural: {e}")))?;
        if ctx.world.try_res::<MeshLibrary>().is_none() {
            ctx.resource(MeshLibrary::default());
        }
        if ctx.world.try_res::<ProcScene>().is_none() {
            ctx.resource(ProcScene::default());
        }
        ctx.resource(registry);
        for h in &mut self.hosts {
            h.module.build(ctx)?;
        }
        Ok(())
    }

    fn start(&mut self, world: &mut World) {
        for h in &mut self.hosts {
            h.module.start(world);
        }
    }

    fn frame(&mut self, world: &mut World) {
        for h in &mut self.hosts {
            h.module.frame(world);
        }
        self.draw(world);
    }

    fn visibility_changed(&mut self, world: &mut World, hidden: bool) {
        for h in &mut self.hosts {
            h.module.visibility_changed(world, hidden);
        }
    }

    fn status(&self, world: &World, out: &mut String) {
        let n = world.try_res::<Registry>().map_or(0, |r| r.assets().count());
        let placed = world.try_res::<ProcScene>().map_or(0, |s| s.placements.len());
        let tris: u32 = self.cache.iter().map(|c| c.triangles).sum();
        out.push_str(&format!("procedural: {n} assets, {placed} placed, {tris} tris"));
        if let Some(e) = &self.error {
            out.push_str(&format!(" ({e})"));
        }
    }
}

impl ProcModule {
    /// Make (or remake) each placed asset's meshes, draw the placements
    /// whose asset is on, and free the meshes of assets no longer placed
    /// (the mesh library has 248 custom slots).
    fn draw(&mut self, world: &mut World) {
        let Some(scene) = world.try_res::<ProcScene>().map(|s| s.clone()) else { return };
        let mut draws: Vec<MeshInstance> = Vec::new();
        #[cfg(feature = "surface")]
        let has_models = world.try_res::<evani_render::model::Models>().is_some();
        #[cfg(feature = "surface")]
        let mut glbs: Vec<(usize, Option<Vec<u8>>)> = Vec::new();
        {
            let Some(mut lib) = world.try_res_mut::<MeshLibrary>() else { return };
            // Assets that left the scene give their slots back.
            #[cfg(feature = "surface")]
            let spare = &mut self.spare;
            self.cache.retain(|c| {
                let keep = scene.placements.iter().any(|p| p.feature == c.feature);
                if !keep {
                    for (id, _, _) in &c.parts {
                        lib.remove(*id);
                    }
                    #[cfg(feature = "surface")]
                    spare.extend(c.model);
                }
                keep
            });
            if scene.placements.is_empty() {
                return;
            }
            let Some(registry) = world.try_res::<Registry>() else { return };
            let reg = world.res::<ParamRegistry>();
            let look = world.try_res::<LookData>();
            // Tiered mesh density (round 9): carved relief is refined less
            // on the lower tiers; a tier change remakes the assets.
            let density = world.try_res::<evani_core::tiers::Tiers>().map_or(1.0, |t| crate::detail::Density::for_tier(&t.active().id));
            for p in &scene.placements {
                let Some(a) = registry.get(&p.feature) else { continue };
                if a.info().path != Path::Cpu || !registry.is_on(&reg, &p.feature) || self.drawn_by_host.contains(&p.feature.as_str()) {
                    continue;
                }
                let at = self.cache.iter().position(|c| c.feature == p.feature);
                let stale = at.is_none_or(|i| {
                    let c = &self.cache[i];
                    c.style != scene.style || c.density != density || reg.any_changed_since(&c.ids, c.at)
                });
                if stale {
                    let Some(g) = registry.generate_at(&reg, &p.feature, &scene.style, density) else { continue };
                    // Typed building-block links (a game that adds the
                    // `Composition` resource): layers and base joins on the
                    // asset's own parts, never an extra mesh.
                    let output = match world.try_res::<crate::compose::Composition>() {
                        Some(c) => crate::compose::apply(c.graph, &p.feature, g.output, &mut |f| registry.generate_at(&reg, f, &scene.style, density).map(|g| g.output)),
                        None => g.output,
                    };
                    let triangles = output.triangles();
                    let mut pieces: Vec<(evani_render::mesh::Mesh, Option<[f32; 4]>, &'static str)> = Vec::new();
                    // Surfaced parts go to the textured-model path when this
                    // build has it (and the app has models); else they draw
                    // flat in their colour like any part.
                    #[cfg(feature = "surface")]
                    let mut surfaced: Vec<(evani_render::mesh::Mesh, crate::surface::Surface, Vec<[f32; 2]>)> = Vec::new();
                    match output {
                        Output::Mesh(m) => pieces.push((m, None, "")),
                        Output::Parts(ps) => {
                            for q in ps {
                                #[cfg(feature = "surface")]
                                if let (Some(sf), true) = (q.surface, has_models) {
                                    surfaced.push((q.mesh, sf, q.uvs));
                                    continue;
                                }
                                pieces.push((q.mesh, Some(q.color), q.material));
                            }
                        }
                        Output::Module => continue,
                    }
                    let ids: Vec<ParamId> = registry.param_names(&p.feature).iter().filter_map(|n| reg.id(n)).collect();
                    // Reuse the asset's slots where it has them, add or free the rest.
                    let mut old = at.map(|i| std::mem::take(&mut self.cache[i].parts)).unwrap_or_default();
                    let mut parts = Vec::with_capacity(pieces.len());
                    for (mesh, color, material) in pieces {
                        let id = match old.pop() {
                            Some((id, _, _)) => {
                                lib.replace(id, mesh);
                                id
                            }
                            None => match lib.try_add(mesh) {
                                Ok(id) => id,
                                Err(e) => {
                                    self.error = Some(e.to_string());
                                    continue;
                                }
                            },
                        };
                        parts.push((id, color, material));
                    }
                    for (id, _, _) in old {
                        lib.remove(id);
                    }
                    #[cfg(feature = "surface")]
                    let model = at.and_then(|i| self.cache[i].model);
                    let cached = Cached {
                        feature: p.feature.clone(),
                        parts,
                        ids,
                        at: reg.revision(),
                        density,
                        style: scene.style.clone(),
                        triangles,
                        #[cfg(feature = "surface")]
                        model,
                    };
                    let slot = match at {
                        Some(i) => {
                            self.cache[i] = cached;
                            i
                        }
                        None => {
                            self.cache.push(cached);
                            self.cache.len() - 1
                        }
                    };
                    #[cfg(feature = "surface")]
                    {
                        let refs: Vec<(&evani_render::mesh::Mesh, &crate::surface::Surface, &[[f32; 2]])> = surfaced.iter().map(|(m, s, u)| (m, s, u.as_slice())).collect();
                        glbs.push((slot, crate::surface::glb_given(&refs)));
                    }
                    #[cfg(not(feature = "surface"))]
                    let _ = slot;
                }
                let Some(c) = self.cache.iter().find(|c| c.feature == p.feature) else { continue };
                let (values, _) = registry.values(&reg, &p.feature).unwrap_or_default();
                let col = values.color("color");
                for (id, color, material) in &c.parts {
                    let mut m = MeshInstance::new(*id, p.position);
                    m.scale = [p.scale; 3];
                    m.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], p.yaw);
                    let name = if material.is_empty() { a.info().material } else { material };
                    m.material = look.as_ref().map_or(0, |l| RenderWorld::material(l, name));
                    m.color = color.unwrap_or([col[0], col[1], col[2], 1.0]);
                    draws.push(m);
                }
            }
        }
        #[cfg(feature = "surface")]
        self.draw_models(world, &scene, glbs);
        if let Some(mut rw) = world.try_res_mut::<RenderWorld>() {
            for m in draws {
                rw.draw_mesh(m);
            }
        }
    }

    /// The surfaced models: new or remade ones replace the asset's own,
    /// reuse a spare one or are added (an asset whose surfaced parts went
    /// away gives its model back); then every placed, switched-on asset with
    /// a model is drawn.
    #[cfg(feature = "surface")]
    fn draw_models(&mut self, world: &mut World, scene: &ProcScene, glbs: Vec<(usize, Option<Vec<u8>>)>) {
        let on: Vec<bool> = {
            let (Some(registry), Some(reg)) = (world.try_res::<Registry>(), world.try_res::<ParamRegistry>()) else { return };
            scene.placements.iter().map(|p| registry.is_on(&reg, &p.feature)).collect()
        };
        let Some(mut models) = world.try_res_mut::<evani_render::model::Models>() else { return };
        for (slot, glb) in glbs {
            let Some(c) = self.cache.get_mut(slot) else { continue };
            match glb {
                Some(bytes) => {
                    let id = match c.model.or_else(|| self.spare.pop()) {
                        Some(id) => {
                            models.replace_glb(id, &bytes);
                            id
                        }
                        None => models.add_glb("procedural/", &bytes),
                    };
                    c.model = Some(id);
                }
                None => self.spare.extend(c.model.take()),
            }
        }
        for (p, on) in scene.placements.iter().zip(on) {
            let Some(id) = self.cache.iter().find(|c| c.feature == p.feature).and_then(|c| c.model) else { continue };
            if on {
                let q = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], p.yaw);
                models.draw(evani_render::model::ModelInstance::new(id, p.position).rotation(q).scale([p.scale; 3]));
            }
        }
    }

    /// The custom meshes this module holds now (all parts of every cached
    /// asset).
    pub fn meshes_held(&self) -> usize {
        self.cache.iter().map(|c| c.parts.len()).sum()
    }
}

/// Whether a bool param is on (true when it isn't registered).
pub fn param_on(reg: &ParamRegistry, name: &str) -> bool {
    !matches!(reg.id(name).and_then(|id| reg.value(id)), Some(ParamValue::Bool(false)))
}
