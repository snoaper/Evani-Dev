use core::cell::{Ref, RefMut};

use super::world::Component;
use super::{Entity, SparseSet, World};

/// Something that can be queried: `&T`, `&mut T` or a tuple of up to six.
pub trait QueryData {
    /// Borrow guards held for the duration of the query.
    type Guard<'w>;
    /// Item handed to the query closure.
    type Item<'a>;

    /// Borrow the storages. `None` if any storage is missing (empty result).
    fn borrow(world: &World) -> Option<Self::Guard<'_>>;
    /// The smallest entity list among the borrowed storages (drives iteration).
    fn driver<'g>(guard: &'g Self::Guard<'_>) -> &'g [Entity];
    /// Fetch the item for `e`, or `None` if `e` lacks one of the components.
    fn fetch<'a>(guard: &'a mut Self::Guard<'_>, e: Entity) -> Option<Self::Item<'a>>;
}

fn borrow_conflict<T>() {
    if cfg!(debug_assertions) {
        panic!(
            "query borrow conflict on {} (already borrowed mutably elsewhere)",
            core::any::type_name::<T>()
        );
    }
}

impl<T: Component> QueryData for &T {
    type Guard<'w> = Ref<'w, SparseSet<T>>;
    type Item<'a> = &'a T;

    fn borrow(world: &World) -> Option<Self::Guard<'_>> {
        let cell = world.storage::<T>()?;
        match cell.try_borrow() {
            Ok(g) => Some(g),
            Err(_) => {
                borrow_conflict::<T>();
                None
            }
        }
    }
    fn driver<'g>(guard: &'g Self::Guard<'_>) -> &'g [Entity] {
        guard.entities()
    }
    fn fetch<'a>(guard: &'a mut Self::Guard<'_>, e: Entity) -> Option<Self::Item<'a>> {
        guard.get(e)
    }
}

impl<T: Component> QueryData for &mut T {
    type Guard<'w> = RefMut<'w, SparseSet<T>>;
    type Item<'a> = &'a mut T;

    fn borrow(world: &World) -> Option<Self::Guard<'_>> {
        let cell = world.storage::<T>()?;
        match cell.try_borrow_mut() {
            Ok(g) => Some(g),
            Err(_) => {
                borrow_conflict::<T>();
                None
            }
        }
    }
    fn driver<'g>(guard: &'g Self::Guard<'_>) -> &'g [Entity] {
        guard.entities()
    }
    fn fetch<'a>(guard: &'a mut Self::Guard<'_>, e: Entity) -> Option<Self::Item<'a>> {
        guard.get_mut(e)
    }
}

macro_rules! impl_query_tuple {
    ($($name:ident $idx:tt),+) => {
        impl<$($name: QueryData),+> QueryData for ($($name,)+) {
            type Guard<'w> = ($($name::Guard<'w>,)+);
            type Item<'a> = ($($name::Item<'a>,)+);

            fn borrow(world: &World) -> Option<Self::Guard<'_>> {
                Some(($($name::borrow(world)?,)+))
            }
            fn driver<'g>(guard: &'g Self::Guard<'_>) -> &'g [Entity] {
                let mut best: Option<&'g [Entity]> = None;
                $(
                    let d = $name::driver(&guard.$idx);
                    best = match best {
                        Some(b) if b.len() <= d.len() => Some(b),
                        _ => Some(d),
                    };
                )+
                best.unwrap_or(&[])
            }
            fn fetch<'a>(guard: &'a mut Self::Guard<'_>, e: Entity) -> Option<Self::Item<'a>> {
                Some(($($name::fetch(&mut guard.$idx, e)?,)+))
            }
        }
    };
}

impl_query_tuple!(A 0);
impl_query_tuple!(A 0, B 1);
impl_query_tuple!(A 0, B 1, C 2);
impl_query_tuple!(A 0, B 1, C 2, D 3);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4);
impl_query_tuple!(A 0, B 1, C 2, D 3, E 4, F 5);

/// A live query holding storage borrows. Iterate with [`Query::for_each`].
///
/// Iteration order is the dense order of the smallest storage involved, which
/// depends only on the order of inserts/removes: deterministic.
pub struct Query<'w, Q: QueryData> {
    guard: Option<Q::Guard<'w>>,
}

impl<'w, Q: QueryData> Query<'w, Q> {
    pub(crate) fn new(world: &'w World) -> Self {
        Self {
            guard: Q::borrow(world),
        }
    }

    /// Call `f` for every entity that has all queried components.
    pub fn for_each(&mut self, mut f: impl for<'a> FnMut(Entity, Q::Item<'a>)) {
        let Some(g) = self.guard.as_mut() else {
            return;
        };
        let n = Q::driver(g).len();
        for i in 0..n {
            let e = Q::driver(g)[i];
            if let Some(item) = Q::fetch(g, e) {
                f(e, item);
            }
        }
    }

    /// The item for one entity.
    pub fn get(&mut self, e: Entity) -> Option<Q::Item<'_>> {
        let g = self.guard.as_mut()?;
        Q::fetch(g, e)
    }

    /// Upper bound on matches (length of the driving storage).
    pub fn len_hint(&self) -> usize {
        self.guard.as_ref().map(|g| Q::driver(g).len()).unwrap_or(0)
    }

    /// Exact number of matching entities (walks the driving storage).
    pub fn count(&mut self) -> usize {
        let mut n = 0;
        self.for_each(|_, _| n += 1);
        n
    }
}
