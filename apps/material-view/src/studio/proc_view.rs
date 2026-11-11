//! The procedural section's live view: the `procedural` scene (library.json)
//! draws the open item in a scene that fits it, with the engine's own
//! effects (evani-material-nature's water and glass, evani-material-proc's
//! sky dome), so what the page's sliders move is exactly what a game gets.
//!
//! - **Ocean:** the spectral sea (evani-material-nature's part
//!   `spectral-sea`, its own pass; ocean pass 2) to the horizon over a bed
//!   as deep as `material.nature.water.sea_depth`; or against a
//!   sand beach that rises as the `beach_*` params say (the sand moves with
//!   them). Every ocean preset is the spectral sea (the library's choice,
//!   2026-11-02; the older analytic sea stays the nature pass's water that
//!   games get with `nature = ["ocean"]`, unchanged).
//! - **Lake and river:** the nature pass's water in grassy banks (a pond or
//!   a long channel), its bed shallow and coloured by the preset.
//! - **Glass:** a bottle, a pane and a thick block on a checker floor, so
//!   tint, refraction and reflection read against a pattern.
//! - **Sky:** a wide, slightly raised view over a meadow and far hills.
//!
//! [`ProcState`] keeps the section's state: the open item, its preset, the
//! values each item had (so switching items keeps each one's tuning) and
//! the values the rest of the studio had before the section opened (put
//! back when it closes, so a stormy sky never follows the user back to the
//! materials). While the scene shows, the dome's sun is the item's own
//! (`sky.follow_light` off) and the scene light comes from that sun, so the
//! sun slider moves the glints and highlights too.

use std::collections::HashMap;

use evani_core::ecs::{Stage, SystemDesc, World};
use evani_core::params::{ParamRegistry, ParamValue};
use evani_core::plugin::PluginContext;
use evani_kit_material_view::{Command, Viewer};
use evani_material_nature::{Kind, NatureSurfaces, Sea, SeaSurfaces, Surface};
use evani_render::{MeshId, RenderWorld};

use super::proc_data::{self as pd, Item, ProcLib, Shape, Stage as Staging};
use crate::scenes::common::{FLOOR_SIZE, contact, material, mesh};

/// The scene the section draws in (library.json).
pub const PROC_SCENE: &str = "procedural";

/// What the page (or a test) asks of the section (the page is wasm only).
#[derive(Debug, Clone, PartialEq)]
#[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
pub enum ProcCommand {
    /// Open an item (its viewer material, camera and values).
    Open(String),
    /// Apply one of the open item's presets.
    Preset(String),
    /// One param back to the open preset's value.
    Reset(String),
    /// Every param of the open item back to the open preset.
    ResetAll,
    /// Switch a library item on or off (its `procedural.*.on` param).
    Toggle(String),
}

/// The section's state (a resource).
#[derive(Debug, Clone)]
pub struct ProcState {
    pub lib: ProcLib,
    /// The procedural scene was showing last frame.
    pub on: bool,
    /// The open item's id.
    pub item: String,
    /// The preset each item has on (by item id).
    pub preset: HashMap<String, String>,
    /// Each item's values when it was last left.
    stash: HashMap<String, Vec<(String, ParamValue)>>,
    /// The studio's values from before the section opened.
    outside: Vec<(String, ParamValue)>,
    /// The library item the shadow map was last fitted to.
    shadow_item: String,
    /// What the nature pass was handed (the item and its staging).
    handed: Option<Vec<Surface>>,
    /// The spectral sea was handed to its pass (the sea presets).
    sea_handed: bool,
    pub pending: Vec<ProcCommand>,
    /// Bumped on every change the page shows (item, preset).
    pub revision: u64,
    /// The library was checked against the registry (on the first frame,
    /// once every module has registered its params).
    checked: bool,
    /// The (item, preset) whose staged camera the viewer has (so a user's
    /// own orbit is kept until the preset changes), and the item's own
    /// camera from before a staged one, put back for presets without one.
    cam_for: Option<(String, String)>,
    cam_home: Option<evani_kit_material_view::orbit::Orbit>,
    /// The deck camera's lift over the long swell (m) and the target height
    /// it was written with (so a Reset or a new camera starts afresh).
    lift: f32,
    lift_written: f32,
    /// The grass handed to evani-grass (a grass item, or a library terrain
    /// that wears blades).
    grass_handed: Option<GrassHand>,
    /// The library asset placed in the procedural module's scene.
    asset_handed: Option<String>,
    /// The water look the shoreline's own replaced as it opened (put back
    /// when another asset opens; the tab's leaving restores its own).
    shore_prev: Vec<(String, ParamValue)>,
    /// What the library view's studio was last written for: the item, the
    /// viewer's light preset and sun drag (None: the studio's own values).
    /// Written once per change, so a live edit (the Look Lab, an agent, a
    /// shot script's paramSet) stays until the next one.
    view_for: Option<(String, usize, u32, u32)>,
    /// The asset and whether its close-up was on when the subject fit was
    /// last set (a close-up frames part of the asset: no fit pulls back).
    fit_key: (Option<String>, bool),
}

impl ProcState {
    pub fn new(lib: ProcLib) -> Self {
        let item = lib.ready().next().map(|i| i.id.clone()).unwrap_or_default();
        ProcState { lib, on: false, item, preset: HashMap::new(), stash: HashMap::new(), outside: Vec::new(), handed: None, sea_handed: false, pending: Vec::new(), revision: 0, checked: false, cam_for: None, cam_home: None, lift: 0.0, lift_written: f32::NAN, grass_handed: None, asset_handed: None, shore_prev: Vec::new(), shadow_item: String::new(), view_for: None, fit_key: (None, false) }
    }

    #[cfg_attr(not(any(target_arch = "wasm32", test)), allow(dead_code))]
    pub fn push(&mut self, c: ProcCommand) {
        self.pending.push(c);
    }

    pub fn open_item(&self) -> Option<&Item> {
        self.lib.item(&self.item)
    }

    /// The open item's staging key (`ocean`, `grass`, `library`...).
    pub fn stage_key(&self) -> String {
        self.open_item().map_or_else(|| self.item.clone(), |i| i.stage_as.clone())
    }

    /// The open item's preset id.
    pub fn open_preset(&self) -> Option<&str> {
        self.preset.get(&self.item).map(String::as_str)
    }

    /// The open item's staging (its preset's, else the item's first preset's).
    pub fn staging(&self) -> Staging {
        let Some(it) = self.open_item() else { return Staging::default() };
        let p = self.open_preset().and_then(|id| it.preset(id)).or_else(|| it.presets.first());
        p.and_then(|p| p.stage).or_else(|| it.presets.iter().find_map(|p| p.stage)).unwrap_or_default()
    }

    /// What the open item's params are under its open preset.
    pub fn baseline(&self, reg: &ParamRegistry) -> Vec<(String, ParamValue)> {
        let Some(it) = self.open_item() else { return Vec::new() };
        let p = self.open_preset().and_then(|id| it.preset(id));
        pd::baseline(reg, it, p)
    }
}

pub fn install(ctx: &mut PluginContext) {
    match ProcLib::load() {
        Ok(lib) => {
            #[cfg(not(feature = "procedural"))]
            let lib = {
                let mut lib = lib;
                lib.without_library();
                lib
            };
            ctx.resource(ProcState::new(lib));
        }
        Err(e) => {
            ctx.world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: procedural: {e}"));
        }
    }
}

/// The frame system, after the kit's viewer (its light lands first) and the
/// other scenes' (the garden hands its surfaces over first).
pub fn install_system(ctx: &mut PluginContext) {
    ctx.system(SystemDesc::new("material-view.scene.procedural", Stage::Frame, frame));
}

/// The dome's sun as the scene's light direction (towards the sun).
fn sun_dir(reg: &ParamRegistry) -> Option<[f32; 3]> {
    let get = |n: &str| match reg.id(n).and_then(|id| reg.value(id)) {
        Some(ParamValue::F32(x)) => Some(x),
        _ => None,
    };
    // Never quite at or under the horizon: the ground would go black.
    let el = get("sky.sun_elevation")?.max(2.0);
    Some(evani_material_proc::sky::sun_direction(el, get("sky.sun_azimuth")?))
}

fn set(reg: &mut ParamRegistry, name: &str, v: ParamValue) {
    if let Some(id) = reg.id(name)
        && reg.value(id) != Some(v)
    {
        let _ = reg.set(id, v);
    }
}

fn frame(world: &mut World) {
    let Some(mut st) = world.try_res_mut::<ProcState>().map(|mut s| std::mem::replace(&mut *s, ProcState::new(ProcLib::default()))) else { return };
    step(world, &mut st);
    world.insert_resource(st);
}

