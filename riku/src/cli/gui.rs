//! Lanzamiento del visor de escritorio (`riku-gui`).
//!
//! Estrategia de localización del binario:
//! 1. `$RIKU_GUI_BIN` si está definido y existe.
//! 2. Hermano del ejecutable actual (instalación lado-a-lado).
//! 3. `target/{release,debug}` resuelto desde el ejecutable o desde
//!    `CARGO_MANIFEST_DIR` (entorno de desarrollo).
//! 4. Fallback: `cargo run --package riku-gui` desde la raíz del workspace.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

pub(super) fn run(file: Option<PathBuf>) -> Result<(), String> {
    let args: Vec<OsString> = file.into_iter().map(|p| p.into_os_string()).collect();
    run_with_args(args)
}

pub(super) fn run_with_args(args: Vec<OsString>) -> Result<(), String> {
    if let Some(bin) = locate_binary() {
        let status = Command::new(bin)
            .args(&args)
            .status()
            .map_err(|e| e.to_string())?;
        return if status.success() {
            Ok(())
        } else {
            Err(format!("riku-gui finalizó con error: {status}"))
        };
    }

    let workspace_root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .ok_or_else(|| "No se pudo resolver la raíz del workspace.".to_string())?;

    let mut cargo = Command::new("cargo");
    cargo
        .args(["run", "--package", "riku-gui", "--bin", "riku-gui"])
        .current_dir(workspace_root);
    if !args.is_empty() {
        cargo.arg("--").args(&args);
    }

    let status = cargo.status().map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("No se pudo iniciar riku-gui: {status}"))
    }
}

fn locate_binary() -> Option<PathBuf> {
    let bin_name = format!("riku-gui{}", std::env::consts::EXE_SUFFIX);

    if let Ok(path) = std::env::var("RIKU_GUI_BIN") {
        let candidate = PathBuf::from(&path);
        if candidate.exists() {
            return Some(candidate);
        }
    }

    if let Some(p) = locate_near_current_exe(&bin_name) {
        return Some(p);
    }

    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    for profile in ["release", "debug"] {
        if let Some(parent) = manifest_dir.parent() {
            let candidate = parent.join("target").join(profile).join(&bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}

fn locate_near_current_exe(bin_name: &str) -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let dir = exe.parent()?.canonicalize().ok()?;

    let sibling = dir.join(bin_name);
    if sibling.exists() {
        return Some(sibling);
    }

    for ancestor in [dir.parent(), dir.parent().and_then(|p| p.parent())] {
        let Some(p) = ancestor else { continue };
        for profile in ["release", "debug"] {
            let candidate = p.join(profile).join(bin_name);
            if candidate.exists() {
                return Some(candidate);
            }
        }
    }

    None
}
