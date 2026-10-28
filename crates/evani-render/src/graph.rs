//! A small render graph: passes declare the targets they read and write;
//! the graph orders them (stable topological sort) and validates the wiring.
//! GPU resources are allocated by the renderer from the compiled order (next
//! stage); this file is pure data and is unit-tested natively.

use std::collections::BTreeMap;

/// What kind of texture a target is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TargetKind {
    /// Linear HDR colour (rgba16float).
    HdrColor,
    /// Depth (depth32float).
    Depth,
    /// Display-referred colour after tonemapping (rgba8unorm).
    Ldr,
    /// The canvas. Write-only.
    Swapchain,
    /// A GPU buffer a compute pass writes and later passes read (feature
    /// `compute`). Not a texture: the renderer doesn't allocate it; the
    /// system that owns the compute pass does.
    #[cfg(feature = "compute")]
    Buffer,
}

impl TargetKind {
    /// A texture the renderer allocates (everything but the canvas and
    /// compute buffers).
    pub fn is_texture(self) -> bool {
        match self {
            TargetKind::Swapchain => false,
            #[cfg(feature = "compute")]
            TargetKind::Buffer => false,
            _ => true,
        }
    }
}

/// How a pass runs on the GPU.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum PassKind {
    /// A render pass (draws into its output targets).
    #[default]
    Render,
    /// A compute pass (dispatches workgroups; writes buffer targets). Only
    /// with feature `compute`, which only effects that need it turn on.
    #[cfg(feature = "compute")]
    Compute,
}

/// A named render target.
#[derive(Debug, Clone, PartialEq)]
pub struct TargetDesc {
    pub name: &'static str,
    pub kind: TargetKind,
    /// Size relative to the render resolution (bloom mips use 0.5, 0.25...).
    pub scale: f32,
    /// Only alive inside one frame; may alias other transient targets.
    pub transient: bool,
}

impl TargetDesc {
    pub const fn new(name: &'static str, kind: TargetKind) -> Self {
        Self {
            name,
            kind,
            scale: 1.0,
            transient: false,
        }
    }

    pub const fn scaled(mut self, scale: f32) -> Self {
        self.scale = scale;
        self
    }

    pub const fn transient(mut self) -> Self {
        self.transient = true;
        self
    }
}

/// A render pass as the graph sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct PassDesc {
    /// Unique, dotted: `forward`, `bloom.bright`, `vignette`, `tonemap`.
    pub name: &'static str,
    /// Registering module/effect: `render`, `fx.bloom`...
    pub owner: &'static str,
    pub inputs: Vec<&'static str>,
    pub outputs: Vec<&'static str>,
    /// Shader slots the pass calls (`tonemap`, `bloom_weight`).
    pub slots: Vec<&'static str>,
    /// Tie-break among passes the dependencies leave unordered (lower first).
    pub order: i32,
    /// Disabled passes stay in the graph (no rebuild on toggle) but are
    /// skipped; their outputs are then taken from their first input.
    pub enabled: bool,
    pub kind: PassKind,
}

impl PassDesc {
    pub fn new(name: &'static str, owner: &'static str) -> Self {
        Self {
            name,
            owner,
            inputs: Vec::new(),
            outputs: Vec::new(),
            slots: Vec::new(),
            order: 0,
            enabled: true,
            kind: PassKind::Render,
        }
    }

    /// A compute pass (feature `compute`): it writes buffer targets that
    /// the passes reading them draw from, so the graph runs it first.
    #[cfg(feature = "compute")]
    pub fn compute(name: &'static str, owner: &'static str) -> Self {
        Self {
            kind: PassKind::Compute,
            ..Self::new(name, owner)
        }
    }

