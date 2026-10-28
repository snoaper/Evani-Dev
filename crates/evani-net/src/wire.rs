//! Byte encoding for net messages: LEB128 varints for unsigned numbers,
//! zigzag varints for signed ones, exact little-endian bits for floats.
//!
//! Floats are sent as their exact bits, never rounded, so a snapshot decodes
//! to the very state the server simulated and client prediction can compare
//! states bit for bit. A game that wants smaller snapshots stores quantized
//! values in its state ([`quantize`], [`dequantize`]), so the server and the
//! client simulate the same rounded numbers.
//!
//! Decoding never panics: every read is bounds-checked and returns a
//! [`WireError`], so a corrupt or hostile packet is dropped, not trusted.

use core::fmt;

/// Why a packet could not be decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireError {
    /// The packet ended before the value did.
    Truncated,
    /// A varint ran past the width of its type.
    Overflow,
    /// A tag, bool or text this version does not accept.
    Invalid,
    /// A list claims more items than the packet has bytes for.
    TooLong,
}

impl fmt::Display for WireError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            WireError::Truncated => "packet truncated",
            WireError::Overflow => "number too large",
            WireError::Invalid => "invalid value",
            WireError::TooLong => "list longer than the packet",
        })
    }
}

/// A growable output buffer. Reuse one (call [`Writer::clear`]) instead of
/// making one per packet.
#[derive(Debug, Default, Clone)]
pub struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_capacity(bytes: usize) -> Self {
        Self {
            buf: Vec::with_capacity(bytes),
        }
    }

    pub fn clear(&mut self) {
        self.buf.clear();
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.buf
    }

    pub fn into_bytes(self) -> Vec<u8> {
        self.buf
    }

    pub fn u8(&mut self, v: u8) {
        self.buf.push(v);
    }

    /// Raw bytes, no length prefix.
    pub fn bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }

    /// LEB128: 7 bits per byte, high bit set while more follow.
    pub fn varint(&mut self, mut v: u64) {
        loop {
            let byte = (v & 0x7f) as u8;
            v >>= 7;
            if v == 0 {
                self.buf.push(byte);
                return;
            }
            self.buf.push(byte | 0x80);
        }
    }

    /// Zigzag (0, -1, 1, -2, ... -> 0, 1, 2, 3, ...) then varint, so small
    /// negative numbers stay small.
    pub fn zigzag(&mut self, v: i64) {
        self.varint(((v << 1) ^ (v >> 63)) as u64);
    }

    pub fn f32(&mut self, v: f32) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }

    pub fn f64(&mut self, v: f64) {
        self.buf.extend_from_slice(&v.to_bits().to_le_bytes());
    }

    pub fn u64_fixed(&mut self, v: u64) {
        self.buf.extend_from_slice(&v.to_le_bytes());
    }

    pub fn put<T: Wire>(&mut self, v: &T) {
        v.write(self);
    }
}

/// A cursor over one received packet.
#[derive(Debug, Clone)]
pub struct Reader<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Reader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Bytes not read yet.
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    pub fn is_empty(&self) -> bool {
        self.remaining() == 0
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn u8(&mut self) -> Result<u8, WireError> {
        let b = *self.data.get(self.pos).ok_or(WireError::Truncated)?;
        self.pos += 1;
        Ok(b)
    }

    /// The next `n` raw bytes.
    pub fn take(&mut self, n: usize) -> Result<&'a [u8], WireError> {
        let end = self.pos.checked_add(n).ok_or(WireError::Truncated)?;
        let s = self.data.get(self.pos..end).ok_or(WireError::Truncated)?;
        self.pos = end;
        Ok(s)
    }

    /// Everything not read yet.
    pub fn rest(&mut self) -> &'a [u8] {
        let s = self.data.get(self.pos..).unwrap_or(&[]);
        self.pos = self.data.len();
        s
    }

    pub fn varint(&mut self) -> Result<u64, WireError> {
        let mut v = 0u64;
        let mut shift = 0u32;
        loop {
            let b = self.u8()?;
            let low = u64::from(b & 0x7f);
            if shift == 63 && low > 1 {
                return Err(WireError::Overflow);
            }
            v |= low << shift;
            if b & 0x80 == 0 {
                return Ok(v);
            }
            shift += 7;
            if shift > 63 {
                return Err(WireError::Overflow);
            }
        }
    }

    pub fn varint_u32(&mut self) -> Result<u32, WireError> {
        u32::try_from(self.varint()?).map_err(|_| WireError::Overflow)
    }

    pub fn zigzag(&mut self) -> Result<i64, WireError> {
        let v = self.varint()?;
        Ok(((v >> 1) as i64) ^ -((v & 1) as i64))
    }

    pub fn f32(&mut self) -> Result<f32, WireError> {
        let b = self.take(4)?;
        let mut a = [0u8; 4];
        a.copy_from_slice(b);
        Ok(f32::from_bits(u32::from_le_bytes(a)))
    }

    pub fn f64(&mut self) -> Result<f64, WireError> {
        Ok(f64::from_bits(self.u64_fixed()?))
    }

    pub fn u64_fixed(&mut self) -> Result<u64, WireError> {
        let b = self.take(8)?;
        let mut a = [0u8; 8];
        a.copy_from_slice(b);
        Ok(u64::from_le_bytes(a))
    }

    /// A list length, refused when the packet could not hold that many
    /// items of at least `min_item_bytes` each (so a forged length never
    /// makes the reader allocate gigabytes).
    pub fn list_len(&mut self, min_item_bytes: usize) -> Result<usize, WireError> {
        let n = self.varint_u32()? as usize;
        if n.saturating_mul(min_item_bytes.max(1)) > self.remaining() {
            return Err(WireError::TooLong);
        }
        Ok(n)
    }

    pub fn get<T: Wire>(&mut self) -> Result<T, WireError> {
        T::read(self)
    }
}

