//! The combat module's live tunables, `combat.*`: every number in
//! [`Tuning`], so a sweep or `param_set` from the live tools changes the
//! fight on the next fixed step, and a kit bakes its chosen values into its
//! parameter file.
//!
//! Names: `combat.<gun>.<field>` for the guns (`rifle`, `pistol`,
//! `launcher`; every [`WeaponStats`] field), `combat.grenade.*`,
//! `combat.rocket.*`, `combat.bot.*` (every [`crate::BotTune`] field),
//! `combat.sound.*`, and the top-level `combat.health`, `combat.armour`,
//! `combat.helmet`, `combat.run_speed` and `combat.respawn`.
//!
//! [`CombatModule`] registers them and puts a [`CombatParams`] resource in
//! the world. A game reads the table every fixed step with
//! [`CombatParams::apply`] (no allocation; names are left as they are) or
//! builds a fresh one with [`CombatParams::tuning`].

use core::fmt::Write;

use evani_core::ecs::World;
use evani_core::params::{BakeTarget, BoolParam, F32Param, I32Param, ParamDef, ParamRegistry};
use evani_core::plugin::{EngineModule, PluginContext, PluginError};

use crate::tune::{BotTune, GrenadeStats, RocketStats, SoundTune, Tuning, WeaponStats};

/// One `f32` field of a stats struct: its key, range, unit, description
/// and where it lives.
struct Field<T> {
    key: &'static str,
    min: f32,
    max: f32,
    unit: &'static str,
    what: &'static str,
    at: fn(&mut T) -> &mut f32,
}

const fn f<T>(
    key: &'static str,
    min: f32,
    max: f32,
    unit: &'static str,
    what: &'static str,
    at: fn(&mut T) -> &mut f32,
) -> Field<T> {
    Field {
        key,
        min,
        max,
        unit,
        what,
        at,
    }
}

const WEAPON: [Field<WeaponStats>; 19] = [
    f(
        "damage",
        0.0,
        500.0,
        "hp",
        "Damage of a chest hit at point blank.",
        |w| &mut w.damage,
    ),
    f(
        "armor_pen",
        0.0,
        1.0,
        "share",
        "Share of the damage that goes through armour.",
        |w| &mut w.armor_pen,
    ),
    f(
        "head_mult",
        0.0,
        10.0,
        "x",
        "Damage multiplier for a head hit.",
        |w| &mut w.head_mult,
    ),
    f(
        "stomach_mult",
        0.0,
        10.0,
        "x",
        "Damage multiplier for a stomach hit.",
        |w| &mut w.stomach_mult,
    ),
    f(
        "leg_mult",
        0.0,
        10.0,
        "x",
        "Damage multiplier for a leg hit.",
        |w| &mut w.leg_mult,
    ),
    f(
        "range_mod",
        0.0,
        1.0,
        "share",
        "Share of the damage kept per 9.525 m of range (1 = no falloff).",
        |w| &mut w.range_mod,
    ),
    f("interval", 0.01, 5.0, "s", "Seconds between shots.", |w| {
        &mut w.interval
    }),
    f("reload", 0.0, 10.0, "s", "Seconds to reload.", |w| {
        &mut w.reload
    }),
    f(
        "range",
        0.0,
        1000.0,
        "m",
        "Longest shot (0 = no limit for falloff).",
        |w| &mut w.range,
    ),
    f(
        "spread",
        0.0,
        0.5,
        "rad",
        "Cone half-angle standing still with a settled gun.",
        |w| &mut w.spread,
    ),
    f(
        "spread_move",
        0.0,
        1.0,
        "rad",
        "Extra cone at full run speed.",
        |w| &mut w.spread_move,
    ),
    f(
        "spread_air",
        0.0,
        1.0,
        "rad",
        "Extra cone in the air.",
        |w| &mut w.spread_air,
    ),
    f(
        "spread_fire",
        0.0,
        0.2,
        "rad",
        "Extra cone per shot of heat.",
        |w| &mut w.spread_fire,
    ),
    f(
        "crouch_mult",
        0.0,
        2.0,
        "x",
        "Crouching multiplies the cone by this.",
        |w| &mut w.crouch_mult,
    ),
    f(
        "kick",
        0.0,
        0.5,
        "rad",
        "View kick up per shot, growing with heat.",
        |w| &mut w.kick,
    ),
    f(
        "kick_side",
        0.0,
        2.0,
        "share",
        "Sideways kick as a share of the up kick (random sign).",
        |w| &mut w.kick_side,
    ),
    f(
        "kick_max",
        0.0,
        1.5,
        "rad",
        "Most view kick that builds up.",
        |w| &mut w.kick_max,
    ),
    f(
        "recover",
        0.0,
        10.0,
        "rad/s",
        "How fast the view comes back down.",
        |w| &mut w.recover,
    ),
    f(
        "cool",
        0.0,
        50.0,
        "shots/s",
        "Heat lost per second after a short pause.",
        |w| &mut w.cool,
    ),
];