/// Viewing choices while a library asset is staged (Realism): no rim light,
/// so the side away from the sun is the darkest part of the object, not a
/// pale sky-coloured rim (Chris, 2026-11-04, on the rocks). Put back when
/// another item opens or the tab is left. Rounds 6 and 7 dropped the film
/// rig's back kick and wrap and cut its fill, bounce and reflections; round
/// 7 measured the shadow sides darker but the whole picture dimmer.
///
/// Round 8 (the research plan, docs/procedural-realism-progress.md) puts a
/// photo studio in its place, measured first: the viewer had no soft
/// shadows and no shadow cascades built at all (the hard, stepped edges of
/// every round), and the film's two-colour sky lit the shadow sides blue.
/// * The light: a warm low sun as the key, soft shadows with a physical
///   size (`pcss_light_deg`: about 14 cm of penumbra a metre from the
///   contact), no shadow tint.
/// * The ambient: the engine's sky light (`look.stylizedpbr.studio`) turned
///   into a studio of soft panels (`render.sky.studio`: an overhead light,
///   a key softbox on the sun's side, a dim fill card opposite, walls and a
///   floor bounce) for the diffuse light and the reflections, its ambient
///   occlusion on the ambient and reflections only, never the sun.
/// * The picture: Khronos PBR Neutral (`look.stylizedpbr.tonemap`), a fixed
///   exposure and saturation 1, so a grey stays its grey.
/// * The backdrop: no sky dome; a warm pale grey the floor fades into (the
///   haze, set per item from its camera) and a gentle depth of field with a
///   1-step dither (the studio's miniature pass at `grade` 0).
///
/// The studio itself is the engine's (`evani_procedural::studio::PRESET`), so
/// a game that applies it sees a library asset as the library shows it; the
/// viewer adds only what a photograph adds round it (`VIEWER_EXTRAS`).
#[cfg(feature = "procedural")]
use evani_procedural_lib::base::studio::{PRESET as STUDIO, coerce};
#[cfg(not(feature = "procedural"))]
const STUDIO: [(&str, ParamValue); 0] = [];

/// The viewer's own additions round the studio: no sky dome or sun disc, a
/// warm pale backdrop the lit floor fades into, and a gentle depth of field
/// with a 1-step dither (the studio's miniature pass at `grade` 0).
pub const VIEWER_EXTRAS: [(&str, ParamValue); 7] = [
    ("render.clear_color", ParamValue::Color([0.8, 0.74, 0.63])),
    ("sky.visible", ParamValue::Bool(false)),
    ("sky.sun_disc", ParamValue::Bool(false)),
    ("render.studio_pass.mode", ParamValue::Enum(3)),
    ("render.studio_pass.focus", ParamValue::F32(0.5)),
    ("render.studio_pass.band", ParamValue::F32(0.5)),
    ("render.studio_pass.blur", ParamValue::F32(0.3)),
];
/// The extras' last: the miniature pass as a plain depth of field.
const DOF_GRADE: (&str, ParamValue) = ("render.studio_pass.grade", ParamValue::F32(0.0));

/// Everything the library view sets: the studio, then the viewer's extras.
pub fn library_view() -> impl Iterator<Item = (&'static str, ParamValue)> {
    STUDIO.into_iter().chain(VIEWER_EXTRAS).chain([DOF_GRADE])
}
/// The studio haze (look params), set per item from its camera distance:
/// the floor starts to fade at `HAZE.0` times the distance and is the
/// backdrop at `HAZE.1` times.
const HAZE_PARAMS: [&str; 2] = ["look.stylizedpbr.haze_near", "look.stylizedpbr.haze_far"];
const HAZE: (f32, f32) = (1.7, 5.5);

#[cfg(not(feature = "procedural"))]
fn coerce(like: ParamValue, v: ParamValue) -> Option<ParamValue> {
    (like.ty() == v.ty()).then_some(v)
}
/// The shadow map's half width (m), fitted to a library asset while it is
/// staged.
const SHADOW_DISTANCE: &str = "fx.shadows.distance";

/// The shoreline library asset: it hands the nature pass its own water.
#[cfg(feature = "procedural-water")]
const SHORE: &str = "water-shorelines";

