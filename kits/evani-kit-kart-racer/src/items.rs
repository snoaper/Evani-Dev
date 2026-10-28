//! Items: the five original pick-ups, the position-weighted odds table from
//! `items.json`, and the things items leave on the track.

use evani_core::rng::Pcg32;
use serde::Deserialize;

/// The five items (all original).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Item {
    /// Instant boost.
    ZoomPop,
    /// Dropped behind; the first kart to touch it spins out.
    GooBlob,
    /// Thrown ahead on an arc, bounces, explodes on contact or after a fuse.
    BoingBomb,
    /// Homes in on the racer one place ahead.
    SeekerStar,
    /// Blocks the next hit.
    ShieldBubble,
}

impl Item {
    pub const ALL: [Item; 5] = [
        Item::ZoomPop,
        Item::GooBlob,
        Item::BoingBomb,
        Item::SeekerStar,
        Item::ShieldBubble,
    ];

    /// Stable id used in `items.json` and the HUD.
    pub fn id(self) -> &'static str {
        match self {
            Item::ZoomPop => "zoom_pop",
            Item::GooBlob => "goo_blob",
            Item::BoingBomb => "boing_bomb",
            Item::SeekerStar => "seeker_star",
            Item::ShieldBubble => "shield_bubble",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Item::ZoomPop => "Zoom Pop",
            Item::GooBlob => "Goo Blob",
            Item::BoingBomb => "Boing Bomb",
            Item::SeekerStar => "Seeker Star",
            Item::ShieldBubble => "Shield Bubble",
        }
    }

    pub fn from_id(id: &str) -> Option<Item> {
        Item::ALL.into_iter().find(|i| i.id() == id)
    }
}

/// A kart's item slot.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Slot {
    #[default]
    Empty,
    /// The roulette is spinning; `item` is already drawn.
    Rolling { left: f32, item: Item },
    Holding(Item),
}

#[derive(Debug, Clone, Deserialize)]
struct ItemsJson {
    items: Vec<String>,
    bands: BandsJson,
}

#[derive(Debug, Clone, Deserialize)]
struct BandsJson {
    front: Vec<f32>,
    mid: Vec<f32>,
    back: Vec<f32>,
}

/// Weights per position band, in [`Item::ALL`] order.
#[derive(Debug, Clone, PartialEq)]
pub struct OddsTable {
    pub front: [f32; 5],
    pub mid: [f32; 5],
    pub back: [f32; 5],
}

impl OddsTable {
    pub fn from_json(json: &str) -> Result<Self, String> {
        let j: ItemsJson = serde_json::from_str(json).map_err(|e| format!("items.json: {e}"))?;
        let mut t = OddsTable {
            front: [0.0; 5],
            mid: [0.0; 5],
            back: [0.0; 5],
        };
        if [j.bands.front.len(), j.bands.mid.len(), j.bands.back.len()] != [j.items.len(); 3] {
            return Err("items.json: every band needs one weight per item".into());
        }
        for (k, id) in j.items.iter().enumerate() {
            let item = Item::from_id(id).ok_or_else(|| format!("items.json: unknown item {id}"))?;
            let i = item as usize;
            t.front[i] = j.bands.front[k].max(0.0);
            t.mid[i] = j.bands.mid[k].max(0.0);
            t.back[i] = j.bands.back[k].max(0.0);
        }
        Ok(t)
    }

    /// Weights for band value `v` (0 = leading, 1 = last), linearly blended
    /// front -> mid -> back.
    pub fn weights(&self, v: f32) -> [f32; 5] {
        let v = v.clamp(0.0, 1.0);
        let (a, b, t) = if v < 0.5 {
            (&self.front, &self.mid, v * 2.0)
        } else {
            (&self.mid, &self.back, (v - 0.5) * 2.0)
        };
        let mut w = [0.0; 5];
        for i in 0..5 {
            w[i] = a[i] + (b[i] - a[i]) * t;
        }
        w
    }