const GRENADE: [Field<GrenadeStats>; 10] = [
    f(
        "damage",
        0.0,
        500.0,
        "hp",
        "Grenade damage at the centre of the blast.",
        |g| &mut g.damage,
    ),
    f(
        "radius",
        0.0,
        30.0,
        "m",
        "Grenade blast radius; damage falls linearly to 0 there.",
        |g| &mut g.radius,
    ),
    f(
        "armor_pen",
        0.0,
        1.0,
        "share",
        "Share of the grenade's damage that goes through armour.",
        |g| &mut g.armor_pen,
    ),
    f(
        "fuse",
        0.05,
        10.0,
        "s",
        "Seconds from the throw to the blast.",
        |g| &mut g.fuse,
    ),
    f("speed", 0.0, 60.0, "m/s", "Throw speed.", |g| &mut g.speed),
    f(
        "loft",
        -0.5,
        1.0,
        "rad",
        "Extra upward pitch on the throw.",
        |g| &mut g.loft,
    ),
    f(
        "gravity",
        0.0,
        50.0,
        "m/s²",
        "Gravity on a grenade.",
        |g| &mut g.gravity,
    ),
    f(
        "restitution",
        0.0,
        1.0,
        "share",
        "Bounce: share of the normal speed kept.",
        |g| &mut g.restitution,
    ),
    f(
        "friction",
        0.0,
        1.0,
        "share",
        "Share of the sliding speed lost per bounce.",
        |g| &mut g.friction,
    ),
    f("size", 0.01, 1.0, "m", "Grenade collision radius.", |g| {
        &mut g.size
    }),
];

const ROCKET: [Field<RocketStats>; 7] = [
    f(
        "damage",
        0.0,
        500.0,
        "hp",
        "Rocket damage at the centre of the blast.",
        |r| &mut r.damage,
    ),
    f(
        "radius",
        0.0,
        30.0,
        "m",
        "Rocket blast radius; damage falls linearly to 0 there.",
        |r| &mut r.radius,
    ),
    f(
        "armor_pen",
        0.0,
        1.0,
        "share",
        "Share of the rocket's damage that goes through armour.",
        |r| &mut r.armor_pen,
    ),
    f("speed", 0.0, 200.0, "m/s", "Rocket launch speed.", |r| {
        &mut r.speed
    }),
    f(
        "gravity",
        0.0,
        50.0,
        "m/s²",
        "Gravity on a rocket (a slight drop over long shots).",
        |r| &mut r.gravity,
    ),
    f("size", 0.01, 1.0, "m", "Rocket collision radius.", |r| {
        &mut r.size
    }),
    f(
        "life",
        0.1,
        30.0,
        "s",
        "Seconds of flight before a rocket goes off on its own.",
        |r| &mut r.life,
    ),
];

