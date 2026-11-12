//! Project tasks. Use `cargo xtask <task>` to run a task.

use std::env;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "\
Usage: cargo xtask <task> [args...]

Tasks:
  install    Build emul in release mode and install it with `cargo install`.
             Extra arguments go to `cargo install`, e.g. `--root ~/.local`.";

fn main() -> ExitCode {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("install") => install(args.collect()),
        Some("-h" | "--help" | "help") => {
            println!("{USAGE}");
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("error: unknown task `{other}`\n\n{USAGE}");
            ExitCode::from(2)
        }
        None => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// The workspace root: the parent of the directory of this crate.
fn project_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().expect("xtask lives inside the workspace").to_path_buf()
}

fn install(extra: Vec<String>) -> ExitCode {
    let cargo = env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .arg("install")
        .arg("--path")
        .arg(project_root())
        .arg("--locked")
        .args(extra)
        .status();
    match status {
        Ok(s) if s.success() => ExitCode::SUCCESS,
        Ok(s) => ExitCode::from(s.code().unwrap_or(1).clamp(1, 255) as u8),
        Err(e) => {
            eprintln!("error: could not run cargo: {e}");
            ExitCode::FAILURE
        }
    }
}
