//! Determinism: the same seed and the same inputs give the same game, tick
//! for tick. A bot plays each level through the pointer (press on a piece,
//! drag onto its neighbour) and the whole game state (board, pieces and
//! their animation, pops, blasts, floating text, score, goals,
//! Puff) is folded into one hash every 20 ticks.
//!
//! The expected hashes were recorded on the kit before its pieces moved into
//! the ECS (commit 679d20d, `Game` holding the board and the effects), so
//! this test also proves the move changed no gameplay and no visual state.

use evani_core::input::PrimaryPointer;

use crate::Tune;
use crate::board::{Kind, Piece, Rules};
use crate::ecs::{Blast, Cell, Floats, Motion, Popping};
use crate::game::{Fns, FxFlags, Mood, Phase, PlayerInput, Request, SimWorld, Step, default_match_score_in};
use crate::layout::{CellLimits, layout};
use crate::level::levels_from_json;

/// FNV-1a, 64 bit.
struct Fnv(u64);

impl Fnv {
    fn new() -> Self {
        Self(0xcbf2_9ce4_8422_2325)
    }
    fn u32(&mut self, v: u32) {
        for b in v.to_le_bytes() {
            self.0 ^= u64::from(b);
            self.0 = self.0.wrapping_mul(0x0100_0000_01b3);
        }
    }
    fn f(&mut self, v: f32) {
        self.u32(v.to_bits());
    }
    fn i(&mut self, v: i32) {
        self.u32(v as u32);
    }
}

fn kind_n(k: Kind) -> u32 {
    match k {
        Kind::Normal => 0,
        Kind::StreakH => 1,
        Kind::StreakV => 2,
        Kind::Bomb => 3,
        Kind::Prism => 4,
    }
}

fn phase_n(p: Phase) -> u32 {
    match p {
        Phase::Title => 0,
        Phase::Intro => 1,
        Phase::Play => 2,
        Phase::Rush => 3,
        Phase::Won => 4,
        Phase::Lost => 5,
    }
}

fn mood_n(m: Mood) -> u32 {
    match m {
        Mood::Idle => 0,
        Mood::Happy => 1,
        Mood::Excited => 2,
        Mood::Worried => 3,
        Mood::Sad => 4,
        Mood::Cheer => 5,
    }
}

/// The whole game state, folded (in the same order as before the ECS move:
/// cells by index, then pops and blasts oldest first).
fn fold(h: &mut Fnv, sim: &SimWorld) {
    let w = &sim.world;
    let g = sim.game();
    let grid = sim.grid();
    h.u32(phase_n(g.phase));
    h.u32(match g.step {
        Step::Idle => 0,
        Step::Swap { .. } => 1,
        Step::Invalid { .. } => 2,
        Step::Clear { .. } => 3,
        Step::Fall => 4,
        Step::Shuffle { .. } => 5,
    });
    h.i(g.moves_left);
    h.i(g.moves_used);
    h.u32(g.score);
    h.u32(u32::from(g.stars));
    h.u32(g.cascade);
    h.u32(g.best_cascade);
    h.u32(g.selected.map_or(u32::MAX, |s| s as u32));
    h.u32(g.cursor as u32);
    for c in g.collected {
        h.u32(c);
    }
    h.f(g.trauma);
    h.u32(mood_n(grid.puff(w).mood));
    for f in &grid.board.frost {
        h.u32(u32::from(*f));
    }
    for (i, c) in grid.board.cells.iter().enumerate() {
        match c {
            None => {
                assert!(grid.slots[i].is_none(), "cell {i}: an entity without a piece");
                h.u32(0xFFFF_FFFF);
            }
            Some(p) => {
                let e = grid.slots[i].expect("a piece's entity");
                let pc = *w.get::<Piece>(e).expect("its Piece");
                assert_eq!(pc, *p, "cell {i}: the board and the entity agree");
                assert_eq!(w.get::<Cell>(e).map(|c| usize::from(c.0)), Some(i), "cell {i}: its Cell");
                h.u32(pc.id);
                h.u32(u32::from(pc.color));
                h.u32(kind_n(pc.kind));
                let a = *w.get::<Motion>(e).expect("its Motion");
                h.f(a.off[0]);
                h.f(a.off[1]);
                h.f(a.vy);
                h.f(a.land);
                h.f(a.swap_from[0]);
                h.f(a.swap_from[1]);
                h.f(a.swap_t);
                h.f(a.born);
            }
        }
    }
    h.u32(grid.pops.len() as u32);
    for &e in &grid.pops {
        let p = *w.get::<Popping>(e).expect("a pop");
        h.f(p.x);
        h.f(p.y);
        h.f(p.t);
        h.u32(w.get::<Piece>(e).expect("its piece").id);
    }
    h.u32(grid.blasts.len() as u32);
    for &e in &grid.blasts {
        let b = *w.get::<Blast>(e).expect("a blast");
        h.f(b.x);
        h.f(b.y);
        h.f(b.t);
        h.u32(kind_n(b.kind));
        h.u32(u32::from(b.color));
    }
    let floats = w.res::<Floats>();
    h.u32(floats.items.len() as u32);
    for f in &floats.items {
        h.u32(f.id);
        h.f(f.x);
        h.f(f.y);
        h.f(f.age);
        h.u32(u32::from(f.style));
        for b in f.text.as_str().bytes() {
            h.u32(u32::from(b));
        }
    }
}

