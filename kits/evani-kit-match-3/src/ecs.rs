//! The game's objects in the ECS: every piece on the board is an entity
//! ([`Piece`], [`Cell`], [`Motion`]); a cleared piece keeps its entity while
//! it pops ([`Popping`] instead of `Cell`); a special going off is a
//! [`Blast`] entity; Puff the mascot is an entity with [`Puff`]. Floating
//! text lives in a fixed pool ([`Floats`]) that never allocates while
//! playing. Sparkles and confetti are not kept here at all: they are
//! emitted into the shared GPU particle system (`crate::sparks`), with
//! their random numbers drawn from [`FxStream`].
//!
//! The rules ([`crate::board::Board`]) work on a plain grid of pieces for
//! speed and unit tests. [`Grid`] holds that grid as a view of the
//! entities: [`Grid::gather`] rebuilds it from the entities at the start of
//! every tick, and [`Grid::sync`] writes every board change back (entities
//! follow their piece by id, so a piece's animation travels with it).

use evani_core::ecs::{Entity, World};
use evani_core::rng::Pcg32;

use crate::board::{Board, Kind, Piece};

/// Component names as the live tools show them.
pub const COMPONENTS: [&str; 6] = [
    "match_3.piece",
    "match_3.cell",
    "match_3.motion",
    "match_3.popping",
    "match_3.blast",
    "match_3.puff",
];

/// Register the components with their display names.
pub fn register(world: &mut World) {
    world.register_component::<Piece>(COMPONENTS[0]);
    world.register_component::<Cell>(COMPONENTS[1]);
    world.register_component::<Motion>(COMPONENTS[2]);
    world.register_component::<Popping>(COMPONENTS[3]);
    world.register_component::<Blast>(COMPONENTS[4]);
    world.register_component::<Puff>(COMPONENTS[5]);
}

/// Where a piece sits: its cell index (`y * w + x`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell(pub u16);

/// A piece's visual motion (offsets in cells, timers in s). The rules never
/// read it.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Motion {
    /// Drawn this far (cells) from its cell; tweens back to 0.
    pub off: [f32; 2],
    /// Fall speed (cells/s, down positive) while `off[1] < 0`.
    pub vy: f32,
    /// Landing squash timer (counts down).
    pub land: f32,
    /// Swap tween: start offset and elapsed time; `swap_t < 0` = none.
    pub swap_from: [f32; 2],
    pub swap_t: f32,
    /// Created as a special this long ago (pop-in), negative = none.
    pub born: f32,
}

impl Motion {
    pub fn new() -> Self {
        Self {
            swap_t: -1.0,
            born: -1.0,
            ..Self::default()
        }
    }
}

/// A cleared piece popping (it is off the board: no [`Cell`]).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Popping {
    /// Cell coordinates (fractional).
    pub x: f32,
    pub y: f32,
    /// Seconds since it started popping (negative while delayed).
    pub t: f32,
}

/// A special going off (visual).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blast {
    pub x: f32,
    pub y: f32,
    pub kind: Kind,
    pub combo: Option<Kind>,
    pub color: u8,
    pub t: f32,
}

/// How Puff feels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mood {
    #[default]
    Idle,
    Happy,
    Excited,
    Worried,
    Sad,
    Cheer,
}

/// Puff the mascot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Puff {
    pub mood: Mood,
    /// Seconds in this mood.
    pub mood_t: f32,
}

/// The board as the rules see it, kept in step with the piece entities,
/// plus the draw order of the pops and blasts.
#[derive(Debug, Clone, Default)]
pub struct Grid {
    /// Shape, frosting and the pieces (a view of the entities).
    pub board: Board,
    /// The entity of the piece in each cell.
    pub slots: Vec<Option<Entity>>,
    /// Popping pieces, oldest first.
    pub pops: Vec<Entity>,
    /// Blasts, oldest first.
    pub blasts: Vec<Entity>,
    /// Puff's entity.
    pub puff: Option<Entity>,
    /// Scratch for [`Grid::sync`]: the entities by piece id.
    by_id: Vec<(u32, Option<Entity>)>,
    /// Entities whose piece left the board in the last [`Grid::sync`].
    gone: Vec<(u32, Entity)>,
}

