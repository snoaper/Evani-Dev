//! The player's game objects in the ECS: the hero, every enemy, every coin
//! still in the level, every checkpoint flag, every moving platform and every
//! short-lived pop (score, dust, sparkle) is an entity. Each carries a
//! [`Slot`] (its index in the level's list of that kind) so the rules always
//! walk them in the same order, whatever the storage order.
//!
//! The rules ([`crate::game::Game`]) work on plain vectors of those objects
//! for speed, unit tests, the attract recording and the bot's look-ahead
//! clones. For the player's game the vectors are a view of the entities:
//! [`Objects::gather`] rebuilds them from the entities at the start of every
//! fixed step and [`Objects::sync`] writes every change back at its end (a
//! coin that is taken despawns, a reset spawns the level's objects again).
//! A live tool that edits a component is therefore seen by the next step.
//!
//! A game that was replaced as a whole between two steps (a staged capture
//! moment, a fresh game) carries an older [`Game::ecs_epoch`]; its vectors
//! are then the newer state and are written to the entities as they are.

use evani_core::ecs::{Entity, World};

use crate::game::{Enemy, Game, Hero, Pop};

/// Component names as the live tools show them.
pub const COMPONENTS: [&str; 7] = [
    "platformer.hero",
    "platformer.enemy",
    "platformer.coin",
    "platformer.flag",
    "platformer.platform",
    "platformer.pop",
    "platformer.slot",
];

/// Register the components with their display names.
pub fn register(world: &mut World) {
    world.register_component::<Hero>(COMPONENTS[0]);
    world.register_component::<Enemy>(COMPONENTS[1]);
    world.register_component::<Coin>(COMPONENTS[2]);
    world.register_component::<Flag>(COMPONENTS[3]);
    world.register_component::<Platform>(COMPONENTS[4]);
    world.register_component::<Pop>(COMPONENTS[5]);
    world.register_component::<Slot>(COMPONENTS[6]);
}

/// An object's index in the level's list of its kind (pops: their order).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Slot(pub u16);

/// A coin still in the level (a taken coin has no entity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Coin;

/// A checkpoint flag; `raised` once the hero has touched it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Flag {
    pub raised: bool,
}

/// A moving platform's top-left now and last step (art px).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Platform {
    pub pos: [f32; 2],
    pub prev: [f32; 2],
}

/// The entities of the player's game, by kind and slot (a world resource).
#[derive(Debug, Default)]
pub struct Objects {
    /// Bumped by every [`Objects::sync`]; a game whose
    /// [`Game::ecs_epoch`] differs was replaced since.
    epoch: u64,
    hero: Option<Entity>,
    enemies: Vec<Entity>,
    /// Per level coin: its entity while it is still there.
    coins: Vec<Option<Entity>>,
    flags: Vec<Entity>,
    plats: Vec<Entity>,
    pops: Vec<Entity>,
}

fn slot(i: usize) -> Slot {
    Slot(u16::try_from(i).unwrap_or(u16::MAX))
}

/// Make `list` exactly `n` entities long (despawn the extra ones, spawn the
/// missing ones with their slot) and return it.
fn fit(w: &mut World, list: &mut Vec<Entity>, n: usize) {
    while list.len() > n {
        if let Some(e) = list.pop() {
            w.despawn(e);
        }
    }
    while list.len() < n {
        let e = w.spawn_with(slot(list.len()));
        list.push(e);
    }
}

