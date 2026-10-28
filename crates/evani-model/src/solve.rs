//! Constraint solving (docs/model_research/06): relationships solved in
//! dependency order, contact found on the actual field, and our own
//! Levenberg-Marquardt for what is left (cycles, inequalities such as
//! "fit inside this box"). Fixed iteration counts and a fixed operation
//! order make every solve deterministic.

use crate::sdf::Node;
use crate::vec::*;

/// Order nodes so every node comes after the ones it depends on. Returns the
/// order and the nodes caught in cycles (solved numerically).
pub fn dependency_order(n: usize, deps: &[Vec<usize>]) -> (Vec<usize>, Vec<usize>) {
    let mut state = vec![0u8; n]; // 0 new, 1 on stack, 2 done
    let mut order = Vec::with_capacity(n);
    let mut cyclic = Vec::new();
    fn visit(v: usize, deps: &[Vec<usize>], state: &mut [u8], order: &mut Vec<usize>, cyclic: &mut Vec<usize>) {
        if state[v] == 2 {
            return;
        }
        if state[v] == 1 {
            cyclic.push(v);
            return;
        }
        state[v] = 1;
        for &d in &deps[v] {
            visit(d, deps, state, order, cyclic);
        }
        state[v] = 2;
        order.push(v);
    }
    for v in 0..n {
        visit(v, deps, &mut state, &mut order, &mut cyclic);
    }
    cyclic.sort_unstable();
    cyclic.dedup();
    (order, cyclic)
}

/// Slide a part along `dir` (unit) until its surface touches `target`'s,
/// overlapping by `embed`. `samples` are points on the part's surface at its
/// current place. Returns the distance moved along `dir` (negative = back).
/// Exact field queries only: the move is the smallest target distance
/// measured from the samples along `dir` by sphere tracing.
pub fn contact_shift(samples: &[V3], target: &Node, dir: V3, embed: f64) -> Option<f64> {
    let mut best: Option<f64> = None;
    for &p in samples {
        // March along dir from p until the target surface is reached.
        let mut t = 0.0;
        let start = target.dist(p);
        if start < 0.0 {
            // Already inside: march backwards to the surface.
            let mut tb = 0.0;
            let mut d = start;
            for _ in 0..200 {
                if d.abs() < 1e-7 {
                    break;
                }
                tb += d; // d < 0 moves backwards
                d = target.dist(add(p, mul(dir, tb)));
                if (0.0..1e-7).contains(&d) {
                    break;
                }
            }
            best = Some(best.map_or(tb, |b: f64| b.min(tb)));
            continue;
        }
        let mut hit = false;
        for _ in 0..400 {
            let d = target.dist(add(p, mul(dir, t)));
            if d < 1e-7 {
                hit = true;
                break;
            }
            t += d;
            if t > 50.0 {
                break;
            }
        }
        if hit {
            best = Some(best.map_or(t, |b: f64| b.min(t)));
        }
    }
    best.map(|b| b + embed)
}

/// Levenberg-Marquardt over `x` with box bounds: minimise the sum of squared
/// residuals. Jacobian by central differences with step `h_i`; a fixed
/// number of iterations; damping raised on a worse step, lowered on a
/// better one. Deterministic for a deterministic `f`.
pub fn levenberg_marquardt(f: &dyn Fn(&[f64]) -> Vec<f64>, x0: &[f64], lo: &[f64], hi: &[f64], h: &[f64], iters: usize) -> (Vec<f64>, f64) {
    let n = x0.len();
    let clampv = |x: &mut [f64]| {
        for i in 0..n {
            x[i] = x[i].clamp(lo[i], hi[i]);
        }
    };
    let mut x = x0.to_vec();
    clampv(&mut x);
    let cost = |r: &[f64]| r.iter().map(|v| v * v).sum::<f64>();
    let mut r = f(&x);
    let mut c = cost(&r);
    let mut lambda = 1e-3;
    for _ in 0..iters {
        if c < 1e-24 {
            break;
        }
        let m = r.len();
        // Jacobian (m x n).
        let mut j = vec![vec![0.0; n]; m];
        for k in 0..n {
            let (mut xp, mut xm) = (x.clone(), x.clone());
            xp[k] = (x[k] + h[k]).min(hi[k]);
            xm[k] = (x[k] - h[k]).max(lo[k]);
            let span = xp[k] - xm[k];
            if span <= 0.0 {
                continue;
            }
            let (rp, rm) = (f(&xp), f(&xm));
            for i in 0..m {
                j[i][k] = (rp[i] - rm[i]) / span;
            }
        }
        // Normal equations (J^T J + lambda diag) dx = -J^T r.
        let mut a = vec![vec![0.0; n]; n];
        let mut g = vec![0.0; n];
        for i in 0..m {
            for p in 0..n {
                g[p] += j[i][p] * r[i];
                for q in 0..n {
                    a[p][q] += j[i][p] * j[i][q];
                }
            }
        }
        let mut improved = false;
        for _ in 0..8 {
            let mut aa = a.clone();
            for (p, row) in aa.iter_mut().enumerate() {
                row[p] += lambda * (a[p][p] + 1e-12);
            }
            let rhs: Vec<f64> = g.iter().map(|v| -v).collect();
            let Some(dx) = solve_dense(aa, rhs) else {
                lambda *= 10.0;
                continue;
            };
            let mut xn: Vec<f64> = x.iter().zip(&dx).map(|(a, b)| a + b).collect();
            clampv(&mut xn);
            let rn = f(&xn);
            let cn = cost(&rn);
            if cn < c {
                x = xn;
                r = rn;
                c = cn;
                lambda = (lambda * 0.3).max(1e-9);
                improved = true;
                break;
            }
            lambda *= 10.0;
        }
        if !improved {
            break;
        }
    }
    (x, c)
}

