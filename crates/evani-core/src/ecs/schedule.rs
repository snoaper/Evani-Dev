use super::World;

/// Where a system runs. All stages except `Frame` run once per fixed
/// simulation tick, in this order. `Frame` runs once per rendered frame after
/// the simulation steps (camera smoothing, UI), with `Time::alpha` set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Stage {
    First,
    PreUpdate,
    Update,
    PostUpdate,
    Last,
    Frame,
}

impl Stage {
    /// Stages run per fixed tick, in order.
    pub const FIXED: [Stage; 5] = [
        Stage::First,
        Stage::PreUpdate,
        Stage::Update,
        Stage::PostUpdate,
        Stage::Last,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Stage::First => "first",
            Stage::PreUpdate => "pre_update",
            Stage::Update => "update",
            Stage::PostUpdate => "post_update",
            Stage::Last => "last",
            Stage::Frame => "frame",
        }
    }
}

/// A system: a named function over the world.
pub type SystemFn = Box<dyn FnMut(&mut World)>;

/// Describes a system to add to the [`Schedule`].
pub struct SystemDesc {
    pub name: &'static str,
    pub stage: Stage,
    /// Sort key within the stage (lower runs first). Ties keep insertion order.
    pub order: i32,
    pub run: SystemFn,
}

impl SystemDesc {
    /// With live code swapping compiled in (dev builds, `hotpatch`), the
    /// system is called through subsecond's jump table, so a patched body
    /// runs from the next call on (docs/LIVE_API.md 10).
    pub fn new(name: &'static str, stage: Stage, run: impl FnMut(&mut World) + 'static) -> Self {
        #[cfg(feature = "hotpatch")]
        let run = crate::hotpatch::wrap_system(run);
        #[cfg(not(feature = "hotpatch"))]
        let run: SystemFn = Box::new(run);
        Self {
            name,
            stage,
            order: 0,
            run,
        }
    }

    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
}

struct Entry {
    desc: SystemDesc,
    seq: u32,
    enabled: bool,
}

/// Ordered system list. Order is `(stage, order, insertion)`: fully
/// deterministic, no hash-map iteration.
#[derive(Default)]
pub struct Schedule {
    entries: Vec<Entry>,
    next_seq: u32,
    sorted: bool,
}

impl Schedule {
    pub fn add(&mut self, desc: SystemDesc) {
        self.entries.push(Entry {
            desc,
            seq: self.next_seq,
            enabled: true,
        });
        self.next_seq += 1;
        self.sorted = false;
    }

    fn sort(&mut self) {
        if !self.sorted {
            self.entries
                .sort_by_key(|e| (e.desc.stage, e.desc.order, e.seq));
            self.sorted = true;
        }
    }

    /// Enable/disable a system by name. Returns false if not found.
    pub fn set_enabled(&mut self, name: &str, enabled: bool) -> bool {
        let mut found = false;
        for e in &mut self.entries {
            if e.desc.name == name {
                e.enabled = enabled;
                found = true;
            }
        }
        found
    }

    /// Run every enabled system of `stage`, applying commands after each.
    pub fn run_stage(&mut self, stage: Stage, world: &mut World) {
        self.sort();
        for e in &mut self.entries {
            if e.desc.stage == stage && e.enabled {
                (e.desc.run)(world);
                world.apply_commands();
            }
        }
    }

    /// Run all fixed-tick stages in order.
    pub fn run_fixed(&mut self, world: &mut World) {
        for stage in Stage::FIXED {
            self.run_stage(stage, world);
        }
    }

    /// `(stage, name, enabled)` in execution order.
    pub fn for_each_system(&mut self, mut f: impl FnMut(Stage, &'static str, bool)) {
        self.sort();
        for e in &self.entries {
            f(e.desc.stage, e.desc.name, e.enabled);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}
