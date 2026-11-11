//! The [`Relief`] resource and its frame system. It runs after
//! `material_proc.draw` (same stage, added after it) and, without touching
//! that system:
//!
//! * sets the relief kit code whenever the kit code's generation moved
//!   (the draw system installs the plain code first, or again on a look
//!   change; a frame later the relief code replaces it);
//! * appends the relief block to `kit_data` right after the world's records
//!   whenever the draw system has (re)sent them, and rewrites it in place
//!   when a param or the tier changes;
//! * reads the `material.relief.*` params and the active tier, rebaking only
//!   the patterns whose params (or bake size) changed.

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamError, ParamRegistry};
use evani_core::plugin::{LookData, PluginContext};
use evani_core::tiers::Tiers;
use evani_render::kit_material::KitMaterial;

use super::bake::{SurfaceBake, bake};
use super::params::ReliefParams;
use super::pattern::Pattern;
use super::{ReliefConfig, header, pack, tier_bake_size, tier_mode, wgsl};
use crate::ProcWorld;
use crate::draw::{decal_slot, sky_slot, world_slot};
use crate::shader::NO_SLOT;

/// Bakes kept for reuse: every pattern at the three tier sizes (Lite, Low,
/// the rest), with room for a few param changes.
const BAKE_CACHE: usize = 24;

/// The relief (a world resource next to [`ProcWorld`]).
#[derive(Debug, Clone)]
pub struct Relief {
    config: ReliefConfig,
    /// The tier the block is for (`None` until the first run: the full mode).
    tier: Option<String>,
    bakes: Vec<SurfaceBake>,
    /// What each bake was made from.
    baked: Vec<(Pattern, u32)>,
    /// Bakes made before, newest last (at most [`BAKE_CACHE`]): a tier switch
    /// back to a size baked before reuses it instead of baking on the frame.
    cache: Vec<((Pattern, u32), SurfaceBake)>,
    block: Vec<[f32; 4]>,
    params: Option<ReliefParams>,
    params_seen: Option<u64>,
    /// The kit-code generation right after this set its code.
    code_seen: Option<u64>,
    /// The record the block starts at in `kit_data` (the world's length).
    base: Option<usize>,
    /// The block in `kit_data` is stale.
    dirty: bool,
    /// The length of the block last put in `kit_data`.
    sent: Option<usize>,
    /// Patterns rebaked so far (for tests and the live tools).
    pub rebakes: u32,
    /// Started by [`install`]: the bakes are made one pattern a frame by
    /// [`Relief::stage`], and nothing reaches the GPU until they are all in.
    staging: bool,
    /// Tier sizes still to bake into the cache once staged, one a frame.
    warm: Vec<(Pattern, u32)>,
}

impl Relief {
    /// Bake every pattern for the full tier, and at the Lite and Low sizes
    /// into the cache, so no tier switch bakes on the frame (Low's first visit
    /// stalled the viewer 125 ms, 2026-10-25).
    pub fn new(config: ReliefConfig) -> Self {
        let mut r = Self::new_staged(config);
        r.staging = false;
        r.warm.clear();
        r.rebake();
        for t in ["lite", "low"] {
            let size = tier_bake_size(r.config.bake_size, t);
            for p in r.config.patterns() {
                r.cached_bake(&p, size);
            }
        }
        r
    }

    /// The same bakes made over the first frames instead of at once (the
    /// start-up task held the page 0.3-0.5 s baking every pattern at three
    /// sizes, 2026-10-29): [`Relief::stage`] bakes one pattern a frame for
    /// the current tier and only then builds the block and lets the system
    /// send code and data, so the finished look is the same; the Lite, Low
    /// and full sizes are then baked into the cache, one a frame.
    pub fn new_staged(config: ReliefConfig) -> Self {
        let mut r = Relief {
            config,
            tier: None,
            bakes: Vec::new(),
            baked: Vec::new(),
            cache: Vec::new(),
            block: Vec::new(),
            params: None,
            params_seen: None,
            code_seen: None,
            base: None,
            dirty: true,
            sent: None,
            rebakes: 0,
            staging: true,
            warm: Vec::new(),
        };
        for t in ["high", "low", "lite"] {
            let size = tier_bake_size(r.config.bake_size, t);
            for p in r.config.patterns() {
                r.warm.push((p, size));
            }
        }
        r
    }

    /// Whether the bakes are still being made (the system sends nothing).
    pub fn staging(&self) -> bool {
        self.staging
    }

    /// One frame of [`Relief::new_staged`]: bake one missing pattern of the
    /// current tier (true: still staging), else, once, build the block from
    /// the bakes; after that bake one cache size a frame.
    pub fn stage(&mut self) -> bool {
        if !self.staging {
            while let Some(w) = self.warm.pop() {
                if !self.cache.iter().any(|(key, _)| *key == w) {
                    self.cached_bake(&w.0, w.1);
                    break;
                }
            }
            return false;
        }
        let size = tier_bake_size(self.config.bake_size, self.tier_id());
        let missing = self.config.patterns().into_iter().find(|p| !self.cache.iter().any(|(key, _)| *key == (*p, size)));
        if let Some(p) = missing {
            self.cached_bake(&p, size);
            return true;
        }
        self.staging = false;
        self.rebake();
        false
    }

