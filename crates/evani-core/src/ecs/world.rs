use core::any::{Any, TypeId};
use core::cell::{Ref, RefCell, RefMut};
use std::collections::BTreeMap;

use super::query::{Query, QueryData};
use super::{Entities, Entity, SparseSet};

/// Marker for component types. Any `'static` type can be a component.
pub trait Component: 'static {}
impl<T: 'static> Component for T {}

/// Type-erased view of a component storage, used for despawn and inspection.
trait ErasedStorage {
    fn remove(&mut self, e: Entity);
    fn len(&self) -> usize;
    fn name(&self) -> &'static str;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

struct Storage<T: Component> {
    name: &'static str,
    set: RefCell<SparseSet<T>>,
}

impl<T: Component> ErasedStorage for Storage<T> {
    fn remove(&mut self, e: Entity) {
        self.set.get_mut().remove(e);
    }
    fn len(&self) -> usize {
        self.set.try_borrow().map(|s| s.len()).unwrap_or(0)
    }
    fn name(&self) -> &'static str {
        self.name
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

/// A deferred world operation.
type CommandOp = Box<dyn FnOnce(&mut World)>;

/// Deferred structural changes recorded while queries hold borrows.
/// Applied by the schedule after each system, in recording order.
#[derive(Default)]
pub struct Commands {
    despawn: Vec<Entity>,
    ops: Vec<CommandOp>,
}

impl Commands {
    pub fn despawn(&mut self, e: Entity) {
        self.despawn.push(e);
    }

    /// Run an arbitrary structural change later (spawn, insert, remove...).
    pub fn add(&mut self, op: impl FnOnce(&mut World) + 'static) {
        self.ops.push(Box::new(op));
    }

    pub fn is_empty(&self) -> bool {
        self.despawn.is_empty() && self.ops.is_empty()
    }
}

/// The ECS world: entities, sparse-set component storages and resources.
///
/// Storages and resources sit in `RefCell`s so several can be borrowed at
/// once from `&World` (queries over multiple components, systems that read
/// `Input` while writing `Rng`). Structural changes (spawn, despawn, insert,
/// remove) need `&mut World`, or go through [`Commands`].
pub struct World {
    entities: Entities,
    storages: Vec<Box<dyn ErasedStorage>>,
    storage_index: BTreeMap<TypeId, usize>,
    resources: BTreeMap<TypeId, Box<dyn Any>>,
    commands: RefCell<Commands>,
    event_updaters: Vec<fn(&World)>,
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

impl World {
    pub fn new() -> Self {
        Self {
            entities: Entities::default(),
            storages: Vec::new(),
            storage_index: BTreeMap::new(),
            resources: BTreeMap::new(),
            commands: RefCell::new(Commands::default()),
            event_updaters: Vec::new(),
        }
    }

    // ---- components -------------------------------------------------------

    /// Register a component type with a display name (shown by tools). Safe
    /// to call twice. Inserting an unregistered type registers it implicitly.
    pub fn register_component<T: Component>(&mut self, name: &'static str) {
        self.storage_slot::<T>(name);
    }

    fn storage_slot<T: Component>(&mut self, name: &'static str) -> usize {
        let id = TypeId::of::<T>();
        if let Some(&i) = self.storage_index.get(&id) {
            return i;
        }
        let i = self.storages.len();
        self.storages.push(Box::new(Storage::<T> {
            name,
            set: RefCell::new(SparseSet::default()),
        }));
        self.storage_index.insert(id, i);
        i
    }

    pub(crate) fn storage<T: Component>(&self) -> Option<&RefCell<SparseSet<T>>> {
        let i = *self.storage_index.get(&TypeId::of::<T>())?;
        self.storages[i]
            .as_any()
            .downcast_ref::<Storage<T>>()
            .map(|s| &s.set)
    }

    fn storage_mut<T: Component>(&mut self) -> &mut SparseSet<T> {
        let i = self.storage_slot::<T>(core::any::type_name::<T>());
        match self.storages[i].as_any_mut().downcast_mut::<Storage<T>>() {
            Some(s) => s.set.get_mut(),
            None => unreachable!("storage type mismatch"),
        }
    }

    pub fn spawn(&mut self) -> Entity {
        self.entities.spawn()
    }

    /// Spawn and attach one component (chain [`World::insert`] for more).
    pub fn spawn_with<T: Component>(&mut self, value: T) -> Entity {
        let e = self.spawn();
        self.insert(e, value);
        e
    }

