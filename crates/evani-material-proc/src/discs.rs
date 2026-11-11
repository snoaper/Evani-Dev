//! The sun and moon discs (feature `sky-discs`, Asset Studio column 38,
//! 2026-11-08): the sun and the moon drawn into the sky dome at their true
//! apparent size (about half a degree each), so a normal lens shows them as
//! small discs, not the enlarged billboards games often use.
//!
//! - **The sun**: a disc with limb darkening (its rim a little dimmer and
//!   warmer than its centre, as the photosphere is), lit through the dome's
//!   own air (red and dim near the horizon), with its aureole: the bright
//!   forward-scattered glow of the haze round it, a tight core and a wider
//!   skirt.
//! - **The low sun's warmth** (round 2): near the horizon, and most of all
//!   towards the sun, the sky's own scattered light loses its blue and some
//!   green (it has come a long way through the haze), so the band round a
//!   setting sun is a saturated orange while the sky higher up stays blue.
//! - **The moon**: a sphere lit by the sun, so its phase follows from where
//!   the two stand in the sky: a thin crescent close to the sun, a half moon
//!   at 90 degrees, full opposite it. Its face is shaded the way the moon's
//!   dusty surface reflects (Lommel-Seeliger, which keeps the lit face flat
//!   and the crescent bright to its edge) with faint maria; its dark side
//!   gets a little earthshine. The sky's own light lies in front of it, so
//!   it is added to the sky (a daytime moon is pale, its dark side
//!   invisible). A thin crescent is narrower than a pixel, so the moon is
//!   drawn by its lit strip's exact share of each pixel (`pixel`; column
//!   73: along six lines towards the light the strip from the terminator
//!   to the limb is weighed under a tent, so a crescent far thinner than a
//!   pixel stays one smooth tapering arc to its horns, with no beads); the
//!   sun's outline is smoothed over two pixels, and a
//!   low sun's middle can burn out white (`disc_core`).
//! - **The aureole's shape and colour** (column 73): its skirt can lie
//!   along the horizon (`skirt_flat` times wider than tall: the haze that
//!   makes it is thickest along the skyline, so a setting sun's glow is a
//!   flattened oval) and take the sun's own colour (`glow_sun`: its light
//!   came down the sun's line through the air, so it stays a deep orange
//!   above a low sun instead of paling to the shorter view line's yellow).
//! - **The sky's grain** (column 73, `grain`): each colour a hair brighter
//!   or darker from point to point, under one level of an 8-bit screen, so
//!   the dome's slow gradients show no bands.
//! - **The whole skyline's warmth, a soft crescent** (column 73):
//!   `warmth_round` lays a share of the low sun's warmth all round the
//!   horizon (at sunset the whole skyline is warm, most of all under the
//!   sun); `moon_soft` is the half width in pixels of the tent the moon is
//!   sampled under (1.3 a crisp line, 2 a phone camera's soft one) and
//!   `moon_horns` fades a thin crescent's horns before the ends of the
//!   limb (Danjon's shortened cusps).
//! - **The horizon's haze, the glow's broad lobe** (column 73): `haze`
//!   veils the lowest degree or two of the sky with the dim light
//!   `haze_tint` (the thick air and dust lying on the land: a sunset's
//!   orange dulls to a dusky rose band just over the land), fading out
//!   over `haze_height`; `broad` adds a faint third lobe to the
//!   aureole, tens of degrees wide (`broad_width`): the whole sky on the
//!   sun's side a little brighter and warmer.
//! - **The haze towards the sun, the glow under it, an uneven sky**
//!   (column 73's new run): the haze's veil is a bell over the height
//!   (nearly whole for the first half degree) and its light towards the
//!   sun is its own (`haze_sun`: the dust lit from behind, a rose red
//!   under a setting sun, giving way to `haze_tint` over
//!   `haze_sun_width`), so the skyline is a dusky grey band away from
//!   the sun and a rose one under it; `glow_low` keeps the aureole the
//!   sun's own colour below the sun's height too (it sank to a dark red
//!   on the skyline); `uneven` makes the warmth and glow a few percent
//!   stronger here and weaker there in slow swells, so the broad
//!   gradients are not quite regular.
//!   The light on the moon is worked out here, not in the shader
//!   ([`SkyDiscs::moon_light`]): `moon_tilt` turns the lit side round the
//!   moon's centre away from the true sun direction and `moon_phase` sets
//!   the angle between the moon and its light (a thicker or thinner
//!   crescent); both 0 is the physical moon.
//!
//! Records [`crate::layout::SKY_DISCS`] to +10 ([`SkyDiscs::records`]). With
//! `on` false they hold zeros and the dome draws its own sun disc and sky
//! exactly as without the feature.
//!
//! Live params ([`DiscHandles`]): `sky.discs` (on), `sky.disc_size`,
//! `sky.disc_limb`, `sky.disc_glow`, `sky.disc_glow_width`,
//! `sky.disc_skirt`, `sky.disc_skirt_share`, `sky.disc_skirt_flat`,
//! `sky.disc_glow_sun`, `sky.disc_pixel`, `sky.disc_core`,
//! `sky.disc_core_size`, `sky.disc_bright`,
//! `sky.horizon_warmth`, `sky.horizon_warmth_height`,
//! `sky.horizon_warmth_sun`, `sky.horizon_warmth_round`,
//! `sky.horizon_warm_green`,
//! `sky.horizon_warm_blue`, `sky.grain`, `sky.moon` (draw the moon),
//! `sky.moon_elevation`, `sky.moon_azimuth`, `sky.moon_size`,
//! `sky.moon_bright`, `sky.moon_earthshine`, `sky.moon_maria`,
//! `sky.moon_tilt`, `sky.moon_phase`, `sky.moon_soft`, `sky.moon_horns`,
//! `sky.horizon_haze`, `sky.horizon_haze_height`, `sky.horizon_haze_red`,
//! `sky.horizon_haze_green`, `sky.horizon_haze_blue`, `sky.disc_broad`,
//! `sky.disc_broad_width`, `sky.disc_glow_low`, `sky.uneven`,
//! `sky.horizon_haze_sun_red`, `sky.horizon_haze_sun_green`,
//! `sky.horizon_haze_sun_blue`, `sky.horizon_haze_sun_width`.

