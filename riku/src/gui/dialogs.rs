//! Ventanas de la pantalla de inicio: "Comparar versiones…" (lo de `riku
//! diff A B archivo`) y "Diagnóstico" (lo de `riku doctor`). Solo piden y
//! muestran; el trabajo lo hace el mismo núcleo que la CLI.

use eframe::egui::{self, RichText};
use poll_promise::Promise;

use crate::cli::doctor::{self, Mark, Section};
use crate::core::analysis::diff_pair::WORKTREE;
use crate::gui::theme::space;
use crate::gui::tr;

/// Una de las dos versiones: el disco o un commit (hash, rama, tag, `HEAD~2`).
#[derive(Clone)]
pub(crate) struct VersionInput {
    pub disk: bool,
    pub rev: String,
}

impl VersionInput {
    fn rev(rev: &str) -> Self {
        Self { disk: false, rev: rev.to_string() }
    }

    /// Como la entiende el visor (`:worktree` es el disco).
    pub(crate) fn token(&self) -> String {
        if self.disk {
            WORKTREE.to_string()
        } else {
            self.rev.trim().to_string()
        }
    }
}

pub(crate) struct CompareDialog {
    /// Archivos del repo que se pueden comparar (rutas relativas).
    files: Vec<String>,
    pub file: String,
    pub a: VersionInput,
    pub b: VersionInput,
    /// Ramas y tags del repo, para elegir sin escribir.
    refs: Vec<String>,
    query: String,
}

/// Qué pidió el usuario en "Comparar".
pub(crate) enum CompareOutcome {
    Go { file: String, a: String, b: String },
    Cancel,
}

impl CompareDialog {
    /// `file`: el archivo abierto, si hay uno, preseleccionado. Por defecto
    /// compara `HEAD` con el disco (lo que cambió sin commitear).
    pub(crate) fn new(files: Vec<String>, refs: Vec<String>, file: Option<String>) -> Self {
        let file = file.filter(|f| files.contains(f)).or_else(|| files.first().cloned()).unwrap_or_default();
        Self { files, file, a: VersionInput::rev("HEAD"), b: VersionInput { disk: true, rev: String::new() }, refs, query: String::new() }
    }

    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<CompareOutcome> {
        let mut out = None;
        let modal = egui::Modal::new(egui::Id::new("compare_dialog")).show(ctx, |ui| {
            ui.set_width(520.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading(tr!("compare.title"));
            ui.label(RichText::new(tr!("compare.hint")).weak());
            ui.add_space(space::M);

            ui.label(RichText::new(tr!("compare.file")).strong());
            if self.files.is_empty() {
                ui.label(RichText::new(tr!("compare.no_files")).weak());
            } else {
                ui.add(egui::TextEdit::singleline(&mut self.query).hint_text(tr!("compare.filter")).desired_width(f32::INFINITY));
                let q = self.query.to_lowercase();
                egui::ScrollArea::vertical().id_salt("compare_files").max_height(160.0).auto_shrink([false, true]).show(ui, |ui| {
                    for f in self.files.iter().filter(|f| q.is_empty() || f.to_lowercase().contains(&q)) {
                        if ui.selectable_label(*f == self.file, f).clicked() {
                            self.file = f.clone();
                        }
                    }
                });
            }
            ui.add_space(space::M);

            egui::Grid::new("compare_versions").num_columns(2).spacing([space::M, space::S]).show(ui, |ui| {
                ui.label(RichText::new(tr!("compare.before")).strong());
                version_row(ui, "a", &mut self.a, &self.refs);
                ui.end_row();
                ui.label(RichText::new(tr!("compare.after")).strong());
                version_row(ui, "b", &mut self.b, &self.refs);
                ui.end_row();
            });

            ui.add_space(space::M);
            let ready = !self.file.is_empty() && (self.a.disk || !self.a.rev.trim().is_empty()) && (self.b.disk || !self.b.rev.trim().is_empty());
            ui.horizontal(|ui| {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let go = egui::Button::new(RichText::new(tr!("compare.go")).strong()).fill(ui.visuals().selection.bg_fill);
                    if ui.add_enabled(ready, go).clicked() {
                        out = Some(CompareOutcome::Go { file: self.file.clone(), a: self.a.token(), b: self.b.token() });
                    }
                    if ui.button(tr!("picker.cancel")).clicked() {
                        out = Some(CompareOutcome::Cancel);
                    }
                });
            });
        });
        if modal.should_close() && out.is_none() {
            out = Some(CompareOutcome::Cancel);
        }
        out
    }
}

/// Disco o commit, y el commit escrito o elegido de la lista.
fn version_row(ui: &mut egui::Ui, id: &str, v: &mut VersionInput, refs: &[String]) {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut v.disk, true, tr!("compare.disk")).on_hover_text(tr!("compare.disk_hint"));
        ui.selectable_value(&mut v.disk, false, tr!("compare.commit"));
        if !v.disk {
            ui.add(egui::TextEdit::singleline(&mut v.rev).desired_width(140.0).hint_text("HEAD~1"));
            egui::ComboBox::from_id_salt(("compare_refs", id)).selected_text("").width(24.0).show_ui(ui, |ui| {
                for r in ["HEAD", "HEAD~1"].iter().map(|s| s.to_string()).chain(refs.iter().cloned()) {
                    if ui.selectable_label(v.rev == r, &r).clicked() {
                        v.rev = r;
                    }
                }
            });
        }
    });
}

/// "Diagnóstico": lo de `riku doctor`, calculado en un hilo (recorre el PDK).
pub(crate) struct DoctorDialog {
    job: Promise<Vec<Section>>,
}

impl DoctorDialog {
    pub(crate) fn new(repo: std::path::PathBuf, ctx: &egui::Context) -> Self {
        let ctx = ctx.clone();
        let job = Promise::spawn_thread("riku-doctor", move || {
            let out = doctor::sections(&repo);
            ctx.request_repaint();
            out
        });
        Self { job }
    }

    /// `false` cuando el usuario la cierra.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> bool {
        let mut open = true;
        let modal = egui::Modal::new(egui::Id::new("doctor_dialog")).show(ctx, |ui| {
            ui.set_width(620.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading(tr!("doctor.title"));
            ui.add_space(space::S);
            match self.job.ready() {
                None => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(tr!("home.doctor_running"));
                    });
                }
                Some(sections) => {
                    egui::ScrollArea::vertical().max_height(ctx.content_rect().height() * 0.6).show(ui, |ui| {
                        for s in sections {
                            ui.add_space(space::S);
                            ui.label(RichText::new(&s.title).strong());
                            for (mark, text) in &s.items {
                                ui.horizontal_wrapped(|ui| {
                                    let v = ui.visuals();
                                    let (sym, color) = match mark {
                                        Mark::Ok => ("✔", crate::gui::theme::change_color(viewer_core::diff::ChangeKind::Added, v.dark_mode)),
                                        Mark::Warn => ("!", v.warn_fg_color),
                                        Mark::Absent => ("–", v.weak_text_color()),
                                        Mark::Missing => ("✖", v.error_fg_color),
                                    };
                                    ui.label(RichText::new(sym).strong().color(color));
                                    // Los saltos con sangría son para la terminal;
                                    // aquí el texto se ajusta solo.
                                    let text: Vec<&str> = text.lines().map(str::trim).collect();
                                    ui.label(text.join(" "));
                                });
                            }
                        }
                    });
                }
            }
            ui.add_space(space::M);
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(tr!("dialog.close")).clicked() {
                    open = false;
                }
            });
        });
        open && !modal.should_close()
    }
}
