// evani.shader.particles_sim
// Shared particles, compute pass `particles.sim` (feature `particles`).
// Every slot of the spawn-record ring is evaluated in closed form from its
// age (ticks since its fixed-step spawn plus the frame's alpha), so nothing
// is integrated frame by frame and nothing drifts. Writes one instance per
// slot for the sprites and forward passes to draw; dead slots get the key
// no draw matches. Same formula as `particles.rs` `eval` (the CPU reference).

struct Rec {
    pos: vec3<f32>,
    spawn: u32,
    vel: vec3<f32>,
    life: f32,
    color: vec4<f32>,
    uv: vec4<f32>,
    size: f32,
    size_end: f32,
    aspect: f32,
    spin: f32,
    gravity: f32,
    drag: f32,
    key: u32,
    flags: u32,
};

struct Inst {
    center: vec3<f32>,
    width: f32,
    height: f32,
    rot: f32,
    pad: f32,
    key: u32,
    uv: vec4<f32>,
    color: vec4<f32>,
};

struct Sim {
    now: u32,
    frac: f32,
    dt: f32,
    count: u32,
};

const KEY_WORLD: u32 = 0u;
const KEY_DEAD: u32 = 0xffffffffu;
const FADE_TAIL: f32 = 0.25;

@group(0) @binding(0) var<storage, read> recs: array<Rec>;
@group(0) @binding(1) var<storage, read_write> outs: array<Inst>;
@group(0) @binding(2) var<uniform> sim: Sim;

@compute @workgroup_size(64)
fn cs_main(@builtin(global_invocation_id) id: vec3<u32>) {
    let i = id.x;
    if (i >= sim.count || i >= arrayLength(&recs) || i >= arrayLength(&outs)) {
        return;
    }
    let r = recs[i];
    var o: Inst;
    o.key = KEY_DEAD;
    o.uv = r.uv;
    let t = max(f32(sim.now - r.spawn) + sim.frac - 1.0, 0.0) * sim.dt;
    if (t < r.life) {
        // Velocity decays as exp(-drag t): the start velocity carries the
        // particle f, gravity h (power series below drag t = 0.5, where the
        // exact form loses its digits to cancellation).
        let kt = r.drag * t;
        let x = -kt;
        var f = t * (1.0 + x * (1.0 / 2.0 + x * (1.0 / 6.0 + x * (1.0 / 24.0 + x * (1.0 / 120.0
            + x * (1.0 / 720.0 + x * (1.0 / 5040.0 + x / 40320.0)))))));
        var h = t * t * (1.0 / 2.0 + x * (1.0 / 6.0 + x * (1.0 / 24.0 + x * (1.0 / 120.0
            + x * (1.0 / 720.0 + x * (1.0 / 5040.0 + x * (1.0 / 40320.0 + x / 362880.0)))))));
        if (kt >= 0.5) {
            f = (1.0 - exp(-kt)) / r.drag;
            h = (t - f) / r.drag;
        }
        // Gravity pulls down: +y on screen, -y in the world.
        var g = r.gravity;
        if (r.key == KEY_WORLD) {
            g = -g;
        }
        let u = t / r.life;
        let w = r.size * (1.0 + (r.size_end - 1.0) * u);
        let a = r.color.a * clamp((1.0 - u) / FADE_TAIL, 0.0, 1.0);
        o.center = r.pos + r.vel * f + vec3<f32>(0.0, g * h, 0.0);
        o.width = w;
        o.height = w * r.aspect;
        o.rot = r.spin * t;
        o.key = r.key;
        // Premultiplied: additive particles cover nothing (alpha 0).
        o.color = vec4<f32>(r.color.rgb * a, select(a, 0.0, (r.flags & 1u) != 0u));
    }
    outs[i] = o;
}