use evani_core::params::{BoolParam, F32Param, ParamDef, ParamError, ParamRegistry};

/// The records the part fills, from [`crate::layout::SKY_DISCS`].
pub const RECORDS: usize = 11;

/// The sun's and moon's discs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SkyDiscs {
    /// Draw these discs (off: the dome's own sun disc, no moon).
    pub on: bool,
    /// The sun's apparent diameter (degrees; 0.53 as seen from the Earth).
    pub sun_size: f32,
    /// Limb darkening (0: a flat disc; 0.6: the real sun's in visible light;
    /// at most 0.8, so the reddened rim's blue stays positive).
    pub limb: f32,
    /// The aureole round the sun: its strength (a share of the sun's light
    /// through the air), the angle its core falls off over (degrees), how
    /// many times wider its skirt is and the skirt's share of it.
    pub glow: f32,
    pub glow_width: f32,
    pub skirt: f32,
    pub skirt_share: f32,
    /// How many times wider than tall the skirt lies along the horizon
    /// (1: round, as before; 2 to 3: a setting sun's flattened glow).
    pub skirt_flat: f32,
    /// How far the aureole takes the sun's own colour (0: the colour of
    /// the sun's light along each view line, paler higher up, as before;
    /// 1: the colour of the sun's light down its own line through the
    /// air, the same deep orange all over a low sun's glow).
    pub glow_sun: f32,
    /// One pixel's angle (degrees; 60 / 1672 for a 60 degree view on a
    /// 1672 pixel wide picture): the discs' outlines are smoothed and the
    /// moon sampled over two of them.
    pub pixel: f32,
    /// The sun's white core: a share of its red light added to all three
    /// colours, full in the middle and fading out towards the rim, so a low
    /// sun's middle burns out white as in a photograph while its rim stays
    /// orange (0: none; with `disc_bright` 0.003, a few tenths for a sun a
    /// couple of degrees up: core x disc_bright of some thousandths clips
    /// white).
    pub core: f32,
    /// How far out the white core reaches, a share of the disc's radius
    /// (it fades out within an eighth of the radius either side of this):
    /// the burnt-out white disc of a photographed low sun is smaller than
    /// the sun, whose rim stays at the glare's orange.
    pub core_size: f32,
    /// The disc's brightness, a share of the dome's own (1, three.js's,
    /// far past white even for a setting sun). A few times white lets its
    /// soft outline run from the white middle through yellow into the
    /// orange glare instead of ending in a hard yellow ring.
    pub disc_bright: f32,
    /// The low sun's warmth on the sky's scattered light: how strong it is
    /// at the horizon (0: none, 1: the full warm colour), the height
    /// (sine of the elevation) it fades over (a bell: full along the
    /// skyline, about a third left at that height), how much it gathers towards
    /// the sun's side (0: all round the horizon, 1: only towards the sun)
    /// and the warm colour's green and blue (red stays).
    pub warmth: f32,
    pub warmth_height: f32,
    pub warmth_sun: f32,
    /// The share of the warmth that lies all round the horizon, whatever
    /// the bearing (0: all of it gathered by `warmth_sun`, as before; a
    /// tenth or two: the skyline away from a setting sun a dull orange
    /// instead of the dome's own mauve).
    pub warmth_round: f32,
    pub warm_green: f32,
    pub warm_blue: f32,
    /// The sky's fine grain: how far each colour strays from point to
    /// point, a share of its light (0: none; about 0.03 is under one level
    /// of an 8-bit screen: no bands in the slow gradients).
    pub grain: f32,
    /// Draw the moon.
    pub moon: bool,
    /// Where the moon stands: elevation and azimuth (degrees, azimuth from
    /// +z towards +x, as [`crate::sky::sun_direction`]).
    pub moon_elevation: f32,
    pub moon_azimuth: f32,
    /// The moon's apparent diameter (degrees; 0.52 on average).
    pub moon_size: f32,
    /// The sunlit face's radiance (the sky's units before its brightness)
    /// and the earthshine on its dark side (a share of that).
    pub moon_bright: f32,
    pub earthshine: f32,
    /// How dark the maria are (0: an even face).
    pub maria: f32,
    /// How far the lit side is turned round the moon's centre from the
    /// true sun direction (degrees; 0: physical).
    pub moon_tilt: f32,
    /// The angle between the moon and the light on it (degrees; 0: the true
    /// angle to the sun): a wider angle, a thicker crescent.
    pub moon_phase: f32,
    /// The moon's softness: the half width, in pixels, of the tent it is
    /// sampled under (1.3: a crisp thin line, as before; 2: the soft line
    /// of a phone camera's crescent, with no beads along it).
    pub moon_soft: f32,
    /// How fast a thin crescent's horns fade towards the ends of the limb
    /// (0: not at all, as before; 1 to 3: the shortened cusps of a real
    /// thin crescent, whose grazing sunlight is lost in the shadows of the
    /// moon's own relief). Less for a fuller moon, none from half on.
    pub moon_horns: f32,
    /// The haze along the horizon: how far the sky on the skyline is
    /// veiled by the haze's own light `haze_tint` (0: none, as before),
    /// the height it fades out over (sine of the elevation, a bell: about
    /// a third left at that height; 0.015 is under a degree) and that
    /// light (the sky's units before its brightness, as `moon_bright`: a
    /// dim grey-blue, the daylight the haze scatters).
    pub haze: f32,
    pub haze_height: f32,
    pub haze_tint: [f32; 3],
    /// The aureole's broad lobe: its share of the glow (0: none, as
    /// before) and the angle it falls off over (degrees): the faint
    /// brightening of the whole sky on the sun's side.
    pub broad: f32,
    pub broad_width: f32,
    /// How far the aureole keeps the sun's own colour below the sun's
    /// height too (0: not at all, as before: under the sun the glow sinks
    /// to the view line's dark red on the skyline; 1: the sun's own
    /// colour down to the land, as `glow_sun` does above it).
    pub glow_low: f32,
    /// The haze's unevenness: how far the low sun's warmth and glow stray
    /// from place to place, a share of them, in slow swells some tens of
    /// degrees across fixed to the world's bearings (0: none, as before; a
    /// few hundredths: the broad gradients not quite regular).
    pub uneven: f32,
    /// The haze's own light towards the sun (the sky's units, as
    /// `haze_tint`): the dust lit from behind, a rose red under a setting
    /// sun. It gives way to `haze_tint` over the angle `haze_sun_width`
    /// off the sun (degrees; 0: `haze_tint` all round, as before).
    pub haze_sun: [f32; 3],
    pub haze_sun_width: f32,
}