fn step(world: &mut World, st: &mut ProcState) {
    if !st.checked {
        st.checked = true;
        // The procedural library joins once its module has registered it.
        #[cfg(feature = "procedural")]
        if let Some(r) = world.try_res::<evani_procedural_lib::base::Registry>() {
            st.lib.add_library(&r);
            st.revision += 1;
        }
        let p = st.lib.unknown(&world.res::<ParamRegistry>());
        if !p.is_empty() {
            world.res_mut::<evani_core::overlay::ErrorLog>().error(format!("material-view: procedural.json names what the registry lacks: {}", p.join(", ")));
        }
    }
    let (scene, material) = world.try_res::<Viewer>().map_or_else(Default::default, |v| (v.scene.clone(), v.material.clone()));
    let showing = scene == PROC_SCENE;
    let mut names = st.lib.all_params();
    names.push("sky.follow_light".into());
    names.extend(library_view().map(|(n, _)| n.to_string()));
    names.extend(HAZE_PARAMS.iter().map(|n| n.to_string()));
    names.push(SHADOW_DISTANCE.into());
    // The shoreline's water look comes back with the studio's on leaving.
    #[cfg(feature = "procedural-water")]
    names.extend(evani_procedural_lib::shore::WATER_LOOK.iter().map(|(n, _)| n.to_string()));
    // The items' quality switches are viewing choices: put back on leaving.
    for q in st.lib.ready().filter_map(|i| i.quality.clone()) {
        if !names.contains(&q) {
            names.push(q);
        }
    }
    if showing != st.on {
        st.on = showing;
        st.revision += 1;
        if showing {
            // Keep the studio's values; the item's own come back in below.
            st.outside = pd::read(&world.res::<ParamRegistry>(), &names);
            set(&mut world.res_mut::<ParamRegistry>(), "sky.follow_light", ParamValue::Bool(false));
            let item = pd::item_of(&material).filter(|i| st.lib.item(i).is_some_and(|it| it.ready)).map(str::to_string);
            let item = item.unwrap_or_else(|| st.item.clone());
            open(world, st, &item, true);
        } else {
            let now = pd::read(&world.res::<ParamRegistry>(), &st.open_item().map(Item::params).unwrap_or_default());
            st.stash.insert(st.item.clone(), now);
            let back = std::mem::take(&mut st.outside);
            pd::write(&mut world.res_mut::<ParamRegistry>(), &back);
            // Hand the nature pass back only if it still holds ours (the
            // garden may have just handed it its own).
            if let (Some(mine), Some(n)) = (st.handed.take(), world.try_res::<NatureSurfaces>())
                && n.list() == mine
            {
                n.clear();
            }
            if std::mem::take(&mut st.sea_handed)
                && let Some(sea) = world.try_res::<SeaSurfaces>()
            {
                sea.clear();
            }
            hand_grass(world, st, None);
            // The studio's values (just put back) stand, not the shoreline's
            // stash from inside the tab.
            st.shore_prev.clear();
            hand_asset(world, st, None);
            // The viewer puts the next material's own camera on.
            st.cam_for = None;
            st.cam_home = None;
        }
    }
    if !showing {
        st.pending.clear();
        return;
    }
    // A viewer material naming another item (a live agent, the legacy row).
    // (An alias or library item shows its staging's material, which is not
    // another item.)
    if let Some(i) = pd::item_of(&material)
        && i != st.item
        && i != st.stage_key()
        && st.lib.item(i).is_some_and(|it| it.ready)
    {
        let i = i.to_string();
        open(world, st, &i, false);
    }
    for c in std::mem::take(&mut st.pending) {
        match c {
            ProcCommand::Open(id) => {
                if let Some(m) = st.lib.item(&id).filter(|it| it.ready && id != st.item).map(pd::viewer_material) {
                    // Library assets share one viewer material: opening it
                    // again would put its own camera back over the staged one.
                    if let Some(mut v) = world.try_res_mut::<Viewer>()
                        && v.material != m
                    {
                        v.push(Command::Material(m));
                    }
                    open(world, st, &id, false);
                }
            }
            ProcCommand::Preset(p) => {
                let Some(it) = st.open_item().cloned() else { continue };
                if let Some(pr) = it.preset(&p) {
                    pd::apply_preset(&mut world.res_mut::<ParamRegistry>(), &it, Some(pr));
                    st.preset.insert(it.id.clone(), p);
                    st.revision += 1;
                }
            }
            ProcCommand::Reset(name) => {
                let base = st.baseline(&world.res::<ParamRegistry>());
                if let Some(v) = base.into_iter().find(|(n, _)| *n == name) {
                    pd::write(&mut world.res_mut::<ParamRegistry>(), &[v]);
                }
            }
            ProcCommand::ResetAll => {
                let base = st.baseline(&world.res::<ParamRegistry>());
                pd::write(&mut world.res_mut::<ParamRegistry>(), &base);
            }
            ProcCommand::Toggle(id) => {
                if let Some(n) = st.lib.item(&id).and_then(|i| i.on_param.clone()) {
                    let mut reg = world.res_mut::<ParamRegistry>();
                    let on = is_on(&reg, Some(&n));
                    set(&mut reg, &n, ParamValue::Bool(!on));
                    st.revision += 1;
                }
            }
        }
    }
    // The scene's light follows the dome's sun.
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        set(&mut reg, "sky.follow_light", ParamValue::Bool(false));
        if let Some(d) = sun_dir(&reg)
            && let Some(id) = reg.id("render.light.direction")
            && reg.value(id).is_none_or(|v| match v {
                ParamValue::Vec3(c) => c.iter().zip(d).any(|(a, b)| (a - b).abs() > 1e-4),
                _ => true,
            })
        {
            let _ = reg.set(id, ParamValue::Vec3(d));
        }
    }
    // Library assets: the realism view (no rim); other items: the studio's own.
    {
        let library = st.open_item().is_some_and(|i| i.stage_as == "library");
        // The shadow map fitted to the object (a 52 m map gives a 1 m
        // object stepped shadow edges), once per opened item.
        let mut fit = None;
        if library && st.shadow_item != st.item {
            #[cfg(all(feature = "procedural", feature = "material-styles"))]
            if let Some(f) = st.open_item().and_then(|i| i.asset.clone())
                && let Some((hh, across)) = subject_size(world, &f)
            {
                fit = Some((hh.max(across) * 3.0).clamp(4.0, 200.0));
            }
            // Only once fitted: an asset not generated yet on its first
            // frame is fitted on a later one (round 5's stepped shadows were
            // the plain 26 m box).
            if fit.is_some() || !cfg!(all(feature = "procedural", feature = "material-styles")) {
                st.shadow_item = st.item.clone();
            }
        }
        if !library && !st.shadow_item.is_empty() {
            st.shadow_item.clear();
            fit = st.outside.iter().find(|(m, _)| m == SHADOW_DISTANCE).and_then(|(_, v)| if let ParamValue::F32(x) = v { Some(*x) } else { None });
        }
        let light = world.try_res::<Viewer>().map_or((0, 0, 0), |v| (v.light, v.sun_az.to_bits(), v.sun_el.to_bits()));
        let view_for = library.then(|| (st.item.clone(), light.0, light.1, light.2));
        let cam_dist = st.staging().camera.map(|c| c.dist);
        let mut reg = world.res_mut::<ParamRegistry>();
        if let (Some(d), Some(id)) = (fit, reg.id(SHADOW_DISTANCE)) {
            let _ = reg.set(id, ParamValue::F32(d));
        }
        if st.view_for != view_for {
            let haze = cam_dist.map(|d| [ParamValue::F32(d * HAZE.0), ParamValue::F32(d * HAZE.1)]);
            let all = library_view().chain(HAZE_PARAMS.iter().zip(haze.unwrap_or([ParamValue::F32(0.0); 2])).map(|(n, v)| (*n, v)));
            for (n, v) in all {
                let want = if library { Some(v) } else { st.outside.iter().find(|(m, _)| m == n).map(|(_, x)| *x) };
                if let (Some(want), Some(id)) = (want, reg.id(n))
                    && let Some(want) = reg.value(id).and_then(|like| coerce(like, want))
                    && reg.value(id) != Some(want)
                {
                    let _ = reg.set(id, want);
                }
            }
            st.view_for = view_for;
        }
    }
    stage_camera(world, st);
    ride_swell(world, st);
    let staging = st.staging();
    let key = st.stage_key();
    // A library item switched off is removed: nothing of it is handed on.
    let on = is_on(&world.res::<ParamRegistry>(), st.open_item().and_then(|i| i.on_param.as_ref()));
    #[cfg_attr(not(feature = "procedural-water"), allow(unused_mut))]
    let mut want = if on { surfaces(&key, staging) } else { Vec::new() };
    // The shoreline brings its own water: the library's rectangle at its
    // water level, as a game hands it over (its look goes on in hand_asset).
    #[cfg(feature = "procedural-water")]
    if on
        && staging.shape == Shape::Asset
        && st.open_item().and_then(|i| i.asset.as_deref()) == Some(SHORE)
        && let (Some(r), Some(reg)) = (world.try_res::<evani_procedural_lib::base::Registry>(), world.try_res::<ParamRegistry>())
        && let Some(w) = evani_procedural_lib::shore::water(&r, &reg, [0.0; 3], 0.0, 1.0)
    {
        want.push(w);
    }
    if let Some(n) = world.try_res::<NatureSurfaces>()
        && (st.handed.as_ref() != Some(&want) || n.list() != want)
    {
        if want.is_empty() {
            n.clear();
        } else {
            n.set(&want);
        }
        st.handed = Some(want);
    }
    // The spectral sea for the sea presets, nothing for the others.
    let sea = if on { sea_of(&key, staging) } else { None };
    if let Some(s) = world.try_res::<SeaSurfaces>()
        && s.get() != sea
    {
        match sea {
            Some(x) => s.set(x),
            None => s.clear(),
        }
    }
    st.sea_handed = sea.is_some();
    let grass = match staging.shape {
        Shape::Field(k) if on => Some(GrassHand::Field(k)),
        Shape::Asset if on => st.open_item().and_then(|i| i.asset.as_deref()).and_then(|f| asset_grass(&world.res::<ParamRegistry>(), f)),
        _ => None,
    };
    hand_grass(world, st, grass);
    let asset = (on && staging.shape == Shape::Asset).then(|| st.open_item().and_then(|i| i.asset.clone())).flatten();
    hand_asset(world, st, asset);
    // A realism asset's Close-up preset frames part of it: the subject fit
    // (which pulls back to show the whole asset) is off while it shows.
    #[cfg(feature = "material-styles")]
    {
        let key = (st.asset_handed.clone(), st.open_preset() == Some("close-up"));
        if key != st.fit_key {
            #[cfg(feature = "procedural")]
            let size = if key.1 { None } else { key.0.as_deref().and_then(|f| subject_size(world, f)) };
            #[cfg(not(feature = "procedural"))]
            let size = None;
            if let Some(mut fit) = world.try_res_mut::<crate::scenes::fit::SubjectFit>() {
                fit.0 = size;
            }
            st.fit_key = key;
        }
    }
    draw(world, &key, staging);
}

/// Whether a library item's switch is on (true without a switch).
pub fn is_on(reg: &ParamRegistry, param: Option<&String>) -> bool {
    !matches!(param.and_then(|n| reg.id(n)).and_then(|id| reg.value(id)), Some(ParamValue::Bool(false)))
}

/// Hand evani-grass one patch of the kind on show (or take ours back).
#[cfg_attr(not(feature = "procedural"), allow(unused_variables))]
/// Grass handed to evani-grass: a grass item's field (its kind, index into
/// `Kind::ALL`, over the usual square), or the patch a library terrain wears
/// (`evani_procedural_lib::wrapped::turf_grass`, the same call a game makes).
#[derive(Debug, Clone, PartialEq)]
pub enum GrassHand {
    Field(u8),
    #[cfg(feature = "procedural")]
    Patch(evani_procedural_lib::evani_grass::Patch),
    /// A patch on a height grid (the hills: `n` x `n` heights).
    #[cfg(feature = "procedural")]
    Ground(evani_procedural_lib::evani_grass::Patch, usize, Vec<f32>),
}

/// The patch library terrain `feature` wears, from its live params.
#[cfg_attr(not(feature = "procedural"), allow(unused_variables))]
pub fn asset_grass(reg: &ParamRegistry, feature: &str) -> Option<GrassHand> {
    #[cfg(feature = "procedural")]
    {
        let get = |n: &str, d: f32| match reg.id(n).and_then(|id| reg.value(id)) {
            Some(ParamValue::F32(x)) => x,
            _ => d,
        };
        // procedural.<category>.<asset> (`_` for `-`).
        let (c, a) = feature.split_once('-')?;
        let p = format!("procedural.{c}.{}", a.replace('-', "_"));
        #[cfg(feature = "procedural-terrain")]
        if feature == "terrain-rolling-hills" {
            let (patch, n, h) = evani_procedural_lib::wrapped::hills_grass(reg, [0.0; 3]);
            return Some(GrassHand::Ground(patch, n, h));
        }
        return evani_procedural_lib::wrapped::turf_grass(feature, get(&format!("{p}.size"), 30.0), get(&format!("{p}.edge"), 0.3), get(&format!("{p}.bumps"), 0.05), [0.0; 3]).map(GrassHand::Patch);
    }
    #[allow(unreachable_code)]
    None
}

fn hand_grass(world: &mut World, st: &mut ProcState, grass: Option<GrassHand>) {
    if st.grass_handed == grass {
        return;
    }
    #[cfg(feature = "procedural")]
    if let Some(g) = world.try_res::<evani_procedural_lib::evani_grass::Grass>() {
        use evani_procedural_lib::evani_grass::{Kind, Patch};
        g.clear_patches();
        match &grass {
            Some(GrassHand::Field(k)) => {
                if let Some(k) = Kind::ALL.get(usize::from(*k)).copied() {
                    g.add_patch(Patch::new(k, [-GRASS_HALF, -GRASS_HALF], [GRASS_HALF, GRASS_HALF]));
                }
            }
            Some(GrassHand::Patch(p)) => {
                g.add_patch(*p);
            }
            Some(GrassHand::Ground(p, n, h)) => {
                g.add_patch_on(*p, *n, h);
            }
            None => {}
        }
    }
    st.grass_handed = grass;
}

