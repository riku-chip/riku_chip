//! Lanzamiento del visor de escritorio.
//!
//! El visor va dentro del mismo ejecutable (`riku gui …`, feature `gui`).
//! `open` y `diff -f visual` lo arrancan como **proceso hijo del propio
//! binario** (`current_exe()`), así la terminal y el shell interactivo
//! siguen libres mientras la ventana está abierta, y no hay que buscar
//! ningún otro programa.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Command, Stdio};

pub(super) fn run(file: Option<PathBuf>) -> Result<(), String> {
    let args: Vec<OsString> = file.into_iter().map(|p| p.into_os_string()).collect();
    run_with_args(args)
}

/// Lanza `riku gui <args>` en segundo plano y vuelve enseguida.
pub(super) fn run_with_args(args: Vec<OsString>) -> Result<(), String> {
    if !cfg!(feature = "gui") {
        return Err(NO_GUI.into());
    }
    #[cfg(feature = "gui")]
    if cfg!(unix) && !crate::gui::has_display() {
        return Err("el visor necesita un escritorio gráfico (DISPLAY o WAYLAND_DISPLAY); \
                    la CLI funciona igual sin él"
            .into());
    }
    let exe = std::env::current_exe().map_err(|e| format!("no se pudo ubicar el ejecutable: {e}"))?;
    Command::new(exe)
        .arg("gui")
        .args(&args)
        .stdin(Stdio::null())
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("no se pudo iniciar el visor: {e}"))
}

/// Ejecuta el visor en este proceso (subcomando `gui`). Bloquea hasta cerrar.
pub(super) fn run_here(args: Vec<String>) -> Result<(), String> {
    #[cfg(feature = "gui")]
    {
        crate::gui::run(args)
    }
    #[cfg(not(feature = "gui"))]
    {
        let _ = args;
        Err(NO_GUI.into())
    }
}

const NO_GUI: &str = "esta versión de riku se compiló sin visor (feature `gui`); \
                      la CLI funciona igual";