impl Default for SkyDiscs {
    /// Off (the dome as before), with real sizes ready for when it's on.
    fn default() -> Self {
        SkyDiscs {
            on: false,
            sun_size: 0.53,
            limb: 0.6,
            glow: 1.0,
            glow_width: 1.0,
            skirt: 5.0,
            skirt_share: 0.25,
            skirt_flat: 1.0,
            glow_sun: 0.0,
            pixel: 0.036,
            core: 0.0,
            core_size: 0.6,
            disc_bright: 1.0,
            warmth: 0.0,
            warmth_height: 0.1,
            warmth_sun: 0.5,
            warmth_round: 0.0,
            warm_green: 0.75,
            warm_blue: 0.2,
            grain: 0.0,
            moon: true,
            moon_elevation: 25.0,
            moon_azimuth: 0.0,
            moon_size: 0.52,
            moon_bright: 2.5,
            earthshine: 0.002,
            maria: 0.25,
            moon_tilt: 0.0,
            moon_phase: 0.0,
            moon_soft: 1.3,
            moon_horns: 0.0,
            haze: 0.0,
            haze_height: 0.015,
            haze_tint: [1.0; 3],
            broad: 0.0,
            broad_width: 20.0,
            glow_low: 0.0,
            uneven: 0.0,
            haze_sun: [1.0; 3],
            haze_sun_width: 0.0,
        }
    }
}