    pub fn reads(mut self, targets: &[&'static str]) -> Self {
        self.inputs.extend_from_slice(targets);
        self
    }

    pub fn writes(mut self, targets: &[&'static str]) -> Self {
        self.outputs.extend_from_slice(targets);
        self
    }

    pub fn uses_slots(mut self, slots: &[&'static str]) -> Self {
        self.slots.extend_from_slice(slots);
        self
    }

    pub fn order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GraphError {
    DuplicatePass(&'static str),
    DuplicateTarget(&'static str),
    UnknownTarget { pass: &'static str, target: &'static str },
    /// A pass reads a target nothing writes before it.
    UnwrittenInput { pass: &'static str, target: &'static str },
    ReadsSwapchain(&'static str),
    Cycle,
    /// A compute pass writes a texture, or a render pass writes a buffer.
    #[cfg(feature = "compute")]
    WrongOutputKind { pass: &'static str, target: &'static str },
}

impl core::fmt::Display for GraphError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            GraphError::DuplicatePass(p) => write!(f, "pass {p} registered twice"),
            GraphError::DuplicateTarget(t) => write!(f, "target {t} registered twice"),
            GraphError::UnknownTarget { pass, target } => {
                write!(f, "pass {pass} uses unknown target {target}")
            }
            GraphError::UnwrittenInput { pass, target } => {
                write!(f, "pass {pass} reads {target}, which nothing writes before it")
            }
            GraphError::ReadsSwapchain(p) => write!(f, "pass {p} reads the swapchain"),
            GraphError::Cycle => f.write_str("render graph has a cycle"),
            #[cfg(feature = "compute")]
            GraphError::WrongOutputKind { pass, target } => {
                write!(f, "pass {pass} writes {target}: compute passes write buffers, render passes textures")
            }
        }
    }
}

/// The render graph. World resource (inserted by the render module so
/// optional modules such as evani-fx can add passes during `build`).
#[derive(Debug, Clone, Default)]
pub struct RenderGraph {
    targets: Vec<TargetDesc>,
    passes: Vec<PassDesc>,
    order: Vec<usize>,
    compiled: bool,
}

impl RenderGraph {
    /// The standard targets (`hdr`, `depth`, `ldr`, `swapchain`) and passes
    /// (`forward` -> `tonemap` -> `present`).
    pub fn standard() -> Self {
        let mut g = Self::default();
        // Without the 3D path (feature `forward`) there is no depth target
        // and no forward pass; `sprites` then clears `hdr` itself.
        let _ = g.add_target(TargetDesc::new("hdr", TargetKind::HdrColor));
        #[cfg(feature = "forward")]
        let _ = g.add_target(TargetDesc::new("depth", TargetKind::Depth));
        for t in [
            TargetDesc::new("ldr", TargetKind::Ldr),
            TargetDesc::new("swapchain", TargetKind::Swapchain),
        ] {
            let _ = g.add_target(t);
        }
        #[cfg(feature = "forward")]
        let _ = g.add_pass(PassDesc::new("forward", "render").writes(&["hdr", "depth"]).order(0));
        #[cfg(feature = "forward")]
        let sprites = PassDesc::new("sprites", "render").reads(&["hdr"]).writes(&["hdr"]).order(10);
        #[cfg(not(feature = "forward"))]
        let sprites = PassDesc::new("sprites", "render").writes(&["hdr"]).order(10);
        let passes = [
            sprites,
            PassDesc::new("tonemap", "render")
                .reads(&["hdr"])
                .writes(&["ldr"])
                .uses_slots(&["tonemap", "exposure"])
                .order(100),
            PassDesc::new("present", "render").reads(&["ldr"]).writes(&["swapchain"]).order(1000),
        ];
        for p in passes {
            let _ = g.add_pass(p);
        } #[cfg(feature = "msdf")] let _ = g.add_pass(crate::msdf::ui_pass()); // the HUD after post (S3-13b fix)
        g
    }

    pub fn add_target(&mut self, t: TargetDesc) -> Result<(), GraphError> {
        if self.target(t.name).is_some() {
            return Err(GraphError::DuplicateTarget(t.name));
        }
        self.targets.push(t);
        self.compiled = false;
        Ok(())
    }

    pub fn add_pass(&mut self, p: PassDesc) -> Result<(), GraphError> {
        if self.passes.iter().any(|q| q.name == p.name) {
            return Err(GraphError::DuplicatePass(p.name));
        }
        self.passes.push(p);
        self.compiled = false;
        Ok(())
    }

    /// Make an existing pass also read `target` (a compute buffer an engine
    /// pass draws from), so the target's writer runs before it. False if
    /// the pass doesn't exist.
    pub fn add_input(&mut self, pass: &str, target: &'static str) -> bool {
        match self.passes.iter_mut().find(|p| p.name == pass) {
            Some(p) => {
                if !p.inputs.contains(&target) {
                    p.inputs.push(target);
                    self.compiled = false;
                }
                true
            }
            None => false,
        }
    }

    /// Enable or disable a pass without changing the graph's shape.
    pub fn set_enabled(&mut self, pass: &str, enabled: bool) -> bool {
        match self.passes.iter_mut().find(|p| p.name == pass) {
            Some(p) => {
                p.enabled = enabled;
                true
            }
            None => false,
        }
    }

    pub fn target(&self, name: &str) -> Option<&TargetDesc> {
        self.targets.iter().find(|t| t.name == name)
    }

    pub fn pass(&self, name: &str) -> Option<&PassDesc> {
        self.passes.iter().find(|p| p.name == name)
    }

    pub fn targets(&self) -> &[TargetDesc] {
        &self.targets
    }

    /// Order passes and validate. Writers of a target run before its
    /// readers; a pass that reads and writes the same target (in-place, like
    /// `sprites` on `hdr`) runs after the target's pure writers. Among
    /// unconstrained passes, `(order, registration)` decides.
    pub fn compile(&mut self) -> Result<(), GraphError> {
        for p in &self.passes {
            for &t in p.inputs.iter().chain(&p.outputs) {
                let Some(td) = self.target(t) else {
                    return Err(GraphError::UnknownTarget {
                        pass: p.name,
                        target: t,
                    });
                };
                if td.kind == TargetKind::Swapchain && p.inputs.contains(&t) {
                    return Err(GraphError::ReadsSwapchain(p.name));
                }
                #[cfg(feature = "compute")]
                if p.outputs.contains(&t) && ((p.kind == PassKind::Compute) != (td.kind == TargetKind::Buffer)) {
                    return Err(GraphError::WrongOutputKind { pass: p.name, target: t });
                }
            }
        }
        // For each target: a reader runs after its pure writers and after
        // in-place writers with a lower (order, index) key (read after
        // write); in-place writers with a higher key run after the reader
        // (write after read).
        let n = self.passes.len();
        let key = |i: usize| (self.passes[i].order, i);
        let mut deps: Vec<Vec<usize>> = vec![Vec::new(); n];
        let edge = |deps: &mut Vec<Vec<usize>>, after: usize, before: usize| {
            if !deps[after].contains(&before) {
                deps[after].push(before);
            }
        };
        for (r, reader) in self.passes.iter().enumerate() {
            for &t in &reader.inputs {
                for (w, writer) in self.passes.iter().enumerate() {
                    if w == r || !writer.outputs.contains(&t) {
                        continue;
                    }
                    let pure_writer = !writer.inputs.contains(&t);
                    if pure_writer || key(w) < key(r) {
                        edge(&mut deps, r, w);
                    } else {
                        edge(&mut deps, w, r);
                    }
                }
            }
        }
        // Kahn's algorithm, picking the smallest key each time (stable).
        let mut indeg: Vec<usize> = deps.iter().map(Vec::len).collect();
        let mut done = vec![false; n];
        let mut order = Vec::with_capacity(n);
        let mut by_key: BTreeMap<(i32, usize), usize> = BTreeMap::new();
        for (i, _) in indeg.iter().enumerate().filter(|(_, d)| **d == 0) {
            by_key.insert(key(i), i);
        }
        while let Some((_, i)) = by_key.pop_first() {
            done[i] = true;
            order.push(i);
            for (j, d) in deps.iter().enumerate() {
                if !done[j] && d.contains(&i) {
                    indeg[j] -= 1;
                    if indeg[j] == 0 {
                        by_key.insert(key(j), j);
                    }
                }
            }
        }
        if order.len() != n {
            return Err(GraphError::Cycle);
        }
        // Every input must have been written earlier.
        for (pos, &i) in order.iter().enumerate() {
            let p = &self.passes[i];
            for &t in &p.inputs {
                let written = order[..pos]
                    .iter()
                    .any(|&w| self.passes[w].outputs.contains(&t));
                if !written {
                    return Err(GraphError::UnwrittenInput {
                        pass: p.name,
                        target: t,
                    });
                }
            }
        }
        self.order = order;
        self.compiled = true;
        Ok(())
    }

    pub fn is_compiled(&self) -> bool {
        self.compiled
    }

    /// Passes in execution order (after [`RenderGraph::compile`]).
    pub fn ordered(&self) -> impl Iterator<Item = &PassDesc> {
        self.order.iter().map(|&i| &self.passes[i])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(g: &RenderGraph) -> Vec<&'static str> {
        g.ordered().map(|p| p.name).collect()
    }

    /// The expected order with the 3D path's `forward` pass left out when
    /// the feature is off, and the HUD's `ui` pass left out without `msdf`.
    fn expect(all: &[&'static str]) -> Vec<&'static str> {
        all.iter()
            .copied()
            .filter(|&n| cfg!(feature = "forward") || n != "forward")
            .filter(|&n| cfg!(feature = "msdf") || n != "ui")
            .collect()
    }

    #[test]
    fn standard_graph_orders() {
        let mut g = RenderGraph::standard();
        g.compile().unwrap();
        assert_eq!(names(&g), expect(&["forward", "sprites", "tonemap", "ui", "present"]));
    }

    /// Errors become tools (S3-13b fix): with `ui` the HUD pass runs after
    /// the tone map and after every effect that writes `ldr` (the vignette),
    /// and right before `present`, so no post-processing touches the HUD.
    #[cfg(feature = "msdf")]
    #[test]
    fn hud_draws_after_the_tone_map() {
        let mut g = RenderGraph::standard();
        g.add_pass(PassDesc::new("vignette", "fx.vignette").reads(&["ldr"]).writes(&["ldr"]).order(200))
            .unwrap();
        // A late effect on `ldr` registered after the HUD pass still runs
        // before it while its order is below the HUD's.
        g.add_pass(PassDesc::new("late.grade", "fx.test").reads(&["ldr"]).writes(&["ldr"]).order(800))
            .unwrap();
        g.compile().unwrap();
        let order = names(&g);
        let at = |n: &str| order.iter().position(|&p| p == n).unwrap();
        assert!(at("ui") > at("tonemap"), "{order:?}");
        assert!(at("ui") > at("vignette") && at("ui") > at("late.grade"), "{order:?}");
        assert_eq!(at("ui") + 1, at("present"), "{order:?}");
        assert!(at("sprites") < at("tonemap"), "{order:?}");
        let ui = g.pass("ui").unwrap();
        assert_eq!((ui.inputs.as_slice(), ui.outputs.as_slice()), (&["ldr"][..], &["ldr"][..]));
    }

    #[test]
    fn effects_slot_in_by_dependencies() {
        let mut g = RenderGraph::standard();
        g.add_target(TargetDesc::new("bloom.mip1", TargetKind::HdrColor).scaled(0.5).transient())
            .unwrap();
        // Registered after tonemap, but must run before it.
        g.add_pass(
            PassDesc::new("bloom.bright", "fx.bloom")
                .reads(&["hdr"])
                .writes(&["bloom.mip1"])
                .order(50),
        )
        .unwrap();
        g.add_pass(
            PassDesc::new("bloom.composite", "fx.bloom")
                .reads(&["hdr", "bloom.mip1"])
                .writes(&["hdr"])
                .uses_slots(&["bloom_weight"])
                .order(60),
        )
        .unwrap();
        g.add_pass(PassDesc::new("vignette", "fx.vignette").reads(&["ldr"]).writes(&["ldr"]).order(200))
            .unwrap();
        g.compile().unwrap();
        assert_eq!(
            names(&g),
            expect(&[
                "forward",
                "sprites",
                "bloom.bright",
                "bloom.composite",
                "tonemap",
                "vignette",
                "ui",
                "present"
            ])
        );
    }

    #[test]
    fn validation_errors() {
        let mut g = RenderGraph::standard();
        assert_eq!(
            g.add_pass(PassDesc::new("sprites", "x")),
            Err(GraphError::DuplicatePass("sprites"))
        );
        g.add_pass(PassDesc::new("bad", "x").reads(&["nope"])).unwrap();
        assert!(matches!(g.compile(), Err(GraphError::UnknownTarget { .. })));

        let mut g = RenderGraph::standard();
        g.add_pass(PassDesc::new("peek", "x").reads(&["swapchain"])).unwrap();
        assert_eq!(g.compile(), Err(GraphError::ReadsSwapchain("peek")));

        let mut g = RenderGraph::default();
        g.add_target(TargetDesc::new("a", TargetKind::HdrColor)).unwrap();
        g.add_target(TargetDesc::new("b", TargetKind::HdrColor)).unwrap();
        g.add_pass(PassDesc::new("p", "x").reads(&["a"]).writes(&["b"])).unwrap();
        g.add_pass(PassDesc::new("q", "x").reads(&["b"]).writes(&["a"])).unwrap();
        assert_eq!(g.compile(), Err(GraphError::Cycle));

        let mut g = RenderGraph::default();
        g.add_target(TargetDesc::new("a", TargetKind::HdrColor)).unwrap();
        g.add_pass(PassDesc::new("r", "x").reads(&["a"])).unwrap();
        assert!(matches!(g.compile(), Err(GraphError::UnwrittenInput { .. })));
    }

    /// A compute pass registered last still runs before the engine passes
    /// that draw from its buffer, and kinds can't be mixed up.
    #[cfg(feature = "compute")]
    #[test]
    fn compute_pass_orders_before_the_draw() {
        let mut g = RenderGraph::standard();
        g.add_target(TargetDesc::new("particles", TargetKind::Buffer)).unwrap();
        g.add_pass(PassDesc::compute("particles.sim", "fx.particles").writes(&["particles"]).order(500))
            .unwrap();
        assert_eq!(g.add_input("forward", "particles"), cfg!(feature = "forward"));
        assert!(g.add_input("sprites", "particles"));
        assert!(!g.add_input("nope", "particles"));
        g.compile().unwrap();
        assert_eq!(names(&g), expect(&["particles.sim", "forward", "sprites", "tonemap", "ui", "present"]));
        assert_eq!(g.pass("particles.sim").unwrap().kind, PassKind::Compute);
        assert_eq!(g.pass("sprites").unwrap().kind, PassKind::Render);
        assert!(!TargetKind::Buffer.is_texture() && TargetKind::Depth.is_texture());

        let mut bad = RenderGraph::standard();
        bad.add_pass(PassDesc::compute("c", "x").writes(&["hdr"])).unwrap();
        assert_eq!(bad.compile(), Err(GraphError::WrongOutputKind { pass: "c", target: "hdr" }));
        let mut bad = RenderGraph::standard();
        bad.add_target(TargetDesc::new("buf", TargetKind::Buffer)).unwrap();
        bad.add_pass(PassDesc::new("r", "x").writes(&["buf"])).unwrap();
        assert!(matches!(bad.compile(), Err(GraphError::WrongOutputKind { .. })));
    }

    /// The 3D path's targets and pass exist exactly when `forward` is on;
    /// without it `sprites` writes `hdr` first (it clears it).
    #[test]
    fn forward_path_follows_the_feature() {
        let mut g = RenderGraph::standard();
        g.compile().unwrap();
        assert_eq!(g.pass("forward").is_some(), cfg!(feature = "forward"));
        assert_eq!(g.target("depth").is_some(), cfg!(feature = "forward"));
        let sprites = g.pass("sprites").unwrap();
        assert_eq!(sprites.inputs.contains(&"hdr"), cfg!(feature = "forward"));
        assert!(sprites.outputs.contains(&"hdr"));
    }

    #[test]
    fn toggling_keeps_shape() {
        let mut g = RenderGraph::standard();
        g.compile().unwrap();
        assert!(g.set_enabled("sprites", false));
        assert!(g.is_compiled());
        assert!(!g.pass("sprites").unwrap().enabled);
    }
}
