//! Deterministic camera paths. A [`CameraPath`] maps simulation time to a
//! camera pose with no state, so the sandbox orbit, `bench.run`,
//! `capture.frame` and `compare.ab` all see exactly the same frames for the
//! same sim time.

/// Where the camera is and what it looks at.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPose {
    pub eye: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    /// Vertical field of view in degrees.
    pub fov_y_deg: f32,
}

impl Default for CameraPose {
    fn default() -> Self {
        Self {
            eye: [0.0, 6.0, 18.0],
            target: [0.0, 0.0, 0.0],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: 50.0,
        }
    }
}

/// A slow orbit around a centre with a gentle height bob.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraPath {
    pub center: [f32; 3],
    pub radius: f32,
    /// Eye height above the centre.
    pub height: f32,
    /// Seconds per full orbit (negative turns the other way; 0 = still).
    pub period_s: f32,
    /// Height bob amplitude and period.
    pub bob: f32,
    pub bob_period_s: f32,
    /// Start angle in radians.
    pub phase: f32,
    /// Look-at point height above the centre.
    pub look_height: f32,
    pub fov_y_deg: f32,
}

impl CameraPath {
    pub fn orbit(center: [f32; 3], radius: f32, height: f32, period_s: f32) -> Self {
        Self {
            center,
            radius,
            height,
            period_s,
            bob: 0.0,
            bob_period_s: 0.0,
            phase: 0.0,
            look_height: 0.0,
            fov_y_deg: 50.0,
        }
    }

    pub fn with_bob(mut self, amplitude: f32, period_s: f32) -> Self {
        self.bob = amplitude;
        self.bob_period_s = period_s;
        self
    }

    /// The pose at `sim_seconds` (use [`crate::time::Time::render_seconds`]
    /// for smooth motion between ticks). A pure function of its inputs.
    pub fn pose(&self, sim_seconds: f64) -> CameraPose {
        let turn = |period: f32| -> f64 {
            if period == 0.0 {
                0.0
            } else {
                // Wrap in f64 first so long runs keep full precision.
                (sim_seconds / f64::from(period)).rem_euclid(1.0) * core::f64::consts::TAU
            }
        };
        let a = (turn(self.period_s) + f64::from(self.phase)) as f32;
        let b = turn(self.bob_period_s) as f32;
        let [cx, cy, cz] = self.center;
        CameraPose {
            eye: [
                cx + self.radius * a.sin(),
                cy + self.height + self.bob * b.sin(),
                cz + self.radius * a.cos(),
            ],
            target: [cx, cy + self.look_height, cz],
            up: [0.0, 1.0, 0.0],
            fov_y_deg: self.fov_y_deg,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_orbit() {
        let p = CameraPath::orbit([0.0, 0.0, 0.0], 10.0, 4.0, 40.0).with_bob(1.0, 10.0);
        let a = p.pose(0.0);
        assert!((a.eye[2] - 10.0).abs() < 1e-5 && (a.eye[1] - 4.0).abs() < 1e-5);
        let q = p.pose(10.0); // quarter turn
        assert!((q.eye[0] - 10.0).abs() < 1e-4, "{q:?}");
        assert_eq!(p.pose(123.456), p.pose(123.456));
        // A full period later the pose repeats.
        let r = p.pose(40.0 + 3.0);
        let s = p.pose(3.0);
        for i in 0..3 {
            assert!((r.eye[i] - s.eye[i]).abs() < 1e-4);
        }
        let still = CameraPath::orbit([1.0, 2.0, 3.0], 5.0, 1.0, 0.0);
        assert_eq!(still.pose(0.0), still.pose(99.0));
    }
}
