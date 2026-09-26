mod app;
mod entry_picker;
mod launch;
mod polygon_fill;
mod project;
mod sch_painter;
mod scene_painter;

fn main() -> Result<(), eframe::Error> {
    let launch = launch::parse_args();
    let options = eframe::NativeOptions::default();

    eframe::run_native(
        "Riku GUI",
        options,
        Box::new(move |cc| Ok(Box::new(app::RikuGuiApp::new(cc, launch)))),
    )
}