/// A value that goes over the wire. Implement it by hand or with
/// [`crate::wire_struct!`]. `read` must read exactly what `write` wrote.
pub trait Wire: Sized {
    fn write(&self, w: &mut Writer);
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError>;
}

impl Wire for u8 {
    fn write(&self, w: &mut Writer) {
        w.u8(*self);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        r.u8()
    }
}

impl Wire for bool {
    fn write(&self, w: &mut Writer) {
        w.u8(u8::from(*self));
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(WireError::Invalid),
        }
    }
}

macro_rules! wire_unsigned {
    ($($t:ty),*) => {$(
        impl Wire for $t {
            fn write(&self, w: &mut Writer) {
                w.varint(u64::from(*self));
            }
            fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
                <$t>::try_from(r.varint()?).map_err(|_| WireError::Overflow)
            }
        }
    )*};
}
wire_unsigned!(u16, u32, u64);

macro_rules! wire_signed {
    ($($t:ty),*) => {$(
        impl Wire for $t {
            fn write(&self, w: &mut Writer) {
                w.zigzag(i64::from(*self));
            }
            fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
                <$t>::try_from(r.zigzag()?).map_err(|_| WireError::Overflow)
            }
        }
    )*};
}
wire_signed!(i8, i16, i32, i64);

impl Wire for f32 {
    fn write(&self, w: &mut Writer) {
        w.f32(*self);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        r.f32()
    }
}

impl Wire for f64 {
    fn write(&self, w: &mut Writer) {
        w.f64(*self);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        r.f64()
    }
}

impl Wire for String {
    fn write(&self, w: &mut Writer) {
        w.varint(self.len() as u64);
        w.bytes(self.as_bytes());
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let n = r.list_len(1)?;
        let b = r.take(n)?;
        core::str::from_utf8(b)
            .map(str::to_string)
            .map_err(|_| WireError::Invalid)
    }
}

/// Every item must encode to at least one byte (all the impls here do).
impl<T: Wire> Wire for Vec<T> {
    fn write(&self, w: &mut Writer) {
        w.varint(self.len() as u64);
        for v in self {
            v.write(w);
        }
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let n = r.list_len(1)?;
        let mut out = Vec::with_capacity(n);
        for _ in 0..n {
            out.push(T::read(r)?);
        }
        Ok(out)
    }
}

impl<T: Wire> Wire for Option<T> {
    fn write(&self, w: &mut Writer) {
        match self {
            None => w.u8(0),
            Some(v) => {
                w.u8(1);
                v.write(w);
            }
        }
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        match r.u8()? {
            0 => Ok(None),
            1 => Ok(Some(T::read(r)?)),
            _ => Err(WireError::Invalid),
        }
    }
}

impl<T: Wire + Copy + Default, const N: usize> Wire for [T; N] {
    fn write(&self, w: &mut Writer) {
        for v in self {
            v.write(w);
        }
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let mut out = [T::default(); N];
        for slot in &mut out {
            *slot = T::read(r)?;
        }
        Ok(out)
    }
}

macro_rules! wire_tuple {
    ($($n:ident),+) => {
        impl<$($n: Wire),+> Wire for ($($n,)+) {
            #[allow(non_snake_case)]
            fn write(&self, w: &mut Writer) {
                let ($($n,)+) = self;
                $($n.write(w);)+
            }
            fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
                Ok(($($n::read(r)?,)+))
            }
        }
    };
}
wire_tuple!(A, B);
wire_tuple!(A, B, C);
wire_tuple!(A, B, C, D);

/// `impl Wire` for a struct with named fields, written and read in the
/// order listed (list every field):
///
/// ```ignore
/// struct Player { pos: [f32; 3], yaw: f32, hp: u16 }
/// evani_net::wire_struct!(Player { pos, yaw, hp });
/// ```
#[macro_export]
macro_rules! wire_struct {
    ($ty:ty { $($field:ident),* $(,)? }) => {
        impl $crate::wire::Wire for $ty {
            fn write(&self, w: &mut $crate::wire::Writer) {
                $( $crate::wire::Wire::write(&self.$field, w); )*
            }
            fn read(r: &mut $crate::wire::Reader<'_>) -> Result<Self, $crate::wire::WireError> {
                Ok(Self { $( $field: $crate::wire::Wire::read(r)?, )* })
            }
        }
    };
}

/// 64-bit FNV-1a: the default state checksum and protocol hash. Not
/// cryptographic; it only has to notice that two states differ.
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

/// `v` clamped to `[min, max]` and mapped to `bits` bits (1 to 24), rounded
/// to the nearest step. NaN maps to `min`. Store the result (or its
/// [`dequantize`]d value) in the state, so both sides simulate it.
pub fn quantize(v: f32, min: f32, max: f32, bits: u32) -> u32 {
    let bits = bits.clamp(1, 24);
    let steps = ((1u32 << bits) - 1) as f32;
    let span = max - min;
    if span.is_nan() || span <= 0.0 {
        return 0;
    }
    let t = ((v - min) / span).clamp(0.0, 1.0);
    // `as` saturates and maps NaN to 0. At 24 bits `steps + 0.5` rounds up
    // to 2^24 in f32, one past the top code: clamp to the top code.
    ((t * steps + 0.5) as u32).min((1u32 << bits) - 1)
}

/// The value [`quantize`] stood for.
pub fn dequantize(q: u32, min: f32, max: f32, bits: u32) -> f32 {
    let bits = bits.clamp(1, 24);
    let steps = ((1u32 << bits) - 1) as f32;
    min + (max - min) * (q.min(1u32 << bits) as f32 / steps).min(1.0)
}