impl SkyDiscs {
    /// The `kit_data` records from [`crate::layout::SKY_DISCS`]:
    /// `[sun radius (rad), limb, glow (times 0.04), glow width (rad)]`,
    /// `[moon direction (towards it), moon radius (rad); 0: no moon]`,
    /// `[moon radiance, earthshine, maria, pixel (rad)]`,
    /// `[warmth, warmth height, how fast the warmth falls off away from
    /// the sun's bearing (per unit of 1 - cos), the sun's white core]`,
    /// `[warm green, warm blue, skirt width (times the core's), skirt
    /// share]`, `[the light on the moon (unit, towards it), the disc's
    /// brightness]`, `[the skirt's flattening (at least 1), the aureole's
    /// share of the sun's own colour, the sky's grain, the white core's
    /// radius]`, `[the warmth's share all round the horizon, the moon's
    /// softness (pixels), its horns' fade, the haze's height]`, `[the
    /// haze's colour, its share on the skyline]`, `[the aureole's broad
    /// lobe, its width (rad), the glow's share of the sun's own colour
    /// below the sun, the haze's unevenness]`, `[the haze's light towards
    /// the sun, the angle it gives way over (rad; 0: none)]`, with the sun
    /// towards `sun`. All zero when off.
    pub fn records(&self, sun: [f32; 3]) -> [[f32; 4]; RECORDS] {
        if !self.on {
            return [[0.0; 4]; RECORDS];
        }
        let half = |deg: f32| (deg.max(0.01) * 0.5).to_radians();
        let m = crate::sky::sun_direction(self.moon_elevation, self.moon_azimuth);
        let moon_r = if self.moon { half(self.moon_size) } else { 0.0 };
        let l = self.moon_light(sun);
        [
            [half(self.sun_size), self.limb.clamp(0.0, 0.8), self.glow.max(0.0) * 0.04, self.glow_width.max(0.01).to_radians()],
            [m[0], m[1], m[2], moon_r],
            [self.moon_bright.max(0.0), self.earthshine.max(0.0), self.maria.clamp(0.0, 1.0), self.pixel.max(0.0).to_radians()],
            [self.warmth.clamp(0.0, 1.0), self.warmth_height.max(0.005), WARM_FALL * self.warmth_sun.clamp(0.0, 1.0), self.core.clamp(0.0, 5.0)],
            [self.warm_green.clamp(0.0, 1.0), self.warm_blue.clamp(0.0, 1.0), self.skirt.max(1.0), self.skirt_share.clamp(0.0, 1.0)],
            [l[0], l[1], l[2], self.disc_bright.clamp(0.0, 1.0)],
            [self.skirt_flat.clamp(1.0, 10.0), self.glow_sun.clamp(0.0, 1.0), self.grain.clamp(0.0, 0.2), self.core_size.clamp(0.1, 1.0)],
            [self.warmth_round.clamp(0.0, 1.0), self.moon_soft.clamp(0.5, 4.0), self.moon_horns.clamp(0.0, 8.0), self.haze_height.max(0.002)],
            [self.haze_tint[0].clamp(0.0, 400.0), self.haze_tint[1].clamp(0.0, 400.0), self.haze_tint[2].clamp(0.0, 400.0), self.haze.clamp(0.0, 1.0)],
            [self.broad.clamp(0.0, 1.0), self.broad_width.clamp(1.0, 90.0).to_radians(), self.glow_low.clamp(0.0, 1.0), self.uneven.clamp(0.0, 0.5)],
            [self.haze_sun[0].clamp(0.0, 400.0), self.haze_sun[1].clamp(0.0, 400.0), self.haze_sun[2].clamp(0.0, 400.0), self.haze_sun_width.clamp(0.0, 90.0).to_radians()],
        ]
    }

    /// The low sun's warmth on the clear sky looking along `dir` with the
    /// sun towards `sun`: the factor per channel the shader puts on the
    /// sky's scattered light (1 when off), before the haze's unevenness
    /// (`uneven`: the shader's strays a few percent either way).
    pub fn warm(&self, sun: [f32; 3], dir: [f32; 3]) -> [f32; 3] {
        if !self.on || self.warmth <= 0.0 {
            return [1.0; 3];
        }
        let r = self.records(sun);
        let flat = |v: [f32; 3]| {
            let l = (v[0] * v[0] + v[2] * v[2]).sqrt().max(1e-6);
            [v[0] / l, v[2] / l]
        };
        let (h, s) = (flat(dir), flat(sun));
        let c = h[0] * s[0] + h[1] * s[1];
        let y = dir[1].max(0.0) / r[3][1];
        let g = ((c - 1.0) * r[3][2]).exp();
        let k = r[3][0] * (-y * y).exp() * (g + (1.0 - g) * r[7][0]);
        [1.0, 1.0 - k * (1.0 - r[4][0]), 1.0 - k * (1.0 - r[4][1])]
    }

