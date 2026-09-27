//! La ventana del visor: estado, arranque, preferencias, atajos y el
//! cuadro (`ui`), que reparte el trabajo en `panels` (las zonas de la
//! ventana) y `loading` (abrir, recargar y recibir cargas).

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui;
use viewer_core::backend::ViewerBackend;

use crate::gui::canvas::Readout;
use crate::gui::content::{Content, DiffContext};
use crate::gui::history::{self, HistoryPanel};
use crate::gui::launch::LaunchArgs;
use crate::gui::loader::Loader;
use crate::gui::motion::theme_fade_alpha;
use crate::gui::project::ProjectEntry;
use crate::gui::theme::{space, CanvasTheme};
use crate::gui::toast::{ToastKind, Toasts};
use crate::gui::{i18n, tr};

mod loading;
mod panels;

pub struct RikuGuiApp {
    project_root: PathBuf,
    project_tree: ProjectEntry,
    selected_path: Option<PathBuf>,
    status: String,
    error: Option<String>,

    /// Lo que muestra el lienzo.
    content: Content,
    /// De qué diff viene lo que se ve (`None`: un archivo suelto). Se fija
    /// junto con `content`.
    diff: Option<DiffContext>,
    /// Carga en segundo plano (una sola; la nueva cancela la anterior).
    loader: Loader,
    /// Backends registrados. El primero que responda `accepts()` gana.
    backends: Vec<Arc<dyn ViewerBackend>>,
    /// Extensiones que saben abrir los backends (filtro del árbol).
    openable: Vec<String>,
    /// Expresiones de la vista de formas de onda (`--expr` o las de la sesión
    /// anterior); se aplican a cada `.raw` que se abre.
    #[cfg(feature = "spice")]
    wave_exprs: Vec<String>,

    // ─── Preferencias (persisten entre sesiones) ────────────────────────────
    /// Dibujar etiquetas de texto en el lienzo.
    show_labels: bool,
    /// Árbol de proyecto con todos los archivos, no solo los que se abren.
    show_all_files: bool,
    /// Sin animaciones ni inercia (accesibilidad: movimiento reducido).
    reduce_motion: bool,
    /// Resumir en bloques lo menor a un píxel al alejarse (layouts grandes).
    simplify: bool,
    /// Lado máximo de un bloque de nivel de detalle, en píxeles
    /// (`RIKU_LOD_PX` para ajustarlo; por defecto `viewer_core::index::BLOCK_PX`).
    block_px: f64,
    /// `RIKU_PROFILE`: imprimir tiempos de pintado por cuadro.
    profile: bool,

    /// Lecturas del lienzo para la barra de estado (del cuadro anterior).
    readout: Readout,
    /// Título aplicado a la ventana (para enviarlo solo cuando cambia).
    window_title: String,
    /// Mensajes temporales (feedback de estado, completado, aviso, error).
    toasts: Toasts,
    /// Reloj de egui del frame actual (para fechar los mensajes).
    now: f64,
    /// Archivos abiertos recientemente (persistente), el último primero.
    recent: Vec<String>,
    /// Área del lienzo en el último frame (los mensajes se anclan ahí).
    canvas_rect: Option<egui::Rect>,
    /// Tema del frame anterior (para detectar el cambio y fundirlo).
    last_dark: Option<bool>,
    /// Fundido en curso al cambiar de tema: (inicio, fondo del tema anterior).
    theme_fade: Option<(f64, egui::Color32)>,
    /// Panel History: el historial con su grafo (abajo, H).
    history: HistoryPanel,
}

/// Claves de persistencia (eframe storage).
const PREF_LABELS: &str = "riku.show_labels";
const PREF_ALL_FILES: &str = "riku.show_all_files";
const PREF_REDUCE_MOTION: &str = "riku.reduce_motion";
const PREF_SIMPLIFY: &str = "riku.simplify";
const PREF_RECENT: &str = "riku.recent_files";
const PREF_LANG: &str = "riku.lang";
const PREF_HISTORY_H: &str = "riku.history_height";
#[cfg(feature = "spice")]
const PREF_WAVE_EXPRS: &str = "riku.wave_exprs";
/// Cuántos archivos recientes se recuerdan.
const MAX_RECENT: usize = 6;

