//! HUD layout from tunables (docs/KIT_PROCESS.md, "Look numbers are
//! tunables"): where the banners, the item slot, the incoming-item warning
//! and the minimap sit, and how big they are.
//!
//! Every value is a live tunable `kit.kart_racer.hud_*` with its default
//! here in code. Sizes and offsets are in HUD units `u` (the page's `--u`,
//! about 1 % of the screen's short side; see [`crate::draw::hud_unit`]).
//!
//! * The page reads them as CSS variables (`--hud-banner-top` and so on,
//!   written on `#ui` by the app when they change, [`HudLayout::style_attr`]);
//!   its stylesheet carries the same defaults as fallbacks
//!   (`var(--hud-banner-top, 17)`), so the page looks the same before the
//!   first frame and in a build without the app's writer.
//! * The minimap is drawn by the kit and reads the `minimap_*` values
//!   directly ([`HudLayout::minimap_origin`]).
//!
//! A change in the Look Lab or via `param.set` moves things on the next
//! frame: no rebuild, no reload.

use evani_core::params::{F32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{PluginContext, PluginError};

macro_rules! layout {
    ($( $field:ident = $def:expr, [$min:expr, $max:expr], $unit:literal, $css:literal, $desc:literal; )*) => {
        /// The HUD layout values for one frame (defaults = the kart HUD as designed).
        #[derive(Debug, Clone, Copy, PartialEq)]
        pub struct HudLayout {
            $(pub $field: f32,)*
        }

        impl Default for HudLayout {
            fn default() -> Self {
                Self { $($field: $def,)* }
            }
        }

        /// Handles of the `kit.kart_racer.hud_*` tunables (a world resource).
        #[derive(Debug, Clone, Copy)]
        pub struct HudLayoutParams {
            $($field: F32Param,)*
        }

        impl HudLayoutParams {
            pub fn register(kit: &mut PluginContext) -> Result<Self, PluginError> {
                Ok(Self {
                    $($field: kit.param_f32(
                        ParamDef::f32(concat!("kit.kart_racer.hud_", stringify!($field)), $def)
                            .range($min, $max)
                            .unit($unit)
                            .describe($desc),
                    )?,)*
                })
            }

            pub fn read(&self, reg: &ParamRegistry) -> HudLayout {
                HudLayout { $($field: reg.f32(self.$field),)* }
            }
        }

        impl HudLayout {
            /// `(full param name, CSS variable or "" when only the kit's drawing uses it, value)`.
            pub fn entries(&self) -> Vec<(&'static str, &'static str, f32)> {
                vec![$((concat!("kit.kart_racer.hud_", stringify!($field)), $css, self.$field),)*]
            }
        }
    };
}

layout! {
    banner_top = 15.0, [0.0, 80.0], "u", "--hud-banner-top", "LAP and FINAL LAP banners: top edge, from the top of the player's view (15 u is just under the item slot, the highest it sits without covering it).";
    banner_size = 7.0, [1.0, 30.0], "u", "--hud-banner-size", "LAP, FINAL LAP and FINISH! banners: letter size.";
    wrong_top = 19.0, [0.0, 80.0], "u", "--hud-wrong-top", "WRONG WAY banner: top edge (under the item slot and its warning).";
    wrong_size = 7.0, [1.0, 30.0], "u", "--hud-wrong-size", "WRONG WAY banner: letter size.";
    finish_top = 17.0, [0.0, 80.0], "u", "--hud-finish-top", "FINISH! banner: top edge.";
    finish_place_top = 27.0, [0.0, 80.0], "u", "--hud-finish-place-top", "The big finishing place under FINISH!: top edge.";
    finish_place_size = 14.0, [2.0, 40.0], "u", "--hud-finish-place-size", "The big finishing place: letter size.";
    finish_short_top = 10.0, [0.0, 80.0], "u", "--hud-finish-short-top", "FINISH! banner on a short screen (a phone held sideways): top edge, higher than on a tall one so the player marker over the kart stays clear of the place.";
    finish_place_short_top = 19.0, [0.0, 80.0], "u", "--hud-finish-place-short-top", "The big finishing place on a short screen: top edge (its foot clears the player marker over the kart).";
    count_short_top = 30.0, [0.0, 80.0], "%", "--hud-count-short-top", "Countdown 3-2-1 on a short screen (a phone held sideways, one player): top edge of its line, in % of the view's height, so the numeral sits under the start lights instead of on them.";
    slot_size = 13.0, [4.0, 30.0], "u", "--hud-slot-size", "Item slot: width (the icon and padding scale with the unit).";
    threat_gap = 1.4, [0.0, 10.0], "u", "--hud-threat-gap", "Incoming-item warning: gap under the item slot.";
    threat_size = 3.2, [1.0, 10.0], "u", "--hud-threat-size", "Incoming-item warning: letter size (never under 12 px).";
    minimap_scale = 1.0, [0.25, 3.0], "x", "", "Minimap size, times its size for the screen (150 px at 1080p).";
    minimap_right = 2.2, [0.0, 40.0], "u", "", "Minimap: gap from the right edge of the player's view.";
    minimap_gap = 0.8, [0.0, 20.0], "u", "", "Minimap: gap under the race timer.";
    minimap_ink = 0.96, [0.0, 1.0], "", "", "Minimap panel: opacity of its dark ink fill. Blended in linear light, so 0.96 reads as about 85 % ink over a bright sky or sign.";
}

/// A number as CSS writes it: up to 3 decimals, no trailing zeros.
fn css_num(v: f32) -> String {
    let r = (f64::from(v) * 1000.0).round() / 1000.0;
    // num::short: the `{}` text of a 3-decimal number without float formatting code.
    evani_core::num::short(r, 3).to_string()
}

impl HudLayout {
    /// The page's CSS variables and their values.
    pub fn css_vars(&self) -> Vec<(&'static str, f32)> {
        self.entries().into_iter().filter(|(_, css, _)| !css.is_empty()).map(|(_, css, v)| (css, v)).collect()
    }

    /// The `style` attribute for `#ui`: `--hud-banner-top:17;--hud-banner-size:7;…`.
    pub fn style_attr(&self) -> String {
        let mut s = String::with_capacity(320);
        for (css, v) in self.css_vars() {
            s.push_str(css);
            s.push(':');
            s.push_str(&css_num(v));
            s.push(';');
        }
        s
    }

    /// Minimap side (px) for a HUD unit, from the screen-size rule
    /// ([`crate::draw::minimap_size`]) times `minimap_scale`.
    pub fn minimap_size(&self, u: f32, split: bool) -> f32 {
        crate::draw::minimap_size(u, split) * self.minimap_scale.max(0.0)
    }

    /// The minimap's top-left corner in a view: `minimap_right` from the
    /// view's right edge, `minimap_gap` under the race timer (1.4 u from the
    /// top, its text 3.2 u tall at a 1.3 line height).
    pub fn minimap_origin(&self, view: [f32; 4], u: f32, size: f32) -> [f32; 2] {
        [view[0] + view[2] - self.minimap_right * u - size, view[1] + (1.4 + 3.2 * 1.3 + self.minimap_gap) * u]
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::draw::{hud_unit, minimap_origin, minimap_size};

    #[test]
    fn defaults_are_the_designed_layout() {
        let l = HudLayout::default();
        assert_eq!((l.banner_top, l.wrong_top, l.finish_place_top, l.slot_size), (15.0, 19.0, 27.0, 13.0));
        // The minimap at the default layout is exactly where it was drawn before.
        for (vmin, split) in [(1080.0, false), (390.0, false), (1080.0, true)] {
            let u = hud_unit(vmin, split);
            let m = l.minimap_size(u, split);
            assert_eq!(m, minimap_size(u, split));
            let view = [0.0, 0.0, 1920.0, 1080.0];
            let (a, b) = (l.minimap_origin(view, u, m), minimap_origin(view, u, m));
            assert!((a[0] - b[0]).abs() < 1e-3 && (a[1] - b[1]).abs() < 1e-3, "{a:?} {b:?}");
        }
    }

    #[test]
    fn the_style_attribute_lists_every_page_value() {
        let l = HudLayout { wrong_top: 30.0, threat_gap: 2.25, ..HudLayout::default() };
        let s = l.style_attr();
        assert!(s.starts_with("--hud-banner-top:15;--hud-banner-size:7;--hud-wrong-top:30;"), "{s}");
        assert!(s.contains("--hud-threat-gap:2.25;--hud-threat-size:3.2;"), "{s}");
        assert!(!s.contains("minimap"), "the minimap is drawn by the kit, not the page");
        assert_eq!(s.matches(':').count(), 13);
        assert_eq!(css_num(1.0 / 3.0), "0.333");
    }

    /// `css_num` writes the same text as the `format!("{r}")` it replaced
    /// (lean-data), over and past the range of every layout value.
    #[test]
    fn css_numbers_match_the_old_float_formatting() {
        let old = |v: f32| {
            let r = (f64::from(v) * 1000.0).round() / 1000.0;
            format!("{r}")
        };
        for k in -100_000..=600_000i32 {
            let v = k as f32 * 0.000_73;
            assert_eq!(css_num(v), old(v), "{v}");
        }
        for v in [0.0, -0.0, 0.0005, -0.0005, 1.0 / 3.0, 2.25, 3.2, 15.0, 100.0, 999.9995] {
            assert_eq!(css_num(v), old(v), "{v}");
        }
    }

    #[test]
    fn minimap_values_move_and_scale_it() {
        let l = HudLayout { minimap_scale: 1.5, minimap_right: 10.0, minimap_gap: 5.0, ..HudLayout::default() };
        let u = 10.0;
        let m = l.minimap_size(u, false);
        assert!((m - minimap_size(u, false) * 1.5).abs() < 1e-3);
        let [x, y] = l.minimap_origin([100.0, 50.0, 800.0, 600.0], u, m);
        assert!((x - (900.0 - 100.0 - m)).abs() < 1e-3);
        assert!((y - (50.0 + (1.4 + 4.16 + 5.0) * 10.0)).abs() < 1e-3);
    }

    #[test]
    fn they_register_as_live_tunables_with_their_defaults() {
        let a = evani_core::App::new("t").with_kit(crate::KartRacerPlugin);
        let p = *a.world.res::<HudLayoutParams>();
        assert_eq!(p.read(&a.world.res::<ParamRegistry>()), HudLayout::default());
        let mut reg = a.world.res_mut::<ParamRegistry>();
        let id = reg.id("kit.kart_racer.hud_wrong_top").expect("registered");
        assert_eq!(reg.def(id).unwrap().owner, "kit.kart-racer");
        // A live change (param.set) is what the next frame reads.
        let h = reg.f32_handle("kit.kart_racer.hud_wrong_top").unwrap();
        reg.set_f32(h, 30.0).unwrap();
        assert_eq!(p.read(&reg).wrong_top, 30.0);
        assert_eq!(reg.def(id).unwrap().unit, "u");
    }
}
