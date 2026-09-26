use eframe::egui;

mod app;
mod entry_picker;
mod label_layout;
mod launch;
mod motion;
mod polygon_fill;
mod project;
mod sch_painter;
mod scene_painter;
mod theme;
mod toast;

fn main() -> Result<(), eframe::Error> {
    let launch = launch::parse_args();
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
}
