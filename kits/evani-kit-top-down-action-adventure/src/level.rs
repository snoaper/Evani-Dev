//! The dungeon from `data/level.json`: rooms on a grid, joined by doorways.
//!
//! A room is 16 x 11 tiles of 16 art px. Its outer ring is wall; a doorway is
//! a gap in that ring (two tiles wide on the north and south walls, tiles 7
//! and 8; two tall on the west and east walls, tiles 4 and 5), carved by the
//! code wherever a link joins the room to a neighbour. Tile characters:
//! `#` wall, `.` floor, `,` floor (decor), `P` pit (blocks walkers, not
//! bolts or fliers), `S` floor switch, `G` gate (shut until its room's
//! switch is latched).

use serde::Deserialize;

/// Tile edge in art pixels.
pub const TILE: f32 = 16.0;
pub const ROOM_W: usize = 16;
pub const ROOM_H: usize = 11;
/// A room in art pixels.
pub const ROOM_PX_W: f32 = ROOM_W as f32 * TILE;
pub const ROOM_PX_H: f32 = ROOM_H as f32 * TILE;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    N,
    E,
    S,
    W,
}

impl Side {
    pub const ALL: [Side; 4] = [Side::N, Side::E, Side::S, Side::W];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn opposite(self) -> Side {
        match self {
            Side::N => Side::S,
            Side::E => Side::W,
            Side::S => Side::N,
            Side::W => Side::E,
        }
    }

    /// Grid step to the neighbour on this side.
    pub fn delta(self) -> (i32, i32) {
        match self {
            Side::N => (0, -1),
            Side::E => (1, 0),
            Side::S => (0, 1),
            Side::W => (-1, 0),
        }
    }

    fn parse(s: &str) -> Option<Side> {
        match s {
            "n" => Some(Side::N),
            "e" => Some(Side::E),
            "s" => Some(Side::S),
            "w" => Some(Side::W),
            _ => None,
        }
    }
}