    /// Remove the entity and all its components. False if already dead.
    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.entities.despawn(e) {
            return false;
        }
        for s in &mut self.storages {
            s.remove(e);
        }
        true
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        self.entities.is_alive(e)
    }

    /// Attach or replace a component. Ignored (returns false) for dead entities.
    pub fn insert<T: Component>(&mut self, e: Entity, value: T) -> bool {
        if !self.entities.is_alive(e) {
            return false;
        }
        self.storage_mut::<T>().insert(e, value);
        true
    }

    pub fn remove<T: Component>(&mut self, e: Entity) -> Option<T> {
        self.storage_mut::<T>().remove(e)
    }

    pub fn has<T: Component>(&self, e: Entity) -> bool {
        self.storage::<T>()
            .and_then(|s| s.try_borrow().ok().map(|s| s.contains(e)))
            .unwrap_or(false)
    }

    pub fn get<T: Component>(&self, e: Entity) -> Option<Ref<'_, T>> {
        let set = self.storage::<T>()?.borrow();
        Ref::filter_map(set, |s| s.get(e)).ok()
    }

    pub fn get_mut<T: Component>(&self, e: Entity) -> Option<RefMut<'_, T>> {
        let set = self.storage::<T>()?.borrow_mut();
        RefMut::filter_map(set, |s| s.get_mut(e)).ok()
    }

    /// Two different entities' `T` at once, both writable (pairwise
    /// gameplay: kart-against-kart bumps, a projectile and the kart it
    /// hits). `None` if `a == b`, either lacks `T`, or `T` is borrowed.
    pub fn get2_mut<T: Component>(&self, a: Entity, b: Entity) -> Option<(RefMut<'_, T>, RefMut<'_, T>)> {
        let mut set = self.storage::<T>()?.try_borrow_mut().ok()?;
        set.get2_mut(a, b)?;
        Some(RefMut::map_split(set, |s| match s.get2_mut(a, b) {
            Some(pair) => pair,
            None => unreachable!("checked above"),
        }))
    }

    /// Every entity that has `T`, in iteration (dense) order. For loops
    /// that need the list while changing the world (spawning, despawning,
    /// pairs through [`World::get2_mut`]).
    pub fn entities_with<T: Component>(&self) -> Vec<Entity> {
        self.storage::<T>()
            .map(|s| s.borrow().entities().to_vec())
            .unwrap_or_default()
    }

    /// Every entity that has `T`, ordered by `key` (ties keep iteration
    /// order). Iteration order is deterministic but changes when an entity
    /// is despawned (swap-remove); when gameplay depends on order (who wins
    /// a tie, which pair resolves first) give the component an explicit
    /// slot and iterate by it.
    pub fn sorted_entities<T: Component, K: Ord>(&self, mut key: impl FnMut(&T) -> K) -> Vec<Entity> {
        let Some(s) = self.storage::<T>() else {
            return Vec::new();
        };
        let s = s.borrow();
        let mut v: Vec<(K, Entity)> = s.iter().map(|(e, t)| (key(t), e)).collect();
        v.sort_by(|a, b| a.0.cmp(&b.0));
        v.into_iter().map(|(_, e)| e).collect()
    }

    /// Number of entities that have `T`.
    pub fn count<T: Component>(&self) -> usize {
        self.storage::<T>().map_or(0, |s| s.borrow().len())
    }

    /// Despawn every entity that has `T` (a race or level reset: give every
    /// gameplay object a marker component and clear them in one call).
    /// Returns how many went. Deterministic: dense order.
    pub fn despawn_all<T: Component>(&mut self) -> usize {
        let list = self.entities_with::<T>();
        for &e in &list {
            self.despawn(e);
        }
        list.len()
    }

