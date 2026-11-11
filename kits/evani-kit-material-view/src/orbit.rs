//! The viewer's camera: an orbit round a target (azimuth and elevation in
//! degrees, distance in metres) and the turntable, whose angle is a pure
//! function of the fixed tick count. Same shape as the model viewer's
//! (`evani-kit-model-view/src/orbit.rs`), with per-scene distance limits.

use evani_core::camera::CameraPose;

use crate::library::OrbitData;

/// Vertical field of view, degrees.
pub const FOV_Y_DEG: f32 = 35.0;
/// Elevation limits, degrees (never straight down or under the ground).
pub const EL_MIN: f32 = -8.0;
pub const EL_MAX: f32 = 85.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub az: f32,
    pub el: f32,
    pub dist: f32,
    /// The point looked at.
    pub target: [f32; 3],
    pub min_dist: f32,
    pub max_dist: f32,
    /// Vertical field of view, degrees ([`FOV_Y_DEG`] unless a scene picks
    /// its own lens: the procedural library's realism view, about 70 mm).
    pub fov: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self { az: 30.0, el: 15.0, dist: 3.0, target: [0.0, 0.5, 0.0], min_dist: 0.3, max_dist: 30.0, fov: FOV_Y_DEG }
    }
}

impl Orbit {
    /// From the data, keeping `limits`' distance limits when the data has none.
    pub fn from_data(d: &OrbitData, limits: Option<&Orbit>) -> Self {
        let base = limits.copied().unwrap_or_default();
        Self {
            az: d.az,
            el: d.el,
            dist: d.dist,
            target: d.target,
            min_dist: d.min_dist.unwrap_or(base.min_dist),
            max_dist: d.max_dist.unwrap_or(base.max_dist),
            fov: FOV_Y_DEG,
        }
        .clamped()
    }

    /// Clamp elevation and distance into their limits.
    pub fn clamped(mut self) -> Self {
        self.el = self.el.clamp(EL_MIN, EL_MAX);
        let lo = self.min_dist.max(0.05);
        self.dist = self.dist.clamp(lo, self.max_dist.max(lo));
        self.az = self.az.rem_euclid(360.0);
        self.fov = if self.fov.is_finite() { self.fov.clamp(5.0, 90.0) } else { FOV_Y_DEG };
        self
    }

    /// The camera, with `extra_az` degrees of turntable added.
    pub fn pose(&self, extra_az: f32) -> CameraPose {
        let (az, el) = ((self.az + extra_az).to_radians(), self.el.to_radians());
        let t = self.target;
        CameraPose {
            eye: [
                t[0] + self.dist * el.cos() * az.sin(),
                t[1] + self.dist * el.sin(),
                t[2] + self.dist * el.cos() * az.cos(),
            ],
            target: t,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: self.fov,
        }
    }

    /// Slide the target across the view: `dx`, `dy` are fractions of the
    /// view height, `dy` positive downwards as on screen (the scene follows
    /// the finger).
    pub fn pan(&mut self, dx: f32, dy: f32) {
        let az = self.az.to_radians();
        let (s, c) = az.sin_cos();
        // Screen right and screen up on the ground plane / vertical.
        let right = [c, 0.0, -s];
        let k = self.dist * (self.fov.to_radians() * 0.5).tan() * 2.0;
        for (i, r) in right.iter().enumerate() {
            self.target[i] -= r * dx * k;
        }
        self.target[1] = (self.target[1] + dy * k).clamp(-1.0, 20.0);
    }

    /// Zoom by a factor (below 1 moves in).
    pub fn zoom(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.dist *= factor;
            *self = self.clamped();
        }
    }
}

/// The turntable: the camera circles at `deg_per_s`, counted in fixed ticks
/// from an anchor, so the angle at tick N is the same on every run.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Turntable {
    pub on: bool,
    /// Angle at `anchor_tick`, degrees.
    pub anchor_deg: f32,
    pub anchor_tick: u64,
}

impl Turntable {
    /// The angle (degrees, 0..360) at `tick` for a 60 Hz step.
    pub fn angle(&self, tick: u64, deg_per_s: f32) -> f32 {
        if !self.on {
            return self.anchor_deg.rem_euclid(360.0);
        }
        let ticks = tick.saturating_sub(self.anchor_tick) as f64;
        (f64::from(self.anchor_deg) + ticks * f64::from(deg_per_s) / 60.0).rem_euclid(360.0) as f32
    }

    /// Switch on or off at `tick`, keeping the current angle.
    pub fn set(&mut self, on: bool, tick: u64, deg_per_s: f32) {
        if on != self.on {
            self.anchor_deg = self.angle(tick, deg_per_s);
            self.anchor_tick = tick;
            self.on = on;
        }
    }

    /// Back to angle zero (a reset), keeping on or off.
    pub fn zero(&mut self, tick: u64) {
        self.anchor_deg = 0.0;
        self.anchor_tick = tick;
    }
}