    /// The light on the moon (unit, towards it) with the sun towards
    /// `sun`: the sun's own direction (it is so far away that it lights the
    /// moon along the same direction it lights the Earth), turned round
    /// the moon's centre by `moon_tilt` and set at `moon_phase` from the
    /// moon when that is not 0.
    pub fn moon_light(&self, sun: [f32; 3]) -> [f32; 3] {
        let m = crate::sky::sun_direction(self.moon_elevation, self.moon_azimuth);
        let n = (sun[0] * sun[0] + sun[1] * sun[1] + sun[2] * sun[2]).sqrt().max(1e-6);
        let s = sun.map(|v| v / n);
        let c = (m[0] * s[0] + m[1] * s[1] + m[2] * s[2]).clamp(-1.0, 1.0);
        // The way from the moon to the sun on the sky (unit, across m).
        let p = [s[0] - m[0] * c, s[1] - m[1] * c, s[2] - m[2] * c];
        let pl = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if pl < 1e-6 {
            return s;
        }
        let u = p.map(|v| v / pl);
        let x = [m[1] * u[2] - m[2] * u[1], m[2] * u[0] - m[0] * u[2], m[0] * u[1] - m[1] * u[0]];
        let (sw, cw) = self.moon_tilt.to_radians().sin_cos();
        let e = if self.moon_phase > 0.0 { self.moon_phase.min(180.0).to_radians() } else { c.acos() };
        let (se, ce) = e.sin_cos();
        [0, 1, 2].map(|i| m[i] * ce + (u[i] * cw + x[i] * sw) * se)
    }

    /// The moon's phase as seen from the ground with the sun towards `sun`:
    /// the lit share of its disc (0 new, 1 full), from the angle between
    /// it and its light ([`SkyDiscs::moon_light`]).
    pub fn moon_lit_share(&self, sun: [f32; 3]) -> f32 {
        let m = crate::sky::sun_direction(self.moon_elevation, self.moon_azimuth);
        let l = self.moon_light(sun);
        0.5 * (1.0 - (m[0] * l[0] + m[1] * l[1] + m[2] * l[2]).clamp(-1.0, 1.0))
    }
}

/// How fast the warmth falls off away from the sun's bearing at
/// `warmth_sun` 1: `exp(-(1 - cos) * this)`, about a fifth at 20 degrees
/// off (round 4: the reference's horizon keeps its mauve a few degrees from
/// the sun).
const WARM_FALL: f32 = 25.0;

/// `(suffix, min, max, step, description)` per float param, in
/// [`SkyDiscs`] field order (the two switches left out).
const FLOATS: [(&str, f32, f32, f32, &str); 42] = [
    ("disc_size", 0.05, 10.0, 0.01, "The sun's apparent diameter (degrees; 0.53 is the real sun)"),
    ("disc_limb", 0.0, 0.8, 0.01, "The sun's limb darkening (0: a flat disc; 0.6: the real sun's)"),
    ("disc_glow", 0.0, 60.0, 0.01, "The aureole round the sun: the haze's bright glow close to it"),
    ("disc_glow_width", 0.05, 20.0, 0.01, "How far the aureole's core reaches from the sun (degrees)"),
    ("disc_skirt", 1.0, 30.0, 0.01, "How many times wider the aureole's skirt is than its core"),
    ("disc_skirt_share", 0.0, 1.0, 0.01, "The skirt's share of the aureole"),
    ("disc_skirt_flat", 1.0, 10.0, 0.01, "How many times wider than tall the aureole's skirt lies along the horizon (1: round)"),
    ("disc_glow_sun", 0.0, 1.0, 0.01, "How far the aureole takes the sun's own colour (0: each view line's, paler higher up; 1: the low sun's deep orange all over)"),
    ("disc_pixel", 0.0, 1.0, 0.0005, "One pixel's angle (degrees): the discs' outlines are smoothed and the moon sampled over it"),
    ("disc_core", 0.0, 5.0, 0.001, "The sun's white core: a share of its red light added to all colours, fading out to the rim (0: none)"),
    ("disc_core_size", 0.1, 1.0, 0.01, "How far out the sun's white core reaches (a share of the disc's radius)"),
    ("disc_bright", 0.0, 1.0, 0.001, "The sun disc's brightness, a share of the dome's own (1); lower lets its soft edge grade from white through yellow to orange"),
    ("horizon_warmth", 0.0, 1.0, 0.01, "The low sun's warmth on the sky near the horizon (0: none)"),
    ("horizon_warmth_height", 0.005, 1.0, 0.005, "How high the warmth reaches (sine of the elevation it fades over)"),
    ("horizon_warmth_sun", 0.0, 1.0, 0.01, "How much the warmth gathers towards the sun's side (0: all round the horizon)"),
    ("horizon_warmth_round", 0.0, 1.0, 0.01, "The share of the warmth that lies all round the horizon, whatever the bearing (0: none)"),
    ("horizon_warm_green", 0.0, 1.0, 0.01, "The warm colour's green (red stays)"),
    ("horizon_warm_blue", 0.0, 1.0, 0.01, "The warm colour's blue (red stays)"),
    ("grain", 0.0, 0.2, 0.001, "The sky's fine grain, a share of its light (0: none; 0.03: under one level of an 8-bit screen, no bands in its gradients)"),
    ("moon_elevation", -10.0, 90.0, 0.1, "The moon's height over the horizon (degrees)"),
    ("moon_azimuth", -180.0, 180.0, 0.1, "The moon's bearing (degrees from +z towards +x)"),
    ("moon_size", 0.05, 10.0, 0.01, "The moon's apparent diameter (degrees; 0.52 is the real moon)"),
    ("moon_bright", 0.0, 400.0, 0.05, "How bright the moon's sunlit face is (the sky's own units)"),
    ("moon_earthshine", 0.0, 0.5, 0.001, "The faint earthshine on the moon's dark side (a share of its sunlit face)"),
    ("moon_maria", 0.0, 1.0, 0.01, "How dark the moon's maria are (0: an even face)"),
    ("moon_tilt", -180.0, 180.0, 0.5, "Turns the moon's lit side round its centre from the true sun direction (degrees; 0: physical)"),
    ("moon_phase", 0.0, 180.0, 0.5, "The angle between the moon and the light on it (degrees; 0: the true angle to the sun; wider: a thicker crescent)"),
    ("moon_soft", 0.5, 4.0, 0.05, "The moon's softness: the half width in pixels of the tent it is sampled under (1.3: crisp; 2: a phone camera's soft crescent)"),
    ("moon_horns", 0.0, 8.0, 0.05, "How fast a thin crescent's horns fade towards the ends of the limb (0: not at all)"),
    ("horizon_haze", 0.0, 1.0, 0.01, "The haze along the horizon: how far the sky on the skyline is veiled by the haze's own light (0: none)"),
    ("horizon_haze_height", 0.002, 1.0, 0.001, "The height the horizon's haze fades out over (sine of the elevation, a bell; 0.015 is under a degree)"),
    ("horizon_haze_red", 0.0, 400.0, 0.01, "The red of the haze's own light (the sky's own units)"),
    ("horizon_haze_green", 0.0, 400.0, 0.01, "The green of the haze's own light (the sky's own units)"),
    ("horizon_haze_blue", 0.0, 400.0, 0.01, "The blue of the haze's own light (the sky's own units)"),
    ("disc_broad", 0.0, 1.0, 0.001, "The aureole's broad lobe, a share of the glow: the whole sky on the sun's side a little brighter (0: none)"),
    ("disc_broad_width", 1.0, 90.0, 0.5, "The angle the aureole's broad lobe falls off over (degrees)"),
    ("disc_glow_low", 0.0, 1.0, 0.01, "How far the aureole keeps the sun's own colour below the sun's height too (0: it sinks to the view line's dark red on the skyline)"),
    ("uneven", 0.0, 0.5, 0.005, "The haze's unevenness: how far the low sun's warmth and glow stray from place to place, in slow swells (0: none)"),
    ("horizon_haze_sun_red", 0.0, 400.0, 0.01, "The red of the haze's own light towards the sun (the sky's own units)"),
    ("horizon_haze_sun_green", 0.0, 400.0, 0.01, "The green of the haze's own light towards the sun (the sky's own units)"),
    ("horizon_haze_sun_blue", 0.0, 400.0, 0.01, "The blue of the haze's own light towards the sun (the sky's own units)"),
    ("horizon_haze_sun_width", 0.0, 90.0, 0.1, "The angle off the sun the haze's sunward light gives way to its light all round over (degrees; 0: the same light all round)"),
];