impl RikuGuiApp {
    pub fn new(cc: &eframe::CreationContext<'_>, launch: LaunchArgs) -> Self {
        // Load a system font explicitly — egui's embedded font sometimes fails with glow backend
        let mut fonts = egui::FontDefinitions::default();
        for path in [
            "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
            "/usr/share/fonts/truetype/ubuntu/Ubuntu-R.ttf",
        ] {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "system".to_owned(),
                    egui::FontData::from_owned(bytes).into(),
                );
                fonts.families.entry(egui::FontFamily::Proportional)
                    .or_default().insert(0, "system".to_owned());
                fonts.families.entry(egui::FontFamily::Monospace)
                    .or_default().insert(0, "system".to_owned());
                break;
            }
        }
        cc.egui_ctx.set_fonts(fonts);
        crate::gui::theme::install_style(&cc.egui_ctx);
        let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));

        // Ruta absoluta: con `riku-gui archivo.gds` el parent de una ruta
        // relativa es "" y el árbol de proyecto quedaba vacío.
        let launch_path = launch.file.as_deref().map(|p| std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf()));
        let (project_root, selected_path): (PathBuf, Option<PathBuf>) = match launch_path {
            Some(path) if path.is_file() => {
                let root = path.parent().map(Path::to_path_buf).unwrap_or_else(|| cwd.clone());
                (root, Some(path))
            }
            Some(path) if path.is_dir() => (path, None),
            _ => (cwd.clone(), None),
        };

        let pref = |key: &str, default: bool| {
            cc.storage.and_then(|s| eframe::get_value::<bool>(s, key)).unwrap_or(default)
        };
        let show_labels = pref(PREF_LABELS, true);
        let show_all_files = pref(PREF_ALL_FILES, false);
        let reduce_motion = pref(PREF_REDUCE_MOTION, false);
        let simplify = pref(PREF_SIMPLIFY, true);
        let recent: Vec<String> = cc.storage.and_then(|s| eframe::get_value(s, PREF_RECENT)).unwrap_or_default();
        let saved_lang: Option<String> = cc.storage.and_then(|s| eframe::get_value(s, PREF_LANG));
        i18n::set(&i18n::initial(saved_lang.as_deref()));

        // Los módulos del ejecutable deciden qué formatos se pueden abrir.
        let modules = crate::modules::registry();
        let backends: Vec<Arc<dyn ViewerBackend>> = modules.viewers();
        #[allow(unused_mut)]
        let mut openable: Vec<String> =
            backends.iter().flat_map(|b| b.info().extensions.iter().map(|e| e.to_string())).collect();
        #[cfg(feature = "spice")]
        openable.push("raw".to_string());
        let project_tree = ProjectEntry::build(&project_root, show_all_files, &openable);
        let history_h: f32 = cc.storage.and_then(|s| eframe::get_value(s, PREF_HISTORY_H)).unwrap_or(history::DEFAULT_HEIGHT);
        let history = HistoryPanel::new(&project_root, history_h);

        let mut app = Self {
            project_root,
            project_tree,
            selected_path,
            status: tr!("status.ready"),
            error: None,
            content: Content::Empty,
            diff: None,
            loader: Loader::new(),
            backends,
            openable,
            #[cfg(feature = "spice")]
            wave_exprs: if launch.exprs.is_empty() {
                cc.storage.and_then(|s| eframe::get_value(s, PREF_WAVE_EXPRS)).unwrap_or_default()
            } else {
                launch.exprs.clone()
            },
            show_labels,
            show_all_files,
            reduce_motion,
            simplify,
            profile: std::env::var_os("RIKU_PROFILE").is_some(),
            block_px: std::env::var("RIKU_LOD_PX")
                .ok()
                .and_then(|v| v.parse().ok())
                .filter(|v: &f64| *v > 0.0)
                .unwrap_or(viewer_core::index::BLOCK_PX),
            readout: Readout::default(),
            window_title: String::new(),
            toasts: Toasts::default(),
            now: 0.0,
            recent,
            canvas_rect: None,
            last_dark: None,
            theme_fade: None,
            history,
        };

        // Modo diff: commits pasados desde el CLI
        if let (Some(file), Some(ca), Some(cb)) = (&launch.file, &launch.commit_a, &launch.commit_b) {
            let repo = launch.repo.as_deref().unwrap_or(Path::new("."));
            match app.load_backend_diff(repo, ca, cb, file, launch.cell.clone(), false) {
                Ok(()) => app.status = format!("Diff {} → {}", ca, cb),
                Err(e) => app.fail(&tr!("error.diff"), e),
            }
        } else if let Some(path) = app.selected_path.clone() {
            app.remember_recent(&path);
            if app.load_via_backend(&path, launch.cell.clone()) {
                app.status = tr!("status.loading", file = path.display());
            } else {
                app.status = tr!("status.unsupported", file = path.display());
            }
        }

        app
    }

    fn refresh_tree(&mut self) {
        self.project_tree = ProjectEntry::build(&self.project_root, self.show_all_files, &self.openable);
    }

    /// Feedback breve (estado, completado, aviso) en un mensaje temporal.
    fn notify(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toasts.push(kind, text, self.now);
    }

    /// Error: queda en la barra de estado y en un mensaje que no se va solo.
    fn fail(&mut self, what: &str, e: impl std::fmt::Display) {
        let msg = format!("{what}: {e}");
        self.status = tr!("status.error", what = what);
        self.error = Some(msg.clone());
        self.toasts.push(ToastKind::Error, msg, self.now);
    }

    /// Recuerda un archivo abierto (el más reciente primero, sin repetir).
    fn remember_recent(&mut self, path: &Path) {
        let p = path.to_string_lossy().to_string();
        self.recent.retain(|r| *r != p);
        self.recent.insert(0, p);
        self.recent.truncate(MAX_RECENT);
    }

    /// Encuadrar todo, pedido por el usuario (se anima salvo movimiento reducido).
    fn request_fit(&mut self) {
        if let Some(bs) = self.content.scene_mut() {
            bs.needs_fit = true;
            bs.animate_fit = true;
        }
    }

    /// Atajos de teclado, solo si ningún campo de texto tiene el foco (si no,
    /// escribir "f" en el buscador de celdas encuadraría la vista).
    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        if ctx.input(|i| i.key_pressed(egui::Key::H)) {
            self.history.toggle();
        }
        let (fit, labels, zoom_in, zoom_out) = ctx.input(|i| {
            (
                i.key_pressed(egui::Key::F),
                i.key_pressed(egui::Key::L),
                // Todas las pulsaciones del cuadro: con un layout pesado, varias
                // caen en el mismo y no deben perderse.
                i.num_presses(egui::Key::Plus) + i.num_presses(egui::Key::Equals),
                i.num_presses(egui::Key::Minus),
            )
        });
        if fit {
            self.request_fit();
        }
        if labels {
            self.show_labels = !self.show_labels;
            // Con el teclado no se ve el botón cambiar: confirmarlo.
            let msg = if self.show_labels { tr!("toast.labels_on") } else { tr!("toast.labels_off") };
            self.notify(ToastKind::Info, msg);
        }
        if let Some(bs) = self.content.scene_mut() {
            let step: f64 = 1.25;
            let presses = zoom_in as i32 - zoom_out as i32;
            let factor = (presses != 0).then(|| step.powi(presses));
            if let Some(f) = factor {
                bs.pending_zoom = Some(bs.pending_zoom.unwrap_or(1.0) * f);
            }
        }
    }

}

