//! The viewer's camera: an orbit around the model (azimuth and elevation in
//! degrees, distance in metres), and the turntable, whose angle is a pure
//! function of the fixed tick count (docs/EVANI_MODEL_TAB.md 4.1).

use evani_core::camera::CameraPose;

/// Vertical field of view of the viewer camera, degrees.
pub const FOV_Y_DEG: f32 = 35.0;
/// Elevation limits, degrees (never straight down or under the ground).
pub const EL_MIN: f32 = -5.0;
pub const EL_MAX: f32 = 85.0;
/// Distance limits, metres.
pub const DIST_MIN: f32 = 0.2;
pub const DIST_MAX: f32 = 200.0;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub az: f32,
    pub el: f32,
    pub dist: f32,
    /// The point looked at.
    pub target: [f32; 3],
}

impl Default for Orbit {
    fn default() -> Self {
        Self { az: 35.0, el: 15.0, dist: 4.2, target: [0.0, 0.9, 0.0] }
    }
}

impl Orbit {
    /// Clamp elevation and distance into their limits.
    pub fn clamped(mut self) -> Self {
        self.el = self.el.clamp(EL_MIN, EL_MAX);
        self.dist = self.dist.clamp(DIST_MIN, DIST_MAX);
        self.az = self.az.rem_euclid(360.0);
        self
    }

    pub fn pose(&self) -> CameraPose {
        let (az, el) = (self.az.to_radians(), self.el.to_radians());
        let t = self.target;
        CameraPose {
            eye: [
                t[0] + self.dist * el.cos() * az.sin(),
                t[1] + self.dist * el.sin(),
                t[2] + self.dist * el.cos() * az.cos(),
            ],
            target: t,
            up: [0.0, 1.0, 0.0],
            fov_y_deg: FOV_Y_DEG,
        }
    }

    /// Look at a bounding sphere (`center`, `radius`) so it fills the view
    /// with a margin, keeping azimuth and elevation.
    pub fn framed(self, center: [f32; 3], radius: f32, aspect: f32) -> Self {
        let half_v = (FOV_Y_DEG.to_radians() * 0.5).tan();
        let half = if aspect > 0.0 && aspect < 1.0 { half_v * aspect } else { half_v };
        let dist = radius.max(0.05) / half.atan().sin() * 1.15;
        Self { target: center, dist, ..self }.clamped()
    }

    /// Whether a sphere is entirely outside the view (then a swap re-frames).
    pub fn misses(&self, center: [f32; 3], radius: f32, aspect: f32) -> bool {
        let p = self.pose();
        let f = sub(p.target, p.eye);
        let fl = len(f).max(1e-6);
        let fwd = [f[0] / fl, f[1] / fl, f[2] / fl];
        let to = sub(center, p.eye);
        let depth = dot(to, fwd);
        if depth < -radius {
            return true;
        }
        let half_v = (FOV_Y_DEG.to_radians() * 0.5).tan();
        let half_h = half_v * aspect.max(0.1);
        let lateral = (len(to).powi(2) - depth * depth).max(0.0).sqrt();
        lateral - radius > depth.max(0.0) * half_v.max(half_h) * 1.05
    }
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn len(a: [f32; 3]) -> f32 {
    dot(a, a).sqrt()
}

/// The turntable: the model turns about the vertical axis at `deg_per_s`,
/// counted in fixed ticks from an anchor, so the angle at tick N is the same
/// on every run.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Turntable {
    pub on: bool,
    /// Angle at `anchor_tick`, degrees.
    pub anchor_deg: f32,
    pub anchor_tick: u64,
}

impl Default for Turntable {
    fn default() -> Self {
        Self { on: true, anchor_deg: 0.0, anchor_tick: 0 }
    }
}

impl Turntable {
    /// The angle (degrees, 0..360) at `tick` for a 60 Hz step.
    pub fn angle(&self, tick: u64, deg_per_s: f32) -> f32 {
        if !self.on {
            return self.anchor_deg.rem_euclid(360.0);
        }
        let ticks = tick.saturating_sub(self.anchor_tick) as f64;
        (self.anchor_deg as f64 + ticks * deg_per_s as f64 / 60.0).rem_euclid(360.0) as f32
    }

    /// Switch on or off at `tick`, keeping the current angle.
    pub fn set(&mut self, on: bool, tick: u64, deg_per_s: f32) {
        if on != self.on {
            self.anchor_deg = self.angle(tick, deg_per_s);
            self.anchor_tick = tick;
            self.on = on;
        }
    }
}