/// The registry handles of the disc params.
#[derive(Debug, Clone, Copy)]
pub struct DiscHandles {
    on: BoolParam,
    moon: BoolParam,
    f: [F32Param; 42],
}

impl DiscHandles {
    /// Register the disc params, starting at `start` (owner `material-proc`).
    pub fn register(reg: &mut ParamRegistry, start: &SkyDiscs) -> Result<Self, ParamError> {
        let owner = crate::params::OWNER;
        let on = reg.register_bool(
            ParamDef::bool("sky.discs", start.on)
                .describe("Draw the sun and moon at their true size (off: the dome's own sun disc, no moon)")
                .owner(owner),
        )?;
        let moon = reg.register_bool(ParamDef::bool("sky.moon", start.moon).describe("Draw the moon (with sky.discs on)").owner(owner))?;
        let s = start;
        let v = [
            s.sun_size,
            s.limb,
            s.glow,
            s.glow_width,
            s.skirt,
            s.skirt_share,
            s.skirt_flat,
            s.glow_sun,
            s.pixel,
            s.core,
            s.core_size,
            s.disc_bright,
            s.warmth,
            s.warmth_height,
            s.warmth_sun,
            s.warmth_round,
            s.warm_green,
            s.warm_blue,
            s.grain,
            s.moon_elevation,
            s.moon_azimuth,
            s.moon_size,
            s.moon_bright,
            s.earthshine,
            s.maria,
            s.moon_tilt,
            s.moon_phase,
            s.moon_soft,
            s.moon_horns,
            s.haze,
            s.haze_height,
            s.haze_tint[0],
            s.haze_tint[1],
            s.haze_tint[2],
            s.broad,
            s.broad_width,
            s.glow_low,
            s.uneven,
            s.haze_sun[0],
            s.haze_sun[1],
            s.haze_sun[2],
            s.haze_sun_width,
        ];
        let mut f = Vec::with_capacity(FLOATS.len());
        for (i, (suffix, min, max, step, describe)) in FLOATS.into_iter().enumerate() {
            f.push(reg.register_f32(ParamDef::f32(&format!("sky.{suffix}"), v[i]).range(min, max).step(step).describe(describe).owner(owner))?);
        }
        let f: [F32Param; 42] = f.try_into().map_err(|_| ParamError::Unknown("sky.discs".into()))?;
        Ok(DiscHandles { on, moon, f })
    }

