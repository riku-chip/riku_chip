mod app;
mod project;
mod scene_painter;
mod sch_painter;

use std::path::PathBuf;

fn main() -> Result<(), eframe::Error> {
    let initial_path = std::env::args_os().nth(1).map(PathBuf::from);
    let options = eframe::NativeOptions::default();

    eframe::run_native(
        "Riku GUI",
        options,
        Box::new(move |cc| Ok(Box::new(app::RikuGuiApp::new(cc, initial_path.clone())))),
    )
}