    /// Query over one or more components, e.g.
    /// `world.query::<(&mut Pos, &Vel)>().for_each(|e, (p, v)| ...)`.
    /// Iterates the smallest storage involved, in its dense order.
    pub fn query<Q: QueryData>(&self) -> Query<'_, Q> {
        Query::new(self)
    }

    /// Number of live entities.
    pub fn entity_count(&self) -> usize {
        self.entities.len()
    }

    pub fn entities(&self) -> &Entities {
        &self.entities
    }

    /// `(name, count)` for every registered component storage, in
    /// registration order.
    pub fn component_stats(&self, mut f: impl FnMut(&'static str, usize)) {
        for s in &self.storages {
            f(s.name(), s.len());
        }
    }

    // ---- resources --------------------------------------------------------

    /// Insert or replace a singleton resource.
    pub fn insert_resource<R: 'static>(&mut self, value: R) {
        self.resources
            .insert(TypeId::of::<R>(), Box::new(RefCell::new(value)));
    }

    pub fn remove_resource<R: 'static>(&mut self) -> Option<R> {
        let b = self.resources.remove(&TypeId::of::<R>())?;
        b.downcast::<RefCell<R>>().ok().map(|c| c.into_inner())
    }

    pub fn has_resource<R: 'static>(&self) -> bool {
        self.resources.contains_key(&TypeId::of::<R>())
    }

    fn resource_cell<R: 'static>(&self) -> Option<&RefCell<R>> {
        self.resources
            .get(&TypeId::of::<R>())?
            .downcast_ref::<RefCell<R>>()
    }

    pub fn try_res<R: 'static>(&self) -> Option<Ref<'_, R>> {
        self.resource_cell::<R>()?.try_borrow().ok()
    }

    pub fn try_res_mut<R: 'static>(&self) -> Option<RefMut<'_, R>> {
        self.resource_cell::<R>()?.try_borrow_mut().ok()
    }

    /// Borrow a resource the engine guarantees exists (Time, Input, Rng,
    /// ParamRegistry, ...). Panics with the type name if it is missing,
    /// which is a programming error, not a runtime condition.
    pub fn res<R: 'static>(&self) -> Ref<'_, R> {
        match self.resource_cell::<R>() {
            Some(c) => c.borrow(),
            None => panic!("missing resource {}", core::any::type_name::<R>()),
        }
    }

    /// Mutable variant of [`World::res`].
    pub fn res_mut<R: 'static>(&self) -> RefMut<'_, R> {
        match self.resource_cell::<R>() {
            Some(c) => c.borrow_mut(),
            None => panic!("missing resource {}", core::any::type_name::<R>()),
        }
    }

    /// Direct mutable access when holding `&mut World` (no runtime borrow).
    pub fn res_get_mut<R: 'static>(&mut self) -> Option<&mut R> {
        self.resources
            .get_mut(&TypeId::of::<R>())?
            .downcast_mut::<RefCell<R>>()
            .map(RefCell::get_mut)
    }

    // ---- commands & events --------------------------------------------------

    /// Deferred structural changes; usable inside query closures.
    pub fn commands(&self) -> RefMut<'_, Commands> {
        self.commands.borrow_mut()
    }

    /// Apply queued commands (called by the schedule after each system).
    pub fn apply_commands(&mut self) {
        loop {
            let (despawn, ops) = {
                let c = self.commands.get_mut();
                if c.is_empty() {
                    return;
                }
                (
                    core::mem::take(&mut c.despawn),
                    core::mem::take(&mut c.ops),
                )
            };
            for &e in &despawn {
                self.despawn(e);
            }
            for op in ops {
                op(self);
            }
            // Give the despawn buffer's capacity back to avoid re-allocating.
            let c = self.commands.get_mut();
            if c.despawn.is_empty() {
                let mut d = despawn;
                d.clear();
                c.despawn = d;
            }
        }
    }

    /// Register an event type: inserts an [`crate::events::EventQueue<T>`]
    /// resource and swaps its buffers once per simulation tick.
    pub fn add_event<T: 'static>(&mut self) {
        if self.has_resource::<crate::events::EventQueue<T>>() {
            return;
        }
        self.insert_resource(crate::events::EventQueue::<T>::default());
        self.event_updaters.push(|w: &World| {
            if let Some(mut q) = w.try_res_mut::<crate::events::EventQueue<T>>() {
                q.update();
            }
        });
    }

    /// Send an event (it is visible to readers from the next tick).
    /// Returns false if the event type was never registered.
    pub fn send_event<T: 'static>(&self, ev: T) -> bool {
        match self.try_res_mut::<crate::events::EventQueue<T>>() {
            Some(mut q) => {
                q.send(ev);
                true
            }
            None => false,
        }
    }

    /// Swap all event buffers (called once per tick by the app).
    pub fn update_events(&self) {
        for f in &self.event_updaters {
            f(self);
        }
    }
}
