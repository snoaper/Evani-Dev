//! Float kernel for the cross-target determinism probe. No dependencies, so
//! the same file compiles into the native test and, through
//! `float_probe_wasm.rs`, into a wasm32 module with no imports.
//!
//! Each family runs one kind of float operation over seeded inputs and
//! folds the exact result bits into an FNV-1a hash. Equal hashes on two
//! targets mean the family gave the same bits on every input.

/// Family names, by index.
pub const FAMILIES: [&str; 12] = [
    "add-sub-mul-div",
    "sqrt",
    "mul_add",
    "min-max-abs-copysign",
    "floor-ceil-trunc-round",
    "int-float-casts",
    "physics-step",
    "sin-cos-tan",
    "asin-acos-atan-atan2",
    "exp-ln-log2-exp2",
    "powf-powi-cbrt-hypot",
    "nan-bits",
];

/// Families IEEE 754 defines exactly (correctly rounded, or exact), so
/// every conforming target must agree. The rest go through a maths
/// library (`libm`) that is free to differ in the last bit, plus NaN
/// payloads, which wasm leaves open.
pub const EXACT: usize = 7;

struct Gen(u64);

impl Gen {
    fn next(&mut self) -> u32 {
        // xorshift64*
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        (self.0.wrapping_mul(0x2545_f491_4f6c_dd1d) >> 32) as u32
    }

    /// Uniform-ish in [lo, hi).
    fn f(&mut self, lo: f32, hi: f32) -> f32 {
        lo + (hi - lo) * ((self.next() >> 8) as f32 * (1.0 / 16_777_216.0))
    }

    /// Any finite f32, including subnormals and huge values.
    fn any(&mut self) -> f32 {
        let v = f32::from_bits(self.next());
        if v.is_finite() { v } else { 1.0 }
    }
}

struct Hash(u64);

impl Hash {
    fn put(&mut self, bits: u64) {
        for i in 0..8 {
            self.0 ^= (bits >> (i * 8)) & 0xff;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    fn f(&mut self, v: f32) {
        self.put(u64::from(v.to_bits()));
    }

    fn d(&mut self, v: f64) {
        self.put(v.to_bits());
    }
}

/// Run `family` over `iters` inputs; the hash of every result's bits.
// The NaN family makes NaNs on purpose (`z / z`, `a - a`).
#[allow(clippy::eq_op)]
pub fn run(family: u32, iters: u32) -> u64 {
    let mut g = Gen(0x9e37_79b9_7f4a_7c15 ^ u64::from(family));
    let mut h = Hash(0xcbf2_9ce4_8422_2325);
    for _ in 0..iters {
        match family {
            0 => {
                let (a, b) = (g.any(), g.any());
                let (c, d) = (g.f(-1000.0, 1000.0), g.f(0.001, 50.0));
                h.f(a + b);
                h.f(a - b);
                h.f(a * b);
                if b != 0.0 {
                    h.f(a / b);
                }
                h.f(c / d);
                h.f(c * d + c);
                let (x, y) = (f64::from(c), f64::from(d));
                h.d(x / y);
                h.d(x * y - y);
            }
            1 => {
                let a = g.any().abs();
                h.f(a.sqrt());
                h.f(g.f(0.0, 10.0).sqrt());
                h.d(f64::from(g.f(0.0, 1e6)).sqrt());
            }
            2 => {
                let (a, b, c) = (g.f(-100.0, 100.0), g.f(-100.0, 100.0), g.f(-100.0, 100.0));
                h.f(a.mul_add(b, c));
                h.d(f64::from(a).mul_add(f64::from(b), f64::from(c)));
            }
            3 => {
                let (a, b) = (g.any(), g.any());
                h.f(a.min(b));
                h.f(a.max(b));
                h.f(a.abs());
                h.f(a.copysign(b));
                h.f(-a);
                h.f(a.clamp(-1.0, 1.0));
            }
            4 => {
                let a = g.f(-1e6, 1e6);
                h.f(a.floor());
                h.f(a.ceil());
                h.f(a.trunc());
                h.f(a.round());
                h.f(a.fract());
                h.f((a * 0.5).round());
            }
            5 => {
                let a = g.any();
                let i = g.next() as i32;
                h.put(a as i32 as u32 as u64);
                h.put(u64::from(a as u32));
                h.put(a as i64 as u64);
                h.f(i as f32);
                h.f((i as u32) as f32);
                h.d(f64::from(a));
                h.f(f64::from(a) as f32);
                h.f((f64::from(g.f(-1.0, 1.0)) * 1e40) as f32);
            }
            6 => {
                // A kart-like integration step: speed, drag, steer by a
                // rotation built from a normalised vector (no trig).
                let (mut x, mut y, mut vx, mut vy) = (0.0f32, 0.0f32, 0.0f32, 0.0f32);
                let (mut fx, mut fy) = (1.0f32, 0.0f32);
                for _ in 0..16 {
                    let steer = g.f(-0.05, 0.05);
                    let (nfx, nfy) = (fx - fy * steer, fy + fx * steer);
                    let len = (nfx * nfx + nfy * nfy).sqrt();
                    fx = nfx / len;
                    fy = nfy / len;
                    let thr = g.f(0.0, 1.0);
                    vx = (vx + fx * thr * (1.0 / 60.0) * 20.0) * 0.985;
                    vy = (vy + fy * thr * (1.0 / 60.0) * 20.0) * 0.985;
                    let sp = (vx * vx + vy * vy).sqrt();
                    if sp > 30.0 {
                        vx *= 30.0 / sp;
                        vy *= 30.0 / sp;
                    }
                    x += vx * (1.0 / 60.0);
                    y += vy * (1.0 / 60.0);
                }
                h.f(x);
                h.f(y);
                h.f(vx);
                h.f(vy);
            }
            7 => {
                let a = g.f(-10.0, 10.0);
                h.f(a.sin());
                h.f(a.cos());
                h.f(a.tan());
                h.f((a * 1000.0).sin());
                let d = f64::from(a);
                h.d(d.sin());
                h.d(d.cos());
            }
            8 => {
                let a = g.f(-1.0, 1.0);
                let (y, x) = (g.f(-5.0, 5.0), g.f(-5.0, 5.0));
                h.f(a.asin());
                h.f(a.acos());
                h.f(y.atan());
                h.f(y.atan2(x));
                h.d(f64::from(y).atan2(f64::from(x)));
            }
            9 => {
                let a = g.f(-20.0, 20.0);
                let p = g.f(0.001, 1000.0);
                h.f(a.exp());
                h.f(a.exp2());
                h.f(p.ln());
                h.f(p.log2());
                h.f(p.log10());
                h.d(f64::from(a).exp());
                h.d(f64::from(p).ln());
            }
            10 => {
                let (a, b) = (g.f(0.0, 10.0), g.f(-3.0, 3.0));
                h.f(a.powf(b));
                h.f(a.powi(3));
                h.f(a.powi(-2));
                h.f(b.cbrt());
                h.f(a.hypot(b));
                h.d(f64::from(a).powf(f64::from(b)));
            }
            _ => {
                // NaN results: their sign and payload are not fixed by wasm.
                let a = g.f(1.0, 2.0);
                let z = a - a;
                h.f(z / z);
                h.f((-a).sqrt());
                h.f(f32::INFINITY - f32::INFINITY * a);
                h.f(f32::INFINITY * z);
            }
        }
    }
    h.0
}
