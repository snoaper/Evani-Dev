//! Footprint geometry for the support rule: the convex hull of the held
//! points and how far the centre of mass lies outside it.

/// Convex hull of `pts` (counter-clockwise, collinear points dropped;
/// monotone chain). Sorts `pts` in place.
pub fn hull(pts: &mut [[f32; 2]], out: &mut Vec<[f32; 2]>) {
    out.clear();
    pts.sort_by(|a, b| a[0].total_cmp(&b[0]).then(a[1].total_cmp(&b[1])));
    if pts.len() < 3 {
        out.extend_from_slice(pts);
        return;
    }
    let cross = |o: [f32; 2], a: [f32; 2], b: [f32; 2]| {
        (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])
    };
    for &p in pts.iter() {
        while out.len() >= 2 && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
            out.pop();
        }
        out.push(p);
    }
    let lower = out.len() + 1;
    for &p in pts.iter().rev().skip(1) {
        while out.len() >= lower && cross(out[out.len() - 2], out[out.len() - 1], p) <= 0.0 {
            out.pop();
        }
        out.push(p);
    }
    out.pop();
}

/// How far `p` lies outside convex polygon `h` (ccw; 1 or 2 points allowed),
/// and the nearest point on it: `(distance, nearest)`. Inside: `(0, p)`.
pub fn outside(h: &[[f32; 2]], p: [f32; 2]) -> (f32, [f32; 2]) {
    let n = h.len();
    if n >= 3 {
        let inside = (0..n).all(|i| {
            let (a, b) = (h[i], h[(i + 1) % n]);
            (b[0] - a[0]) * (p[1] - a[1]) - (b[1] - a[1]) * (p[0] - a[0]) >= -1e-6
        });
        if inside {
            return (0.0, p);
        }
    }
    let mut best = (f32::MAX, p);
    for i in 0..n {
        let (a, b) = (h[i], h[(i + 1) % n]);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let l2 = dx * dx + dy * dy;
        let t = if l2 > 0.0 {
            (((p[0] - a[0]) * dx + (p[1] - a[1]) * dy) / l2).clamp(0.0, 1.0)
        } else {
            0.0
        };
        let q = [a[0] + dx * t, a[1] + dy * t];
        let d = (p[0] - q[0]).hypot(p[1] - q[1]);
        if d < best.0 {
            best = (d, q);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hull_of_a_square_with_inner_points() {
        let mut pts = vec![
            [0.0, 0.0],
            [1.0, 0.0],
            [1.0, 1.0],
            [0.0, 1.0],
            [0.5, 0.5],
            [0.5, 0.0],
        ];
        let mut h = Vec::new();
        hull(&mut pts, &mut h);
        assert_eq!(h.len(), 4);
        assert_eq!(outside(&h, [0.5, 0.5]).0, 0.0);
        let (d, q) = outside(&h, [2.0, 0.5]);
        assert!((d - 1.0).abs() < 1e-6 && (q[0] - 1.0).abs() < 1e-6 && (q[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn degenerate_hulls() {
        let mut h = Vec::new();
        hull(&mut [[1.0, 1.0]], &mut h);
        assert!((outside(&h, [1.0, 2.0]).0 - 1.0).abs() < 1e-6);
        hull(&mut [], &mut h);
        assert_eq!(outside(&h, [0.0, 0.0]).0, f32::MAX);
    }
}