impl Grid {
    /// An empty grid with room for a board's worth of pops and blasts.
    pub fn new(puff: Entity) -> Self {
        Self {
            pops: Vec::with_capacity(128),
            blasts: Vec::with_capacity(16),
            by_id: Vec::with_capacity(128),
            gone: Vec::with_capacity(64),
            puff: Some(puff),
            ..Self::default()
        }
    }

    /// Rebuild the board's pieces from the piece entities.
    pub fn gather(&mut self, w: &World) {
        let n = self.board.len();
        self.slots.clear();
        self.slots.resize(n, None);
        let (cells, slots) = (&mut self.board.cells, &mut self.slots);
        for c in cells.iter_mut() {
            *c = None;
        }
        w.query::<(&Piece, &Cell)>().for_each(|e, (p, c)| {
            let i = usize::from(c.0);
            if i < n {
                cells[i] = Some(*p);
                slots[i] = Some(e);
            }
        });
    }

    /// Write the board's pieces back to the entities after a board change:
    /// a piece keeps its entity (and so its [`Motion`]) wherever it moved,
    /// a new piece gets a new entity, and the entities of pieces that left
    /// the board wait in `gone` for [`Grid::take_gone`].
    pub fn sync(&mut self, w: &mut World) {
        for (_, e) in self.gone.drain(..) {
            w.despawn(e);
        }
        self.by_id.clear();
        for e in self.slots.iter().flatten() {
            if let Some(p) = w.get::<Piece>(*e) {
                self.by_id.push((p.id, Some(*e)));
            }
        }
        self.by_id.sort_unstable_by_key(|x| x.0);
        let n = self.board.len();
        self.slots.resize(n, None);
        for i in 0..n {
            self.slots[i] = match self.board.cells[i] {
                None => None,
                Some(p) => {
                    let known = match self.by_id.binary_search_by_key(&p.id, |x| x.0) {
                        Ok(k) => self.by_id[k].1.take(),
                        Err(_) => None,
                    };
                    let e = match known {
                        Some(e) => e,
                        None => {
                            let e = w.spawn();
                            w.insert(e, Motion::new());
                            e
                        }
                    };
                    w.insert(e, p);
                    w.insert(e, Cell(i as u16));
                    Some(e)
                }
            };
        }
        for &(id, e) in &self.by_id {
            if let Some(e) = e {
                self.gone.push((id, e));
            }
        }
    }

    /// The entity of a piece that just left the board, by piece id.
    pub fn take_gone(&mut self, id: u32) -> Option<Entity> {
        let k = self.gone.iter().position(|g| g.0 == id)?;
        Some(self.gone.swap_remove(k).1)
    }

    /// Despawn every piece, pop and blast (a new board).
    pub fn clear(&mut self, w: &mut World) {
        w.despawn_all::<Piece>();
        w.despawn_all::<Blast>();
        self.slots.clear();
        self.pops.clear();
        self.blasts.clear();
        self.gone.clear();
    }

    /// A piece's motion, by cell.
    pub fn motion<'w>(&self, w: &'w World, cell: usize) -> Option<core::cell::RefMut<'w, Motion>> {
        let e = (*self.slots.get(cell)?)?;
        w.get_mut::<Motion>(e)
    }

    /// Puff's mood (idle without a Puff).
    pub fn puff(&self, w: &World) -> Puff {
        self.puff.and_then(|e| w.get::<Puff>(e).map(|p| *p)).unwrap_or_default()
    }
}

/// The random stream of the sparkles and confetti. It is its own stream, so
/// effects on or off never change the game, and it is reseeded with every
/// level.
#[derive(Debug, Clone, Default)]
pub struct FxStream(pub Pcg32);

/// Most floating texts at once.
pub const MAX_FLOATS: usize = 24;
/// Longest floating text (bytes).
pub const FLOAT_TEXT: usize = 24;

