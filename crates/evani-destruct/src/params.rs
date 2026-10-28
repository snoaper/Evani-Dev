//! The live `destruct.*` params: one per [`Tuning`] field worth tuning, all
//! owned by `destruct`, defaults from [`Tuning::default`] (Hexagon's values).

use evani_core::params::{ParamDef, ParamError, ParamRegistry};
use evani_core::plugin::PluginContext;

use crate::tuning::Tuning;

macro_rules! destruct_params {
    ($( $field:ident : $kind:ident, $lo:expr, $hi:expr, $unit:expr, $doc:expr; )*) => {
        /// World resource: the handles of every `destruct.*` param.
        #[derive(Debug, Clone, Copy)]
        pub struct DestructParams {
            $( pub $field: destruct_params!(@ty $kind), )*
        }

        /// Every param name, in registration order.
        pub const NAMES: &[&str] = &[$( concat!("destruct.", stringify!($field)) ),*];

        impl DestructParams {
            /// Register them all.
            pub fn register(ctx: &mut PluginContext) -> Result<Self, ParamError> {
                let d = Tuning::default();
                Ok(Self {
                    $( $field: destruct_params!(@def ctx, $kind, concat!("destruct.", stringify!($field)), d.$field, $lo, $hi, $unit, $doc)?, )*
                })
            }

            /// The tuning as the params stand now.
            pub fn read(&self, reg: &ParamRegistry) -> Tuning {
                let mut t = Tuning::default();
                $( destruct_params!(@get t.$field, reg, $kind, self.$field); )*
                t
            }
        }
    };
    (@ty f32) => { evani_core::params::F32Param };
    (@ty i32) => { evani_core::params::I32Param };
    (@def $ctx:ident, f32, $name:expr, $d:expr, $lo:expr, $hi:expr, $unit:expr, $doc:expr) => {
        $ctx.param_f32(ParamDef::f32($name, $d).range($lo, $hi).unit($unit).describe($doc).owner("destruct"))
    };
    (@def $ctx:ident, i32, $name:expr, $d:expr, $lo:expr, $hi:expr, $unit:expr, $doc:expr) => {
        $ctx.param_i32(ParamDef::i32($name, $d as i32).range($lo, $hi).unit($unit).describe($doc).owner("destruct"))
    };
    (@get $t:ident . $field:ident, $reg:ident, f32, $h:expr) => { $t.$field = $reg.f32($h) };
    (@get $t:ident . $field:ident, $reg:ident, i32, $h:expr) => { $t.$field = $reg.i32($h).max(0) as _ };
}

destruct_params! {
    cell_min_solid: f32, 0.0, 1.0, "share", "Share of a cell's 27 samples that must be left for it to carry load.";
    support_min: f32, 0.0, 1.0, "share", "Share of a group's footprint the floor must hold for it to stand.";
    tip_margin: f32, 0.0, 1.0, "m", "How far the centre of mass may sit past the held hull before the group tips.";
    tip_release: f32, 0.05, 1.5, "rad", "Tilt at which a block turning on its edge slides off and falls free.";
    tip_max: f32, 0.0, 1.57, "rad", "Largest tilt a falling block reaches.";
    tip_spin: f32, 0.0, 5.0, "rad/s", "Starting turn speed of falling concrete (half for edge tips and slabs).";
    floor_sliver_min: f32, 0.0, 4.0, "m", "Floor joins thinner than this carry nothing.";
    floor_crumb_max: f32, 0.0, 10.0, "m²", "Floor pieces up to this area are cut away instead of falling.";
    fall_gravity: f32, 1.0, 60.0, "m/s²", "Gravity for falling concrete.";
    fall_max: i32, 1.0, 64.0, "blocks", "Most blocks falling at once; more crumble where they stand.";
    collapse_hole_r: f32, 0.0, 3.0, "m", "Impact crater radius per landed block (times the cube root of its cells).";
    collapse_hole_max: f32, 0.0, 4.0, "m", "Cap on the impact crater radius.";
    collapse_up: f32, 0.0, 2.0, "m", "How far above the touch-down point the impact crater is centred.";
    collapse_debris: i32, 0.0, 40.0, "chunks/cell", "Debris chunks per cell of a landed block (at most 80).";
    debris_per_m: f32, 0.0, 64.0, "chunks/m", "Debris chunks per metre of hole radius.";
    debris_min: i32, 0.0, 64.0, "chunks", "Fewest chunks a hole throws.";
    debris_size_min: f32, 0.005, 0.5, "m", "Smallest gravel.";
    debris_size_max: f32, 0.01, 1.5, "m", "Largest lump.";
    debris_gravity: f32, 0.0, 60.0, "m/s²", "Debris gravity.";
    debris_bounce: f32, 0.0, 1.0, "share", "Share of speed a chunk keeps on a bounce.";
    debris_friction: f32, 0.0, 1.0, "share", "Share of sliding speed and spin kept on every bounce.";
    debris_rest_speed: f32, 0.0, 5.0, "m/s", "Below this speed on a floor a chunk lies still.";
    debris_settle: f32, 0.0, 30.0, "s", "Time at rest before a chunk fades.";
    debris_fade: f32, 0.05, 5.0, "s", "Sink-and-shrink time of a fading chunk.";
    debris_life: f32, 0.5, 60.0, "s", "Longest life of a chunk.";
}