/// Place the library asset on show in the procedural module's scene (or
/// take it back).
#[cfg_attr(not(feature = "procedural"), allow(unused_variables))]
fn hand_asset(world: &mut World, st: &mut ProcState, feature: Option<String>) {
    if st.asset_handed == feature {
        return;
    }
    // An asset bigger than the viewer reaches is shown as a scale model
    // (`pd::view_scale`: the mountains' 4.4 km massif).
    #[cfg(feature = "procedural")]
    let k = feature.as_deref().and_then(|f| asset_bounds(world, f)).map_or(1.0, |(lo, hi)| pd::view_scale(lo, hi));
    #[cfg(feature = "procedural")]
    if let Some(mut s) = world.try_res_mut::<evani_procedural_lib::base::ProcScene>() {
        s.clear();
        if let Some(f) = &feature {
            let i = s.place(f, [0.0, 0.0, 0.0]);
            s.placements[i].scale = k;
        }
    }
    // The shoreline's coastal water look (lace foam, swash fronts, caustics,
    // its depth colours), put on as it opens and taken off as it closes.
    #[cfg(feature = "procedural-water")]
    {
        let mut reg = world.res_mut::<ParamRegistry>();
        if st.asset_handed.as_deref() == Some(SHORE) {
            pd::write(&mut reg, &std::mem::take(&mut st.shore_prev));
        }
        if feature.as_deref() == Some(SHORE) {
            let names: Vec<String> = evani_procedural_lib::shore::WATER_LOOK.iter().map(|(n, _)| n.to_string()).collect();
            st.shore_prev = pd::read(&reg, &names);
            let _ = evani_procedural_lib::shore::apply_water_look(&mut reg);
        }
    }
    // The asset fits the part of the view the panels leave open (a half
    // window stacks the cards and the panel over the view): its size goes
    // to the view's fit, which centres it there and pulls back to fit.
    #[cfg(feature = "material-styles")]
    {
        #[cfg(feature = "procedural")]
        let size = feature.as_deref().and_then(|f| subject_size(world, f));
        #[cfg(not(feature = "procedural"))]
        let size = None;
        if let Some(mut fit) = world.try_res_mut::<crate::scenes::fit::SubjectFit>() {
            fit.0 = size;
        }
    }
    st.asset_handed = feature;
}

/// A library asset's bounds (m) as it is now set.
#[cfg(feature = "procedural")]
fn asset_bounds(world: &World, feature: &str) -> Option<([f32; 3], [f32; 3])> {
    use evani_procedural_lib::base::Registry;
    let reg = world.res::<ParamRegistry>();
    let style = world.try_res::<evani_procedural_lib::base::ProcScene>().map(|s| s.style.clone())?;
    let g = world.try_res::<Registry>()?.generate(&reg, feature, &style)?;
    g.output.bounds()
}

/// A library asset's half height and half width (m) as it is now set and
/// shown (a scale model past `pd::VIEW_MAX`).
#[cfg(all(feature = "procedural", feature = "material-styles"))]
fn subject_size(world: &World, feature: &str) -> Option<(f32, f32)> {
    let (lo, hi) = asset_bounds(world, feature).map(|(lo, hi)| pd::view_bounds(lo, hi))?;
    // Seen from the side and above: the footprint's diagonal across, the
    // height plus some of the footprint up.
    let (w, d, h) = (hi[0] - lo[0], hi[2] - lo[2], hi[1] - lo[1]);
    let across = (w * w + d * d).sqrt() * 0.5;
    Some(((h * 0.5 + across * 0.35).max(0.15), across.max(0.15)))
}

/// The grass patch's half size (m).
const GRASS_HALF: f32 = 14.0;

/// A preset with its own camera (the storm presets: low across the sea, as
/// from a deck) puts it on when it is picked; a preset without one puts the
/// item's own camera back. Only when the item or preset changes, and only
/// once the viewer shows the item (its own camera lands first), so the
/// user's orbiting is kept in between.
fn stage_camera(world: &mut World, st: &mut ProcState) {
    let Some(preset) = st.open_preset().map(str::to_string) else { return };
    let key = (st.item.clone(), preset);
    if st.cam_for.as_ref() == Some(&key) {
        return;
    }
    let Some(mut v) = world.try_res_mut::<Viewer>() else { return };
    if st.open_item().is_none_or(|it| v.material != pd::viewer_material(it)) {
        return;
    }
    // Another item since: the viewer has put that item's camera on.
    if st.cam_for.as_ref().is_some_and(|(i, _)| *i != key.0) {
        st.cam_home = None;
    }
    match st.staging().camera {
        Some(c) => {
            let home = *st.cam_home.get_or_insert(v.home);
            // A big asset's fitted camera may stand farther out than the
            // viewer's usual limit, a small one's closer in.
            let o = evani_kit_material_view::orbit::Orbit { az: home.az, el: c.el, dist: c.dist, target: c.target, min_dist: home.min_dist.min(c.dist * 0.2), max_dist: home.max_dist.max(c.dist * 4.0), fov: c.fov }.clamped();
            v.orbit = o;
            v.home = o;
            v.revision += 1;
        }
        None => {
            if let Some(home) = st.cam_home.take() {
                v.orbit = home;
                v.home = home;
                v.revision += 1;
            }
        }
    }
    st.cam_for = Some(key);
}

/// Clearance the deck camera keeps over the long swell under it (m): the
/// shorter waves the swell's filter leaves out stand at most about this.
pub const DECK_CLEAR: f32 = 3.0;

/// A preset's deck camera rides the long swell, as a ship's deck does: when
/// a crest passing under it would cover the eye, the camera rises with it
/// (the view's pitch, so the horizon's place on screen, stays), and settles
/// back as it passes, so the storm's taller crests never put the view under
/// the water. The lift goes on the viewer's orbit each frame and comes off
/// again the next, so the user's own orbit, pan and Reset are kept.
fn ride_swell(world: &mut World, st: &mut ProcState) {
    // Only over the spectral sea (a library asset's camera is not a deck).
    let riding = st.on && st.staging().shape.spectral() && st.staging().camera.is_some() && st.cam_for.as_ref().is_some_and(|(i, _)| *i == st.item);
    let Some(mut orbit) = world.try_res::<Viewer>().map(|v| v.orbit) else { return };
    // Our last lift off again, if the orbit still carries it.
    if (orbit.target[1] - st.lift_written).abs() < 1e-4 {
        orbit.target[1] -= st.lift;
    }
    st.lift = 0.0;
    st.lift_written = f32::NAN;
    if riding {
        let pose = orbit.pose(0.0);
        let h = {
            let reg = world.res::<ParamRegistry>();
            world.try_res::<SeaSurfaces>().and_then(|s| s.height_at(&reg, [pose.eye[0], pose.eye[2]], 60.0)).unwrap_or(0.0)
        };
        let lift = (SURFACE_Y + h + DECK_CLEAR - pose.eye[1]).max(0.0);
        orbit.target[1] += lift;
        st.lift = lift;
        st.lift_written = orbit.target[1];
    }
    if let Some(mut v) = world.try_res_mut::<Viewer>()
        && v.orbit.target[1] != orbit.target[1]
    {
        v.orbit.target[1] = orbit.target[1];
    }
}

/// The spectral sea the item shows: an open sea at the water's height,
/// reaching 40 km each way, for the open-sea and beach stagings.
pub fn sea_of(item: &str, s: Staging) -> Option<Sea> {
    // `item` is the staging key (an alias stages as its target).
    (matches!(item, "ocean" | "water") && s.shape.spectral()).then(|| Sea::new(SURFACE_Y, [0.0, 0.0], SEA_REACH))
}

/// Open item `id`: the current item's values kept, `id`'s own put back (or
/// its first preset on first open).
fn open(world: &mut World, st: &mut ProcState, id: &str, entering: bool) {
    let Some(it) = st.lib.item(id).cloned() else { return };
    if !entering && st.on && id != st.item {
        let now = pd::read(&world.res::<ParamRegistry>(), &st.open_item().map(Item::params).unwrap_or_default());
        st.stash.insert(st.item.clone(), now);
        // The item left takes its quality preview with it (the lake's Low
        // must not flatten the glass): back to what the studio had.
        if let Some(q) = st.open_item().and_then(|i| i.quality.clone())
            && it.quality.as_ref() != Some(&q)
            && let Some(v) = st.outside.iter().find(|(n, _)| *n == q).cloned()
        {
            pd::write(&mut world.res_mut::<ParamRegistry>(), &[v]);
        }
    }
    st.item = it.id.clone();
    match st.stash.get(&it.id) {
        Some(vals) => {
            let vals = vals.clone();
            pd::write(&mut world.res_mut::<ParamRegistry>(), &vals);
        }
        None => {
            let first = it.presets.first();
            pd::apply_preset(&mut world.res_mut::<ParamRegistry>(), &it, first);
            if let Some(p) = first {
                st.preset.insert(it.id.clone(), p.id.clone());
            }
        }
    }
    st.revision += 1;
}

