//! "Abrir carpeta…": un selector propio, dibujado con egui. Los diálogos
//! nativos en Linux dependen de GTK o de un portal de escritorio, que no
//! están en el contenedor de diseño ni por WSLg; este anda en cualquier
//! lado. Se navega por las carpetas o se pega una ruta; las que son un
//! repositorio Git llevan una marca.
//!
//! En una laptop con Windows (Riku en un contenedor o en WSL) se puede pegar
//! la ruta de Windows (`C:\Users\…`): se traduce a la carpeta montada, y hay
//! atajos a las carpetas de Windows que el contenedor ve (ver
//! [`crate::core::host_paths`]).

use std::path::{Path, PathBuf};

use eframe::egui::{self, RichText};

use crate::core::host_paths::{self, HostMount};
use crate::gui::theme::space;
use crate::gui::tr;

struct Dir {
    name: String,
    path: PathBuf,
    is_repo: bool,
}

pub(crate) struct FolderPicker {
    dir: PathBuf,
    /// Ruta que se escribe o pega arriba (Enter para ir).
    path_text: String,
    dirs: Vec<Dir>,
    show_hidden: bool,
    error: Option<String>,
    /// Carpetas de Windows montadas (vacío fuera de un contenedor o WSL).
    mounts: Vec<HostMount>,
}

/// Qué eligió el usuario.
pub(crate) enum Picked {
    Folder(PathBuf),
    Cancel,
}

fn is_repo(dir: &Path) -> bool {
    dir.join(".git").exists()
}

impl FolderPicker {
    pub(crate) fn new(start: &Path) -> Self {
        let mut p = Self {
            dir: PathBuf::new(),
            path_text: String::new(),
            dirs: Vec::new(),
            show_hidden: false,
            error: None,
            mounts: host_paths::host_mounts(),
        };
        p.go(start.to_path_buf());
        p
    }