    /// Draw one item for band value `v` with the race's seeded RNG.
    pub fn draw(&self, v: f32, rng: &mut Pcg32) -> Item {
        let w = self.weights(v);
        let total: f32 = w.iter().sum();
        if total <= 0.0 {
            return Item::ZoomPop;
        }
        let mut r = rng.next_f32() * total;
        for (i, &wi) in w.iter().enumerate() {
            if r < wi {
                return Item::ALL[i];
            }
            r -= wi;
        }
        // Float rounding: the last item with any weight.
        Item::ALL[w.iter().rposition(|&x| x > 0.0).unwrap_or(0)]
    }
}

/// Band value of a racer at `place` (0-based) among `racers`.
pub fn band_value(place: usize, racers: usize) -> f32 {
    if racers <= 1 {
        0.0
    } else {
        place as f32 / (racers - 1) as f32
    }
}

/// An item box on the track.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ItemBox {
    pub pos: [f32; 3],
    /// Seconds until it reappears (0 = there).
    pub hidden: f32,
}

/// A Goo Blob lying on the track.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Goo {
    pub pos: [f32; 3],
    pub life: f32,
    pub owner: usize,
    /// Can't hit its owner for this long.
    pub grace: f32,
}

/// A Boing Bomb in flight.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Bomb {
    pub pos: [f32; 3],
    pub vel: [f32; 3],
    pub fuse: f32,
    pub bounces: u8,
    pub owner: usize,
    pub grace: f32,
    pub seg: usize,
}

/// A Seeker Star following the track.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Seeker {
    pub s: f32,
    pub lateral: f32,
    pub pos: [f32; 3],
    pub owner: usize,
    pub target: Option<usize>,
    pub life: f32,
}

/// How long a blast lasts (s): the length of its shockwave ring, crown and
/// debris (visual only).
pub const BLAST_LIFE_S: f32 = 0.8;

/// An explosion's expanding ball (visual only).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Blast {
    pub pos: [f32; 3],
    pub radius: f32,
    pub t: f32,
}

#[cfg(test)]
#[allow(clippy::needless_range_loop)]
mod tests {
    use super::*;

    fn table() -> OddsTable {
        OddsTable::from_json(include_str!("../data/items.json")).unwrap()
    }

    #[test]
    fn bands_blend_linearly() {
        let t = table();
        assert_eq!(t.weights(0.0), t.front);
        assert_eq!(t.weights(0.5), t.mid);
        assert_eq!(t.weights(1.0), t.back);
        let q = t.weights(0.25);
        for i in 0..5 {
            assert!((q[i] - (t.front[i] + t.mid[i]) * 0.5).abs() < 1e-5);
        }
        assert_eq!(band_value(0, 8), 0.0);
        assert_eq!(band_value(7, 8), 1.0);
    }

    #[test]
    fn odds_follow_position() {
        let t = table();
        let mut rng = Pcg32::seeded(42);
        let count = |v: f32, rng: &mut Pcg32| {
            let mut c = [0u32; 5];
            for _ in 0..20_000 {
                c[t.draw(v, rng) as usize] += 1;
            }
            c
        };
        let lead = count(0.0, &mut rng);
        let last = count(1.0, &mut rng);
        // Weight 0 means never.
        assert_eq!(lead[Item::SeekerStar as usize], 0);
        assert_eq!(last[Item::GooBlob as usize], 0);
        // The leader sees mostly goo and shields, the last kart seekers and boosts.
        assert!(lead[Item::GooBlob as usize] > 8_000, "{lead:?}");
        assert!(last[Item::SeekerStar as usize] > 6_000, "{last:?}");
        // Frequencies match the weights within 2 %.
        for i in 0..5 {
            let want = t.back[i] / t.back.iter().sum::<f32>();
            let got = last[i] as f32 / 20_000.0;
            assert!((want - got).abs() < 0.02, "item {i}: {want} vs {got}");
        }
    }

    #[test]
    fn rejects_unknown_items() {
        let bad = r#"{"items":["banana"],"bands":{"front":[1],"mid":[1],"back":[1]}}"#;
        assert!(OddsTable::from_json(bad).is_err());
    }
}
