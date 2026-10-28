/// An entity id: slot index plus generation. A despawned slot is reused with a
/// higher generation, so stale ids never alias new entities.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Entity {
    index: u32,
    generation: u32,
}

impl Entity {
    pub const fn index(self) -> u32 {
        self.index
    }

    pub const fn generation(self) -> u32 {
        self.generation
    }

    /// Pack into one u64 (generation in the high bits), e.g. for JSON or JS.
    pub const fn to_bits(self) -> u64 {
        ((self.generation as u64) << 32) | self.index as u64
    }

    pub const fn from_bits(bits: u64) -> Self {
        Self {
            index: bits as u32,
            generation: (bits >> 32) as u32,
        }
    }
}

/// Entity allocator with a LIFO free list (deterministic reuse order).
#[derive(Debug, Default)]
pub struct Entities {
    generations: Vec<u32>,
    alive: Vec<bool>,
    free: Vec<u32>,
    len: usize,
}

impl Entities {
    pub fn spawn(&mut self) -> Entity {
        if let Some(index) = self.free.pop() {
            let i = index as usize;
            self.alive[i] = true;
            self.len += 1;
            Entity {
                index,
                generation: self.generations[i],
            }
        } else {
            let index = self.generations.len() as u32;
            self.generations.push(0);
            self.alive.push(true);
            self.len += 1;
            Entity {
                index,
                generation: 0,
            }
        }
    }

    /// Returns false if the entity was already dead.
    pub fn despawn(&mut self, e: Entity) -> bool {
        if !self.is_alive(e) {
            return false;
        }
        let i = e.index as usize;
        self.alive[i] = false;
        self.generations[i] = self.generations[i].wrapping_add(1);
        self.free.push(e.index);
        self.len -= 1;
        true
    }

    pub fn is_alive(&self, e: Entity) -> bool {
        let i = e.index as usize;
        i < self.generations.len() && self.alive[i] && self.generations[i] == e.generation
    }

    /// Number of live entities.
    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Visit live entities in index order (deterministic).
    pub fn for_each_alive(&self, mut f: impl FnMut(Entity)) {
        for (i, (&alive, &generation)) in self.alive.iter().zip(&self.generations).enumerate() {
            if alive {
                f(Entity {
                    index: i as u32,
                    generation,
                });
            }
        }
    }
}
