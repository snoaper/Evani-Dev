//! Hand-rolled sparse-set ECS.
//!
//! * [`Entity`] = index + generation, LIFO free list.
//! * One [`SparseSet`] per component type; removal is `swap_remove`.
//! * Queries over several components iterate the smallest set and probe the
//!   others ([`World::query`]).
//! * [`Schedule`] runs named systems in `(stage, order, insertion)` order.
//!
//! Nothing here iterates a hash map, so simulation order is deterministic.

mod entity;
mod query;
mod schedule;
mod sparse;
mod world;

pub use entity::{Entities, Entity};
pub use query::{Query, QueryData};
pub use schedule::{Schedule, Stage, SystemDesc, SystemFn};
pub use sparse::SparseSet;
pub use world::{Commands, Component, World};

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Pos(f32, f32);
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Vel(f32, f32);
    #[derive(Debug, Clone, Copy, PartialEq)]
    struct Tag;

    /// The pieces a kit needs to keep its gameplay objects in the ECS
    /// (Gate 0 question 2): pairs, ordered lists, counts and a reset.
    #[test]
    fn kit_objects_pairs_order_and_reset() {
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct Kart {
            slot: u8,
            speed: f32,
        }
        #[derive(Debug, Clone, Copy, PartialEq)]
        struct RaceObject;
        let mut w = World::new();
        let karts: Vec<Entity> = (0..4u8)
            .map(|slot| {
                let e = w.spawn_with(Kart { slot, speed: 10.0 * f32::from(slot) });
                w.insert(e, RaceObject);
                e
            })
            .collect();
        let goo = w.spawn_with(RaceObject);
        assert_eq!(w.count::<Kart>(), 4);
        assert_eq!(w.count::<RaceObject>(), 5);
        // Pairs: kart 1 bumps kart 3, both change.
        {
            let (mut a, mut b) = w.get2_mut::<Kart>(karts[1], karts[3]).unwrap();
            core::mem::swap(&mut a.speed, &mut b.speed);
        }
        assert_eq!(w.get::<Kart>(karts[1]).unwrap().speed, 30.0);
        assert_eq!(w.get::<Kart>(karts[3]).unwrap().speed, 10.0);
        assert!(w.get2_mut::<Kart>(karts[2], karts[2]).is_none());
        assert!(w.get2_mut::<Kart>(karts[2], goo).is_none());
        // Despawning reorders the dense list (swap-remove); the slot order
        // stays.
        w.despawn(karts[0]);
        let dense: Vec<u8> = w.entities_with::<Kart>().iter().map(|&e| w.get::<Kart>(e).unwrap().slot).collect();
        assert_eq!(dense, [3, 1, 2]);
        let by_slot: Vec<u8> = w
            .sorted_entities::<Kart, _>(|k| k.slot)
            .iter()
            .map(|&e| w.get::<Kart>(e).unwrap().slot)
            .collect();
        assert_eq!(by_slot, [1, 2, 3]);
        // Reset: every race object goes, other entities stay.
        let keep = w.spawn_with(Pos(0.0, 0.0));
        assert_eq!(w.despawn_all::<RaceObject>(), 4);
        assert_eq!(w.count::<Kart>(), 0);
        assert_eq!(w.entity_count(), 1);
        assert!(w.is_alive(keep));
        // Same operations, same order and ids: deterministic.
        let run = || {
            let mut w = World::new();
            let es: Vec<Entity> = (0..6).map(|i| w.spawn_with(Pos(i as f32, 0.0))).collect();
            w.despawn(es[2]);
            w.despawn(es[4]);
            w.spawn_with(Pos(9.0, 0.0));
            let mut v = Vec::new();
            w.query::<&Pos>().for_each(|e, p| v.push((e.to_bits(), p.0)));
            v
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn spawn_despawn_generations() {
        let mut w = World::new();
        let a = w.spawn();
        let b = w.spawn();
        assert!(w.despawn(a));
        assert!(!w.despawn(a));
        let c = w.spawn();
        assert_eq!(c.index(), a.index());
        assert_ne!(c.generation(), a.generation());
        assert!(!w.is_alive(a));
        assert!(w.is_alive(b) && w.is_alive(c));
        assert_eq!(w.entity_count(), 2);
        assert_eq!(Entity::from_bits(c.to_bits()), c);
    }

    #[test]
    fn sparse_set_swap_remove_keeps_lookup() {
        let mut ents = Entities::default();
        let es: Vec<Entity> = (0..5).map(|_| ents.spawn()).collect();
        let mut s = SparseSet::default();
        for (i, &e) in es.iter().enumerate() {
            s.insert(e, i as i32);
        }
        assert_eq!(s.remove(es[1]), Some(1));
        assert_eq!(s.len(), 4);
        // Last element moved into slot 1.
        assert_eq!(s.entities()[1], es[4]);
        for (i, &e) in es.iter().enumerate() {
            if i == 1 {
                assert_eq!(s.get(e), None);
            } else {
                assert_eq!(s.get(e), Some(&(i as i32)));
            }
        }
        assert_eq!(s.insert(es[0], 99), Some(0));
        s.clear();
        assert!(s.is_empty() && s.get(es[0]).is_none());
    }

    #[test]
    fn stale_entity_does_not_see_new_components() {
        let mut w = World::new();
        let a = w.spawn_with(Pos(1.0, 1.0));
        w.despawn(a);
        let b = w.spawn_with(Pos(2.0, 2.0));
        assert_eq!(a.index(), b.index());
        assert!(w.get::<Pos>(a).is_none());
        assert_eq!(*w.get::<Pos>(b).unwrap(), Pos(2.0, 2.0));
        assert!(!w.insert(a, Vel(0.0, 0.0)));
    }

    #[test]
    fn multi_component_query_and_mutation() {
        let mut w = World::new();
        w.register_component::<Pos>("pos");
        let mut moving = Vec::new();
        for i in 0..10 {
            let e = w.spawn_with(Pos(i as f32, 0.0));
            if i % 2 == 0 {
                w.insert(e, Vel(1.0, 2.0));
                moving.push(e);
            }
        }
        let mut n = 0;
        w.query::<(&mut Pos, &Vel)>().for_each(|_, (p, v)| {
            p.0 += v.0;
            p.1 += v.1;
            n += 1;
        });
        assert_eq!(n, 5);
        for e in moving {
            assert_eq!(w.get::<Pos>(e).unwrap().1, 2.0);
        }
        assert_eq!(w.query::<(&Pos, &Vel, &Tag)>().count(), 0);
        assert_eq!(w.query::<&Pos>().count(), 10);
        let mut names = Vec::new();
        w.component_stats(|name, len| names.push((name, len)));
        assert_eq!(names[0], ("pos", 10));
    }

    #[test]
    fn query_order_is_deterministic() {
        fn build() -> Vec<u32> {
            let mut w = World::new();
            let es: Vec<Entity> = (0..50).map(|i| w.spawn_with(i as u32)).collect();
            for e in es.iter().step_by(3) {
                w.despawn(*e);
            }
            for i in 0..10 {
                w.spawn_with(100 + i as u32);
            }
            let mut out = Vec::new();
            w.query::<&u32>().for_each(|_, v| out.push(*v));
            out
        }
        assert_eq!(build(), build());
    }

    #[test]
    fn commands_apply_after_query() {
        let mut w = World::new();
        for i in 0..6 {
            w.spawn_with::<i32>(i);
        }
        w.query::<&i32>().for_each(|e, v| {
            if v % 2 == 1 {
                w.commands().despawn(e);
            }
        });
        w.commands().add(|w| {
            w.spawn_with(42i32);
        });
        w.apply_commands();
        let mut vals = Vec::new();
        w.query::<&i32>().for_each(|_, v| vals.push(*v));
        vals.sort();
        assert_eq!(vals, vec![0, 2, 4, 42]);
    }

    #[test]
    fn resources_borrow_independently() {
        let mut w = World::new();
        w.insert_resource(5u32);
        w.insert_resource(String::from("x"));
        {
            let a = w.res::<u32>();
            let mut b = w.res_mut::<String>();
            b.push_str(&a.to_string());
        }
        assert_eq!(&*w.res::<String>(), "x5");
        assert!(w.try_res::<f64>().is_none());
        *w.res_get_mut::<u32>().unwrap() += 1;
        assert_eq!(w.remove_resource::<u32>(), Some(6));
    }

    #[test]
    fn schedule_runs_in_stage_then_order_then_insertion() {
        let mut w = World::new();
        w.insert_resource(Vec::<&'static str>::new());
        let mut s = Schedule::default();
        let log = |name: &'static str| {
            move |w: &mut World| w.res_mut::<Vec<&'static str>>().push(name)
        };
        s.add(SystemDesc::new("late", Stage::PostUpdate, log("late")));
        s.add(SystemDesc::new("b", Stage::Update, log("b")).order(5));
        s.add(SystemDesc::new("a", Stage::Update, log("a")).order(5));
        s.add(SystemDesc::new("first", Stage::Update, log("first")).order(-1));
        s.add(SystemDesc::new("frame", Stage::Frame, log("frame")));
        s.add(SystemDesc::new("off", Stage::Update, log("off")));
        assert!(s.set_enabled("off", false));
        s.run_fixed(&mut w);
        assert_eq!(*w.res::<Vec<&str>>(), vec!["first", "b", "a", "late"]);
        s.run_stage(Stage::Frame, &mut w);
        assert_eq!(w.res::<Vec<&str>>().last(), Some(&"frame"));
    }
}
