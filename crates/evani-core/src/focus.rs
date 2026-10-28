//! Asset focus (core feature `focus`, which only evani-live turns on, so a
//! shipped game carries none of it): show one of the game's assets on its
//! own, on a calm turntable camera, and switch back to the full game without
//! reloading the page (`focus.*` in docs/LIVE_API.md 6.28).
//!
//! * Kits list their assets in [`KitPlugin::focus_assets`]
//!   (crate::plugin::KitPlugin), under their own `live` feature (which turns
//!   on `evani-core/focus`). [`crate::App::add_kit`] collects them into the
//!   [`FocusAssets`] resource, each tagged with its kit.
//! * evani-live inserts [`FocusState`] and drives it (`focus.set`,
//!   `focus.clear`, the page's `?focus=<id>`).
//! * While an asset is focused, the kit that owns it draws only that asset
//!   (plus its static backdrop when [`FocusState::backdrop`] is on), calls
//!   [`FocusState::frame`] with where the asset is, and puts the returned
//!   pose on the camera. Core computes the turntable; the kit never does.
//! * With [`FocusState::paused`] on, the core loop runs no fixed ticks (the
//!   game is frozen) but keeps render time running, so the asset's own
//!   animation (a spin, a bob, a colour cycle) still plays.

use crate::camera::CameraPose;

/// One asset a kit can show on its own.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusAsset {
    /// Unique id across the game, e.g. `item_box` (`focus.set { asset }`).
    pub id: String,
    /// What people call it, e.g. `Item box`.
    pub label: String,
    /// Free-form kind: `prop`, `vehicle`, `item`, `character`, `scenery`,
    /// `track`...
    pub kind: String,
    /// Params that tune this asset: exact names, or prefixes ending in `.`
    /// or `_` (e.g. `kit.kart_racer.item_box_`). `schema.get` lists them
    /// first while the asset is focused.
    pub params: Vec<String>,
    /// Rough bounding radius in metres, used to frame it until the kit
    /// reports the live one.
    pub radius: f32,
    /// The kit that owns it (`kart_racer`), filled in by `add_kit`.
    pub owner: String,
    /// The source file that defines it, relative to the engine folder with
    /// forward slashes (`kits/evani-kit-kart-racer/src/draw.rs`); empty when
    /// the kit doesn't say. Evani.App's asset view shows it and sends it with
    /// feedback on the asset.
    pub path: String,
}

impl FocusAsset {
    pub fn new(id: &str, label: &str) -> Self {
        Self {
            id: id.to_string(),
            label: label.to_string(),
            kind: "prop".to_string(),
            params: Vec::new(),
            radius: 1.0,
            owner: String::new(),
            path: String::new(),
        }
    }

    /// The source file that defines the asset (see [`FocusAsset::path`]).
    pub fn path(mut self, path: &str) -> Self {
        self.path = path.replace('\\', "/");
        self
    }

    pub fn kind(mut self, kind: &str) -> Self {
        self.kind = kind.to_string();
        self
    }

    /// Add tuning params: exact names or prefixes (see [`FocusAsset::params`]).
    pub fn params<'a>(mut self, names: impl IntoIterator<Item = &'a str>) -> Self {
        self.params.extend(names.into_iter().map(str::to_string));
        self
    }

    pub fn radius(mut self, r: f32) -> Self {
        self.radius = r.max(0.01);
        self
    }

    /// Does `param` tune this asset?
    pub fn tunes(&self, param: &str) -> bool {
        self.params.iter().any(|p| {
            if p.ends_with('.') || p.ends_with('_') {
                param.starts_with(p.as_str())
            } else {
                param == p
            }
        })
    }
}

/// Every focusable asset, in registration order. World resource, inserted by
/// `add_kit` (feature `focus`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct FocusAssets {
    assets: Vec<FocusAsset>,
    /// The kit currently registering (set by `add_kit`).
    owner: String,
    /// Duplicate ids, reported as errors by `add_kit`.
    pub(crate) duplicates: Vec<String>,
}

