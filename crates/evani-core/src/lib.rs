//! Evani.Dev core: the part of the engine every game ships.
//!
//! * [`app`]: the [`App`] builder and the per-frame loop (fixed-step sim at
//!   60 Hz, interpolated frame stage, modules, audio, overlay).
//! * [`ecs`]: sparse-set ECS with multi-component queries and an ordered,
//!   deterministic schedule.
//! * [`time`], [`input`], [`assets`], [`scene`], [`events`], [`rng`], [`save`],
//!   [`audio`], [`overlay`], [`particles`] (the queue kits emit into): the
//!   runtime services.
//! * [`params`], [`curve`], [`expr`], [`formulas`], [`slots`], [`tiers`],
//!   [`effects`], [`quality`] (adaptive quality and tier warm-up), [`camera`],
//!   [`capture`] (renderer capture/bench interface): the tunable data that live adaptation (evani-live, dev only)
//!   edits and that `bake.export` writes back. They live here because shipped
//!   games read the baked values too.
//! * [`plugin`]: `KitPlugin`, `LookPlugin`, `EngineModule`.
//! * [`play`]: the play mode (play, or free roam from `?mode=free` /
//!   `#mode=free`), the fail events kits report, and the free-roam test
//!   helper every playground's kit uses (docs/PLAYGROUNDS.md).
//! * [`version`]: the engine version and commit, and the version of every
//!   Evani.Dev part compiled into the game (overlay, `session.status`).
//! * `web` (wasm only): the browser runner (requestAnimationFrame, DOM input,
//!   Gamepad API, Web Audio, localStorage, fetch, overlay element).

pub mod app;
pub mod assets;
pub mod audio;
pub mod camera;
pub mod capture;
pub mod curve;
pub mod ecs;
pub mod effects;
pub mod events;
pub mod expr;
#[cfg(feature = "focus")]
pub mod focus;
pub mod formulas;
#[cfg(feature = "hotpatch")]
pub mod hotpatch;
pub mod input; pub mod look_fx;
pub mod num;
pub mod overlay;
pub mod params;
pub mod particles;
pub mod play;
pub mod plugin; #[cfg(feature = "pointer-lock")] pub mod pointer_lock;
pub mod quality;
pub mod rng;
pub mod save;
pub mod scene;
pub mod slots;
pub mod tiers;
pub mod time;
#[cfg(feature = "touch")]
pub mod touch;
pub mod version;
#[cfg(all(target_arch = "wasm32", not(evani_headless)))]
pub mod web;

pub use app::App;

/// The names most games and plugins need.
pub mod prelude {
    pub use crate::app::App;
    pub use crate::audio::{AudioMixer, Bus, SoundId};
    pub use crate::ecs::{Commands, Entity, Query, Schedule, Stage, SystemDesc, World};
    pub use crate::effects::{BudgetStatus, EffectInfo, EffectsStatus, StyleClasses};
    pub use crate::events::{EngineEvent, EventQueue};
    pub use crate::formulas::{FormulaJson, Formulas};
    pub use crate::input::{ActionDef, ActionId, Input};
    pub use crate::overlay::{ErrorLog, Overlay, Platform, RenderStats};
    pub use crate::play::{PlayMode, PlayOutcomes};
    pub use crate::particles::{Emit, ParticleQueue, Space as ParticleSpace, Spread};
    pub use crate::params::{
        BoolParam, EnumParam, F32Param, I32Param, ParamDef, ParamRegistry, Vec3Param,
    };
    pub use crate::plugin::{
        BuildInfo, EngineModule, KitPlugin, LookContext, LookData, LookPlugin, MaterialDesc,
        PluginContext, PluginError,
    };
    pub use crate::rng::Pcg32;
    pub use crate::save::SaveData;
    pub use crate::scene::{Scene, SceneTransition};
    pub use crate::slots::ShaderSlots;
    pub use crate::tiers::{TierPreset, Tiers};
    pub use crate::time::Time;
}