    /// The bake of `p` at `size`: from the cache, else baked now and kept.
    fn cached_bake(&mut self, p: &Pattern, size: u32) -> SurfaceBake {
        let want = (*p, size);
        if let Some((_, b)) = self.cache.iter().find(|(key, _)| *key == want) {
            return b.clone();
        }
        let b = bake(p, size);
        if self.cache.len() >= BAKE_CACHE {
            self.cache.remove(0);
        }
        self.cache.push((want, b.clone()));
        self.rebakes += 1;
        b
    }

    pub fn config(&self) -> &ReliefConfig {
        &self.config
    }

    /// The baked surfaces (brick, cobble, tiles, planks).
    pub fn bakes(&self) -> &[SurfaceBake] {
        &self.bakes
    }

    /// The block appended to `kit_data`.
    pub fn block(&self) -> &[[f32; 4]] {
        &self.block
    }

    /// The record the block starts at, once the system has placed it.
    pub fn base(&self) -> Option<usize> {
        self.base
    }

    fn tier_id(&self) -> &str {
        self.tier.as_deref().unwrap_or("high")
    }

    /// Change the config (without params); the system sends it next run.
    pub fn set_config(&mut self, config: ReliefConfig) {
        if config != self.config {
            self.config = config;
            if !self.staging {
                self.rebake();
            }
        }
    }

    /// Change the tier the block is built for.
    pub fn set_tier(&mut self, tier: &str) {
        if self.tier.as_deref() != Some(tier) {
            self.tier = Some(tier.to_string());
            if !self.staging {
                self.rebake();
            }
        }
    }

    /// Rebake what changed, then rebuild the block (the header alone when
    /// nothing needed a bake).
    fn rebake(&mut self) {
        let size = tier_bake_size(self.config.bake_size, self.tier_id());
        let pats = self.config.patterns();
        let mut baked_any = false;
        for (k, p) in pats.iter().enumerate() {
            let want = (*p, size);
            if self.baked.get(k) == Some(&want) {
                continue;
            }
            let b = self.cached_bake(p, size);
            if k < self.bakes.len() {
                self.bakes[k] = b;
                self.baked[k] = want;
            } else {
                self.bakes.push(b);
                self.baked.push(want);
            }
            baked_any = true;
        }
        let mode = tier_mode(self.tier_id());
        if baked_any || self.block.is_empty() {
            self.block = pack(&self.config, &self.bakes, mode);
        } else {
            let h = header(&self.config, &self.bakes, mode);
            self.block[..h.len()].copy_from_slice(&h);
        }
        self.dirty = true;
    }

    /// Register the params (defaults from the current config).
    pub fn register_params(&mut self, reg: &mut ParamRegistry) -> Result<(), ParamError> {
        self.params = Some(ReliefParams::register(reg, &self.config)?);
        self.params_seen = None;
        Ok(())
    }

    fn sync_params(&mut self, reg: Option<&ParamRegistry>) {
        let (Some(h), Some(r)) = (self.params.as_ref(), reg) else {
            return;
        };
        if self.params_seen == Some(r.revision()) {
            return;
        }
        self.params_seen = Some(r.revision());
        let c = h.read(r, &self.config);
        self.set_config(c);
    }
}

/// Add the relief after `evani_material_proc::install`: bake, register the
/// params and add the system.
pub fn install(ctx: &mut PluginContext, config: ReliefConfig) -> Result<(), String> {
    let mut r = Relief::new_staged(config);
    {
        let mut reg = ctx.world.res_mut::<ParamRegistry>();
        r.register_params(&mut reg).map_err(|e| e.to_string())?;
    }
    ctx.resource(r);
    ctx.system(system());
    Ok(())
}

/// The relief system (add it after `material_proc.draw`).
pub fn system() -> SystemDesc {
    SystemDesc::new("material_proc.relief", Stage::Frame, run)
}

/// One frame: params and tier, then code and data.
pub fn run(world: &mut World) {
    let Some((ws, ds, ss)) = world.try_res::<LookData>().map(|l| {
        let (ws, _) = world_slot(&l);
        (
            ws,
            decal_slot(&l, ws).map_or(NO_SLOT, |d| d.0),
            sky_slot(&l, ws).map_or(NO_SLOT, |s| s.0),
        )
    }) else {
        return;
    };
    let Some(base) = world.try_res::<ProcWorld>().map(|p| p.data().len()) else {
        return;
    };
    let Some(mut rel) = world.try_res_mut::<Relief>() else {
        return;
    };
    let tier = world.try_res::<Tiers>().map(|t| t.active().id.clone());
    if let Some(t) = tier {
        rel.set_tier(&t);
    }
    rel.sync_params(world.try_res::<ParamRegistry>().as_deref());
    if rel.stage() {
        return;
    }
    let Some(mut km) = world.try_res_mut::<KitMaterial>() else {
        return;
    };
    if rel.base != Some(base) {
        rel.base = Some(base);
        rel.code_seen = None;
        rel.dirty = true;
    }
    if rel.code_seen != Some(km.generations().0) {
        if let Some(code) = wgsl(ws, ds, ss, base) {
            km.set_wgsl(code);
        }
        rel.code_seen = Some(km.generations().0);
    }
    let len = km.data().len();
    let ours = rel.sent.is_some_and(|s| len == base + s);
    // Append right after the world's records when the draw system has just
    // (re)sent them; rewrite our own tail when it is stale. Anything else
    // (no world records yet, a tail we didn't write) waits.
    if len == base || (ours && rel.dirty) {
        let d = km.data_mut();
        d.truncate(base);
        d.extend_from_slice(&rel.block);
        rel.sent = Some(rel.block.len());
        rel.dirty = false;
    }
}
