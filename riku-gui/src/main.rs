mod app;
mod launch;
mod project;
mod sch_painter;

fn main() -> Result<(), eframe::Error> {
    let launch = launch::parse_args();
    let options = eframe::NativeOptions::default();

    eframe::run_native(
        "Riku GUI",
        options,
        Box::new(move |cc| Ok(Box::new(app::RikuGuiApp::new(cc, launch)))),
    )
}