impl Objects {
    /// Entities that belong to the player's game (tests and tools).
    pub fn len(&self) -> usize {
        usize::from(self.hero.is_some())
            + self.enemies.len()
            + self.coins.iter().flatten().count()
            + self.flags.len()
            + self.plats.len()
            + self.pops.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Rebuild `g`'s objects from the entities. Returns false (and leaves
    /// `g` as it is) when `g` was replaced since the last sync or nothing
    /// was synced yet.
    pub fn gather(&self, w: &World, g: &mut Game) -> bool {
        if self.epoch == 0 || g.ecs_epoch != self.epoch {
            return false;
        }
        if let Some(h) = self.hero.and_then(|e| w.get::<Hero>(e)) {
            g.hero = *h;
        }
        g.enemies.clear();
        g.enemies.extend(self.enemies.iter().filter_map(|&e| w.get::<Enemy>(e).map(|x| *x)));
        g.coins_taken.clear();
        g.coins_taken.extend(self.coins.iter().map(|c| !c.is_some_and(|e| w.has::<Coin>(e))));
        g.flags.clear();
        g.flags.extend(self.flags.iter().map(|&e| w.get::<Flag>(e).is_some_and(|f| f.raised)));
        g.plats.clear();
        g.plats_prev.clear();
        for &e in &self.plats {
            let p = w.get::<Platform>(e).map(|p| *p).unwrap_or_default();
            g.plats.push(p.pos);
            g.plats_prev.push(p.prev);
        }
        g.pops.clear();
        g.pops.extend(self.pops.iter().filter_map(|&e| w.get::<Pop>(e).map(|x| *x)));
        true
    }

    /// Write `g`'s objects to the entities: spawn what is new, despawn what
    /// went (a taken coin, a finished pop), update the rest.
    pub fn sync(&mut self, w: &mut World, g: &mut Game) {
        let hero = match self.hero {
            Some(e) if w.is_alive(e) => e,
            _ => w.spawn_with(Slot(0)),
        };
        w.insert(hero, g.hero);
        self.hero = Some(hero);

        fit(w, &mut self.enemies, g.enemies.len());
        for (&e, x) in self.enemies.iter().zip(&g.enemies) {
            w.insert(e, *x);
        }

        // Coins: an entity per coin still in the level.
        let n = g.coins_taken.len();
        while self.coins.len() > n {
            if let Some(Some(e)) = self.coins.pop() {
                w.despawn(e);
            }
        }
        self.coins.resize(n, None);
        for (i, taken) in g.coins_taken.iter().enumerate() {
            match (self.coins[i], *taken) {
                (Some(e), true) => {
                    w.despawn(e);
                    self.coins[i] = None;
                }
                (None, false) => {
                    let e = w.spawn_with(slot(i));
                    w.insert(e, Coin);
                    self.coins[i] = Some(e);
                }
                _ => {}
            }
        }

        fit(w, &mut self.flags, g.flags.len());
        for (&e, raised) in self.flags.iter().zip(&g.flags) {
            w.insert(e, Flag { raised: *raised });
        }

        fit(w, &mut self.plats, g.plats.len());
        for (i, &e) in self.plats.iter().enumerate() {
            let pos = g.plats[i];
            let prev = g.plats_prev.get(i).copied().unwrap_or(pos);
            w.insert(e, Platform { pos, prev });
        }

        fit(w, &mut self.pops, g.pops.len());
        for (&e, x) in self.pops.iter().zip(&g.pops) {
            w.insert(e, *x);
        }

        self.epoch += 1;
        g.ecs_epoch = self.epoch;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::Request;
    use crate::{Platformer, PlatformerPlugin};
    use evani_core::App;

    fn app() -> App {
        let mut a = App::new("t").with_kit(PlatformerPlugin);
        a.start();
        a
    }

    #[test]
    fn every_object_of_the_level_is_an_entity() {
        let mut a = app();
        a.run_headless(3, 1000.0 / 60.0);
        let p = a.world.res::<Platformer>();
        let l = &p.game.level;
        assert_eq!(a.world.count::<Hero>(), 1);
        assert_eq!(a.world.count::<Enemy>(), l.beetles.len() + l.hoppers.len());
        assert_eq!(a.world.count::<Coin>(), l.coins.len());
        assert_eq!(a.world.count::<Flag>(), l.checkpoints.len());
        assert_eq!(a.world.count::<Platform>(), l.platforms.len());
        assert!(a.world.entity_count() > 20, "{}", a.world.entity_count());
        assert_eq!(a.world.res::<Objects>().len(), a.world.entity_count());
    }

    #[test]
    fn the_rules_read_the_entities() {
        let mut a = app();
        a.world.res_mut::<Platformer>().game.request(Request::Start);
        a.run_headless(10, 1000.0 / 60.0);
        // Move the hero entity (as a live tool would): the next step starts there.
        let e = a.world.entities_with::<Hero>()[0];
        let x0 = a.world.get::<Hero>(e).map(|h| h.body.pos[0]).unwrap_or_default();
        if let Some(mut h) = a.world.get_mut::<Hero>(e) {
            h.body.pos[0] = x0 + 48.0;
        }
        a.run_headless(1, 1000.0 / 60.0);
        let x = a.world.res::<Platformer>().game.hero.body.pos[0];
        assert!((x - (x0 + 48.0)).abs() < 2.0, "{x} vs {}", x0 + 48.0);
        // Take a coin's entity away: the game counts that coin as taken.
        let c = a.world.sorted_entities::<Slot, _>(|s| *s).into_iter().find(|&e| a.world.has::<Coin>(e));
        let Some(c) = c else { panic!("no coin entity") };
        let k = a.world.get::<Slot>(c).map(|s| usize::from(s.0)).unwrap_or_default();
        a.world.despawn(c);
        a.run_headless(1, 1000.0 / 60.0);
        assert!(a.world.res::<Platformer>().game.coins_taken[k]);
    }

    #[test]
    fn a_taken_coin_despawns_and_a_restart_brings_it_back() {
        let mut a = app();
        let coins = a.world.res::<Platformer>().game.level.coins.len();
        {
            let mut p = a.world.res_mut::<Platformer>();
            p.game.request(Request::Start);
        }
        a.run_headless(2, 1000.0 / 60.0);
        {
            let mut p = a.world.res_mut::<Platformer>();
            // A replaced game state (as a staged moment does) with the first coin taken.
            let mut g = p.game.clone();
            g.ecs_epoch = 0;
            g.coins_taken[0] = true;
            p.game = g;
        }
        a.run_headless(2, 1000.0 / 60.0);
        assert!(a.world.count::<Coin>() < coins);
        a.world.res_mut::<Platformer>().game.request(Request::Restart);
        a.run_headless(2, 1000.0 / 60.0);
        assert_eq!(a.world.count::<Coin>(), coins);
    }
}
