//! `cargo-xtask`: Cargo runs it for `cargo xtask <cmd>` when the folder has
//! no `xtask` alias (a game outside the engine, or an installed engine pack,
//! whose `.cargo/config.toml` drops the alias). It starts the prebuilt
//! `evani` in its own folder (`<pack>/tools/bin/`) with the same arguments,
//! so `cargo xtask check` never compiles the build tool and never writes
//! into the read-only pack (docs/ENGINE_PACK.md section 2).

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// The arguments for `evani`: Cargo passes the subcommand name (`xtask`)
/// first; it is not one of evani's commands.
fn evani_args(mut args: Vec<OsString>) -> Vec<OsString> {
    if args.first().is_some_and(|a| a == "xtask") {
        args.remove(0);
    }
    args
}

/// `evani(.exe)` beside this program.
fn evani_beside(exe: Option<PathBuf>) -> Option<PathBuf> {
    Some(exe?.parent()?.join(format!("evani{}", std::env::consts::EXE_SUFFIX)))
}

fn main() -> ExitCode {
    let args = evani_args(std::env::args_os().skip(1).collect());
    let Some(evani) = evani_beside(std::env::current_exe().ok()) else {
        eprintln!("cargo-xtask: can't tell which folder this program is in");
        return ExitCode::FAILURE;
    };
    match Command::new(&evani).args(&args).status() {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1)),
        Err(e) => {
            eprintln!("cargo-xtask: can't start {}: {e} (it ships beside this program in the engine pack's tools/bin)", evani.display());
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cargo_s_subcommand_name_is_dropped_and_evani_is_beside_it() {
        let a = |v: &[&str]| v.iter().map(OsString::from).collect::<Vec<_>>();
        assert_eq!(evani_args(a(&["xtask", "check", "--app-dir", "x"])), a(&["check", "--app-dir", "x"]));
        assert_eq!(evani_args(a(&["check"])), a(&["check"]));
        assert_eq!(evani_args(a(&[])), a(&[]));
        let bin = PathBuf::from("pack").join("tools").join("bin");
        assert_eq!(evani_beside(Some(bin.join("cargo-xtask"))), Some(bin.join(format!("evani{}", std::env::consts::EXE_SUFFIX))));
        assert_eq!(evani_beside(None), None);
    }
}
