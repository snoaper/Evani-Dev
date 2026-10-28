//! The bake: grow locks over a compiled tier and write them into its `.glb`
//! as one more solid, [`SOLID`] (`fur_locks`). Build time only: a game gets
//! the locks as plain triangles in the model file it already downloads, so
//! the engine and the game's code are unchanged (0 runtime bytes).

use evani_model::collide;
use evani_model::compile::{self, TierMesh};
use evani_model::gltf;
use evani_model::intent;
use serde_json::{Map, Value, json};

use crate::{FurLocks, LockError, LockParams, SOLID, Stats, fur_locks, lerr};

/// `tm` with the locks added as the solid `fur_locks` (world space, one
/// instance at the origin, never mirrored). Each lock keeps the part of the
/// triangle it grew from, so region colours and skin weights follow it.
/// The tier's triangle count includes the locks afterwards; the locks have
/// their own cap ([`crate::MAX_TRIS`]) on top of the tier's budget, so turn
/// `kit.fur_locks.count` down for a light tier.
pub fn bake_tier(tm: &TierMesh, params: &LockParams) -> Result<(TierMesh, FurLocks), LockError> {
    if tm.solids.iter().any(|s| s.solid == SOLID) {
        return Err(lerr(format!("this tier already has a `{SOLID}` solid; bake once")));
    }
    let surface = crate::FurSurface::from_solids(&tm.model, &tm.solids);
    let locks = fur_locks(&surface, params)?;
    let mut out = tm.clone();
    if !locks.solid.mesh.tris.is_empty() {
        out.tris += locks.solid.mesh.tris.len();
        out.solids.push(locks.solid.clone());
    }
    Ok((out, locks))
}

/// What one bake made.
#[derive(Clone, Debug)]
pub struct Baked {
    pub glb: Vec<u8>,
    pub stats: Stats,
    /// Triangles of the tier before the locks.
    pub body_tris: usize,
    pub tier: String,
}

/// The `extras.evani.fur_locks` record written into the file.
pub fn record(stats: &Stats, params: &LockParams) -> Value {
    json!({
        "solid": SOLID,
        "locks": stats.locks,
        "triangles": stats.triangles,
        "seed": params.get("kit.fur_locks.seed"),
        "tool": concat!("evani-model-fur-locks ", env!("CARGO_PKG_VERSION")),
    })
}

/// Compile tier `tier` of the `.evmodel` text `src`, grow its locks and
/// write the `.glb` (no normal maps: the plain mesh with region colours and,
/// for a skinned model, skin weights). The same input gives the same bytes.
pub fn bake_glb(src: &str, tier: &str, params: &LockParams, threads: usize) -> Result<Baked, LockError> {
    let diags = |d: Vec<intent::Diag>| lerr(d.iter().filter(|x| !x.warning).map(|x| format!("{}: {} (fix: {})", x.code, x.msg, x.fix)).collect::<Vec<_>>().join("; "));
    let j = intent::parse_text(src).map_err(diags)?;
    let only = [tier.to_string()];
    let tiers = compile::compile(&j, Some(&only), threads).map_err(diags)?;
    let Some(tm) = tiers.into_iter().find(|t| t.tier.name == tier) else {
        return Err(lerr(format!("the model has no `{tier}` tier")));
    };
    let body_tris = tm.tris;
    let (baked, locks) = bake_tier(&tm, params)?;
    let col = collide::build_collision(&baked.model);
    let mut meta = Map::new();
    meta.insert("fur_locks".into(), record(&locks.stats, params));
    let glb = gltf::write_tier(&baked, &col, None, &meta);
    Ok(Baked { glb, stats: locks.stats, body_tris, tier: tier.to_string() })
}
