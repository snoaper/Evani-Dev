//! The Asset & Style Studio in the viewer (feature `studio`; the app turns
//! it on with `material-studio`). The contracts are evani-studio's: this
//! module holds the state as a world resource and wires it to the viewer's
//! machinery, once a frame, in one system ([`sync_system`]):
//!
//! 1. **Params in.** A style control moved over its registered param (the
//!    panel, or evani-live `param_set`) becomes that style's control value;
//!    a catalogue switch moved while the studio shows a record (a number,
//!    an extra, a condition) becomes an edit remembered for that record.
//! 2. **Commands**, in order ([`StudioCommand`]): picks by the auto-match
//!    rule, controls, edits, light, quality, compare, save, a variation
//!    tile, reset, restore, open a saved combination.
//! 3. **Params out.** The record's style controls are written back to their
//!    params, so `schema_get` always shows what is drawn.
//! 4. **The bridge** ([`bridge`]), only while the studio is on screen
//!    ([`Studio::active`]): the record's material opens its catalogue row
//!    (`kit.material_view.cat.open`) and the record's remembered edits are
//!    laid over it; the orbit camera is stored on leaving a record and put
//!    back on returning; the light preset and the quality tier follow
//!    `View`, and the light keys write back into it.
//! 5. **Resolve.** When the state moved, [`Studio::current`] is resolved
//!    again and [`Studio::builds`] counts what had to be rebuilt: a new
//!    geometry key, a new variant key, a new pass key. A light or quality
//!    change moves none of them (acceptance test 7).
//!
//! The scene draws [`Studio::current`] (that is `Studio::state.resolve(..)`)
//! and nothing else; the variations strip is [`variations::variations`].
//!
//! Params (owner `kit.material_view`): `studio.<style>.<control>` (every
//! control each style lists, `evani_studio::styles::control_param`),
//! `studio.variations` and `studio.variations_marked`.
//!
//! WP1 (docs/studio/WORK_PACKAGES.md) owns this file and the `studio_*.rs`
//! files beside it.

use std::rc::Rc;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::plugin::{PluginContext, PluginError};
/// The contracts (the app and agents reach them through the kit).
pub use evani_studio;
pub use evani_studio::state::Tweak;
pub use evani_studio::{Catalogs, Compat, Notice, Pick, Resolved, Selection, StudioState};

#[path = "studio_bridge.rs"]
pub mod bridge;
#[path = "studio_params.rs"]
pub mod params;
#[cfg(test)]
#[path = "studio_tests.rs"]
mod tests;
#[path = "studio_variations.rs"]
pub mod variations;

pub use variations::{Variation, variations, variations_for};

/// The asset, style and material catalogues, parsed and cross-checked once
/// (shared, read-only).
#[derive(Debug, Clone)]
pub struct StudioData(pub Rc<Catalogs>);

/// What the page (or an agent over evani-live) asks the studio to do.
#[derive(Debug, Clone, PartialEq)]
pub enum StudioCommand {
    /// Pick an asset, a style or a material, or "Show on a horse".
    Pick(Pick),
    /// One of the current style's controls (voxel size, toon bands...).
    Control { id: String, value: f32 },
    /// A tuned value of the current combination (remembered for it): a
    /// catalogue switch by its param name.
    Edit { name: String, value: Tweak },
    /// A light preset by id: changes nothing else.
    Light(String),
    /// A quality tier by id (`auto` leaves the tier to the engine): changes
    /// nothing else.
    Quality(String),
    /// Side by side: 2 to 4 records, or none.
    Compare(Vec<Selection>),
    /// Save the current combination under a name.
    Save(String),
    /// A variations tile: this material on the same asset in the same style.
    /// It never moves the asset (even one the auto-match chose); a tile that
    /// doesn't go on the asset says why and changes nothing.
    Variation(String),
    /// The current combination's edits and camera dropped: back to the
    /// material's own values.
    Reset,
    /// Load a stored state (the page's storage, `Studio::storage_due`):
    /// ids the catalogues no longer hold are dropped.
    Restore(String),
    /// Open a saved combination by name: its record and its settings.
    Open(String),
}

/// What changing the record had to rebuild, counted since start: the
/// geometry stage (a new geometry key), the variant (a new variant key: the
/// bindings, the surface, the edits) and the screen pass (a new pass key).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Builds {
    pub geometry: u64,
    pub variant: u64,
    pub pass: u64,
}

/// The studio's state and its command queue.
#[derive(Debug, Clone)]
pub struct Studio {
    pub state: StudioState,
    pub pending: Vec<StudioCommand>,
    /// The last refused command's reason (the page shows it; never a dialog).
    pub error: Option<String>,
    /// The studio view is on screen. Only then does the bridge drive the
    /// catalogue's open row, the orbit camera and the light, so the legacy
    /// scenes keep theirs. The studio's scene (or page) sets it.
    pub active: bool,
    /// The current record, resolved (the scene draws this).
    pub current: Option<Resolved>,
    pub builds: Builds,
    resolved_at: u64,
    stored_at: u64,
    /// A `Restore` replaced the whole state this frame: the bridge starts
    /// over without storing the old record's camera into the new state.
    restored: bool,
}

impl Studio {
    pub fn new(state: StudioState) -> Self {
        let stored_at = state.revision;
        Self { state, pending: Vec::new(), error: None, active: false, current: None, builds: Builds::default(), resolved_at: 0, stored_at, restored: false }
    }

    pub fn push(&mut self, c: StudioCommand) {
        self.pending.push(c);
    }

    /// The state as JSON when it changed since the last call (the page writes
    /// it to its storage); `None` when nothing changed.
    pub fn storage_due(&mut self) -> Option<String> {
        if self.state.revision == self.stored_at {
            return None;
        }
        self.stored_at = self.state.revision;
        self.state.to_json().ok()
    }