/// Play one scenario and return (hash, moves made, final phase).
fn run(level: Option<usize>, best: bool, ticks: u32) -> (u64, u32, Phase) {
    let t = Tune::default();
    let fx = FxFlags::default();
    let fns = Fns {
        match_score: &default_match_score_in,
    };
    let mut sim = SimWorld::new(levels_from_json(crate::LEVELS_JSON).unwrap(), Rules::default());
    sim.game_mut().layout = layout(1280.0, 720.0, 8, 8, CellLimits::default());
    if let Some(l) = level {
        sim.request(Request::Start(l));
    }
    let mut h = Fnv::new();
    let mut moves = 0;
    // A drag in progress: (press position, target position, ticks left).
    let mut drag: Option<([f32; 2], [f32; 2], u32)> = None;
    for tick in 0..ticks {
        let mut input = PlayerInput::default();
        if let Some((from, to, left)) = drag {
            // Second tick: the pointer has moved onto the neighbour.
            input.pointer = PrimaryPointer {
                pos: to,
                press_pos: from,
                down: left > 1,
                released: left == 1,
                ..PrimaryPointer::default()
            };
            drag = if left > 1 { Some((from, to, left - 1)) } else { None };
        } else {
            let g = sim.game();
            let grid = sim.grid();
            if g.phase == Phase::Play
                && g.step == Step::Idle
                && let Some(m) = if best { grid.board.best_move() } else { grid.board.worst_move() }
            {
                let (ax, ay) = grid.board.xy(m.a);
                let (bx, by) = grid.board.xy(m.b);
                let pa = g.layout.center(ax as f32, ay as f32);
                let pb = g.layout.center(bx as f32, by as f32);
                input.pointer = PrimaryPointer {
                    pos: pa,
                    press_pos: pa,
                    down: true,
                    pressed: true,
                    ..PrimaryPointer::default()
                };
                drag = Some((pa, pb, 2));
                moves += 1;
            }
        }
        sim.tick(&input, &t, &fns, &fx, 1.0 / 60.0);
        sim.game_mut().events.clear();
        if tick % 20 == 0 {
            fold(&mut h, &sim);
        }
    }
    fold(&mut h, &sim);
    let phase = sim.game().phase;
    (h.0, moves, phase)
}


#[test]
fn same_seed_and_inputs_give_the_same_board_hash() {
    let scenarios: [(&str, Option<usize>, bool, u32); 5] = [
        ("title attract board", None, true, 60 * 30),
        ("level 1, best moves", Some(0), true, 60 * 90),
        ("level 1, weakest moves", Some(0), false, 60 * 90),
        ("level 2, best moves", Some(1), true, 60 * 120),
        ("level 3, best moves", Some(2), true, 60 * 120),
    ];
    let mut got = Vec::new();
    for (name, level, best, ticks) in scenarios {
        let a = run(level, best, ticks);
        let b = run(level, best, ticks);
        assert_eq!(a, b, "{name}: two runs differ");
        got.push((name, a));
    }
    for (name, (hash, moves, phase)) in &got {
        eprintln!("{name}: hash {hash:#018x}, {moves} moves, ends {}", phase.as_str());
    }
    let hashes: Vec<u64> = got.iter().map(|(_, (h, _, _))| *h).collect();
    assert_eq!(hashes, EXPECTED, "the game changed: {got:?}");
}

/// Sparkles left the fold when they moved onto the shared GPU particle
/// system (kit 003, core request "shared particles"): the kit keeps no
/// particle pool any more, and particles are visual only. The hashes were
/// re-recorded once for that and nothing else: with the sparkle block taken
/// out of the fold on the code before the move, the same five scenarios gave
/// exactly these values, so the board, pieces, pops, blasts, floating text,
/// score, goals and Puff are bit for bit what they were (the old values with
/// sparkles folded in: 0x3554_452d_b9f4_d253, 0xd180_e713_6885_a1b8,
/// 0x6cf6_1291_eb45_139a, 0x507f_ebb9_5851_f48b, 0x4f97_9378_db57_a3da).
///
/// Recorded before the ECS move (see the module docs), then re-recorded once
/// for an intended change to the floating text only (kit 003 review, after
/// the geometry check): score texts now end before the pops do (9/10 of the
/// pop time, at most `float_s`) and texts
/// within one text size merge into one, and combo callouts last 0.8 s instead
/// of 1.1 s, so no popup covers falling pieces. Moves and outcomes are
/// unchanged, and with the old float rules put back the old hashes still came
/// out (0x1b67_df19_c664_e0b4, 0x7052_ac7f_0fb9_c42b, 0x24d0_c632_677a_ce6a,
/// 0xb96b_f8ca_2c7e_93d0, 0x9ade_f0f6_797c_66f0).
const EXPECTED: [u64; 5] = [
    0xd431_bb57_86c0_f706, // title attract board
    0xc24e_64f1_9cce_a11a, // level 1, best moves: 15 moves, won
    0xa654_2321_8d63_2197, // level 1, weakest moves: 18 moves, lost
    0xdec8_7886_5fe0_6785, // level 2, best moves: 11 moves, won
    0x284f_eb73_53fc_26cb, // level 3, best moves: 17 moves, won
];