const BOT: [Field<BotTune>; 20] = [
    f(
        "react",
        0.0,
        5.0,
        "s",
        "Seconds a foe must be in sight before a bot's first shot.",
        |b| &mut b.react,
    ),
    f(
        "aim_err",
        0.0,
        0.5,
        "rad",
        "Bot aim error at range (one axis).",
        |b| &mut b.aim_err,
    ),
    f(
        "aim_near",
        0.0,
        100.0,
        "m",
        "Bot aim error grows near: err * (1 + aim_near / max(d, 6)).",
        |b| &mut b.aim_near,
    ),
    f(
        "settle",
        0.0,
        20.0,
        "s",
        "Seconds for a bot's aim error to settle.",
        |b| &mut b.settle,
    ),
    f(
        "settle_min",
        0.0,
        1.0,
        "share",
        "Share of the aim error left once settled.",
        |b| &mut b.settle_min,
    ),
    f(
        "scan",
        0.01,
        5.0,
        "s",
        "Seconds between a bot's looks for foes.",
        |b| &mut b.scan,
    ),
    f(
        "scan_jitter",
        0.0,
        5.0,
        "s",
        "Random extra time added to each scan period.",
        |b| &mut b.scan_jitter,
    ),
    f("turn", 0.1, 60.0, "rad/s", "A bot's fastest turn.", |b| {
        &mut b.turn
    }),
    f(
        "fire_cone",
        0.0,
        1.0,
        "rad",
        "A bot fires when its aim is this close to where it wants it.",
        |b| &mut b.fire_cone,
    ),
    f("view", 0.0, 500.0, "m", "How far a bot sees.", |b| {
        &mut b.view
    }),
    f(
        "fov",
        0.0,
        3.2,
        "rad",
        "A bot's field of view either side of its heading.",
        |b| &mut b.fov,
    ),
    f(
        "hearing",
        0.0,
        5.0,
        "x",
        "Sound radii are scaled by this when a bot listens.",
        |b| &mut b.hearing,
    ),
    f(
        "lost",
        0.0,
        20.0,
        "s",
        "Seconds a bot keeps pressing a lost foe before investigating.",
        |b| &mut b.lost,
    ),
    f(
        "investigate",
        0.0,
        60.0,
        "s",
        "Seconds an investigation lasts at most.",
        |b| &mut b.investigate,
    ),
    f(
        "retreat_hp",
        0.0,
        500.0,
        "hp",
        "Health under which a bot backs off once.",
        |b| &mut b.retreat_hp,
    ),
    f(
        "retreat_time",
        0.0,
        20.0,
        "s",
        "Seconds a retreat lasts.",
        |b| &mut b.retreat_time,
    ),
    f(
        "strafe",
        0.05,
        10.0,
        "s",
        "Seconds between strafe flips while fighting up close.",
        |b| &mut b.strafe,
    ),
    f(
        "burst_range",
        0.0,
        500.0,
        "m",
        "Beyond this range a bot taps bursts.",
        |b| &mut b.burst_range,
    ),
    f("burst_pause", 0.0, 5.0, "s", "Pause between bursts.", |b| {
        &mut b.burst_pause
    }),
    f(
        "reach",
        0.05,
        5.0,
        "m",
        "A bot reaches a waypoint within this.",
        |b| &mut b.reach,
    ),
];

const SOUND: [Field<SoundTune>; 4] = [
    f(
        "gunshot",
        0.0,
        500.0,
        "m",
        "How far a gunshot carries.",
        |s| &mut s.gunshot,
    ),
    f(
        "footstep",
        0.0,
        100.0,
        "m",
        "How far a running footstep carries.",
        |s| &mut s.footstep,
    ),
    f("blast", 0.0, 500.0, "m", "How far a blast carries.", |s| {
        &mut s.blast
    }),
    f(
        "bounce",
        0.0,
        100.0,
        "m",
        "How far a grenade bounce carries.",
        |s| &mut s.bounce,
    ),
];

const TOP: [Field<Tuning>; 4] = [
    f("health", 1.0, 1000.0, "hp", "Health at spawn.", |t| {
        &mut t.health
    }),
    f("armour", 0.0, 1000.0, "ap", "Armour at spawn.", |t| {
        &mut t.armour
    }),
    f(
        "run_speed",
        0.1,
        20.0,
        "m/s",
        "Speed counted as full run for the move spread.",
        |t| &mut t.run_speed,
    ),
    f(
        "respawn",
        0.0,
        60.0,
        "s",
        "Seconds before a dead player comes back.",
        |t| &mut t.respawn,
    ),
];