    /// Resolve again if the state moved; count what changed.
    fn refresh(&mut self, c: &Catalogs) {
        if self.state.revision == self.resolved_at && self.current.is_some() {
            return;
        }
        self.resolved_at = self.state.revision;
        match self.state.resolve(c) {
            Ok(r) => {
                match &self.current {
                    Some(old) => {
                        self.builds.geometry += u64::from(old.geometry_key != r.geometry_key);
                        self.builds.variant += u64::from(old.variant_key != r.variant_key);
                        self.builds.pass += u64::from(old.pass_key != r.pass_key);
                    }
                    None => {
                        self.builds = Builds { geometry: self.builds.geometry + 1, variant: self.builds.variant + 1, pass: self.builds.pass + 1 };
                    }
                }
                self.current = Some(r);
            }
            Err(e) => self.error = Some(e),
        }
    }
}

/// The shipped catalogues: catalog/assets/assets.json, the styles' `studio`
/// blocks and the material catalogue (the copy the recipe crate embeds).
pub fn catalogs() -> Result<Catalogs, String> {
    Catalogs::parse(evani_studio::ASSETS_JSON, crate::styles::STYLES_JSON, evani_material_recipes::catalog::MATERIALS_JSON)
}

/// Open saved combination `name`: its record (both parts now chosen on
/// purpose) and its settings.
fn open_saved(state: &mut StudioState, c: &Catalogs, name: &str) -> Result<(), String> {
    let saved = state.saved.iter().find(|s| s.name == name).cloned().ok_or_else(|| format!("no saved combination `{name}`"))?;
    StudioState::check(c, &saved.selection)?;
    state.memory.insert(saved.selection.key(), saved.settings);
    state.selection = saved.selection;
    state.explicit.asset = true;
    state.explicit.material = true;
    state.pending = None;
    state.notice = Notice::None;
    state.revision += 1;
    Ok(())
}

/// Apply one command to the state (pure: no world). `Restore` is the
/// system's ([`sync_system`]); here it is refused.
pub fn apply(state: &mut StudioState, c: &Catalogs, cmd: StudioCommand) -> Result<(), String> {
    match cmd {
        StudioCommand::Pick(p) => state.apply(c, p),
        StudioCommand::Control { id, value } => state.set_control(c, &id, value),
        StudioCommand::Edit { name, value } => {
            state.settings_mut().edits.insert(name, value);
            Ok(())
        }
        StudioCommand::Light(l) => {
            state.set_light(&l);
            Ok(())
        }
        StudioCommand::Quality(q) => {
            state.set_quality(&q);
            Ok(())
        }
        StudioCommand::Compare(list) => state.set_compare(c, list),
        StudioCommand::Save(name) => {
            state.save(&name);
            Ok(())
        }
        StudioCommand::Variation(m) => {
            // The tile keeps the asset: for this one pick it counts as chosen
            // on purpose, and its auto-match standing is put back after.
            let was = state.explicit.asset;
            state.explicit.asset = true;
            let r = state.apply(c, Pick::Material(m));
            state.explicit.asset = was;
            r
        }
        StudioCommand::Reset => {
            if state.memory.remove(&state.selection.key()).is_some() {
                state.revision += 1;
            }
            Ok(())
        }
        StudioCommand::Open(name) => open_saved(state, c, &name),
        StudioCommand::Restore(_) => Err("restore runs in the studio system".into()),
    }
}

/// The frame's work, in the order the module docs give (Stage::Frame).
pub fn sync_system(world: &mut World) {
    let Some(data) = world.try_res::<StudioData>().map(|d| d.0.clone()) else { return };
    if world.try_res::<Studio>().is_none() {
        return;
    }
    // 1. Params in.
    params::read(world, &data);
    bridge::capture(world);
    // 2. Commands.
    let lights = bridge::light_ids(world);
    {
        let mut s = world.res_mut::<Studio>();
        for cmd in std::mem::take(&mut s.pending) {
            let r = match cmd {
                StudioCommand::Restore(json) => StudioState::from_json(&json, &data).map(|state| {
                    s.state = state;
                    s.stored_at = s.state.revision;
                    s.resolved_at = 0;
                    s.restored = true;
                }),
                StudioCommand::Light(l) if !lights.is_empty() && !lights.contains(&l) => Err(format!("no light preset `{l}` (the presets are {})", lights.join(", "))),
                cmd => apply(&mut s.state, &data, cmd),
            };
            if let Err(e) = r {
                s.error = Some(e);
            }
        }
        params::prune(&mut s.state, &data);
    }
    // 3. Params out.
    params::write(world, &data);
    // 4. The bridge.
    bridge::sync(world, &data);
    params::settle(world);
    // 5. Resolve.
    world.res_mut::<Studio>().refresh(&data);
}

/// Register the studio's data, state, params and system. The app calls this
/// only when built with its `material-studio` feature, after
/// `catalogue::install` (the bridge drives the catalogue's switches).
pub fn install(ctx: &mut PluginContext) -> Result<(), PluginError> {
    let c = catalogs().map_err(PluginError)?;
    let state = StudioState::new(&c).map_err(PluginError)?;
    let handles = params::register(ctx, &c)?;
    ctx.data_file("material_view.assets", evani_studio::ASSETS_JSON);
    ctx.resource(StudioData(Rc::new(c)));
    ctx.resource(Studio::new(state));
    ctx.resource(handles);
    ctx.resource(bridge::Bridge::default());
    ctx.system(SystemDesc::new("material_view.studio", Stage::Frame, sync_system));
    Ok(())
}