/// Gaussian elimination with partial pivoting (rows in order).
pub fn solve_dense(mut a: Vec<Vec<f64>>, mut b: Vec<f64>) -> Option<Vec<f64>> {
    let n = b.len();
    for col in 0..n {
        let mut piv = col;
        for r in col + 1..n {
            if a[r][col].abs() > a[piv][col].abs() {
                piv = r;
            }
        }
        if a[piv][col].abs() < 1e-300 {
            return None;
        }
        a.swap(col, piv);
        b.swap(col, piv);
        for r in col + 1..n {
            let f = a[r][col] / a[col][col];
            if f == 0.0 {
                continue;
            }
            for c in col..n {
                a[r][c] -= f * a[col][c];
            }
            b[r] -= f * b[col];
        }
    }
    let mut x = vec![0.0; n];
    for i in (0..n).rev() {
        let mut s = b[i];
        for j in i + 1..n {
            s -= a[i][j] * x[j];
        }
        x[i] = s / a[i][i];
    }
    Some(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_puts_dependencies_first_and_finds_cycles() {
        // 0 <- 1 <- 2, and 3 <-> 4.
        let deps = vec![vec![], vec![0], vec![1], vec![4], vec![3]];
        let (order, cyc) = dependency_order(5, &deps);
        let pos = |v: usize| order.iter().position(|&x| x == v).unwrap();
        assert!(pos(0) < pos(1) && pos(1) < pos(2));
        assert_eq!(cyc, vec![3]);
    }

    #[test]
    fn contact_closes_the_gap_exactly() {
        // A box resting on a ground slab: samples on its bottom face 0.3 m up.
        let slab = Node::RoundBox { half: [2.0, 0.1, 2.0], r: 0.0 };
        let samples: Vec<V3> = (0..5).map(|i| [-0.2 + 0.1 * i as f64, 0.4, 0.0]).collect();
        let s = contact_shift(&samples, &slab, [0.0, -1.0, 0.0], 0.0).unwrap();
        assert!((s - 0.3).abs() < 1e-6, "{s}");
        // Starting inside: a negative move backs out.
        let inside: Vec<V3> = vec![[0.0, 0.05, 0.0]];
        let s = contact_shift(&inside, &slab, [0.0, -1.0, 0.0], 0.0).unwrap();
        assert!((s + 0.05).abs() < 1e-6, "{s}");
        // Surface-aware: on a dome the lowest contact is found, not the box top.
        let dome = Node::Sphere { r: 1.0 };
        let s = contact_shift(&[[0.5, 2.0, 0.0], [0.0, 2.0, 0.0]], &dome, [0.0, -1.0, 0.0], 0.0).unwrap();
        assert!((s - 1.0).abs() < 1e-6, "{s}");
    }

    #[test]
    fn lm_solves_a_small_cycle_and_respects_bounds() {
        // Three parts on a line, each attached to the next and the last to the
        // first with a gap: x1 - x0 = 1, x2 - x1 = 1, x0 - x2 = -2 (consistent),
        // plus x0 = 0.5; then a bound that stops x2 at 2.2.
        let f = |x: &[f64]| vec![x[1] - x[0] - 1.0, x[2] - x[1] - 1.0, x[0] - x[2] + 2.0, x[0] - 0.5];
        let (x, c) = levenberg_marquardt(&f, &[0.0, 0.0, 0.0], &[-10.0; 3], &[10.0; 3], &[1e-4; 3], 50);
        assert!(c < 1e-16, "{c}");
        assert!((x[0] - 0.5).abs() < 1e-7 && (x[1] - 1.5).abs() < 1e-7 && (x[2] - 2.5).abs() < 1e-7, "{x:?}");
        let (x, _) = levenberg_marquardt(&f, &[0.0, 0.0, 0.0], &[-10.0; 3], &[10.0, 10.0, 2.2], &[1e-4; 3], 50);
        assert!(x[2] <= 2.2 + 1e-12);
        // Repeatable.
        let (y, _) = levenberg_marquardt(&f, &[0.0, 0.0, 0.0], &[-10.0; 3], &[10.0, 10.0, 2.2], &[1e-4; 3], 50);
        assert_eq!(x, y);
    }
}
