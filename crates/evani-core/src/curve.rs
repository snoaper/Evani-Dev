//! Piecewise-linear curves with editable control points.
//!
//! Used for CPU-side formulas (LOD distances, camera smoothing, adaptive
//! quality thresholds). Evaluation is a binary search plus a lerp, no
//! allocation. Outside the first/last point the curve is flat (clamped).

use serde::{Deserialize, Serialize};

/// Maximum control points per curve (keeps editing and JSON bounded).
pub const MAX_POINTS: usize = 64;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    /// Control points `[x, y]`, strictly increasing in x.
    points: Vec<[f32; 2]>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CurveError {
    Empty,
    TooManyPoints,
    NotFinite,
    /// x values must be strictly increasing.
    Unsorted,
    BadIndex,
}

impl core::fmt::Display for CurveError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            CurveError::Empty => "a curve needs at least one point",
            CurveError::TooManyPoints => "too many control points (max 64)",
            CurveError::NotFinite => "control points must be finite",
            CurveError::Unsorted => "x values must be strictly increasing",
            CurveError::BadIndex => "no such control point",
        })
    }
}

impl Curve {
    /// Build from points; they are sorted by x. Duplicate x is an error.
    pub fn new(points: &[[f32; 2]]) -> Result<Self, CurveError> {
        let mut pts = points.to_vec();
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        let c = Self { points: pts };
        c.validate()?;
        Ok(c)
    }

    /// Constant curve.
    pub fn constant(y: f32) -> Self {
        Self {
            points: vec![[0.0, y]],
        }
    }

    /// Straight line through two points.
    pub fn linear(x0: f32, y0: f32, x1: f32, y1: f32) -> Result<Self, CurveError> {
        Self::new(&[[x0, y0], [x1, y1]])
    }

    pub fn validate(&self) -> Result<(), CurveError> {
        if self.points.is_empty() {
            return Err(CurveError::Empty);
        }
        if self.points.len() > MAX_POINTS {
            return Err(CurveError::TooManyPoints);
        }
        if self
            .points
            .iter()
            .any(|p| !p[0].is_finite() || !p[1].is_finite())
        {
            return Err(CurveError::NotFinite);
        }
        if self.points.windows(2).any(|w| w[0][0] >= w[1][0]) {
            return Err(CurveError::Unsorted);
        }
        Ok(())
    }

    pub fn points(&self) -> &[[f32; 2]] {
        &self.points
    }

    /// Evaluate at `x` (flat outside the end points; NaN input gives the
    /// first point's y).
    pub fn eval(&self, x: f32) -> f32 {
        let p = &self.points;
        let Some(first) = p.first() else {
            return 0.0;
        };
        if x.is_nan() || x <= first[0] {
            return first[1];
        }
        let last = p[p.len() - 1];
        if x >= last[0] {
            return last[1];
        }
        // First index with point.x > x; it is >= 1 here.
        let hi = p.partition_point(|q| q[0] <= x);
        let a = p[hi - 1];
        let b = p[hi];
        let t = (x - a[0]) / (b[0] - a[0]);
        a[1] + (b[1] - a[1]) * t
    }

    /// Move one control point, keeping x order. On error nothing changes.
    pub fn set_point(&mut self, index: usize, point: [f32; 2]) -> Result<(), CurveError> {
        if index >= self.points.len() {
            return Err(CurveError::BadIndex);
        }
        let old = self.points[index];
        self.points[index] = point;
        if let Err(e) = self.validate() {
            self.points[index] = old;
            return Err(e);
        }
        Ok(())
    }

    /// Insert a point at its sorted position; returns its index.
    pub fn insert_point(&mut self, point: [f32; 2]) -> Result<usize, CurveError> {
        if self.points.len() >= MAX_POINTS {
            return Err(CurveError::TooManyPoints);
        }
        if !point[0].is_finite() || !point[1].is_finite() {
            return Err(CurveError::NotFinite);
        }
        let i = self.points.partition_point(|q| q[0] < point[0]);
        if self.points.get(i).is_some_and(|q| q[0] == point[0]) {
            return Err(CurveError::Unsorted);
        }
        self.points.insert(i, point);
        Ok(i)
    }

    /// Remove a point (the last remaining point cannot be removed).
    pub fn remove_point(&mut self, index: usize) -> Result<[f32; 2], CurveError> {
        if index >= self.points.len() {
            return Err(CurveError::BadIndex);
        }
        if self.points.len() == 1 {
            return Err(CurveError::Empty);
        }
        Ok(self.points.remove(index))
    }

    pub fn from_json(json: &str) -> Result<Self, String> {
        let c: Curve = serde_json::from_str(json).map_err(|e| e.to_string())?;
        c.validate().map_err(|e| e.to_string())?;
        Ok(c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eval_interpolates_and_clamps() {
        let c = Curve::new(&[[10.0, 0.0], [0.0, 1.0], [20.0, 2.0]]).unwrap();
        assert_eq!(c.points()[0], [0.0, 1.0]);
        assert_eq!(c.eval(-5.0), 1.0);
        assert_eq!(c.eval(0.0), 1.0);
        assert_eq!(c.eval(5.0), 0.5);
        assert_eq!(c.eval(10.0), 0.0);
        assert_eq!(c.eval(15.0), 1.0);
        assert_eq!(c.eval(20.0), 2.0);
        assert_eq!(c.eval(1e9), 2.0);
        assert_eq!(c.eval(f32::NAN), 1.0);
        assert_eq!(Curve::constant(3.0).eval(100.0), 3.0);
    }

    #[test]
    fn rejects_bad_curves() {
        assert_eq!(Curve::new(&[]), Err(CurveError::Empty));
        assert_eq!(
            Curve::new(&[[0.0, 0.0], [0.0, 1.0]]),
            Err(CurveError::Unsorted)
        );
        assert_eq!(Curve::new(&[[f32::NAN, 0.0]]), Err(CurveError::NotFinite));
        let many: Vec<[f32; 2]> = (0..65).map(|i| [i as f32, 0.0]).collect();
        assert_eq!(Curve::new(&many), Err(CurveError::TooManyPoints));
        assert!(Curve::from_json(r#"{"points":[[1,0],[0,1]]}"#).is_err());
        assert!(Curve::from_json(r#"{"points":[[0,0],[1,1]]}"#).is_ok());
    }

    #[test]
    fn editing_keeps_order() {
        let mut c = Curve::linear(0.0, 0.0, 10.0, 10.0).unwrap();
        assert_eq!(c.insert_point([5.0, 0.0]).unwrap(), 1);
        assert_eq!(c.eval(5.0), 0.0);
        assert!(c.insert_point([5.0, 1.0]).is_err());
        // Moving point 1 past point 2 is rejected and leaves the curve intact.
        assert_eq!(c.set_point(1, [11.0, 0.0]), Err(CurveError::Unsorted));
        assert_eq!(c.points()[1], [5.0, 0.0]);
        c.set_point(1, [5.0, 5.0]).unwrap();
        assert_eq!(c.eval(2.5), 2.5);
        c.remove_point(1).unwrap();
        c.remove_point(0).unwrap();
        assert_eq!(c.remove_point(0), Err(CurveError::Empty));
        assert_eq!(c.set_point(3, [0.0, 0.0]), Err(CurveError::BadIndex));
    }

    #[test]
    fn json_round_trip() {
        let c = Curve::new(&[[0.0, 1.0], [1.0, 2.0]]).unwrap();
        let s = serde_json::to_string(&c).unwrap();
        assert_eq!(Curve::from_json(&s).unwrap(), c);
    }
}