impl FocusAssets {
    /// Register an asset (from [`crate::plugin::KitPlugin::focus_assets`]).
    /// A second asset with an id already taken is dropped and reported.
    pub fn add(&mut self, mut asset: FocusAsset) {
        if self.get(&asset.id).is_some() {
            self.duplicates.push(asset.id);
            return;
        }
        asset.owner.clone_from(&self.owner);
        self.assets.push(asset);
    }

    pub(crate) fn set_owner(&mut self, owner: &str) {
        self.owner = owner.to_string();
    }

    pub fn get(&self, id: &str) -> Option<&FocusAsset> {
        self.assets.iter().find(|a| a.id == id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &FocusAsset> {
        self.assets.iter()
    }

    pub fn len(&self) -> usize {
        self.assets.len()
    }

    pub fn is_empty(&self) -> bool {
        self.assets.is_empty()
    }
}

/// Where the focused asset is this frame, as its kit reported it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FocusFrame {
    /// Centre of the asset (world metres).
    pub center: [f32; 3],
    /// Bounding radius (m).
    pub radius: f32,
    /// The asset's front, as a yaw in degrees (0 = facing +Z): yaw 0 on the
    /// turntable looks at its front.
    pub front_yaw_deg: f32,
}

/// Turntable defaults.
pub const DEFAULT_PITCH_DEG: f32 = 18.0;
pub const DEFAULT_SPIN_DEG_S: f32 = 12.0;
pub const DEFAULT_FOV_DEG: f32 = 35.0;
/// The asset's bounding sphere fills this share of the view height when the
/// distance is automatic.
pub const AUTO_FILL: f32 = 0.55;

/// The focus mode. World resource, inserted by evani-live.
#[derive(Debug, Clone, PartialEq)]
pub struct FocusState {
    /// The focused asset's id, or `None` for the full game.
    pub active: Option<String>,
    /// Orbit angle around the asset in degrees, from its front, growing
    /// counter-clockwise seen from above. The turntable adds to it.
    pub yaw_deg: f32,
    /// Camera elevation in degrees (-89..89).
    pub pitch_deg: f32,
    /// Camera distance from the centre (m); `None` = fit the asset.
    pub distance: Option<f32>,
    /// Turntable speed in degrees per second (0 = still camera).
    pub spin_deg_s: f32,
    /// Vertical field of view in degrees.
    pub fov_deg: f32,
    /// Freeze the game (no fixed ticks) while focused; render time and the
    /// asset's own animation keep running.
    pub paused: bool,
    /// Keep the kit's static backdrop (ground, track, scenery) behind the
    /// asset; off shows the asset alone over the sky.
    pub backdrop: bool,
    /// Which instance when the asset exists several times (e.g. item box 3);
    /// `None` = the kit's choice.
    pub instance: Option<u32>,
    /// What the kit reported last frame (`None` until it draws the asset).
    pub last: Option<FocusFrame>,
    /// Frames the kit drew the asset since it was focused.
    pub frames_drawn: u64,
}

impl Default for FocusState {
    fn default() -> Self {
        Self {
            active: None,
            yaw_deg: 0.0,
            pitch_deg: DEFAULT_PITCH_DEG,
            distance: None,
            spin_deg_s: DEFAULT_SPIN_DEG_S,
            fov_deg: DEFAULT_FOV_DEG,
            paused: true,
            backdrop: true,
            instance: None,
            last: None,
            frames_drawn: 0,
        }
    }
}

impl FocusState {
    /// Focus `id` with the default turntable (the caller checked the id).
    pub fn focus(&mut self, id: &str) {
        *self = Self {
            active: Some(id.to_string()),
            ..Self::default()
        };
    }

