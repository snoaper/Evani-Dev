//! The hole list on the wire (net builds). Only the list and its version
//! are sent: the bucket index is rebuilt on read. Buckets always list their
//! holes in index order (a new hole takes the highest index, every other
//! change reindexes), so the rebuilt index is the one the sender has.

use evani_net::{Reader, Wire, WireError, Writer};

use super::{HN, Hole, Holes};

impl Wire for Hole {
    fn write(&self, w: &mut Writer) {
        w.u8(self.level as u8);
        self.p.write(w);
        w.f32(self.r);
        w.u8(self.seed as u8);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let level = usize::from(r.u8()?);
        if level > 2 {
            return Err(WireError::Invalid);
        }
        Ok(Hole { level, p: r.get()?, r: r.f32()?, seed: u32::from(r.u8()?) })
    }
}

impl Wire for Holes {
    fn write(&self, w: &mut Writer) {
        self.list.write(w);
        self.version.write(w);
    }
    fn read(r: &mut Reader<'_>) -> Result<Self, WireError> {
        let list = r.get()?;
        let version = r.get()?;
        let mut h = Holes { list, buckets: vec![Vec::new(); 3 * HN * HN], version };
        h.reindex();
        Ok(h)
    }
}
