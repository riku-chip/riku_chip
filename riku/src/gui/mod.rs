//! Visor de escritorio (egui/eframe). Se compila con la feature `gui`
//! (activada por defecto) y arranca con `riku gui [archivo] [--cell X]
//! [--repo R --commit-a A --commit-b B]`.
//!
//! `riku open` y `riku diff -f visual` lo lanzan en un proceso hijo (el
//! mismo ejecutable) para no bloquear la terminal ni el shell.

use eframe::egui;

mod app;
mod canvas;
mod content;
mod details_panel;
mod entry_picker;
mod history;
pub(crate) use crate::i18n;
mod label_layout;
mod launch;
mod loader;
mod motion;
mod polygon_fill;
mod project;
mod scene_painter;
mod theme;
mod toast;
#[cfg(feature = "spice")]
mod wave_view;

pub(crate) use crate::i18n::tr;

/// `true` si hay un servidor gráfico al que conectarse (X11 o Wayland).
pub fn has_display() -> bool {
    std::env::var_os("DISPLAY").is_some() || std::env::var_os("WAYLAND_DISPLAY").is_some()
}

/// Abre la ventana con los argumentos dados (sin el nombre del programa ni
/// el subcomando `gui`). Bloquea hasta que se cierra.
pub fn run(args: Vec<String>) -> Result<(), String> {
    if cfg!(unix) && !has_display() {
        return Err("el visor necesita un escritorio gráfico (DISPLAY o WAYLAND_DISPLAY); \
                    la CLI funciona igual sin él"
            .into());
    }
    let launch = launch::parse_args(args.into_iter());
    // Tamaño inicial pensado para layouts: el lienzo necesita espacio entre
    // los paneles laterales (con 800×600 quedaba en menos de la mitad).
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Riku")
            .with_app_id("riku-gui")
            .with_inner_size([1400.0, 900.0])
            // Primera vez maximizada (un layout necesita todo el espacio);
            // después persist_window restaura el tamaño que dejó el usuario.
            .with_maximized(true)
            .with_min_inner_size([900.0, 600.0]),
        persist_window: true,
        ..Default::default()
    };

    eframe::run_native(
        "Riku GUI",
        options,
        Box::new(move |cc| Ok(Box::new(app::RikuGuiApp::new(cc, launch)))),
    )
    .map_err(|e| e.to_string())
}