/// The water surface's height.
const SURFACE_Y: f32 = 0.0;
/// The banks' top above the water.
const BANK_TOP: f32 = 0.14;
/// The lake: centre and half size; the river: centre z and half width.
const LAKE: ([f32; 2], [f32; 2]) = ([0.0, -5.0], [8.0, 5.5]);
const RIVER: (f32, f32) = (-4.0, 2.6);
/// How far the open sea and the river run (half size, m).
const FAR: f32 = 900.0;
/// How far the spectral sea runs (half size, m): past the horizon.
const SEA_REACH: f32 = 40_000.0;
/// The glass table's checker: tile size and tiles per side.
const TILE: f32 = 0.2;
const TILES: i32 = 14;

/// The nature surfaces the item shows.
pub fn surfaces(item: &str, s: Staging) -> Vec<Surface> {
    match item {
        "ocean" | "water" => {
            let (c, half) = match s.shape {
                Shape::Sea => ([0.0, 0.0], [FAR, FAR]),
                Shape::Lake => LAKE,
                Shape::River => ([0.0, RIVER.0], [FAR, RIVER.1]),
                // The spectral sea draws itself (sea_of).
                Shape::Open | Shape::Beach | Shape::Field(_) | Shape::Asset => return Vec::new(),
            };
            vec![Surface::new(Kind::WaterRect, [c[0], SURFACE_Y, c[1]], [half[0], 0.0, half[1]])]
        }
        "glass" => vec![
            // A bottle (round body, narrow neck), a standing pane and a
            // thick block.
            Surface::new(Kind::GlassBottle, [-0.32, 0.2, 0.02], [0.09, 0.11, 0.09]),
            Surface::new(Kind::GlassBottle, [-0.32, 0.445, 0.02], [0.03, 0.06, 0.03]),
            Surface::new(Kind::GlassPane, [0.28, 0.33, -0.22], [0.26, 0.28, 0.012]).with_yaw(-0.45),
            Surface::new(Kind::GlassPane, [0.06, 0.12, 0.34], [0.12, 0.11, 0.12]).with_yaw(0.6),
        ],
        _ => Vec::new(),
    }
}

/// The spectral sea's bed and beach as its params set them.
struct Beach {
    depth: f32,
    slope: f32,
    distance: f32,
    /// Heading towards land (x, z).
    land: [f32; 2],
}

fn beach_of(reg: &ParamRegistry) -> Beach {
    let get = |n: &str, d: f32| match reg.id(&format!("material.nature.water.{n}")).and_then(|id| reg.value(id)) {
        Some(ParamValue::F32(x)) => x,
        _ => d,
    };
    let dir = get("beach_direction", 270.0).to_radians();
    Beach { depth: get("sea_depth", 200.0), slope: get("beach_slope", 0.0), distance: get("beach_distance", 60.0), land: [dir.cos(), dir.sin()] }
}

/// A rock: a low cluster of three squashed boulders breaking the surface
/// (dark, in the stage's rough ground material), with its contact blob.
fn rock(rw: &mut RenderWorld, mat: u32, at: [f32; 3], size: [f32; 3], bed: f32) {
    const ROCK: [f32; 4] = [0.105, 0.098, 0.088, 1.0];
    let parts = [([0.0, 0.0, 0.0], [1.0, 1.0, 1.0]), ([0.42, -0.12, 0.18], [0.62, 0.72, 0.58]), ([-0.36, -0.2, -0.22], [0.5, 0.55, 0.62])];
    for (o, k) in parts {
        let p = [at[0] + o[0] * size[0], at[1] + o[1] * size[1], at[2] + o[2] * size[2]];
        rw.draw_mesh(mesh(MeshId::SPHERE, p, [size[0] * k[0], size[1] * k[1], size[2] * k[2]], mat, ROCK));
    }
    contact(rw, [at[0], bed, at[2]], size[0].max(size[2]) * 0.6, at[1] + size[1] * 0.5 - bed);
}

