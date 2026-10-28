//! Explosions: damage falls linearly from the centre to the radius,
//! measured to the nearest point of the body's upright axis, and only
//! reaches a body the blast can see (its chest or its head).

use evani_physics::collide::{StaticWorld, V3};

use crate::dist;
use crate::hitscan::{Body, nearest_on_axis};

/// The nearest point of a body's axis to `p`.
pub fn nearest_on_body(b: &Body, p: V3) -> V3 {
    nearest_on_axis(b, p)
}

/// Can a blast at `centre` see the body's chest or head?
pub fn blast_sees(world: &StaticWorld, centre: V3, b: &Body) -> bool {
    let c = [centre[0], centre[1] + 0.05, centre[2]];
    world.line_of_sight(c, b.chest()) || world.line_of_sight(c, b.head_centre())
}

/// Raw damage (before armour) a blast of `damage` and `radius` at `centre`
/// does to a body.
pub fn blast_damage(world: &StaticWorld, centre: V3, damage: f32, radius: f32, b: &Body) -> f32 {
    if !b.alive || radius <= 0.0 {
        return 0.0;
    }
    let d = dist(centre, nearest_on_body(b, centre));
    if d >= radius || !blast_sees(world, centre, b) {
        return 0.0;
    }
    damage * (1.0 - d / radius)
}

#[cfg(test)]
mod tests {
    use super::*;
    use evani_physics::collide::WorldBuilder;

    #[test]
    fn blast_falls_off_with_distance_and_needs_line_of_sight() {
        let mut wb = WorldBuilder::new();
        wb.aabb([-20.0, -0.2, -20.0], [20.0, 0.0, 20.0], 1);
        // A wall between the blast and anyone at x < -2.
        wb.aabb([-2.3, 0.0, -10.0], [-2.0, 3.0, 10.0], 2);
        let w = wb.build();
        let c = [0.0, 0.06, 0.0];
        let at = |x: f32| Body::new([x, 0.0, 0.0], 1.372, 1.219);
        let near = blast_damage(&w, c, 98.0, 6.5, &at(1.0));
        let mid = blast_damage(&w, c, 98.0, 6.5, &at(4.0));
        let far = blast_damage(&w, c, 98.0, 6.5, &at(7.0));
        assert!((near - 98.0 * (1.0 - 1.0 / 6.5)).abs() < 1e-3, "{near}");
        assert!(near > mid && mid > 0.0 && far == 0.0, "{near} {mid} {far}");
        // The same distance behind the wall: nothing.
        let hidden = blast_damage(&w, c, 98.0, 6.5, &at(-4.0));
        assert_eq!(hidden, 0.0);
        let dead = Body {
            alive: false,
            ..at(1.0)
        };
        assert_eq!(blast_damage(&w, c, 98.0, 6.5, &dead), 0.0);
    }
}
