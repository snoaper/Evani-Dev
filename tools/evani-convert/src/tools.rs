//! Running the pinned external programs: always by full path, one at a
//! time, at below-normal priority on Windows, with their own thread caps.

use std::path::{Path, PathBuf};
use std::process::Command;

use evani_guide::extools;

/// The programs one conversion needs, located and checked against their pins.
#[derive(Clone, Debug)]
pub struct Tools {
    pub gltfpack: PathBuf,
    pub ktx: PathBuf,
    /// glTF-Transform's CLI script (run with `node`).
    pub gltf_transform: PathBuf,
    /// The npm tree holding glTF-Transform and sharp.
    pub node_modules: PathBuf,
    pub node: PathBuf,
}

/// `name version` for every tool, for the report.
pub fn versions() -> Vec<(String, String)> {
    extools::PINS.iter().filter(|p| !p.optional).map(|p| (p.name.to_owned(), p.version.to_owned())).collect()
}

/// A program on PATH, as a full path (`node` has no pin: the npm tree runs on it).
pub fn which(name: &str) -> Option<PathBuf> {
    let exts: Vec<String> = if cfg!(windows) { vec![".exe".into(), ".cmd".into()] } else { vec![String::new()] };
    let path = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&path) {
        for e in &exts {
            let p = dir.join(format!("{name}{e}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

impl Tools {
    /// Every required tool, or one error per missing one, each naming the fix.
    pub fn locate(root: &Path) -> Result<Tools, String> {
        let mut errs = Vec::new();
        let mut get = |n: &str| match extools::locate(root, n) {
            Ok(p) => Some(p),
            Err(e) => {
                errs.push(e);
                None
            }
        };
        let gltfpack = get("gltfpack");
        let ktx = get("ktx");
        let gt = get("gltf-transform");
        let node = which("node");
        if node.is_none() {
            errs.push("node is not on PATH (glTF-Transform and sharp run on Node.js)\nfix: install Node.js 20 or newer".into());
        }
        match (gltfpack, ktx, gt, node) {
            (Some(gltfpack), Some(ktx), Some(gltf_transform), Some(node)) if errs.is_empty() => {
                let node_modules = gltf_transform.ancestors().find(|a| a.file_name().is_some_and(|n| n == "node_modules")).map(Path::to_path_buf).ok_or("glTF-Transform is not inside a node_modules folder")?;
                Ok(Tools { gltfpack: plain(&gltfpack), ktx: plain(&ktx), gltf_transform: plain(&gltf_transform), node_modules: plain(&node_modules), node })
            }
            _ => Err(errs.join("\n")),
        }
    }
}

/// Below-normal priority and no console window (Windows).
pub fn low(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(BELOW_NORMAL_PRIORITY_CLASS | CREATE_NO_WINDOW);
    }
    cmd
}

/// Run to completion: (exit code, stdout and stderr).
pub fn run_any(cmd: &mut Command, what: &str) -> Result<(Option<i32>, String), String> {
    low(cmd);
    // Thread caps for anything that reads them (sharp's libvips, OpenMP).
    cmd.env("OMP_NUM_THREADS", "1").env("VIPS_CONCURRENCY", "1").env("UV_THREADPOOL_SIZE", "2");
    let out = cmd.output().map_err(|e| format!("{what}: cannot start: {e}"))?;
    Ok((out.status.code(), format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr))))
}

/// Run to completion; the error carries the tail of its output.
pub fn run(cmd: &mut Command, what: &str) -> Result<String, String> {
    low(cmd);
    // Thread caps for anything that reads them (sharp's libvips, OpenMP).
    cmd.env("OMP_NUM_THREADS", "1").env("VIPS_CONCURRENCY", "1").env("UV_THREADPOOL_SIZE", "2");
    let out = cmd.output().map_err(|e| format!("{what}: cannot start: {e}"))?;
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    if out.status.success() {
        Ok(text)
    } else {
        let tail: Vec<&str> = text.lines().rev().take(15).collect();
        let tail: Vec<&str> = tail.into_iter().rev().collect();
        Err(format!("{what} failed ({}):\n{}", out.status, tail.join("\n")))
    }
}

impl Tools {
    pub fn gltfpack(&self, args: &[&str], what: &str) -> Result<String, String> {
        run(Command::new(&self.gltfpack).args(args), what)
    }

    pub fn gltf_transform(&self, args: &[&str], what: &str) -> Result<String, String> {
        run(Command::new(&self.node).arg(&self.gltf_transform).args(args), what)
    }

    /// glTF-Transform, where a non-zero exit is an answer (the validator
    /// exits 1 when it finds errors), not a failure.
    pub fn gltf_transform_any(&self, args: &[&str], what: &str) -> Result<(Option<i32>, String), String> {
        run_any(Command::new(&self.node).arg(&self.gltf_transform).args(args), what)
    }

    pub fn ktx(&self, args: &[&str], what: &str) -> Result<String, String> {
        run(Command::new(&self.ktx).args(args), what)
    }

    /// A Node script that needs packages from the glTF-Transform tree.
    pub fn node_script(&self, script: &Path, args: &[&str], what: &str) -> Result<String, String> {
        run(Command::new(&self.node).arg(script).arg(&self.node_modules).args(args), what)
    }
}

/// `p` as a string for a command line, without Windows' `\\?\` verbatim
/// prefix (`canonicalize` adds it; Node.js can't resolve a script path with it).
pub fn s(p: &Path) -> String {
    let t = p.to_string_lossy();
    match t.strip_prefix(r"\\?\UNC\") {
        Some(rest) => format!(r"\\{rest}"),
        None => t.strip_prefix(r"\\?\").unwrap_or(&t).to_owned(),
    }
}

/// [`s`] as a path, for the programs themselves.
pub fn plain(p: &Path) -> PathBuf {
    PathBuf::from(s(p))
}