/// The side a tile position (inside the room, or one step outside through a
/// doorway) belongs to as a doorway tile, if any.
fn doorway_side(tx: i32, ty: i32) -> Option<Side> {
    let (w, h) = (ROOM_W as i32, ROOM_H as i32);
    let x_gap = (7..=8).contains(&tx);
    let y_gap = (4..=5).contains(&ty);
    if x_gap && (ty == 0 || ty == -1) {
        Some(Side::N)
    } else if x_gap && (ty == h - 1 || ty == h) {
        Some(Side::S)
    } else if y_gap && (tx == 0 || tx == -1) {
        Some(Side::W)
    } else if y_gap && (tx == w - 1 || tx == w) {
        Some(Side::E)
    } else {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tile {
    Floor,
    Wall,
    Pit,
    Switch,
    Gate,
}

/// What a tile position is, for movement.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cell {
    Floor,
    Wall,
    Pit,
    Switch,
    Gate,
    /// A doorway tile of this link (open or locked, see the game's doors).
    Door(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkKind {
    Open,
    Locked,
}

/// A doorway joining room `a` (on `side_a`) to room `b` (on the opposite side).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Link {
    pub a: usize,
    pub side_a: Side,
    pub b: usize,
    pub kind: LinkKind,
}

impl Link {
    pub fn side_b(&self) -> Side {
        self.side_a.opposite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EnemyKind {
    Slime,
    Bat,
    Husk,
    Spitter,
    Warden,
}

impl EnemyKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "slime" => Some(Self::Slime),
            "bat" => Some(Self::Bat),
            "husk" => Some(Self::Husk),
            "spitter" => Some(Self::Spitter),
            "warden" => Some(Self::Warden),
            _ => None,
        }
    }

    /// Half the collision box, art px.
    pub fn half(self) -> f32 {
        match self {
            Self::Slime => 6.0,
            Self::Bat => 5.0,
            Self::Husk => 7.0,
            Self::Spitter => 6.0,
            Self::Warden => 10.0,
        }
    }

    /// Fliers cross pits.
    pub fn flies(self) -> bool {
        self == Self::Bat
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickupKind {
    Heart,
    Key,
    Gem,
    Container,
    Wand,
    Crystal,
}

impl PickupKind {
    fn parse(s: &str) -> Option<Self> {
        match s {
            "heart" => Some(Self::Heart),
            "key" => Some(Self::Key),
            "gem" => Some(Self::Gem),
            "container" => Some(Self::Container),
            "wand" => Some(Self::Wand),
            "crystal" => Some(Self::Crystal),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EnemySpawn {
    pub kind: EnemyKind,
    /// Centre, art px, inside the room.
    pub pos: [f32; 2],
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PickupSpawn {
    pub kind: PickupKind,
    pub pos: [f32; 2],
}

/// Decoration from the look bible's theme props.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropKind {
    Torch,
    Crack,
    Moss,
    Pot,
    Bones,
    Rubble,
    Banner,
    Rack,
    Cobweb,
    Pillar,
    Helmet,
    Puddle,
    Pedestal,
}

impl PropKind {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "torch" => Self::Torch,
            "crack" => Self::Crack,
            "moss" => Self::Moss,
            "pot" => Self::Pot,
            "bones" => Self::Bones,
            "rubble" => Self::Rubble,
            "banner" => Self::Banner,
            "rack" => Self::Rack,
            "cobweb" => Self::Cobweb,
            "pillar" => Self::Pillar,
            "helmet" => Self::Helmet,
            "puddle" => Self::Puddle,
            "pedestal" => Self::Pedestal,
            _ => return None,
        })
    }
}

/// A prop on tile (`tx`, `ty`) of its room.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Prop {
    pub kind: PropKind,
    pub tx: i32,
    pub ty: i32,
}

#[derive(Debug, Clone)]
pub struct Room {
    pub id: String,
    pub name: String,
    pub gx: i32,
    pub gy: i32,
    pub tiles: Vec<Tile>,
    pub enemies: Vec<EnemySpawn>,
    pub pickups: Vec<PickupSpawn>,
    /// Decoration only (no game rules), drawn by the art.
    pub props: Vec<Prop>,
    /// Per side (`Side::index`), the link through that wall, if any.
    pub doors: [Option<usize>; 4],
    pub has_switch: bool,
}

#[derive(Debug, Clone)]
pub struct Level {
    pub name: String,
    pub rooms: Vec<Room>,
    pub links: Vec<Link>,
    pub start_room: usize,
    pub start_pos: [f32; 2],
}

#[derive(Deserialize)]
struct RawSpawn {
    kind: String,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct RawRoom {
    id: String,
    name: String,
    gx: i32,
    gy: i32,
    tiles: Vec<String>,
    #[serde(default)]
    enemies: Vec<RawSpawn>,
    #[serde(default)]
    pickups: Vec<RawSpawn>,
    #[serde(default)]
    props: Vec<RawSpawn>,
}

#[derive(Deserialize)]
struct RawLink {
    a: String,
    side: String,
    b: String,
    kind: String,
}

#[derive(Deserialize)]
struct RawStart {
    room: String,
    x: i32,
    y: i32,
}

#[derive(Deserialize)]
struct RawLevel {
    name: String,
    start: RawStart,
    rooms: Vec<RawRoom>,
    links: Vec<RawLink>,
}

fn centre(x: i32, y: i32) -> [f32; 2] {
    [x as f32 * TILE + TILE * 0.5, y as f32 * TILE + TILE * 0.5]
}

impl Level {
    /// Parse and validate the level data; the error says what is wrong.
    pub fn from_json(json: &str) -> Result<Level, String> {
        let raw: RawLevel = serde_json::from_str(json).map_err(|e| format!("level.json: {e}"))?;
        if raw.rooms.is_empty() {
            return Err("level.json: no rooms".into());
        }
        let find = |id: &str| raw.rooms.iter().position(|r| r.id == id).ok_or_else(|| format!("level.json: no room `{id}`"));
        let mut rooms = Vec::new();
        for r in &raw.rooms {
            if r.tiles.len() != ROOM_H {
                return Err(format!("level.json: room `{}` has {} rows, want {ROOM_H}", r.id, r.tiles.len()));
            }
            let mut tiles = Vec::with_capacity(ROOM_W * ROOM_H);
            let mut has_switch = false;
            for (y, row) in r.tiles.iter().enumerate() {
                if row.chars().count() != ROOM_W {
                    return Err(format!("level.json: room `{}` row {y} has {} tiles, want {ROOM_W}", r.id, row.chars().count()));
                }
                for ch in row.chars() {
                    tiles.push(match ch {
                        '#' => Tile::Wall,
                        '.' | ',' => Tile::Floor,
                        'P' => Tile::Pit,
                        'S' => {
                            has_switch = true;
                            Tile::Switch
                        }
                        'G' => Tile::Gate,
                        other => return Err(format!("level.json: room `{}` row {y}: unknown tile `{other}`", r.id)),
                    });
                }
            }
            let in_room = |s: &RawSpawn| (0..ROOM_W as i32).contains(&s.x) && (0..ROOM_H as i32).contains(&s.y);
            let mut enemies = Vec::new();
            for s in &r.enemies {
                let kind = EnemyKind::parse(&s.kind).ok_or_else(|| format!("level.json: room `{}`: unknown enemy `{}`", r.id, s.kind))?;
                if !in_room(s) {
                    return Err(format!("level.json: room `{}`: enemy outside the room", r.id));
                }
                enemies.push(EnemySpawn { kind, pos: centre(s.x, s.y) });
            }
            let mut pickups = Vec::new();
            for s in &r.pickups {
                let kind = PickupKind::parse(&s.kind).ok_or_else(|| format!("level.json: room `{}`: unknown pickup `{}`", r.id, s.kind))?;
                if !in_room(s) {
                    return Err(format!("level.json: room `{}`: pickup outside the room", r.id));
                }
                pickups.push(PickupSpawn { kind, pos: centre(s.x, s.y) });
            }
            let mut props = Vec::new();
            for s in &r.props {
                let kind = PropKind::parse(&s.kind).ok_or_else(|| format!("level.json: room `{}`: unknown prop `{}`", r.id, s.kind))?;
                if !in_room(s) {
                    return Err(format!("level.json: room `{}`: prop outside the room", r.id));
                }
                props.push(Prop { kind, tx: s.x, ty: s.y });
            }
            rooms.push(Room { id: r.id.clone(), name: r.name.clone(), gx: r.gx, gy: r.gy, tiles, enemies, pickups, props, doors: [None; 4], has_switch });
        }
        let mut links = Vec::new();
        for l in &raw.links {
            let a = find(&l.a)?;
            let b = find(&l.b)?;
            let side_a = Side::parse(&l.side).ok_or_else(|| format!("level.json: link {} -> {}: bad side `{}`", l.a, l.b, l.side))?;
            let kind = match l.kind.as_str() {
                "open" => LinkKind::Open,
                "locked" => LinkKind::Locked,
                other => return Err(format!("level.json: link {} -> {}: bad kind `{other}`", l.a, l.b)),
            };
            let (dx, dy) = side_a.delta();
            if rooms[b].gx != rooms[a].gx + dx || rooms[b].gy != rooms[a].gy + dy {
                return Err(format!("level.json: link {} -> {} does not match the room grid", l.a, l.b));
            }
            let idx = links.len();
            for (room, side) in [(a, side_a), (b, side_a.opposite())] {
                if rooms[room].doors[side.index()].is_some() {
                    return Err(format!("level.json: room `{}` has two links on one side", rooms[room].id));
                }
                rooms[room].doors[side.index()] = Some(idx);
            }
            links.push(Link { a, side_a, b, kind });
        }
        let start_room = find(&raw.start.room)?;
        Ok(Level { name: raw.name, rooms, links, start_room, start_pos: centre(raw.start.x, raw.start.y) })
    }

    /// What the tile `(tx, ty)` of `room` is for movement; positions outside
    /// the room are wall except one step out through a doorway.
    pub fn cell(&self, room: usize, tx: i32, ty: i32) -> Cell {
        let r = &self.rooms[room];
        if let Some(side) = doorway_side(tx, ty) {
            return match r.doors[side.index()] {
                Some(l) => Cell::Door(l),
                None => Cell::Wall,
            };
        }
        if tx < 0 || ty < 0 || tx >= ROOM_W as i32 || ty >= ROOM_H as i32 {
            return Cell::Wall;
        }
        match r.tiles[ty as usize * ROOM_W + tx as usize] {
            Tile::Floor => Cell::Floor,
            Tile::Wall => Cell::Wall,
            Tile::Pit => Cell::Pit,
            Tile::Switch => Cell::Switch,
            Tile::Gate => Cell::Gate,
        }
    }

    /// The room across the doorway on `side` of `room`, with its link.
    pub fn neighbour(&self, room: usize, side: Side) -> Option<(usize, usize)> {
        let l = self.rooms[room].doors[side.index()]?;
        let link = &self.links[l];
        Some((if link.a == room { link.b } else { link.a }, l))
    }
}