    /// Back to the full game.
    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// The focused asset id when kit `owner` owns it (kits call this from
    /// their draw system).
    pub fn active_for<'a>(&'a self, assets: &FocusAssets, owner: &str) -> Option<&'a str> {
        let id = self.active.as_deref()?;
        (assets.get(id)?.owner == owner).then_some(id)
    }

    /// Hold the sim clock this frame?
    pub fn holds_sim(&self) -> bool {
        self.active.is_some() && self.paused
    }

    /// The camera distance for an asset of `radius`.
    pub fn distance_for(&self, radius: f32) -> f32 {
        self.distance.unwrap_or_else(|| {
            let half = (self.fov_deg.clamp(5.0, 150.0) * 0.5).to_radians();
            radius.max(0.01) / (half.sin() * AUTO_FILL)
        })
    }

    /// The turntable pose for an asset at `f`.
    pub fn pose(&self, f: &FocusFrame) -> CameraPose {
        let d = self.distance_for(f.radius);
        let yaw = (f.front_yaw_deg + self.yaw_deg).to_radians();
        let pitch = self.pitch_deg.clamp(-89.0, 89.0).to_radians();
        let c = f.center;
        CameraPose {
            eye: [
                c[0] + d * pitch.cos() * yaw.sin(),
                c[1] + d * pitch.sin(),
                c[2] + d * pitch.cos() * yaw.cos(),
            ],
            target: c,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: self.fov_deg.clamp(5.0, 150.0),
        }
    }

    /// The kit reports where the focused asset is this frame and gets the
    /// camera pose to draw it with.
    pub fn frame(&mut self, f: FocusFrame) -> CameraPose {
        self.last = Some(f);
        self.frames_drawn += 1;
        self.pose(&f)
    }

    /// Advance the turntable by `dt` seconds (the core loop, every frame).
    pub fn advance(&mut self, dt: f32) {
        if self.active.is_some() && self.spin_deg_s != 0.0 {
            self.yaw_deg = (self.yaw_deg + self.spin_deg_s * dt).rem_euclid(360.0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefixes_and_exact_names_pick_params() {
        let a = FocusAsset::new("item_box", "Item box").params(["kit.k.item_box_", "kit.k.ribbon"]);
        assert!(a.tunes("kit.k.item_box_size"));
        assert!(a.tunes("kit.k.ribbon"));
        assert!(!a.tunes("kit.k.ribbon_width"));
        assert!(!a.tunes("kit.k.max_speed"));
    }

    #[test]
    fn duplicate_ids_are_dropped_and_owner_is_set() {
        let mut list = FocusAssets::default();
        list.set_owner("kart_racer");
        list.add(FocusAsset::new("kart", "Kart"));
        list.add(FocusAsset::new("kart", "Other kart"));
        assert_eq!(list.len(), 1);
        assert_eq!(list.get("kart").unwrap().owner, "kart_racer");
        assert_eq!(list.duplicates, vec!["kart".to_string()]);
    }

    #[test]
    fn turntable_frames_the_asset_and_spins() {
        let mut s = FocusState::default();
        s.focus("box");
        s.spin_deg_s = 90.0;
        let f = FocusFrame { center: [1.0, 2.0, 3.0], radius: 1.0, front_yaw_deg: 0.0 };
        let p = s.frame(f);
        assert_eq!(p.target, [1.0, 2.0, 3.0]);
        let d = |p: &CameraPose| (0..3).map(|i| (p.eye[i] - p.target[i]).powi(2)).sum::<f32>().sqrt();
        let auto = d(&p);
        assert!((auto - s.distance_for(1.0)).abs() < 1e-4);
        // The sphere fills AUTO_FILL of the view height.
        let half = (DEFAULT_FOV_DEG * 0.5).to_radians();
        assert!(((1.0 / auto) / half.sin() - AUTO_FILL).abs() < 1e-4);
        s.advance(1.0);
        assert!((s.yaw_deg - 90.0).abs() < 1e-4);
        let q = s.pose(&f);
        assert!((d(&q) - auto).abs() < 1e-4, "same distance after a turn");
        assert!(q.eye[0] > p.eye[0] + 1.0, "a quarter turn moves the eye round");
        // A set distance wins; clear goes back to the full game.
        s.distance = Some(10.0);
        assert!((d(&s.pose(&f)) - 10.0).abs() < 1e-4);
        assert!(s.holds_sim());
        s.clear();
        assert_eq!(s, FocusState::default());
        assert!(!s.holds_sim());
        s.advance(1.0);
        assert_eq!(s.yaw_deg, 0.0, "no turntable when nothing is focused");
    }
}