fn draw(world: &mut World, item: &str, s: Staging) {
    let ground = material(world, "ground");
    let accent = material(world, "accent");
    let beach = beach_of(&world.res::<ParamRegistry>());
    let Some(mut rw) = world.try_res_mut::<RenderWorld>() else { return };
    let rgba = |c: [f32; 3]| [c[0], c[1], c[2], 1.0];
    const GRASS: [f32; 4] = [0.09, 0.2, 0.045, 1.0];
    match item {
        "ocean" | "water" if s.shape.spectral() => {
            // The bed as deep as the sea is set (it shows only in shallow
            // water), and for a beach the sand rising out of it.
            let bed = SURFACE_Y - beach.depth.min(400.0);
            rw.draw_mesh(mesh(MeshId::PLANE, [0.0, bed, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, rgba(s.bed)));
            if s.shape == Shape::Beach && beach.slope > 0.0 {
                let (l, m) = (beach.land, beach.slope);
                // The sand's top is the beach plane y = m (x . l - d); a thick
                // slab tilted about the axis along the shore.
                // It rises to a 3 m dune top, then the land stays flat.
                let n = evani_render::math::normalize([-m * l[0], 1.0, -m * l[1]]);
                let (half, thick, top) = (600.0, 40.0, 3.0);
                let inland = top / m;
                let along = beach.distance + inland - half;
                let rise = m * (inland - half);
                let at = [l[0] * along - n[0] * thick * 0.5, SURFACE_Y + rise - n[1] * thick * 0.5, l[1] * along - n[2] * thick * 0.5];
                let mut sand = mesh(MeshId::CUBE, at, [half * 2.0, thick, half * 2.0], ground, rgba(s.bed));
                sand.rotation = evani_render::math::quat_axis_angle(evani_render::math::normalize([-l[1], 0.0, l[0]]), m.atan());
                rw.draw_mesh(sand);
                let land = beach.distance + inland + 1500.0;
                rw.draw_mesh(mesh(MeshId::CUBE, [l[0] * land, SURFACE_Y + top - thick * 0.5, l[1] * land], [3000.0, thick, 3000.0], ground, rgba(s.bed)));
            }
            // (Nothing else in the sea: a boulder of squashed spheres reads
            // as a test object, and the waves now hide and show it oddly.)
        }
        "ocean" | "water" => {
            let bed = SURFACE_Y - s.depth;
            rw.draw_mesh(mesh(MeshId::PLANE, [0.0, bed, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, rgba(s.bed)));
            // Banks: boxes from under the bed up to the bank top round the water.
            let low = bed - 0.2;
            let h = BANK_TOP - low;
            let y = low + h * 0.5;
            let mut bank = |x0: f32, x1: f32, z0: f32, z1: f32| {
                rw.draw_mesh(mesh(MeshId::CUBE, [(x0 + x1) * 0.5, y, (z0 + z1) * 0.5], [x1 - x0, h, z1 - z0], ground, GRASS));
            };
            match s.shape {
                Shape::Sea => {}
                Shape::Lake => {
                    let ([cx, cz], [hx, hz]) = LAKE;
                    let w = 120.0;
                    bank(cx - hx - w, cx + hx + w, cz + hz, cz + hz + w);
                    bank(cx - hx - w, cx + hx + w, cz - hz - w, cz - hz);
                    bank(cx - hx - w, cx - hx, cz - hz, cz + hz);
                    bank(cx + hx, cx + hx + w, cz - hz, cz + hz);
                }
                Shape::River => {
                    let (cz, hz) = RIVER;
                    bank(-FAR, FAR, cz + hz, cz + hz + 200.0);
                    bank(-FAR, FAR, cz - hz - 200.0, cz - hz);
                }
                Shape::Open | Shape::Beach | Shape::Field(_) | Shape::Asset => {}
            }
            // Rocks breaking the surface (shore foam rings them).
            let rocks: &[([f32; 3], [f32; 3])] = match s.shape {
                Shape::Sea | Shape::Open | Shape::Beach | Shape::Field(_) | Shape::Asset => &[([-4.6, -0.05, -4.5], [2.6, 1.0, 1.9]), ([6.5, -0.1, -13.0], [4.2, 1.6, 3.0])],
                Shape::Lake => &[([-4.2, -0.05, -3.0], [1.2, 0.5, 0.9]), ([4.6, -0.05, -8.0], [1.6, 0.6, 1.2])],
                Shape::River => &[([-2.4, -0.05, -3.6], [1.0, 0.45, 0.8]), ([3.2, -0.05, -4.6], [1.2, 0.5, 0.9])],
            };
            for &(at, size) in rocks {
                rock(&mut rw, ground, at, size, bed);
            }
        }
        "grass" => {
            // Bare soil to the horizon under the patch.
            rw.draw_mesh(mesh(MeshId::PLANE, [0.0, -0.01, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, [0.1, 0.075, 0.05, 1.0]));
        }
        "library" => {
            // A plain light floor: the asset itself is the procedural
            // module's draw (its ProcScene).
            rw.draw_mesh(mesh(MeshId::PLANE, [0.0, -0.002, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, [0.36, 0.35, 0.33, 1.0]));
        }
        "glass" => {
            // The floor a little under the table top, and on it a checker
            // of light and dark tiles whose tops are the table (y = 0).
            rw.draw_mesh(mesh(MeshId::PLANE, [0.0, -0.03, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, [0.5, 0.48, 0.45, 1.0]));
            let off = TILE * TILES as f32 * 0.5;
            for i in 0..TILES {
                for j in 0..TILES {
                    let c = if (i + j) % 2 == 0 { [0.06, 0.056, 0.052, 1.0] } else { [0.72, 0.7, 0.66, 1.0] };
                    let at = [TILE * (i as f32 + 0.5) - off, -0.01, TILE * (j as f32 + 0.5) - off];
                    rw.draw_mesh(mesh(MeshId::CUBE, at, [TILE, 0.02, TILE], accent, c));
                }
            }
            // The pane stands in a small wooden foot.
            let mut foot = mesh(MeshId::CUBE, [0.28, 0.025, -0.22], [0.56, 0.05, 0.06], accent, [0.25, 0.14, 0.07, 1.0]);
            foot.rotation = evani_render::math::quat_axis_angle([0.0, 1.0, 0.0], -0.45);
            rw.draw_mesh(foot);
            for (at, r, h) in [([-0.32, 0.0, 0.02], 0.11, 0.5), ([0.28, 0.0, -0.22], 0.28, 0.6), ([0.06, 0.0, 0.34], 0.15, 0.23)] {
                contact(&mut rw, at, r, h);
            }
        }
        _ => {
            // The sky: a meadow and far hills under a wide view.
            rw.draw_mesh(mesh(MeshId::PLANE, [0.0, 0.0, 0.0], [FLOOR_SIZE, 1.0, FLOOR_SIZE], ground, GRASS));
            for (at, size) in [([-180.0, 0.0, -420.0], [520.0, 70.0, 260.0]), ([160.0, 0.0, -520.0], [600.0, 110.0, 300.0]), ([420.0, 0.0, -300.0], [300.0, 50.0, 200.0])] {
                rw.draw_mesh(mesh(MeshId::SPHERE, at, size, ground, [0.11, 0.2, 0.08, 1.0]));
            }
            rw.draw_mesh(mesh(MeshId::SPHERE, [3.0, 0.3, -9.0], [1.6, 1.1, 1.3], ground, [0.16, 0.15, 0.13, 1.0]));
            contact(&mut rw, [3.0, 0.0, -9.0], 0.9, 0.85);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_core::overlay::ErrorLog;

    const DT: f64 = 1000.0 / 60.0;

    fn f32_of(app: &evani_core::App, n: &str) -> f32 {
        let reg = app.world.res::<ParamRegistry>();
        match reg.id(n).and_then(|id| reg.value(id)) {
            Some(ParamValue::F32(x)) => x,
            other => panic!("{n}: {other:?}"),
        }
    }

    fn open_proc(app: &mut evani_core::App) {
        app.world.res_mut::<Viewer>().push(Command::Scene(PROC_SCENE.into()));
        app.run_headless(2, DT);
    }

    /// A library asset opened is placed in the procedural module's scene;
    /// switched off it is removed (the scene empties), on it is back; the
    /// storm sea hands the spectral sea over and off takes it away.
    #[test]
    fn the_switch_removes_and_restores_an_asset() {
        use evani_procedural_lib::base::ProcScene;
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        open_proc(&mut app);
        let placed = |app: &evani_core::App| app.world.res::<ProcScene>().placements.iter().map(|p| p.feature.clone()).collect::<Vec<_>>();
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("terrain-rolling-hills".into()));
        app.run_headless(2, DT);
        assert_eq!(placed(&app), ["terrain-rolling-hills"]);
        // Its staged camera fits its size (the 24 m square), not the shared
        // material's default, and no deck lift (that is the storm sea's).
        app.run_headless(3, DT);
        let o = app.world.res::<Viewer>().orbit;
        assert!(o.dist > 20.0 && o.target[1] < 1.5, "{o:?}");
        // Review round 3: its size goes to the view's fit (a half window's
        // open part frames it), and it is lit from the camera's side.
        let fit = app.world.res::<crate::scenes::fit::SubjectFit>().0;
        assert!(fit.is_some_and(|(h, w)| w > 10.0 && h > 1.0 && h < w), "{fit:?}");
        // Round 8: the library's sun from the right (the references).
        assert_eq!(f32_of(&app, "sky.sun_azimuth"), 110.0);
        assert!(f32_of(&app, "sky.sun_elevation") > 20.0);
        app.world.res_mut::<ProcState>().push(ProcCommand::Toggle("terrain-rolling-hills".into()));
        app.run_headless(2, DT);
        assert!(placed(&app).is_empty(), "off: removed");
        assert!(!is_on(&app.world.res::<ParamRegistry>(), Some(&"procedural.terrain.rolling_hills.on".to_string())));
        app.world.res_mut::<ProcState>().push(ProcCommand::Toggle("terrain-rolling-hills".into()));
        app.run_headless(2, DT);
        assert_eq!(placed(&app), ["terrain-rolling-hills"], "on: back");
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("storm-sea".into()));
        app.run_headless(2, DT);
        assert!(placed(&app).is_empty(), "another item takes the asset away");
        assert_eq!(app.world.res::<crate::scenes::fit::SubjectFit>().0, None, "and its fit");
        assert_eq!(app.world.res::<ProcState>().open_preset(), Some("storm"));
        assert!(app.world.res::<SeaSurfaces>().get().is_some(), "the storm sea is the spectral sea");
        app.world.res_mut::<ProcState>().push(ProcCommand::Toggle("storm-sea".into()));
        app.run_headless(2, DT);
        assert!(app.world.res::<SeaSurfaces>().get().is_none(), "off: the sea is removed");
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("grass-meadow".into()));
        app.run_headless(2, DT);
        let g = app.world.res::<evani_procedural_lib::evani_grass::Grass>().patches();
        assert_eq!(g.len(), 1, "the meadow is one evani-grass patch");
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>().entries().map(|e| e.message.clone()).collect::<Vec<_>>());
    }

    /// The shoreline brings its own water: opened, the nature pass gets one
    /// water rectangle over its plot at its water level and the coastal
    /// look goes on; another asset opened takes both away and puts the water
    /// look back as it was.
    #[test]
    #[cfg(feature = "procedural-water")]
    fn the_shoreline_hands_its_water_and_takes_it_back() {
        use evani_material_nature::Kind;
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        open_proc(&mut app);
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("terrain-rolling-hills".into()));
        app.run_headless(2, DT);
        let before = f32_of(&app, "material.nature.water.clarity");
        assert_eq!(f32_of(&app, "material.nature.water.coast_lace"), 0.0);
        app.world.res_mut::<ProcState>().push(ProcCommand::Open(SHORE.into()));
        app.run_headless(2, DT);
        let list = app.world.res::<NatureSurfaces>().list();
        assert_eq!(list.len(), 1, "{list:?}");
        assert_eq!(list[0].kind, Kind::WaterRect);
        assert!(list[0].centre[1] > 0.5 && list[0].half[0] > 5.0, "{:?}", list[0]);
        assert!(f32_of(&app, "material.nature.water.coast_lace") > 0.0, "the coastal look is on");
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("terrain-rolling-hills".into()));
        app.run_headless(2, DT);
        assert!(app.world.res::<NatureSurfaces>().list().is_empty(), "its water leaves with it");
        assert_eq!(f32_of(&app, "material.nature.water.coast_lace"), 0.0, "the look is taken off");
        assert_eq!(f32_of(&app, "material.nature.water.clarity"), before);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>().entries().map(|e| e.message.clone()).collect::<Vec<_>>());
    }

    /// Every name the library lists is a registered live param, and every
    /// headline slider is a number or a colour the page can show.
    #[test]
    fn every_slider_is_a_live_param() {
        let mut app = crate::build_app();
        app.run_headless(1, DT);
        let st = app.world.res::<ProcState>().clone();
        let reg = app.world.res::<ParamRegistry>();
        assert!(st.lib.unknown(&reg).is_empty(), "{:?}", st.lib.unknown(&reg));
        for it in st.lib.ready() {
            for n in &it.headline {
                let def = reg.id(n).and_then(|id| reg.def(id)).unwrap();
                assert!(matches!(def.default, ParamValue::F32(_) | ParamValue::Color(_)), "{n}: {:?}", def.ty);
            }
        }
    }

    /// Opening the section shows the ocean (the spectral sea) with its first
    /// preset, the sea handed to its pass, the dome's sun as the light;
    /// leaving puts the studio's sky back and takes the sea away.
    #[test]
    fn entering_and_leaving_keeps_the_studio_values() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        let before = f32_of(&app, "sky.cloud_coverage");
        open_proc(&mut app);
        let st = app.world.res::<ProcState>().clone();
        assert!(st.on);
        assert_eq!((st.item.as_str(), st.open_preset()), ("ocean", Some("calm")));
        assert!((f32_of(&app, "material.nature.water.wind_speed") - 5.0).abs() < 1e-6);
        assert!(app.world.res::<SeaSurfaces>().get().is_some(), "the ocean is the spectral sea");
        assert!(app.world.res::<NatureSurfaces>().list().is_empty(), "no older water under it");
        let reg = app.world.res::<ParamRegistry>();
        assert_eq!(reg.id("sky.follow_light").and_then(|id| reg.value(id)), Some(ParamValue::Bool(false)));
        let want = evani_material_proc::sky::sun_direction(24.0, -125.0);
        match reg.id("render.light.direction").and_then(|id| reg.value(id)) {
            Some(ParamValue::Vec3(d)) => assert!(d.iter().zip(want).all(|(a, b)| (a - b).abs() < 1e-4), "{d:?}"),
            other => panic!("{other:?}"),
        }
        drop(reg);
        // A stormy sky, then back to the studio: the studio's sky returns.
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("storm".into()));
        app.run_headless(1, DT);
        assert!((f32_of(&app, "sky.cloud_coverage") - 0.85).abs() < 1e-6);
        app.world.res_mut::<Viewer>().push(Command::Scene("studio".into()));
        app.run_headless(2, DT);
        assert!(!app.world.res::<ProcState>().on);
        assert!((f32_of(&app, "sky.cloud_coverage") - before).abs() < 1e-6, "the studio's sky is back");
        assert!(app.world.res::<NatureSurfaces>().is_empty(), "the water left with the section");
        assert!(app.world.res::<SeaSurfaces>().get().is_none(), "the sea left with the section");
        // Back again: the storm is still the ocean's.
        open_proc(&mut app);
        assert_eq!(app.world.res::<ProcState>().open_preset(), Some("storm"));
        assert!((f32_of(&app, "material.nature.water.wind_speed") - 24.0).abs() < 1e-6);
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>().entries().map(|e| e.message.clone()).collect::<Vec<_>>());
    }

    /// A preset applied, edited, applied again gives the same values; after
    /// another preset too; and each item keeps its own values.
    #[test]
    fn presets_round_trip() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        open_proc(&mut app);
        let lib = app.world.res::<ProcState>().lib.clone();
        // Every staged item, and two library assets (they share one staging).
        let picked = lib.ready().filter(|i| i.stage_as != "library").chain(lib.ready().filter(|i| i.stage_as == "library").take(2));
        for it in picked {
            app.world.res_mut::<ProcState>().push(ProcCommand::Open(it.id.clone()));
            app.run_headless(2, DT);
            assert_eq!(app.world.res::<ProcState>().item, it.id);
            assert_eq!(app.world.res::<Viewer>().material, pd::viewer_material(it), "the camera and material follow");
            let names = it.params();
            for p in &it.presets {
                app.world.res_mut::<ProcState>().push(ProcCommand::Preset(p.id.clone()));
                app.run_headless(1, DT);
                let first = pd::read(&app.world.res::<ParamRegistry>(), &names);
                let want = pd::baseline(&app.world.res::<ParamRegistry>(), it, Some(p));
                assert_eq!(first, want, "{}/{}", it.id, p.id);
                for (n, v) in &p.params {
                    assert!(first.iter().any(|(k, x)| k == n && Some(*x) == app.world.res::<ParamRegistry>().id(n).and_then(|id| app.world.res::<ParamRegistry>().def(id).and_then(|d| evani_kit_material_view::library::value_for(d, v)))), "{n}");
                }
                // An edit, another preset, this one again: the same values.
                let n = &it.headline[0];
                // (A library asset has one preset, its defaults: itself again.)
                let other = it.presets.iter().find(|q| q.id != p.id).unwrap_or(p);
                if let Some(ParamValue::F32(x)) = first.iter().find(|(k, _)| k == n).map(|(_, v)| *v) {
                    app.world.res_mut::<ParamRegistry>().set_by_name(n, ParamValue::F32(x * 0.5 + 0.01)).unwrap();
                }
                for q in [&other.id, &p.id] {
                    app.world.res_mut::<ProcState>().push(ProcCommand::Preset(q.clone()));
                    app.run_headless(1, DT);
                }
                assert_eq!(pd::read(&app.world.res::<ParamRegistry>(), &names), first, "{}/{} again", it.id, p.id);
                // An edit, then Reset: back to the preset.
                if let Some(ParamValue::F32(x)) = first.iter().find(|(k, _)| k == n).map(|(_, v)| *v) {
                    app.world.res_mut::<ParamRegistry>().set_by_name(n, ParamValue::F32(x * 0.5 + 0.01)).unwrap();
                }
                app.world.res_mut::<ProcState>().push(ProcCommand::Reset(n.clone()));
                app.run_headless(1, DT);
                assert_eq!(pd::read(&app.world.res::<ParamRegistry>(), &names), first, "{}/{} reset", it.id, p.id);
                // Several edits, then Reset all: back to the preset.
                for h in &it.headline {
                    if let Some(ParamValue::F32(x)) = first.iter().find(|(k, _)| k == h).map(|(_, v)| *v) {
                        app.world.res_mut::<ParamRegistry>().set_by_name(h, ParamValue::F32(x * 0.7 + 0.02)).unwrap();
                    }
                }
                app.world.res_mut::<ProcState>().push(ProcCommand::ResetAll);
                app.run_headless(1, DT);
                assert_eq!(pd::read(&app.world.res::<ParamRegistry>(), &names), first, "{}/{} reset all", it.id, p.id);
            }
        }
        // Each item keeps its own: the ocean's last preset is still on.
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("ocean".into()));
        app.run_headless(2, DT);
        let beach = lib.item("ocean").unwrap().preset("beach").unwrap().clone();
        assert!((f32_of(&app, "material.nature.water.beach_slope") - 0.05).abs() < 1e-5, "{beach:?}");
    }

    /// Each item stages its own scene: what the nature pass gets and that
    /// something is drawn and grounded.
    #[test]
    fn each_item_stages_its_scene() {
        thread_local! { static SEEN: std::cell::RefCell<(usize, usize)> = const { std::cell::RefCell::new((0, 0)) }; }
        fn grab(w: &mut World) {
            if let Some(rw) = w.try_res::<RenderWorld>() {
                SEEN.with_borrow_mut(|s| *s = (rw.meshes.len(), rw.blobs.len()));
            }
        }
        let mut app = crate::build_app().with_system(SystemDesc::new("test.proc.seen", Stage::Frame, grab).order(i32::MAX));
        app.run_headless(2, DT);
        open_proc(&mut app);
        for (id, kinds) in [("ocean", vec![]), ("water", vec![Kind::WaterRect]), ("glass", vec![Kind::GlassBottle, Kind::GlassBottle, Kind::GlassPane, Kind::GlassPane]), ("sky", vec![])] {
            app.world.res_mut::<ProcState>().push(ProcCommand::Open(id.into()));
            app.run_headless(2, DT);
            let got: Vec<Kind> = app.world.res::<NatureSurfaces>().list().iter().map(|s| s.kind).collect();
            assert_eq!(got, kinds, "{id}");
            // The ocean is the spectral sea's, and only the ocean.
            assert_eq!(app.world.res::<SeaSurfaces>().get().is_some(), id == "ocean", "{id}: the spectral sea");
            let (meshes, blobs) = SEEN.with_borrow(|s| *s);
            if id == "ocean" {
                // The open sea shows water only: no test rocks or balls in it
                // (storm-swell round 6), so nothing to ground.
                assert!(meshes >= 1 && blobs == 0, "{id}: {meshes} meshes, {blobs} blobs");
            } else {
                assert!(meshes > 1 && blobs > 0, "{id}: {meshes} meshes, {blobs} blobs");
            }
        }
        // The river preset narrows the water to a channel.
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("water".into()));
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("river".into()));
        app.run_headless(2, DT);
        let half = app.world.res::<NatureSurfaces>().list()[0].half;
        assert!((half[2] - RIVER.1).abs() < 1e-6 && half[0] > 100.0, "{half:?}");
    }

    /// Preset isolation with the spectral sea: every ocean preset hands the
    /// sea pass a Sea (no water rectangle to the nature pass); after the
    /// storm, the beach or the tropical shallows, the lake, the river and the
    /// pond get their own water back, the sea gone, and the lake's own
    /// settings (no flow, no whitecaps, no tropical glow), and leaving the
    /// section clears the sea.
    #[test]
    fn the_spectral_sea_stays_with_its_presets() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        open_proc(&mut app);
        let sea_level = |app: &evani_core::App| app.world.res::<SeaSurfaces>().get();
        let lake = app.world.res::<ProcState>().lib.item("water").unwrap().preset("lake").unwrap().clone();
        let lake_val = |n: &str| lake.params.iter().find(|(k, _)| k == &format!("material.nature.water.{n}")).and_then(|(_, v)| v.as_f64()).unwrap_or(0.0) as f32;
        let ocean = app.world.res::<ProcState>().lib.item("ocean").unwrap().clone();
        for p in &ocean.presets {
            let p = p.id.as_str();
            app.world.res_mut::<ProcState>().push(ProcCommand::Open("ocean".into()));
            app.world.res_mut::<ProcState>().push(ProcCommand::Preset(p.into()));
            app.run_headless(2, DT);
            assert!(sea_level(&app).is_some(), "{p}: the sea is on");
            assert!(app.world.res::<NatureSurfaces>().list().is_empty(), "{p}: no water rectangle");
            for preset in ["lake", "river", "pond"] {
                app.world.res_mut::<ProcState>().push(ProcCommand::Open("water".into()));
                app.world.res_mut::<ProcState>().push(ProcCommand::Preset(preset.into()));
                app.run_headless(2, DT);
                assert!(sea_level(&app).is_none(), "{p} then {preset}: the sea is gone");
                assert_eq!(app.world.res::<NatureSurfaces>().list()[0].kind, Kind::WaterRect, "{preset}");
                // The lake has exactly its own settings after any sea.
                if preset == "lake" {
                    for n in ["swell_direction", "swell", "choppiness", "flow_speed", "sss_strength", "whitecap_amount", "foam", "wave_strength"] {
                        assert_eq!(f32_of(&app, &format!("material.nature.water.{n}")), lake_val(n), "{p} then lake: {n}");
                    }
                    assert_eq!(f32_of(&app, "material.nature.water.flow_speed"), 0.0, "the lake stays still");
                    assert!(f32_of(&app, "material.nature.water.sss_strength") < 0.5, "only a faint glow on the lake");
                }
            }
        }
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("ocean".into()));
        app.run_headless(2, DT);
        // A sea preset sets every sea setting it lists; another resets them.
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("beach".into()));
        app.run_headless(1, DT);
        assert!(f32_of(&app, "material.nature.water.beach_slope") > 0.0);
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("open".into()));
        app.run_headless(1, DT);
        assert_eq!(f32_of(&app, "material.nature.water.beach_slope"), 0.0, "the open sea has no beach");
        // The quality switch is kept across presets (a viewing choice).
        app.world.res_mut::<ParamRegistry>().set_by_name("material.nature.water.sea_quality", ParamValue::F32(3.0)).unwrap();
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("rough".into()));
        app.run_headless(1, DT);
        assert_eq!(f32_of(&app, "material.nature.water.sea_quality"), 3.0);
        // Leaving the section takes the sea with it.
        app.world.res_mut::<Viewer>().push(Command::Scene("studio".into()));
        app.run_headless(2, DT);
        assert!(sea_level(&app).is_none(), "the sea left with the section");
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0, "{:?}", app.world.res::<ErrorLog>().entries().map(|e| e.message.clone()).collect::<Vec<_>>());
    }

    /// The storm presets look low across the sea, as from a deck, so the
    /// crests stand against the sky; a preset without a camera of its own
    /// puts the ocean's own back; the user's orbiting is kept until the
    /// preset changes; leaving the section and coming back puts the storm's
    /// camera on again; the storm settings go with the calm sea's preset.
    #[test]
    fn storm_presets_bring_their_low_camera() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        open_proc(&mut app);
        let home = app.world.res::<Viewer>().home;
        let ocean = app.world.res::<ProcState>().lib.item("ocean").unwrap().clone();
        let cam = |p: &str| ocean.preset(p).and_then(|p| p.stage).and_then(|s| s.camera);
        let storm = cam("storm").expect("the storm sea has its own camera");
        assert!((8.0..=10.0).contains(&storm.eye_height()) && storm.el <= 3.0, "a deck's height, looking low: {storm:?}");
        assert!(cam("rough").is_some() && cam("calm").is_none() && cam("beach").is_none());
        // The camera as staged (the deck's lift over the swell taken off).
        let at = |app: &evani_core::App| {
            let v = app.world.res::<Viewer>();
            let lift = app.world.res::<ProcState>().lift;
            (v.orbit.el, v.orbit.dist, [v.orbit.target[0], v.orbit.target[1] - lift, v.orbit.target[2]])
        };
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("storm".into()));
        app.run_headless(1, DT);
        assert_eq!(at(&app), (storm.el, storm.dist, storm.target));
        assert_eq!(app.world.res::<Viewer>().home.el, storm.el, "Reset goes back to the storm's camera");
        assert!(f32_of(&app, "material.nature.water.mid_waves") > 0.5 && f32_of(&app, "material.nature.water.spindrift") > 0.5);
        // The deck rides the swell: over a minute of storm, at every quality
        // level, the eye stays clear of the sea under it.
        let mut lifted = 0;
        for q in [3.0, 2.0, 1.0] {
            app.world.res_mut::<ParamRegistry>().set_by_name("material.nature.water.sea_quality", ParamValue::F32(q)).unwrap();
            for _ in 0..60 {
                app.run_headless(15, DT);
                let eye = app.world.res::<Viewer>().orbit.pose(0.0).eye;
                let h = app.world.res::<SeaSurfaces>().height_at(&app.world.res::<ParamRegistry>(), [eye[0], eye[2]], 60.0).unwrap();
                assert!(eye[1] - h > DECK_CLEAR - 0.8, "quality {q}: eye {} over a sea at {h}", eye[1]);
                lifted += usize::from(app.world.res::<ProcState>().lift > 0.0);
            }
        }
        eprintln!("deck camera lifted on {lifted} of 180 samples");
        app.world.res_mut::<ParamRegistry>().set_by_name("material.nature.water.sea_quality", ParamValue::F32(-1.0)).unwrap();
        app.run_headless(1, DT);
        // The user's own orbit stays while the preset does.
        app.world.res_mut::<Viewer>().orbit.el = storm.el + 4.0;
        app.run_headless(2, DT);
        assert_eq!(app.world.res::<Viewer>().orbit.el, storm.el + 4.0);
        // The calm sea: the ocean's own camera and no storm settings.
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("calm".into()));
        app.run_headless(1, DT);
        assert_eq!(at(&app), (home.el, home.dist, home.target));
        for n in ["mid_waves", "foam_streaks", "spindrift"] {
            assert_eq!(f32_of(&app, &format!("material.nature.water.{n}")), 0.0, "calm: {n}");
        }
        // Storm, out of the section and back: the storm's camera again.
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("storm".into()));
        app.run_headless(1, DT);
        app.world.res_mut::<Viewer>().push(Command::Scene("studio".into()));
        app.run_headless(2, DT);
        open_proc(&mut app);
        app.run_headless(1, DT);
        assert_eq!(app.world.res::<ProcState>().open_preset(), Some("storm"));
        assert_eq!(at(&app), (storm.el, storm.dist, storm.target));
        // Another item and back: the glass has its own camera, the ocean
        // gets the storm's again.
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("glass".into()));
        app.run_headless(2, DT);
        assert_ne!(at(&app).0, storm.el);
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("ocean".into()));
        app.run_headless(3, DT);
        assert_eq!(at(&app), (storm.el, storm.dist, storm.target));
        assert_eq!(app.world.res::<ErrorLog>().error_count(), 0);
    }

    /// A quality preview stays with its item: the lake's Low doesn't
    /// flatten the glass, and leaving the section puts the studio's quality
    /// back (the sea's and the nature pass's).
    #[test]
    fn quality_previews_stay_with_their_item() {
        let mut app = crate::build_app();
        app.run_headless(2, DT);
        let nature_q = "material.nature.quality";
        let sea_q = "material.nature.water.sea_quality";
        let (n0, s0) = (f32_of(&app, nature_q), f32_of(&app, sea_q));
        open_proc(&mut app);
        app.world.res_mut::<ParamRegistry>().set_by_name(sea_q, ParamValue::F32(0.0)).unwrap();
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("water".into()));
        app.run_headless(2, DT);
        assert_eq!(f32_of(&app, sea_q), s0, "the ocean's Low left with the ocean");
        app.world.res_mut::<ParamRegistry>().set_by_name(nature_q, ParamValue::F32(1.0)).unwrap();
        // A preset keeps the switch (a viewing choice).
        app.world.res_mut::<ProcState>().push(ProcCommand::Preset("river".into()));
        app.run_headless(1, DT);
        assert_eq!(f32_of(&app, nature_q), 1.0);
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("glass".into()));
        app.run_headless(2, DT);
        assert_eq!(f32_of(&app, nature_q), n0, "the lake's Low left with the lake");
        app.world.res_mut::<ProcState>().push(ProcCommand::Open("water".into()));
        app.run_headless(2, DT);
        app.world.res_mut::<ParamRegistry>().set_by_name(nature_q, ParamValue::F32(2.0)).unwrap();
        app.world.res_mut::<Viewer>().push(Command::Scene("studio".into()));
        app.run_headless(2, DT);
        assert_eq!((f32_of(&app, nature_q), f32_of(&app, sea_q)), (n0, s0), "the studio's quality is back");
    }
}

