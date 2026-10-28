use super::Entity;

const EMPTY: u32 = u32::MAX;

/// Sparse set: `sparse[entity.index] -> dense slot`, with densely packed
/// entities and component values. Removal is `swap_remove` plus patching the
/// moved entity's sparse slot, so iteration is a tight loop over `dense`.
#[derive(Debug)]
pub struct SparseSet<T> {
    sparse: Vec<u32>,
    entities: Vec<Entity>,
    values: Vec<T>,
}

impl<T> Default for SparseSet<T> {
    fn default() -> Self {
        Self {
            sparse: Vec::new(),
            entities: Vec::new(),
            values: Vec::new(),
        }
    }
}

impl<T> SparseSet<T> {
    pub fn with_capacity(cap: usize) -> Self {
        Self {
            sparse: Vec::with_capacity(cap),
            entities: Vec::with_capacity(cap),
            values: Vec::with_capacity(cap),
        }
    }

    #[inline]
    fn slot(&self, e: Entity) -> Option<usize> {
        let d = *self.sparse.get(e.index() as usize)?;
        if d == EMPTY {
            return None;
        }
        let d = d as usize;
        (self.entities[d] == e).then_some(d)
    }

    /// Insert or replace. Returns the previous value for this entity.
    pub fn insert(&mut self, e: Entity, value: T) -> Option<T> {
        let i = e.index() as usize;
        if i >= self.sparse.len() {
            self.sparse.resize(i + 1, EMPTY);
        }
        let d = self.sparse[i];
        if d != EMPTY {
            let d = d as usize;
            if self.entities[d] == e {
                return Some(core::mem::replace(&mut self.values[d], value));
            }
            // Stale entry from an older generation (should not happen when the
            // world removes components on despawn, but stay safe).
            self.remove_at(d);
        }
        self.sparse[i] = self.entities.len() as u32;
        self.entities.push(e);
        self.values.push(value);
        None
    }

    fn remove_at(&mut self, d: usize) -> T {
        let e = self.entities[d];
        self.sparse[e.index() as usize] = EMPTY;
        let last = self.entities.len() - 1;
        if d != last {
            let moved = self.entities[last];
            self.sparse[moved.index() as usize] = d as u32;
        }
        self.entities.swap_remove(d);
        self.values.swap_remove(d)
    }

    pub fn remove(&mut self, e: Entity) -> Option<T> {
        let d = self.slot(e)?;
        Some(self.remove_at(d))
    }

    #[inline]
    pub fn get(&self, e: Entity) -> Option<&T> {
        self.slot(e).map(|d| &self.values[d])
    }

    #[inline]
    pub fn get_mut(&mut self, e: Entity) -> Option<&mut T> {
        self.slot(e).map(|d| &mut self.values[d])
    }

    /// Two different entities' values at once (kart-against-kart contact,
    /// a projectile and its target). `None` if `a == b` or either lacks one.
    pub fn get2_mut(&mut self, a: Entity, b: Entity) -> Option<(&mut T, &mut T)> {
        let (da, db) = (self.slot(a)?, self.slot(b)?);
        if da == db {
            return None;
        }
        let (lo, hi) = (da.min(db), da.max(db));
        let (left, right) = self.values.split_at_mut(hi);
        let (l, h) = (&mut left[lo], &mut right[0]);
        Some(if da < db { (l, h) } else { (h, l) })
    }

    pub fn contains(&self, e: Entity) -> bool {
        self.slot(e).is_some()
    }

    pub fn len(&self) -> usize {
        self.entities.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entities.is_empty()
    }

    /// Entities in dense (iteration) order.
    pub fn entities(&self) -> &[Entity] {
        &self.entities
    }

    pub fn values(&self) -> &[T] {
        &self.values
    }

    pub fn values_mut(&mut self) -> &mut [T] {
        &mut self.values
    }

    pub fn iter(&self) -> impl Iterator<Item = (Entity, &T)> {
        self.entities.iter().copied().zip(self.values.iter())
    }

    pub fn iter_mut(&mut self) -> impl Iterator<Item = (Entity, &mut T)> {
        self.entities.iter().copied().zip(self.values.iter_mut())
    }

    pub fn clear(&mut self) {
        for e in &self.entities {
            self.sparse[e.index() as usize] = EMPTY;
        }
        self.entities.clear();
        self.values.clear();
    }
}
