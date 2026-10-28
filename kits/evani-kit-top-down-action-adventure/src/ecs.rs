//! The player's game objects in the ECS: the hero, every enemy, pickup,
//! shot, doorway and room switch is an entity carrying one `Copy` component.
//!
//! The rules ([`crate::game::Game`]) work on plain vectors of those objects.
//! For the player's game the vectors are a view of the entities:
//! [`Objects::gather`] rebuilds them from the entities at the start of every
//! fixed step and [`Objects::sync`] writes every change back at its end (a
//! slain enemy despawns, a new shot spawns). A live tool that edits a
//! component is therefore seen by the next step.
//!
//! A game that was just reset (`Game::dirty`) holds the newer state: the
//! sim system skips the gather for that step and the sync writes the new
//! objects over the entities.

use evani_core::ecs::{Entity, World};

use crate::game::{Door, Enemy, Game, Latch, Pickup, Player, Shot};

/// Component names as the live tools show them.
pub const COMPONENTS: [&str; 6] = [
    "top_down.player",
    "top_down.enemy",
    "top_down.pickup",
    "top_down.shot",
    "top_down.door",
    "top_down.latch",
];

/// Register the components with their display names.
pub fn register(world: &mut World) {
    world.register_component::<Player>(COMPONENTS[0]);
    world.register_component::<Enemy>(COMPONENTS[1]);
    world.register_component::<Pickup>(COMPONENTS[2]);
    world.register_component::<Shot>(COMPONENTS[3]);
    world.register_component::<Door>(COMPONENTS[4]);
    world.register_component::<Latch>(COMPONENTS[5]);
}

/// The entities of the player's game, by kind (a world resource).
#[derive(Debug, Default)]
pub struct Objects {
    player: Option<Entity>,
    enemies: Vec<Entity>,
    pickups: Vec<Entity>,
    shots: Vec<Entity>,
    doors: Vec<Entity>,
    latches: Vec<Entity>,
}

/// Make `list` exactly as long as `items` and store each item on its entity
/// (despawn the extra entities, spawn the missing ones).
fn write_all<T: Copy + 'static>(w: &mut World, list: &mut Vec<Entity>, items: &[T]) {
    while list.len() > items.len() {
        if let Some(e) = list.pop() {
            w.despawn(e);
        }
    }
    for (i, item) in items.iter().enumerate() {
        match list.get(i) {
            Some(&e) if w.is_alive(e) => {
                w.insert(e, *item);
            }
            Some(_) => {
                let e = w.spawn_with(*item);
                list[i] = e;
            }
            None => {
                let e = w.spawn_with(*item);
                list.push(e);
            }
        }
    }
}

/// The components of `list`, in order (an entity that lost its component is
/// skipped).
fn read_all<T: Copy + 'static>(w: &World, list: &[Entity], out: &mut Vec<T>) {
    out.clear();
    out.extend(list.iter().filter_map(|&e| w.get::<T>(e).map(|x| *x)));
}

impl Objects {
    /// Entities that belong to the player's game (tests and tools).
    pub fn len(&self) -> usize {
        usize::from(self.player.is_some())
            + self.enemies.len()
            + self.pickups.len()
            + self.shots.len()
            + self.doors.len()
            + self.latches.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Rebuild `g`'s objects from the entities. Returns false (and leaves
    /// `g` as it is) when nothing was synced yet.
    pub fn gather(&self, w: &World, g: &mut Game) -> bool {
        let Some(p) = self.player.and_then(|e| w.get::<Player>(e).map(|p| *p)) else {
            return false;
        };
        g.player = p;
        read_all(w, &self.enemies, &mut g.enemies);
        read_all(w, &self.pickups, &mut g.pickups);
        read_all(w, &self.shots, &mut g.shots);
        read_all(w, &self.doors, &mut g.doors);
        read_all(w, &self.latches, &mut g.latches);
        true
    }

    /// Write `g`'s objects to the entities.
    pub fn sync(&mut self, w: &mut World, g: &Game) {
        match self.player {
            Some(e) if w.is_alive(e) => {
                w.insert(e, g.player);
            }
            _ => {
                self.player = Some(w.spawn_with(g.player));
            }
        }
        write_all(w, &mut self.enemies, &g.enemies);
        write_all(w, &mut self.pickups, &g.pickups);
        write_all(w, &mut self.shots, &g.shots);
        write_all(w, &mut self.doors, &g.doors);
        write_all(w, &mut self.latches, &g.latches);
    }
}
