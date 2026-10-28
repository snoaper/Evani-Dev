//! Double-buffered event queues.
//!
//! Events sent during tick N are readable by every system during tick N+1
//! (multi-reader, one-tick latency, deterministic order). Buffers are reused,
//! so steady-state sending does not allocate.

/// One queue per event type; registered with [`crate::ecs::World::add_event`].
#[derive(Debug)]
pub struct EventQueue<T> {
    front: Vec<T>,
    back: Vec<T>,
}

impl<T> Default for EventQueue<T> {
    fn default() -> Self {
        Self {
            front: Vec::new(),
            back: Vec::new(),
        }
    }
}

impl<T> EventQueue<T> {
    pub fn send(&mut self, ev: T) {
        self.back.push(ev);
    }

    /// Events sent during the previous tick, in send order.
    pub fn iter(&self) -> core::slice::Iter<'_, T> {
        self.front.iter()
    }

    pub fn len(&self) -> usize {
        self.front.len()
    }

    pub fn is_empty(&self) -> bool {
        self.front.is_empty()
    }

    /// Events sent so far this tick (not yet visible to readers).
    pub fn pending(&self) -> usize {
        self.back.len()
    }

    /// Swap buffers: pending events become readable, old ones are dropped.
    pub fn update(&mut self) {
        core::mem::swap(&mut self.front, &mut self.back);
        self.back.clear();
    }
}

/// Engine-level events every app gets.
#[derive(Debug, Clone, PartialEq)]
pub enum EngineEvent {
    /// The page became hidden (true) or visible again (false).
    Visibility { hidden: bool },
    /// The active quality tier changed.
    TierChanged { tier: String },
    /// A scene was pushed, popped or replaced.
    SceneChanged { top: Option<String> },
    /// The audio context started after the first user gesture.
    AudioUnlocked,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ecs::World;

    #[test]
    fn one_tick_latency_and_order() {
        let mut q = EventQueue::default();
        q.send(1);
        q.send(2);
        assert!(q.is_empty());
        q.update();
        assert_eq!(q.iter().copied().collect::<Vec<_>>(), vec![1, 2]);
        q.send(3);
        q.update();
        assert_eq!(q.iter().copied().collect::<Vec<_>>(), vec![3]);
        q.update();
        assert!(q.is_empty());
    }

    #[test]
    fn world_event_registration() {
        let mut w = World::new();
        assert!(!w.send_event(5u8));
        w.add_event::<u8>();
        assert!(w.send_event(5u8));
        w.update_events();
        assert_eq!(w.res::<EventQueue<u8>>().len(), 1);
        w.update_events();
        assert_eq!(w.res::<EventQueue<u8>>().len(), 0);
    }
}