impl eframe::App for RikuGuiApp {
    /// Preferencias propias; el tema lo persiste egui junto a su memoria.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, PREF_LABELS, &self.show_labels);
        eframe::set_value(storage, PREF_ALL_FILES, &self.show_all_files);
        eframe::set_value(storage, PREF_REDUCE_MOTION, &self.reduce_motion);
        eframe::set_value(storage, PREF_HISTORY_H, &self.history.height);
        eframe::set_value(storage, PREF_SIMPLIFY, &self.simplify);
        eframe::set_value(storage, PREF_RECENT, &self.recent);
        eframe::set_value(storage, PREF_LANG, &i18n::current());
        #[cfg(feature = "spice")]
        eframe::set_value(storage, PREF_WAVE_EXPRS, &self.wave_exprs);
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.now = ctx.input(|i| i.time);

        // Cambio de tema: fundir desde el fondo anterior en vez de saltar de
        // golpe entre claro y oscuro (salvo movimiento reducido).
        let dark = ctx.theme() == egui::Theme::Dark;
        if self.last_dark.is_some_and(|was| was != dark) && !self.reduce_motion {
            let prev = if dark { egui::Visuals::light() } else { egui::Visuals::dark() };
            self.theme_fade = Some((self.now, CanvasTheme::from_visuals(&prev).background));
        }
        self.last_dark = Some(dark);

        // Arrastrar un archivo desde el explorador lo abre.
        let dropped = ctx.input(|i| i.raw.dropped_files.iter().find_map(|f| f.path.clone()));
        if let Some(path) = dropped {
            self.open_path(&path);
        }

