//! Routes over any graph with integer edge costs (a nav grid, a waypoint
//! net): Dijkstra that keeps parents, then a string pull that drops every
//! point a straight walk can skip.

use std::cmp::Reverse;
use std::collections::BinaryHeap;

use evani_physics::collide::V3;

/// A graph of numbered nodes with weighted links.
pub trait Graph {
    /// How many nodes.
    fn node_count(&self) -> usize;
    /// Links out of a node: `(to, cost)`.
    fn links(&self, node: u32) -> &[(u32, u32)];
}

/// The cheapest node path from `from` to `to` (both ends included), or
/// `None` when `to` cannot be reached. Ties break by node number, so the
/// same graph always gives the same path.
pub fn shortest_path<G: Graph + ?Sized>(g: &G, from: u32, to: u32) -> Option<Vec<u32>> {
    let n = g.node_count();
    if from as usize >= n || to as usize >= n {
        return None;
    }
    let mut cost = vec![u32::MAX; n];
    let mut parent = vec![u32::MAX; n];
    cost[from as usize] = 0;
    let mut heap = BinaryHeap::new();
    heap.push(Reverse((0u32, from)));
    while let Some(Reverse((c, node))) = heap.pop() {
        if node == to {
            break;
        }
        if c > cost[node as usize] {
            continue;
        }
        for &(m, w) in g.links(node) {
            let Some(&old) = cost.get(m as usize) else {
                continue;
            };
            let nc = c.saturating_add(w);
            if nc < old {
                cost[m as usize] = nc;
                parent[m as usize] = node;
                heap.push(Reverse((nc, m)));
            }
        }
    }
    if cost[to as usize] == u32::MAX {
        return None;
    }
    let mut out = vec![to];
    let mut at = to;
    while at != from {
        at = parent[at as usize];
        if at == u32::MAX || out.len() > n {
            return None;
        }
        out.push(at);
    }
    out.reverse();
    Some(out)
}

/// Pull a polyline tight: from each kept point, skip ahead while `clear`
/// says a straight walk reaches the next point too. The ends stay.
pub fn smooth(points: &[V3], clear: impl Fn(V3, V3) -> bool) -> Vec<V3> {
    if points.len() <= 2 {
        return points.to_vec();
    }
    let mut out = vec![points[0]];
    let mut i = 0;
    while i + 1 < points.len() {
        let mut k = i + 1;
        while k + 1 < points.len() && clear(points[i], points[k + 1]) {
            k += 1;
        }
        out.push(points[k]);
        i = k;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 5 x 5 grid with a wall at x = 2 except at z = 4.
    struct Grid(Vec<Vec<(u32, u32)>>);
    impl Graph for Grid {
        fn node_count(&self) -> usize {
            self.0.len()
        }
        fn links(&self, n: u32) -> &[(u32, u32)] {
            &self.0[n as usize]
        }
    }
    fn grid() -> Grid {
        let open =
            |x: i32, z: i32| (0..5).contains(&x) && (0..5).contains(&z) && (x != 2 || z == 4);
        let mut g = vec![Vec::new(); 25];
        for z in 0..5 {
            for x in 0..5 {
                if !open(x, z) {
                    continue;
                }
                for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                    if open(x + dx, z + dz) {
                        g[(z * 5 + x) as usize].push((((z + dz) * 5 + x + dx) as u32, 10));
                    }
                }
            }
        }
        Grid(g)
    }

    #[test]
    fn path_goes_round_the_wall() {
        let g = grid();
        let p = shortest_path(&g, 0, 4).unwrap();
        assert_eq!(p.first(), Some(&0));
        assert_eq!(p.last(), Some(&4));
        assert!(p.contains(&22), "through the gap: {p:?}");
        assert_eq!(p.len(), 13);
        assert_eq!(shortest_path(&g, 0, 12), None);
        assert_eq!(shortest_path(&g, 0, 0), Some(vec![0]));
    }

    #[test]
    fn smooth_drops_points_on_a_straight_run() {
        let pts: Vec<V3> = (0..10).map(|i| [i as f32, 0.0, 0.0]).collect();
        assert_eq!(smooth(&pts, |_, _| true), vec![pts[0], pts[9]]);
        assert_eq!(smooth(&pts, |_, _| false), pts);
    }
}
