//! SHA-256, written from the FIPS 180-4 description.
//!
//! The round constants and initial hash values are defined by the standard
//! as the first 32 bits of the fractional parts of the cube roots of the
//! first 64 primes and the square roots of the first 8 primes. We compute
//! them here with exact integer roots instead of typing tables in.

fn primes(n: usize) -> Vec<u64> {
    let mut out = Vec::with_capacity(n);
    let mut c = 2u64;
    while out.len() < n {
        if out.iter().all(|p| !c.is_multiple_of(*p)) {
            out.push(c);
        }
        c += 1;
    }
    out
}

/// Largest r with r^k <= v (k = 2 or 3), by bisection on integers.
fn iroot(v: u128, k: u32) -> u128 {
    let (mut lo, mut hi) = (0u128, 1u128 << 64);
    while lo < hi {
        let mid = (lo + hi).div_ceil(2);
        let p = if k == 2 { mid.checked_mul(mid) } else { mid.checked_mul(mid).and_then(|m| m.checked_mul(mid)) };
        match p {
            Some(p) if p <= v => lo = mid,
            _ => hi = mid - 1,
        }
    }
    lo
}

/// First 32 bits of the fractional part of the k-th root of p.
fn frac_root(p: u64, k: u32) -> u32 {
    // floor(root(p) * 2^32) = floor(root(p * 2^(32k))).
    let r = iroot((p as u128) << (32 * k), k);
    (r & 0xffff_ffff) as u32
}

pub struct Sha256 {
    h: [u32; 8],
    k: [u32; 64],
    buf: Vec<u8>,
    total: u64,
}

impl Default for Sha256 {
    fn default() -> Self {
        Self::new()
    }
}

impl Sha256 {
    pub fn new() -> Self {
        let p = primes(64);
        let mut k = [0u32; 64];
        for (i, ki) in k.iter_mut().enumerate() {
            *ki = frac_root(p[i], 3);
        }
        let mut h = [0u32; 8];
        for (i, hi) in h.iter_mut().enumerate() {
            *hi = frac_root(p[i], 2);
        }
        Sha256 { h, k, buf: Vec::with_capacity(64), total: 0 }
    }

    pub fn update(&mut self, data: &[u8]) {
        self.total = self.total.wrapping_add(data.len() as u64);
        let mut d = data;
        if !self.buf.is_empty() {
            let need = 64 - self.buf.len();
            let take = need.min(d.len());
            self.buf.extend_from_slice(&d[..take]);
            d = &d[take..];
            if self.buf.len() == 64 {
                let block: [u8; 64] = core::array::from_fn(|i| self.buf[i]);
                self.compress(&block);
                self.buf.clear();
            }
        }
        while d.len() >= 64 {
            let block: [u8; 64] = core::array::from_fn(|i| d[i]);
            self.compress(&block);
            d = &d[64..];
        }
        self.buf.extend_from_slice(d);
    }

    fn compress(&mut self, block: &[u8; 64]) {
        let mut w = [0u32; 64];
        for t in 0..16 {
            w[t] = u32::from_be_bytes([block[4 * t], block[4 * t + 1], block[4 * t + 2], block[4 * t + 3]]);
        }
        for t in 16..64 {
            let s0 = w[t - 15].rotate_right(7) ^ w[t - 15].rotate_right(18) ^ (w[t - 15] >> 3);
            let s1 = w[t - 2].rotate_right(17) ^ w[t - 2].rotate_right(19) ^ (w[t - 2] >> 10);
            w[t] = w[t - 16].wrapping_add(s0).wrapping_add(w[t - 7]).wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = self.h;
        for t in 0..64 {
            let big_s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ (!e & g);
            let t1 = h.wrapping_add(big_s1).wrapping_add(ch).wrapping_add(self.k[t]).wrapping_add(w[t]);
            let big_s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let t2 = big_s0.wrapping_add(maj);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (hi, v) in self.h.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *hi = hi.wrapping_add(v);
        }
    }

    pub fn finish(mut self) -> [u8; 32] {
        let bits = self.total.wrapping_mul(8);
        let mut pad = vec![0x80u8];
        let used = (self.total % 64) as usize;
        let zeros = if used < 56 { 55 - used } else { 119 - used };
        pad.extend(core::iter::repeat_n(0u8, zeros));
        pad.extend_from_slice(&bits.to_be_bytes());
        let total = self.total;
        self.update(&pad);
        self.total = total;
        let mut out = [0u8; 32];
        for (i, v) in self.h.iter().enumerate() {
            out[4 * i..4 * i + 4].copy_from_slice(&v.to_be_bytes());
        }
        out
    }
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let mut s = Sha256::new();
    s.update(data);
    s.finish()
}

pub fn hex(bytes: &[u8]) -> String {
    const D: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(D[(b >> 4) as usize] as char);
        s.push(D[(b & 15) as usize] as char);
    }
    s
}

/// `sha256:<hex>` of `data`.
pub fn tag(data: &[u8]) -> String {
    format!("sha256:{}", hex(&sha256(data)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_vectors() {
        assert_eq!(hex(&sha256(b"")), "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
        assert_eq!(hex(&sha256(b"abc")), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
        assert_eq!(
            hex(&sha256(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn constants_match_the_standard_definition() {
        let s = Sha256::new();
        assert_eq!(s.h[0], 0x6a09e667);
        assert_eq!(s.k[0], 0x428a2f98);
        assert_eq!(s.k[63], 0xc67178f2);
    }

    #[test]
    fn streaming_equals_one_shot() {
        let data: Vec<u8> = (0..1000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut s = Sha256::new();
        for chunk in data.chunks(37) {
            s.update(chunk);
        }
        assert_eq!(s.finish(), sha256(&data));
        let million_a = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha256(&million_a)), "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0");
    }
}