/// The gun slots, in [`CombatParams`] order.
pub const GUNS: [&str; 3] = ["rifle", "pistol", "launcher"];

fn gun(t: &mut Tuning, i: usize) -> &mut WeaponStats {
    match i {
        0 => &mut t.rifle,
        1 => &mut t.pistol,
        _ => &mut t.launcher,
    }
}

fn reg_fields<T: Clone, const N: usize>(
    ctx: &mut PluginContext,
    prefix: &str,
    fields: &[Field<T>; N],
    defaults: &T,
) -> Result<[F32Param; N], PluginError> {
    let mut d = defaults.clone();
    let mut out = Vec::with_capacity(N);
    let mut name = String::new();
    for fl in fields.iter() {
        name.clear();
        let _ = write!(name, "{prefix}{}", fl.key);
        let v = *(fl.at)(&mut d);
        out.push(
            ctx.param_f32(
                ParamDef::f32(&name, v)
                    .range(fl.min, fl.max)
                    .unit(fl.unit)
                    .describe(fl.what)
                    .owner("combat")
                    .bake(BakeTarget::Kit),
            )?,
        );
    }
    <[F32Param; N]>::try_from(out)
        .map_err(|_| PluginError(format!("combat: {prefix}* field count")))
}

fn reg_gun(ctx: &mut PluginContext, slot: &str, w: &WeaponStats) -> Result<GunParams, PluginError> {
    let fields = reg_fields(ctx, &format!("combat.{slot}."), &WEAPON, w)?;
    let auto = ctx.param_bool(
        ParamDef::bool(&format!("combat.{slot}.auto"), w.auto)
            .describe("Keeps firing while the trigger is held.")
            .owner("combat")
            .bake(BakeTarget::Kit),
    )?;
    let mag = ctx.param_i32(count(
        w.mag,
        &format!("combat.{slot}.mag"),
        500.0,
        "rounds",
        "Rounds per magazine.",
    ))?;
    let reserve = ctx.param_i32(count(
        w.reserve,
        &format!("combat.{slot}.reserve"),
        2000.0,
        "rounds",
        "Spare rounds carried at spawn.",
    ))?;
    Ok(GunParams {
        fields,
        auto,
        mag,
        reserve,
    })
}

fn read_fields<T, const N: usize>(
    reg: &ParamRegistry,
    fields: &[Field<T>; N],
    handles: &[F32Param; N],
    into: &mut T,
) {
    for (fl, h) in fields.iter().zip(handles.iter()) {
        *(fl.at)(into) = reg.f32(*h);
    }
}

fn count(def: u32, name: &str, max: f32, unit: &str, what: &str) -> ParamDef {
    ParamDef::i32(name, def as i32)
        .range(0.0, max)
        .unit(unit)
        .describe(what)
        .owner("combat")
        .bake(BakeTarget::Kit)
}

fn as_count(reg: &ParamRegistry, h: I32Param) -> u32 {
    let v = reg.i32(h);
    if v > 0 { v as u32 } else { 0 }
}

/// One gun's handles.
#[derive(Debug, Clone, Copy)]
pub struct GunParams {
    pub fields: [F32Param; 19],
    /// `combat.<gun>.auto`
    pub auto: BoolParam,
    /// `combat.<gun>.mag`
    pub mag: I32Param,
    /// `combat.<gun>.reserve`
    pub reserve: I32Param,
}

/// Handles of every `combat.*` parameter (a world resource).
#[derive(Debug, Clone, Copy)]
pub struct CombatParams {
    /// Rifle, pistol, launcher (the order of [`GUNS`]).
    pub guns: [GunParams; 3],
    pub grenade: [F32Param; 10],
    /// `combat.grenade.carry`
    pub grenade_carry: I32Param,
    pub rocket: [F32Param; 7],
    pub bot: [F32Param; 20],
    /// `combat.bot.burst`
    pub bot_burst: I32Param,
    pub sound: [F32Param; 4],
    /// `combat.health`, `combat.armour`, `combat.run_speed`, `combat.respawn`.
    pub top: [F32Param; 4],
    /// `combat.helmet`
    pub helmet: BoolParam,
}

