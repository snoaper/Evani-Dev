//! WP3 (docs/studio/WORK_PACKAGES.md): the geometry stage's runtime cache.
//!
//! [`Variants`] holds every generated shape by its `GeometryKey` in a memory
//! LRU ([`MEMORY_CAP`] bytes), and a queue of jobs the scene asked for. The
//! viewer (`apps/material-view/src/studio/variants.rs`) keeps one as a world
//! resource and runs [`Variants::step`] once a frame (a voxel or low-poly
//! horse takes about 50 to 70 ms natively, release), so a style change costs
//! one short hitch, never a stall; meanwhile [`Variants::progress`] says
//! "Making voxel horse..." for the page and the scene draws the same asset
//! plain (`Availability::Generate`). A failed job is remembered with its
//! reason (shown, never retried every frame, never replaced by another
//! object).
//!
//! How the scene (WP4) uses it: `request(job)` with the record's key, the
//! asset's source from the loader (WP2) and the resolved geometry; draw
//! `get(key)` once it is `Some`.
//!
//! Still to come (WP3 follow-up): the OPFS cache across visits (50 MB cap,
//! regenerated if storage is refused), splitting a job into 4 ms slices,
//! and fetching `cargo xtask style-bake --write` bakes before generating.

use std::collections::VecDeque;
use std::rc::Rc;

use crate::{AssetRow, Geometry, GeometryKey, SourceMesh, StyledMesh, generator};

/// The memory the LRU may hold.
pub const MEMORY_CAP: usize = 32 * 1024 * 1024;

/// One shape to make.
#[derive(Clone)]
pub struct Job {
    pub key: GeometryKey,
    pub asset: AssetRow,
    pub source: Rc<SourceMesh>,
    pub geometry: Geometry,
}

impl Job {
    /// What the page shows while it runs: "Making voxel horse...".
    pub fn label(&self) -> String {
        format!("Making {} {}...", self.geometry.kind().name(), self.asset.name.to_lowercase())
    }
}

/// Where a key stands.
#[derive(Debug, Clone, PartialEq)]
pub enum Status {
    Ready(Rc<StyledMesh>),
    Making(String),
    Failed(String),
}

struct Entry {
    key: GeometryKey,
    mesh: Rc<StyledMesh>,
    bytes: usize,
}

/// The geometry stage's runtime cache and queue.
#[derive(Default)]
pub struct Variants {
    /// Least recently used first.
    entries: Vec<Entry>,
    bytes: usize,
    queue: VecDeque<Job>,
    failed: Vec<(GeometryKey, String)>,
    /// Generator runs so far (tests and the debug overlay).
    pub made: u64,
}

/// The bytes a styled mesh holds in memory.
pub fn mesh_bytes(m: &StyledMesh) -> usize {
    m.parts.iter().map(|p| (p.positions.len() + p.normals.len() + p.samples.len()) * 12 + p.indices.len() * 4).sum()
}

impl Variants {
    /// The shape for `key`, if made (and marks it recently used).
    pub fn get(&mut self, key: GeometryKey) -> Option<Rc<StyledMesh>> {
        let i = self.entries.iter().position(|e| e.key == key)?;
        let e = self.entries.remove(i);
        let mesh = e.mesh.clone();
        self.entries.push(e);
        Some(mesh)
    }

    /// Ask for a shape: ready at once from memory, else queued once.
    pub fn request(&mut self, job: Job) -> Status {
        if let Some(m) = self.get(job.key) {
            return Status::Ready(m);
        }
        if let Some((_, why)) = self.failed.iter().find(|f| f.0 == job.key) {
            return Status::Failed(why.clone());
        }
        let label = job.label();
        if !self.queue.iter().any(|j| j.key == job.key) {
            self.queue.push_back(job);
        }
        Status::Making(label)
    }

    /// The job running next, for the page ("Making voxel horse...").
    pub fn progress(&self) -> Option<String> {
        self.queue.front().map(Job::label)
    }

    /// Run the next job, if any; true when one ran.
    pub fn step(&mut self) -> bool {
        let Some(job) = self.queue.pop_front() else { return false };
        self.made += 1;
        match generator(job.geometry.kind()).generate(&job.source, &job.asset, &job.geometry) {
            Ok(mesh) => self.store(job.key, mesh),
            Err(e) => self.failed.push((job.key, format!("{}: {e}", job.label()))),
        }
        true
    }

    fn store(&mut self, key: GeometryKey, mesh: StyledMesh) {
        let bytes = mesh_bytes(&mesh);
        self.entries.retain(|e| e.key != key);
        self.entries.push(Entry { key, mesh: Rc::new(mesh), bytes });
        self.bytes = self.entries.iter().map(|e| e.bytes).sum();
        // Drop the least recently used until within the cap (the newest
        // always stays, even alone over it).
        while self.bytes > MEMORY_CAP && self.entries.len() > 1 {
            let old = self.entries.remove(0);
            self.bytes -= old.bytes;
        }
    }

    /// Bytes held in memory.
    pub fn bytes(&self) -> usize {
        self.bytes
    }
}