    /// Entra en `dir` y lista sus carpetas. Si no se puede leer, se queda
    /// donde estaba y lo dice.
    fn go(&mut self, dir: PathBuf) {
        let dir = std::path::absolute(&dir).unwrap_or(dir);
        match std::fs::read_dir(&dir) {
            Ok(entries) => {
                let mut dirs: Vec<Dir> = entries
                    .flatten()
                    .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()) || e.path().is_dir())
                    .map(|e| {
                        let path = e.path();
                        Dir { name: e.file_name().to_string_lossy().to_string(), is_repo: is_repo(&path), path }
                    })
                    .filter(|d| self.show_hidden || !d.name.starts_with('.'))
                    .collect();
                dirs.sort_by_key(|d| d.name.to_lowercase());
                self.dirs = dirs;
                self.path_text = dir.display().to_string();
                self.dir = dir;
                self.error = None;
            }
            Err(e) => {
                self.error = Some(format!("{}: {e}", dir.display()));
                self.path_text = self.dir.display().to_string();
            }
        }
    }

    /// La carpeta de lo escrito en la barra: una ruta de Windows se traduce
    /// a la montada; si no está compartida con el contenedor, el aviso.
    fn resolve(&self, text: &str) -> Result<PathBuf, String> {
        let text = text.trim().trim_matches('"');
        if !host_paths::is_windows_path(text) {
            return Ok(PathBuf::from(text));
        }
        host_paths::to_local(text, &self.mounts).ok_or_else(|| {
            let shared: Vec<&str> = self.mounts.iter().map(|m| m.windows.as_str()).collect();
            let shared = if shared.is_empty() { tr!("picker.none") } else { shared.join(", ") };
            tr!("picker.not_shared", path = text, shared = shared)
        })
    }

    /// El selector como ventana modal. `Some` cuando el usuario decide.
    pub(crate) fn show(&mut self, ctx: &egui::Context) -> Option<Picked> {
        let mut picked = None;
        let mut go_to = None;
        let mut not_shared = None;
        let modal = egui::Modal::new(egui::Id::new("folder_picker")).show(ctx, |ui| {
            ui.set_width(560.0_f32.min(ctx.content_rect().width() - 64.0));
            ui.heading(tr!("picker.title"));
            ui.add_space(space::S);

            ui.horizontal(|ui| {
                if ui.button("↑").on_hover_text(tr!("picker.up")).clicked() {
                    go_to = self.dir.parent().map(Path::to_path_buf);
                }
                if let Some(home) = dirs::home_dir() {
                    if ui.button(tr!("picker.home")).on_hover_text(home.display().to_string()).clicked() {
                        go_to = Some(home);
                    }
                }
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.path_text)
                        .desired_width(f32::INFINITY)
                        .hint_text(tr!("picker.path_hint")),
                );
                if edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    match self.resolve(&self.path_text) {
                        Ok(dir) => go_to = Some(dir),
                        Err(e) => not_shared = Some(e),
                    }
                }
            });
            // Carpetas de la laptop (Windows) que ve el contenedor.
            if !self.mounts.is_empty() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(tr!("picker.laptop")).small().weak());
                    for m in &self.mounts {
                        if ui.small_button(&m.windows).on_hover_text(m.local.display().to_string()).clicked() {
                            go_to = Some(m.local.clone());
                        }
                    }
                });
            }
            if let Some(windows) = host_paths::to_windows(&self.dir, &self.mounts) {
                ui.label(RichText::new(tr!("picker.in_windows", path = windows)).small().weak());
            }
            if let Some(e) = &self.error {
                ui.label(RichText::new(e).small().color(ui.visuals().error_fg_color));
            }
            ui.add_space(space::XS);

            let row_h = ui.spacing().interact_size.y;
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.set_min_height(320.0);
                if self.dirs.is_empty() {
                    ui.label(RichText::new(tr!("picker.no_subfolders")).weak());
                }
                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false, false]).show_rows(
                    ui,
                    row_h,
                    self.dirs.len(),
                    |ui, range| {
                        for d in &self.dirs[range] {
                            ui.horizontal(|ui| {
                                let resp = ui
                                    .add(egui::Button::new(format!("{}/", d.name)).frame(false))
                                    .on_hover_cursor(egui::CursorIcon::PointingHand);
                                if d.is_repo {
                                    ui.label(RichText::new("git").small().color(ui.visuals().selection.bg_fill));
                                }
                                if resp.clicked() {
                                    go_to = Some(d.path.clone());
                                }
                            });
                        }
                    },
                );
            });

            ui.add_space(space::S);
            ui.horizontal(|ui| {
                if ui.checkbox(&mut self.show_hidden, tr!("picker.hidden")).changed() {
                    go_to = Some(self.dir.clone());
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let open = egui::Button::new(RichText::new(tr!("picker.open")).strong())
                        .fill(ui.visuals().selection.bg_fill);
                    if ui.add(open).clicked() {
                        picked = Some(Picked::Folder(self.dir.clone()));
                    }
                    if ui.button(tr!("picker.cancel")).clicked() {
                        picked = Some(Picked::Cancel);
                    }
                    let here = if is_repo(&self.dir) { tr!("picker.is_repo") } else { String::new() };
                    ui.label(RichText::new(here).small().weak());
                });
            });
        });
        if let Some(dir) = go_to {
            self.go(dir);
        } else if let Some(e) = not_shared {
            self.error = Some(e);
        }
        if modal.should_close() && picked.is_none() {
            picked = Some(Picked::Cancel);
        }
        picked
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lista_carpetas_y_marca_repos() {
        let dir = tempfile::tempdir().unwrap();
        for d in ["b", "A/.git", ".oculta", "c"] {
            std::fs::create_dir_all(dir.path().join(d)).unwrap();
        }
        std::fs::write(dir.path().join("archivo.gds"), b"").unwrap();
        let mut p = FolderPicker::new(dir.path());
        let names: Vec<(&str, bool)> = p.dirs.iter().map(|d| (d.name.as_str(), d.is_repo)).collect();
        assert_eq!(names, [("A", true), ("b", false), ("c", false)], "sin ocultas ni archivos, sin mayúsculas primero");

        p.show_hidden = true;
        p.go(dir.path().to_path_buf());
        assert!(p.dirs.iter().any(|d| d.name == ".oculta"));

        // Una ruta que no existe no cambia la carpeta: avisa.
        let before = p.dir.clone();
        p.go(dir.path().join("no-existe"));
        assert_eq!(p.dir, before);
        assert!(p.error.is_some());
    }

    #[test]
    fn una_ruta_de_windows_va_a_su_carpeta_montada() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("riku_chip")).unwrap();
        let mut p = FolderPicker::new(dir.path());
        p.mounts = vec![HostMount { windows: r"C:\Users\U\designs".into(), local: dir.path().to_path_buf() }];
        assert_eq!(p.resolve(r"C:\Users\U\designs\riku_chip"), Ok(dir.path().join("riku_chip")));
        assert_eq!(p.resolve("  /tmp  "), Ok(PathBuf::from("/tmp")));
        let err = p.resolve(r"C:\Users\U\Desktop").unwrap_err();
        assert!(err.contains(r"C:\Users\U\designs"), "{err}");
    }
}