        // Drenar carga async antes de pintar; si sigue en vuelo, solicitar
        // repaint para que el promise se consulte en el siguiente frame.
        self.poll_pending_load();
        if self.loader.busy() {
            ctx.request_repaint();
        }

        self.handle_shortcuts(&ctx);

        // Título de ventana: archivo (y celda) abiertos.
        let file = self.selected_path.as_ref().and_then(|p| p.file_name()).map(|n| n.to_string_lossy().to_string());
        let cell = self.content.scene().and_then(|bs| bs.scene.current_entry().map(str::to_string));
        let title = match (file, cell) {
            (Some(f), Some(c)) => format!("{f} · {c} — Riku"),
            (Some(f), None) => format!("{f} — Riku"),
            _ => "Riku".to_string(),
        };
        if title != self.window_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.window_title = title;
        }

        self.top_bar(ui);
        self.status_bar(ui);

        // ─── History: abajo, a todo el ancho, sobre la barra de estado ───────
        let dt = ctx.input(|i| i.stable_dt).clamp(0.001, 0.05) as f64;
        // Relativo al repo; un diff abierto entre commits ya guarda la ruta
        // relativa.
        let open_file = self.selected_path.as_ref().and_then(|p| {
            let rel = if p.is_relative() { p.as_path() } else { p.strip_prefix(self.history.repo()?).ok()? };
            Some(rel.to_string_lossy().replace('\\', "/"))
        });
        for req in self.history.show(ui, self.now, dt, self.reduce_motion, open_file) {
            self.handle_history_request(req);
        }

        self.left_panel(ui);
        self.right_panel(ui);
        self.central(ui);

        #[cfg(feature = "spice")]
        self.handle_wave_request();

        // Archivo arrastrado sobre la ventana: indicar que se puede soltar.
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            drop_hint(&ctx);
        }
        let area = self.canvas_rect.unwrap_or_else(|| ctx.content_rect());
        self.toasts.show(&ctx, area);

        if let Some((start, from)) = self.theme_fade {
            match theme_fade_alpha(self.now - start) {
                Some(alpha) => {
                    let layer = egui::LayerId::new(egui::Order::Tooltip, egui::Id::new("theme_fade"));
                    ctx.layer_painter(layer).rect_filled(ctx.content_rect(), 0.0, from.gamma_multiply(alpha));
                    ctx.request_repaint();
                }
                None => self.theme_fade = None,
            }
        }
    }
}

/// Velo sobre toda la ventana mientras se arrastra un archivo encima.
fn drop_hint(ctx: &egui::Context) {
    let screen = ctx.content_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::new("drop_hint")));
    let v = ctx.global_style().visuals.clone();
    painter.rect_filled(screen, 0.0, v.window_fill.gamma_multiply(0.85));
    painter.rect_stroke(
        screen.shrink(space::L),
        12.0,
        egui::Stroke::new(2.0_f32, v.selection.stroke.color),
        egui::StrokeKind::Inside,
    );
    painter.text(
        screen.center(),
        egui::Align2::CENTER_CENTER,
        tr!("drop.hint"),
        egui::FontId::proportional(20.0),
        v.strong_text_color(),
    );
}

// ─── Selector de vistas (modo diff) ──────────────────────────────────────────

fn short_hash(s: &str) -> String {
    // Vacío: el "antes" del commit inicial.
    if s.is_empty() {
        "∅".to_string()
    } else if s == crate::core::analysis::diff_set::WORKTREE {
        "worktree".to_string()
    } else {
        s.chars().take(7).collect()
    }
}

/// Error de carga en lenguaje claro. Conserva el mensaje técnico al final
/// (entre paréntesis) para quien necesite diagnosticar.
fn friendly_error(e: &str) -> String {
    let plain = if e.contains("GDSII") || e.contains("input file read error") {
        Some(tr!("error.bad_gds"))
    } else if e.contains("no existe") || e.contains("No such file") {
        Some(tr!("error.missing_file"))
    } else {
        None
    };
    match plain {
        Some(p) => format!("{p} ({e})"),
        None => e.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn friendly_error_explains_corrupt_gds_and_keeps_details() {
        let e = "parse error: GDSII parse: gdstk error: input file read error";
        let f = friendly_error(e);
        assert!(f.starts_with("not a valid GDSII"), "{f}");
        assert!(f.contains(e), "conserva el detalle técnico");
        assert_eq!(friendly_error("otra cosa"), "otra cosa");
    }
}