/// A floating text's words, stored inline (no allocation).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct FloatText {
    bytes: [u8; FLOAT_TEXT],
    len: u8,
}

impl FloatText {
    /// `s`, cut at [`FLOAT_TEXT`] bytes (on a character boundary).
    pub fn new(s: &str) -> Self {
        let mut t = Self::default();
        for ch in s.chars() {
            let mut buf = [0u8; 4];
            if !t.push(ch.encode_utf8(&mut buf)) {
                break;
            }
        }
        t
    }

    /// `+n` (a score).
    pub fn plus(n: u32) -> Self {
        let mut digits = [0u8; 10];
        let mut k = digits.len();
        let mut v = n;
        loop {
            k -= 1;
            digits[k] = b'0' + (v % 10) as u8;
            v /= 10;
            if v == 0 {
                break;
            }
        }
        let mut t = Self::default();
        t.bytes[0] = b'+';
        let d = &digits[k..];
        t.bytes[1..=d.len()].copy_from_slice(d);
        t.len = (d.len() + 1) as u8;
        t
    }

    fn push(&mut self, s: &str) -> bool {
        let (at, n) = (usize::from(self.len), s.len());
        if at + n > FLOAT_TEXT {
            return false;
        }
        self.bytes[at..at + n].copy_from_slice(s.as_bytes());
        self.len = (at + n) as u8;
        true
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.bytes[..usize::from(self.len)]).unwrap_or("")
    }
}

/// Floating text (score, combo callout), in cell coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Floater {
    pub id: u32,
    pub x: f32,
    pub y: f32,
    pub text: FloatText,
    /// 0 score, 1 callout, 2 big callout.
    pub style: u8,
    pub age: f32,
    pub life: f32,
}

/// The floating-text pool: at most [`MAX_FLOATS`], oldest first.
#[derive(Debug, Clone, Default)]
pub struct Floats {
    pub items: Vec<Floater>,
    serial: u32,
}

impl Floats {
    pub fn new() -> Self {
        Self {
            items: Vec::with_capacity(MAX_FLOATS),
            serial: 0,
        }
    }

    /// Add a text; a callout (style 1 or 2) replaces the previous callout,
    /// and a full pool drops its oldest text.
    pub fn add(&mut self, x: f32, y: f32, text: FloatText, style: u8, life: f32) {
        self.serial = self.serial.wrapping_add(1);
        if style >= 1 {
            // One combo callout at a time: the new word replaces the old.
            self.items.retain(|f| f.style == 0);
        }
        if self.items.len() >= MAX_FLOATS {
            self.items.remove(0);
        }
        self.items.push(Floater {
            id: self.serial,
            x,
            y,
            text,
            style,
            age: 0.0,
            life,
        });
    }

    /// Age every text and drop the finished ones.
    pub fn step(&mut self, dt: f32) {
        for f in &mut self.items {
            f.age += dt;
        }
        self.items.retain(|f| f.age < f.life);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn float_text_is_inline_and_cut_safely() {
        assert_eq!(FloatText::plus(0).as_str(), "+0");
        assert_eq!(FloatText::plus(180).as_str(), "+180");
        assert_eq!(FloatText::plus(u32::MAX).as_str(), "+4294967295");
        assert_eq!(FloatText::new("Unstoppable!").as_str(), "Unstoppable!");
        let long = FloatText::new("ééééééééééééééééééééééééé");
        assert_eq!(long.as_str().len(), FLOAT_TEXT);
    }

    #[test]
    fn pools_keep_their_caps_and_order() {
        let mut f = Floats::new();
        for k in 0..30 {
            f.add(k as f32, 0.0, FloatText::plus(k), 0, 1.0);
        }
        assert_eq!(f.items.len(), MAX_FLOATS);
        assert_eq!(f.items.capacity(), MAX_FLOATS, "the pool never grew");
        f.add(0.0, 0.0, FloatText::new("Nice!"), 1, 1.0);
        f.add(0.0, 0.0, FloatText::new("Great!"), 1, 1.0);
        assert_eq!(f.items.iter().filter(|x| x.style >= 1).count(), 1);
    }
}