impl CombatParams {
    /// Register every `combat.*` parameter with [`Tuning::default`] as the
    /// defaults.
    pub fn register(ctx: &mut PluginContext) -> Result<Self, PluginError> {
        let d = Tuning::default();
        Ok(CombatParams {
            guns: [
                reg_gun(ctx, GUNS[0], &d.rifle)?,
                reg_gun(ctx, GUNS[1], &d.pistol)?,
                reg_gun(ctx, GUNS[2], &d.launcher)?,
            ],
            grenade: reg_fields(ctx, "combat.grenade.", &GRENADE, &d.grenade)?,
            grenade_carry: ctx.param_i32(count(
                d.grenade.carry,
                "combat.grenade.carry",
                20.0,
                "grenades",
                "Grenades carried per life.",
            ))?,
            rocket: reg_fields(ctx, "combat.rocket.", &ROCKET, &d.rocket)?,
            bot: reg_fields(ctx, "combat.bot.", &BOT, &d.bot)?,
            bot_burst: ctx.param_i32(count(
                d.bot.burst,
                "combat.bot.burst",
                100.0,
                "shots",
                "Shots per burst when a bot taps at range.",
            ))?,
            sound: reg_fields(ctx, "combat.sound.", &SOUND, &d.sound)?,
            top: reg_fields(ctx, "combat.", &TOP, &d)?,
            helmet: ctx.param_bool(
                ParamDef::bool("combat.helmet", d.helmet)
                    .describe("Spawn armour includes a helmet.")
                    .owner("combat")
                    .bake(BakeTarget::Kit),
            )?,
        })
    }

    /// Write the current values into `t` (names stay as they are; no
    /// allocation), for a game to call every fixed step.
    pub fn apply(&self, reg: &ParamRegistry, t: &mut Tuning) {
        for (i, g) in self.guns.iter().enumerate() {
            let w = gun(t, i);
            read_fields(reg, &WEAPON, &g.fields, w);
            w.auto = reg.bool(g.auto);
            w.mag = as_count(reg, g.mag);
            w.reserve = as_count(reg, g.reserve);
        }
        read_fields(reg, &GRENADE, &self.grenade, &mut t.grenade);
        t.grenade.carry = as_count(reg, self.grenade_carry);
        read_fields(reg, &ROCKET, &self.rocket, &mut t.rocket);
        read_fields(reg, &BOT, &self.bot, &mut t.bot);
        t.bot.burst = as_count(reg, self.bot_burst);
        read_fields(reg, &SOUND, &self.sound, &mut t.sound);
        read_fields(reg, &TOP, &self.top, t);
        t.helmet = reg.bool(self.helmet);
    }

    /// A fresh table from the current values.
    pub fn tuning(&self, reg: &ParamRegistry) -> Tuning {
        let mut t = Tuning::default();
        self.apply(reg, &mut t);
        t
    }
}

/// The combat module (`evani.module.combat`): registers the `combat.*`
/// parameters and adds the [`CombatParams`] resource. Add it next to
/// `PhysicsModule` in a kit or app that fights.
#[derive(Debug, Default)]
pub struct CombatModule {
    params: Option<CombatParams>,
}

impl CombatModule {
    pub fn new() -> Self {
        Self::default()
    }
}

impl EngineModule for CombatModule {
    fn id(&self) -> &'static str {
        "evani.module.combat"
    }

    fn label(&self) -> &'static str {
        "combat"
    }

    fn version(&self) -> Option<&'static str> {
        Some(env!("CARGO_PKG_VERSION"))
    }

    fn build(&mut self, ctx: &mut PluginContext) -> Result<(), PluginError> {
        let p = CombatParams::register(ctx)?;
        ctx.resource(p);
        self.params = Some(p);
        Ok(())
    }

    fn status(&self, world: &World, out: &mut String) {
        out.push_str("hitscan · guns · grenades · rockets · bots");
        if let Some(p) = self.params {
            let reg = world.res::<ParamRegistry>();
            let _ = write!(out, " · health {}", reg.f32(p.top[0]) as i32);
        }
    }
}