    /// The current discs.
    pub fn read(&self, reg: &ParamRegistry) -> SkyDiscs {
        let [sun_size, limb, glow, glow_width, skirt, skirt_share, skirt_flat, glow_sun, pixel, core, core_size, disc_bright, warmth, warmth_height, warmth_sun, warmth_round, warm_green, warm_blue, grain, moon_elevation, moon_azimuth, moon_size, moon_bright, earthshine, maria, moon_tilt, moon_phase, moon_soft, moon_horns, haze, haze_height, haze_red, haze_green, haze_blue, broad, broad_width, glow_low, uneven, haze_sun_red, haze_sun_green, haze_sun_blue, haze_sun_width] =
            self.f.map(|h| reg.f32(h));
        SkyDiscs {
            on: reg.bool(self.on),
            sun_size,
            limb,
            glow,
            glow_width,
            skirt,
            skirt_share,
            skirt_flat,
            glow_sun,
            pixel,
            core,
            core_size,
            disc_bright,
            warmth,
            warmth_height,
            warmth_sun,
            warmth_round,
            warm_green,
            warm_blue,
            grain,
            moon: reg.bool(self.moon),
            moon_elevation,
            moon_azimuth,
            moon_size,
            moon_bright,
            earthshine,
            maria,
            moon_tilt,
            moon_phase,
            moon_soft,
            moon_horns,
            haze,
            haze_height,
            haze_tint: [haze_red, haze_green, haze_blue],
            broad,
            broad_width,
            glow_low,
            uneven,
            haze_sun: [haze_sun_red, haze_sun_green, haze_sun_blue],
            haze_sun_width,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Off, the records are zero (the dome's own sun disc and sky); on, the
    /// radii are the half angles in radians and the moon's direction is a
    /// unit vector at its elevation.
    #[test]
    fn records_hold_true_sizes_and_are_zero_when_off() {
        let sun = crate::sky::sun_direction(2.0, 0.0);
        assert_eq!(SkyDiscs::default().records(sun), [[0.0; 4]; RECORDS]);
        let d = SkyDiscs { on: true, ..SkyDiscs::default() };
        let r = d.records(sun);
        assert!((r[0][0] - 0.265f32.to_radians()).abs() < 1e-6);
        assert!((r[1][3] - 0.26f32.to_radians()).abs() < 1e-6);
        let m = [r[1][0], r[1][1], r[1][2]];
        assert!(((m[0] * m[0] + m[1] * m[1] + m[2] * m[2]).sqrt() - 1.0).abs() < 1e-5);
        assert!((m[1] - 25.0f32.to_radians().sin()).abs() < 1e-5);
        assert!((r[2][3] - 0.036f32.to_radians()).abs() < 1e-7);
        let no_moon = SkyDiscs { on: true, moon: false, ..SkyDiscs::default() };
        assert_eq!(no_moon.records(sun)[1][3], 0.0);
        // The aureole's shape and colour and the sky's grain: as before
        // (round, the view line's colour, no grain) until set.
        assert_eq!(r[6], [1.0, 0.0, 0.0, 0.6]);
        let oval = SkyDiscs { on: true, skirt_flat: 2.5, glow_sun: 1.0, grain: 0.03, core_size: 0.72, ..SkyDiscs::default() };
        assert_eq!(oval.records(sun)[6], [2.5, 1.0, 0.03, 0.72]);
        // The warmth all round, the moon's softness and its horns' fade: as
        // before (none, the crisp 1.3 pixel tent, whole horns) until set.
        assert_eq!(r[7], [0.0, 1.3, 0.0, 0.015]);
        let soft = SkyDiscs { on: true, warmth_round: 0.15, moon_soft: 2.0, moon_horns: 1.5, haze_height: 0.03, ..SkyDiscs::default() };
        assert_eq!(soft.records(sun)[7], [0.15, 2.0, 1.5, 0.03]);
        // The horizon's haze and the aureole's broad lobe: none until set.
        assert_eq!(r[8], [1.0, 1.0, 1.0, 0.0]);
        assert_eq!(r[9], [0.0, 20f32.to_radians(), 0.0, 0.0]);
        let hazy = SkyDiscs { on: true, haze: 0.4, haze_tint: [0.9, 0.8, 1.1], broad: 0.02, broad_width: 30.0, ..SkyDiscs::default() };
        assert_eq!(hazy.records(sun)[8], [0.9, 0.8, 1.1, 0.4]);
        assert_eq!(hazy.records(sun)[9], [0.02, 30f32.to_radians(), 0.0, 0.0]);
        // The glow under the sun, the unevenness and the haze's sunward
        // light: as before (none, the same haze light all round) until set.
        assert_eq!(r[10], [1.0, 1.0, 1.0, 0.0]);
        let dusk = SkyDiscs { on: true, glow_low: 0.6, uneven: 0.04, haze_sun: [3.0, 0.5, 0.2], haze_sun_width: 5.0, ..SkyDiscs::default() };
        assert_eq!(dusk.records(sun)[9][2..], [0.6, 0.04]);
        assert_eq!(dusk.records(sun)[10], [3.0, 0.5, 0.2, 5f32.to_radians()]);
    }

    /// The phase follows the angle to the sun: new beside it, half at 90
    /// degrees, full opposite; 23 degrees off it is a thin crescent.
    #[test]
    fn the_moons_phase_follows_the_sun() {
        let d = SkyDiscs { on: true, moon_elevation: 25.0, moon_azimuth: 0.0, ..SkyDiscs::default() };
        let at = |el: f32, az: f32| d.moon_lit_share(crate::sky::sun_direction(el, az));
        assert!(at(25.0, 0.0) < 1e-4);
        assert!((at(-65.0, 0.0) - 0.5).abs() < 1e-3);
        assert!((at(-25.0, 180.0) - 1.0).abs() < 1e-3);
        let crescent = at(2.0, 0.0);
        assert!(crescent > 0.02 && crescent < 0.06, "{crescent}");
        // The tilt turns the light round the moon and keeps the phase; the
        // phase sets the angle (50 degrees: 18 percent lit), whatever the
        // tilt.
        let sun = crate::sky::sun_direction(2.0, 0.0);
        let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
        let tilted = SkyDiscs { moon_tilt: 60.0, ..d };
        assert!((tilted.moon_lit_share(sun) - crescent).abs() < 1e-4);
        let (l0, l1) = (d.moon_light(sun), tilted.moon_light(sun));
        assert!((dot(l1, l1) - 1.0).abs() < 1e-4 && dot(l0, l1) < 0.999, "{l0:?} {l1:?}");
        let thick = SkyDiscs { moon_tilt: 60.0, moon_phase: 50.0, ..d };
        assert!((thick.moon_lit_share(sun) - 0.5 * (1.0 - 50f32.to_radians().cos())).abs() < 1e-4);
        assert_eq!(d.records(sun)[5][..3], d.moon_light(sun));
    }

    /// The warmth: none when off or at strength 0; at the horizon under the
    /// sun the full warm colour, fading with height and away from the sun.
    #[test]
    fn the_warmth_gathers_low_and_towards_the_sun() {
        let sun = crate::sky::sun_direction(2.0, 0.0);
        assert_eq!(SkyDiscs::default().warm(sun, [0.0, 0.0, 1.0]), [1.0; 3]);
        let d = SkyDiscs { on: true, warmth: 1.0, warmth_height: 0.3, warmth_sun: 1.0, warm_green: 0.7, warm_blue: 0.1, ..SkyDiscs::default() };
        let under = d.warm(sun, [0.0, 0.0, 1.0]);
        assert!((under[1] - 0.7).abs() < 1e-5 && (under[2] - 0.1).abs() < 1e-5 && under[0] == 1.0);
        let high = d.warm(sun, [0.0, 0.6, 0.8]);
        let low = d.warm(sun, [0.0, 0.05, 0.9987]);
        assert!(low[2] < 0.15, "{low:?}");
        let away = d.warm(sun, [0.0, 0.0, -1.0]);
        assert!(high[2] > under[2] + 0.5 && away[2] > 0.99, "{high:?} {away:?}");
        // A share of it all round: the skyline away from the sun warm too,
        // by that share; under the sun as before.
        let round = SkyDiscs { warmth_round: 0.2, ..d };
        let (away, under2) = (round.warm(sun, [0.0, 0.0, -1.0]), round.warm(sun, [0.0, 0.0, 1.0]));
        assert!((away[2] - (1.0 - 0.2 * 0.9)).abs() < 1e-3 && (under2[2] - under[2]).abs() < 1e-5, "{away:?} {under2:?}");
    }

    /// Every param registers once and reads back what it started at.
    #[test]
    fn params_round_trip() {
        let mut reg = ParamRegistry::default();
        let start = SkyDiscs { on: true, moon_elevation: 24.9, glow: 2.5, skirt_flat: 2.5, glow_sun: 0.5, grain: 0.03, core_size: 0.7, warmth: 0.7, warmth_round: 0.15, moon_tilt: -20.0, moon_phase: 50.0, moon_soft: 2.0, moon_horns: 1.5, haze: 0.4, haze_height: 0.03, haze_tint: [0.9, 0.8, 1.1], broad: 0.02, broad_width: 30.0, glow_low: 0.6, uneven: 0.04, haze_sun: [3.0, 0.5, 0.2], haze_sun_width: 5.0, ..SkyDiscs::default() };
        let h = DiscHandles::register(&mut reg, &start);
        assert!(h.is_ok());
        let Ok(h) = h else { return };
        assert_eq!(h.read(&reg), start);
    }
}
