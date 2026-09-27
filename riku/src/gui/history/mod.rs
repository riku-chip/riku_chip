//! Panel **History**: el historial del repo con el grafo de ramas, el
//! resumen semántico de cada commit y, a un clic, el diff de un archivo en el
//! lienzo. Diseño en `docs/diseno/fase7.md` §4.
//!
//! - Datos en dos fases, en hilos aparte: primero commits y grafo (solo Git,
//!   al instante), después los resúmenes (el análisis en paralelo de 6.6).
//!   El detalle de un commit (`riku show`) se pide al elegirlo y queda en
//!   cache.
//! - Movimiento solo donde ayuda y siempre interrumpible: la altura del
//!   panel y la marca de selección siguen resortes críticamente amortiguados
//!   (`motion::spring_step`) desde su valor actual; con **Reduce motion**,
//!   nada se anima. El grafo y el texto no se mueven.
//! - El estado vive en [`model::HistoryModel`] (sin egui); acá solo se dibuja
//!   y se traducen gestos en llamadas al modelo.

pub mod geometry;
pub mod model;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Pos2, RichText, Sense, Stroke};
use poll_promise::Promise;

use crate::core::analysis::log::{self, LogOptions, LogReport};
use crate::core::analysis::show::analyze_show;
use crate::core::analysis::summary::{FileSummary, SummaryCategory};
use crate::core::domain::git_types::ChangeStatus;
use crate::core::domain::ports::RepoRoot;
use crate::core::git::git_service::GitService;
use crate::gui::motion::spring_step;
use crate::gui::theme::{self, space};
use crate::gui::tr;
use geometry::{Metrics, Prim};
use model::{Details, HistoryModel, Request};

/// Respuesta de los resortes del panel (s): la de la vista del lienzo.
const RESPONSE: f64 = 0.3;
/// Alto inicial del panel.
pub const DEFAULT_HEIGHT: f32 = 260.0;
/// Opacidad de las ramas que no se están siguiendo (enfocar atenuando).
const DIM: f32 = 0.35;
/// Espera antes de pedir el detalle del commit elegido (s): recorrer la
/// lista con ↓ no lanza un análisis por cada fila que pasa.
const DETAIL_DEBOUNCE: f64 = 0.15;

type Job<T> = Promise<Result<T, String>>;

pub struct HistoryPanel {
    pub open: bool,
    /// Alto elegido por el usuario (se recuerda entre sesiones).
    pub height: f32,
    anim_h: f64,
    anim_v: f64,
    sel_y: f64,
    sel_v: f64,
    /// Raíz del working tree (las rutas del historial son relativas a ella).
    repo: Option<PathBuf>,
    pub model: HistoryModel,
    graph_job: Option<Job<LogReport>>,
    summary_job: Option<Job<LogReport>>,
    detail_jobs: HashMap<String, Job<Details>>,
    /// Commit elegido cuyo detalle falta, y desde cuándo (debounce).
    detail_wanted: Option<(String, f64)>,
    /// Para que los hilos despierten a la UI al terminar (sin redibujar a
    /// 60 fps mientras esperan).
    ctx: Option<egui::Context>,
    error: Option<String>,
    loaded: bool,
    filter_text: String,
    /// Cuándo llegaron los resúmenes (para fundirlos al aparecer).
    summaries_at: Option<f64>,
    /// Rama resaltada al pasar el mouse por un chip.
    hover_lane: Option<usize>,
    scroll_to_selection: bool,
    last_offset: f32,
    viewport_h: f32,
}

impl HistoryPanel {
    pub fn new(project_root: &Path, height: f32) -> Self {
        let repo = GitService::open(project_root).ok().and_then(|s| s.root().map(Path::to_path_buf));
        Self {
            open: false,
            height,
            anim_h: 0.0,
            anim_v: 0.0,
            sel_y: 0.0,
            sel_v: 0.0,
            repo,
            model: HistoryModel::default(),
            graph_job: None,
            summary_job: None,
            detail_jobs: HashMap::new(),
            detail_wanted: None,
            ctx: None,
            error: None,
            loaded: false,
            filter_text: String::new(),
            summaries_at: None,
            hover_lane: None,
            scroll_to_selection: false,
            last_offset: 0.0,
            viewport_h: 0.0,
        }
    }

    /// Raíz del repo, si el proyecto está dentro de uno.
    pub fn repo(&self) -> Option<&Path> {
        self.repo.as_deref()
    }

    pub fn toggle(&mut self) {
        if self.repo.is_some() {
            self.open = !self.open;
        }
    }

    /// Vuelve a leer el historial (otro filtro, más páginas, un commit nuevo).
    pub fn reload(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        let paths: Vec<String> = self.model.filter.split_whitespace().map(str::to_string).collect();
        let opts = LogOptions {
            limit: Some(self.model.limit()),
            paths,
            graph: true,
            skip_summaries: true,
            ..LogOptions::default()
        };
        self.error = None;
        self.summary_job = None;
        self.graph_job = Some(spawn("riku-history", self.ctx.clone(), move || {
            log::analyze_with_options_path(&repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())
        }));
        self.loaded = true;
    }

    fn start_summaries(&mut self) {
        let Some(repo) = self.repo.clone() else { return };
        // Los mismos commits que el grafo: mismo filtro y mismo límite.
        let paths: Vec<String> = self.model.filter.split_whitespace().map(str::to_string).collect();
        let opts = LogOptions { limit: Some(self.model.limit()), paths, graph: true, ..LogOptions::default() };
        self.summary_job = Some(spawn("riku-history-summaries", self.ctx.clone(), move || {
            log::analyze_with_options_path(&repo, &opts, &crate::modules::registry()).map_err(|e| e.to_string())
        }));
    }

    /// Recoge lo que terminó en segundo plano y pide lo que falta.
    fn poll(&mut self, now: f64) {
        if let Some(r) = take_ready(&mut self.graph_job) {
            // El filtro por archivo es de Git: el grafo llega sin resúmenes
            // también con filtro, y los resúmenes se piden después.
            match r {
                Ok(report) => {
                    self.model.set_graph(report);
                    self.start_summaries();
                }
                Err(e) => self.error = Some(e),
            }
        }
        if let Some(r) = take_ready(&mut self.summary_job) {
            match r {
                Ok(report) => {
                    self.model.set_summaries(report);
                    self.summaries_at = Some(now);
                }
                Err(e) => self.error = Some(e),
            }
        }
        let ready: Vec<String> = self.detail_jobs.iter().filter(|(_, p)| p.ready().is_some()).map(|(k, _)| k.clone()).collect();
        for oid in ready {
            if let Some(p) = self.detail_jobs.remove(&oid) {
                self.model.set_details(oid, p.block_and_take());
            }
        }
        // El detalle del commit elegido, si falta: solo con el panel abierto,
        // cuando la selección quedó quieta un momento y de a uno por vez
        // (el último elegido gana; los que ya terminaron quedan en cache).
        let wanted = self
            .model
            .selected_commit()
            .map(|c| c.info.oid.clone())
            .filter(|oid| self.open && !self.model.details.contains_key(oid) && !self.detail_jobs.contains_key(oid));
        let Some(oid) = wanted else {
            self.detail_wanted = None;
            return;
        };
        let since = match &self.detail_wanted {
            Some((w, t)) if *w == oid => *t,
            _ => {
                self.detail_wanted = Some((oid.clone(), now));
                now
            }
        };
        if now - since < DETAIL_DEBOUNCE || !self.detail_jobs.is_empty() {
            if let Some(ctx) = &self.ctx {
                ctx.request_repaint_after(std::time::Duration::from_secs_f64(DETAIL_DEBOUNCE));
            }
            return;
        }
        if let Some(repo) = self.repo.clone() {
            self.detail_wanted = None;
            let job_oid = oid.clone();
            self.detail_jobs.insert(
                oid,
                spawn("riku-history-show", self.ctx.clone(), move || {
                    let svc = GitService::open(&repo).map_err(|e| e.to_string())?;
                    let opts = riku_kernel::DiffOptions::default();
                    analyze_show(&svc, &job_oid, None, &crate::modules::registry(), &opts)
                        .map(|r| Details::from_show(&r))
                        .map_err(|e| e.to_string())
                }),
            );
        }
    }

    fn busy(&self) -> bool {
        self.graph_job.is_some() || self.summary_job.is_some() || !self.detail_jobs.is_empty()
    }

    /// Dibuja el panel (si está abierto o cerrándose) y devuelve lo que el
    /// usuario pidió abrir. `open_file`: el archivo abierto en el lienzo,
    /// relativo al repo (para **Only this file**).
    pub fn show(&mut self, ui: &mut egui::Ui, now: f64, dt: f64, reduce_motion: bool, open_file: Option<String>) -> Vec<Request> {
        if self.ctx.is_none() {
            self.ctx = Some(ui.ctx().clone());
        }
        if self.open && !self.loaded {
            self.reload();
        }
        self.poll(now);

        // Altura: resorte desde el valor actual; H a mitad de camino lo invierte.
        let target = if self.open { self.height as f64 } else { 0.0 };
        if reduce_motion {
            (self.anim_h, self.anim_v) = (target, 0.0);
        } else {
            (self.anim_h, self.anim_v) = spring_step(self.anim_h, self.anim_v, target, dt, RESPONSE);
        }
        let animating = (self.anim_h - target).abs() > 0.5;
        if animating {
            ui.ctx().request_repaint();
        } else if self.busy() {
            // Los hilos despiertan a la UI al terminar; esto es solo un
            // respaldo, no un sondeo a 60 fps.
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(500));
        }
        if !self.open && !animating {
            self.anim_h = 0.0;
            return self.model.take_requests();
        }

        if self.open && !ui.ctx().egui_wants_keyboard_input() {
            let (down, up, enter) = ui.ctx().input(|i| {
                (i.num_presses(egui::Key::ArrowDown), i.num_presses(egui::Key::ArrowUp), i.key_pressed(egui::Key::Enter))
            });
            let delta = down as i64 - up as i64;
            if delta != 0 {
                self.model.move_selection(delta);
                self.scroll_to_selection = true;
            }
            if enter {
                self.model.open_first();
            }
        }

        let max_h = (ui.available_height() * 0.75).max(160.0);
        let panel = egui::Panel::bottom("history_panel");
        let panel = if animating {
            panel.exact_size(self.anim_h as f32)
        } else {
            panel.resizable(true).default_size(self.height).size_range(140.0..=max_h)
        };
        let shown = panel.show_inside(ui, |ui| self.contents(ui, now, dt, reduce_motion, open_file));
        if self.open && !animating {
            self.height = shown.response.rect.height();
            self.anim_h = self.height as f64;
        }
        self.model.take_requests()
    }

    fn contents(&mut self, ui: &mut egui::Ui, now: f64, dt: f64, reduce_motion: bool, open_file: Option<String>) {
        ui.add_space(space::XS);
        self.header(ui, open_file);
        ui.add_space(space::XS);
        egui::Panel::right("history_details")
            .resizable(true)
            .default_size(320.0)
            .size_range(220.0..=560.0)
            .show_inside(ui, |ui| self.details(ui));
        egui::CentralPanel::default().show_inside(ui, |ui| self.list(ui, now, dt, reduce_motion));
    }

    /// Una línea: título, filtro y estado (dónde estoy y qué está pasando).
    fn header(&mut self, ui: &mut egui::Ui, open_file: Option<String>) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(tr!("history.title")).strong());
            ui.add_space(space::S);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.filter_text)
                    .hint_text(tr!("history.filter_hint"))
                    .desired_width(180.0),
            );
            let submitted = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let cleared = edit.changed() && self.filter_text.trim().is_empty() && !self.model.filter.is_empty();
            if submitted || cleared {
                self.model.set_filter(&self.filter_text);
                self.reload();
            }
            if !self.model.filter.is_empty() && ui.small_button("✕").on_hover_text(tr!("history.clear_filter")).clicked() {
                self.filter_text.clear();
                self.model.set_filter("");
                self.reload();
            }
            // Solo tiene sentido con un archivo abierto.
            if let Some(f) = open_file.filter(|f| *f != self.model.filter) {
                if ui.button(tr!("history.only_this_file")).on_hover_text(f.clone()).clicked() {
                    self.filter_text = f.clone();
                    self.model.set_filter(&f);
                    self.reload();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if self.model.has_more() && self.graph_job.is_none() && ui.button(tr!("history.load_more")).clicked() {
                    self.model.load_more();
                    self.reload();
                }
                let status = if let Some(e) = &self.error {
                    RichText::new(e).color(ui.visuals().error_fg_color)
                } else if self.graph_job.is_some() {
                    RichText::new(tr!("history.loading")).weak()
                } else {
                    let mut s = tr!("history.commits", count = self.model.commits.len());
                    if self.summary_job.is_some() {
                        s = format!("{s} · {}", tr!("history.analyzing"));
                    }
                    RichText::new(s).weak()
                };
                ui.label(status);
            });
        });
    }

    fn list(&mut self, ui: &mut egui::Ui, now: f64, dt: f64, reduce_motion: bool) {
        if self.model.commits.is_empty() {
            let msg = if self.graph_job.is_some() { tr!("history.loading") } else { tr!("history.empty") };
            ui.centered_and_justified(|ui| ui.label(RichText::new(msg).weak()));
            return;
        }
        let m = Metrics::default();
        let n = self.model.commits.len();
        ui.spacing_mut().item_spacing.y = 0.0;
        let mut area = egui::ScrollArea::vertical().id_salt("history_rows").auto_shrink([false, false]);
        if std::mem::take(&mut self.scroll_to_selection) {
            if let Some(i) = self.model.selected {
                let (y, vh) = (i as f32 * m.row_h, self.viewport_h.max(m.row_h));
                let mut off = self.last_offset;
                if y < off {
                    off = y;
                } else if y + m.row_h > off + vh {
                    off = y + m.row_h - vh;
                }
                area = area.vertical_scroll_offset(off);
            }
        }
        let out = area.show_rows(ui, m.row_h, n, |ui, range| self.rows(ui, range, &m, now, dt, reduce_motion));
        self.last_offset = out.state.offset.y;
        self.viewport_h = out.inner_rect.height();
    }

    fn rows(&mut self, ui: &mut egui::Ui, range: std::ops::Range<usize>, m: &Metrics, now: f64, dt: f64, reduce_motion: bool) {
        let dark = ui.visuals().dark_mode;
        let painter = ui.painter().clone();
        let highlight = painter.add(egui::Shape::Noop);
        let first = range.start.saturating_sub(1);
        let graph_cols = (first..range.end)
            .filter_map(|i| self.model.commits[i].graph.as_ref())
            .map(geometry::width)
            .max()
            .unwrap_or(1);
        let graph_w = graph_cols as f32 * m.col_w + space::S;
        let x0 = ui.max_rect().left() + space::XS;
        let top0 = ui.cursor().top();
        let now_unix = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs() as i64);
        // Los resúmenes aparecen con un fundido corto, sin mover nada.
        let fade = match (self.summaries_at, reduce_motion) {
            (Some(t), false) => ((now - t) / 0.2).clamp(0.0, 1.0) as f32,
            _ => 1.0,
        };
        let mut hover_lane = None;

        for i in range.clone() {
            let (rect, resp) = ui.allocate_exact_size(egui::vec2(ui.available_width(), m.row_h), Sense::click());
            let v = ui.visuals();
            // Respuesta al apretar, no al soltar.
            if resp.is_pointer_button_down_on() {
                painter.rect_filled(rect, 4.0, v.widgets.active.weak_bg_fill.gamma_multiply(0.6));
            } else if resp.hovered() {
                painter.rect_filled(rect, 4.0, v.widgets.hovered.weak_bg_fill.gamma_multiply(0.5));
            }
            if resp.clicked() {
                self.model.select(i);
            }
            if resp.double_clicked() {
                self.model.select(i);
                self.model.open_first();
            }
            let c = &self.model.commits[i];
            let lane = c.graph.as_ref().map_or(0, |g| g.lane);
            let mut x = x0 + graph_w;
            let mid = rect.center().y;

            // Id corto: monoespaciado, chico, terciario.
            let id = text_galley(&painter, &c.info.short_id, egui::FontId::monospace(11.5), v.weak_text_color(), f32::INFINITY);
            painter.galley(Pos2::new(x, mid - id.size().y / 2.0), id.clone(), v.weak_text_color());
            x += id.size().x + space::S;

            // Chips de refs: HEAD relleno; ramas y tags con el color de su rama.
            for r in &c.refs {
                let color = lane_color(lane, dark);
                let head = r == "HEAD";
                let text_color = if head { v.selection.stroke.color } else { color };
                let g = text_galley(&painter, r, egui::FontId::proportional(11.0), text_color, 160.0);
                let chip = egui::Rect::from_min_size(Pos2::new(x, mid - 8.0), egui::vec2(g.size().x + 2.0 * space::XS + 2.0, 16.0));
                if head {
                    painter.rect_filled(chip, 6.0, v.selection.bg_fill);
                } else {
                    painter.rect_stroke(chip, 6.0, Stroke::new(1.0_f32, color), egui::StrokeKind::Inside);
                }
                painter.galley(Pos2::new(chip.left() + space::XS + 1.0, mid - g.size().y / 2.0), g, text_color);
                if ui.rect_contains_pointer(chip) {
                    hover_lane = Some(lane);
                }
                x = chip.right() + space::XS;
            }

            // A la derecha: resumen, autor, fecha (de menos a más fijo).
            let date_w = 56.0;
            let author_w = 110.0;
            let badge_w = 130.0;
            let right = rect.right() - space::S;
            let date = relative_time(c.info.timestamp, now_unix);
            let dg = text_galley(&painter, &date, egui::FontId::proportional(11.5), v.weak_text_color(), date_w);
            painter.galley(Pos2::new(right - dg.size().x, mid - dg.size().y / 2.0), dg, v.weak_text_color());
            let ag = text_galley(&painter, &c.info.author, egui::FontId::proportional(11.5), v.weak_text_color(), author_w);
            painter.galley(Pos2::new(right - date_w - space::S - ag.size().x, mid - ag.size().y / 2.0), ag, v.weak_text_color());
            let badge_right = right - date_w - author_w - 2.0 * space::S;
            if self.model.summaries_ready && !c.is_merge {
                paint_badge(&painter, badge_right, mid, badge_w, &c.files, dark, fade, v.weak_text_color());
            }

            // El mensaje: lo primero que se lee; ocupa lo que queda.
            let msg_w = (badge_right - badge_w - space::S - x).max(40.0);
            let first_line = c.info.message.lines().next().unwrap_or("");
            let mg = text_galley(&painter, first_line, egui::FontId::proportional(13.0), v.text_color(), msg_w);
            painter.galley(Pos2::new(x, mid - mg.size().y / 2.0), mg, v.text_color());

            let resp = resp.on_hover_text_at_pointer(format!("{}\n{} · {}", c.info.message.trim(), c.info.author, full_date(c.info.timestamp)));
            let _ = resp;
        }
        self.hover_lane = hover_lane;

        // Marca de selección: se desliza desde donde está (interrumpible).
        if let Some(sel) = self.model.selected {
            let target = sel as f64;
            if reduce_motion || (self.sel_y - target).abs() > 30.0 {
                (self.sel_y, self.sel_v) = (target, 0.0);
            } else {
                (self.sel_y, self.sel_v) = spring_step(self.sel_y, self.sel_v, target, dt, 0.25);
                if (self.sel_y - target).abs() > 0.001 {
                    ui.ctx().request_repaint();
                }
            }
            let y = top0 + (self.sel_y as f32 - range.start as f32) * m.row_h;
            let r = egui::Rect::from_min_size(Pos2::new(ui.max_rect().left(), y), egui::vec2(ui.max_rect().width(), m.row_h));
            painter.set(highlight, egui::Shape::rect_filled(r, 4.0, ui.visuals().selection.bg_fill.gamma_multiply(0.35)));
        }

        // El grafo encima de los fondos: primero los tramos, después los nodos.
        let bg = ui.visuals().panel_fill;
        let mut prims = Vec::new();
        for i in first..range.end {
            let Some(g) = &self.model.commits[i].graph else { continue };
            let top = top0 + (i as f32 - range.start as f32) * m.row_h;
            prims.extend(geometry::edges(g, m, x0, top));
        }
        for i in range.clone() {
            let c = &self.model.commits[i];
            let Some(g) = &c.graph else { continue };
            let top = top0 + (i as f32 - range.start as f32) * m.row_h;
            prims.push(geometry::node(g, m, x0, top, c.is_merge));
        }
        for p in prims {
            let mut color = lane_color(p.lane(), dark);
            if self.hover_lane.is_some_and(|l| l != p.lane()) {
                color = color.gamma_multiply(DIM);
            }
            match p {
                Prim::Line { from, to, .. } => {
                    painter.line_segment([Pos2::from(from), Pos2::from(to)], Stroke::new(1.5_f32, color));
                }
                Prim::Curve { points, .. } => {
                    painter.add(egui::epaint::CubicBezierShape::from_points_stroke(
                        points.map(Pos2::from),
                        false,
                        Color32::TRANSPARENT,
                        Stroke::new(1.5_f32, color),
                    ));
                }
                Prim::Node { center, radius, hollow, .. } => {
                    let c = Pos2::from(center);
                    // Un anillo del color del fondo separa el nodo de las líneas.
                    painter.circle_filled(c, radius + 1.5, bg);
                    if hollow {
                        painter.circle(c, radius - 0.5, bg, Stroke::new(1.5_f32, color));
                    } else {
                        painter.circle_filled(c, radius, color);
                    }
                }
            }
        }
    }

    /// El commit elegido: mensaje completo, autor, padres y archivos.
    fn details(&mut self, ui: &mut egui::Ui) {
        let Some(c) = self.model.selected_commit().cloned() else {
            ui.add_space(space::M);
            ui.label(RichText::new(tr!("history.select_hint")).weak());
            return;
        };
        egui::ScrollArea::vertical().id_salt("history_details").auto_shrink([false, false]).show(ui, |ui| {
            ui.add_space(space::XS);
            ui.horizontal_wrapped(|ui| {
                ui.monospace(RichText::new(&c.info.short_id).weak());
                for r in &c.refs {
                    ui.label(RichText::new(r).small().strong());
                }
            });
            ui.add(egui::Label::new(RichText::new(c.info.message.trim()).strong()).wrap());
            ui.label(RichText::new(format!("{} · {}", c.info.author, full_date(c.info.timestamp))).small().weak());
            if c.parents.len() > 1 {
                ui.label(RichText::new(tr!("history.merge_note")).small().weak());
            } else if c.parents.is_empty() {
                ui.label(RichText::new(tr!("history.root_note")).small().weak());
            }
            ui.add_space(space::S);
            match self.model.details.get(&c.info.oid) {
                None => {
                    ui.label(RichText::new(tr!("history.loading")).weak());
                }
                Some(Err(e)) => {
                    ui.label(RichText::new(e).color(ui.visuals().error_fg_color));
                }
                Some(Ok(d)) => {
                    let files = d.files.clone();
                    if files.is_empty() {
                        ui.label(RichText::new(tr!("history.no_files")).weak());
                    }
                    for f in files {
                        self.file_row(ui, &f);
                    }
                }
            }
        });
    }

    fn file_row(&mut self, ui: &mut egui::Ui, f: &model::DetailFile) {
        let dark = ui.visuals().dark_mode;
        let enabled = f.openable();
        let (letter, kind) = match f.status {
            Some(ChangeStatus::Added) => ("A", viewer_core::diff::ChangeKind::Added),
            Some(ChangeStatus::Removed) => ("D", viewer_core::diff::ChangeKind::Removed),
            _ => ("M", viewer_core::diff::ChangeKind::Modified),
        };
        ui.horizontal(|ui| {
            ui.label(RichText::new(letter).monospace().color(theme::change_color(kind, dark)));
            let name = RichText::new(&f.path);
            let resp = ui.add_enabled(enabled, egui::Button::selectable(false, name).truncate());
            let resp = if enabled {
                resp.on_hover_text(tr!("history.open_file_hint"))
            } else {
                resp.on_disabled_hover_text(tr!("history.no_module"))
            };
            if resp.clicked() {
                self.model.open(&f.path);
            }
        });
        if let Some(s) = &f.summary {
            let text = counts_text(s);
            if !text.is_empty() {
                ui.label(RichText::new(text).small().weak());
            }
        }
    }
}

/// Un trabajo en un hilo aparte que, al terminar, pide un cuadro nuevo para
/// que el resultado se vea enseguida.
fn spawn<T: Send + 'static>(
    name: &str,
    ctx: Option<egui::Context>,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Job<T> {
    Promise::spawn_thread(name.to_string(), move || {
        let out = f();
        if let Some(ctx) = ctx {
            ctx.request_repaint();
        }
        out
    })
}

fn take_ready<T: Send + 'static>(job: &mut Option<Job<T>>) -> Option<Result<T, String>> {
    if job.as_ref()?.ready().is_some() {
        job.take().map(Promise::block_and_take)
    } else {
        None
    }
}

/// Texto de una línea recortado con "…" al ancho dado.
fn text_galley(painter: &egui::Painter, text: &str, font: egui::FontId, color: Color32, max_w: f32) -> std::sync::Arc<egui::Galley> {
    let mut job = egui::text::LayoutJob::single_section(text.to_string(), egui::TextFormat { font_id: font, color, ..Default::default() });
    job.wrap = egui::text::TextWrapping { max_width: max_w, max_rows: 1, break_anywhere: true, overflow_character: Some('…') };
    painter.layout_job(job)
}

/// Colores de las ramas: 8 por tema, con contraste contra el fondo del panel.
pub fn lane_color(lane: usize, dark: bool) -> Color32 {
    const DARK: [(u8, u8, u8); 8] = [
        (94, 166, 255),
        (255, 159, 67),
        (72, 207, 140),
        (239, 104, 158),
        (178, 140, 255),
        (242, 201, 76),
        (64, 200, 214),
        (255, 118, 105),
    ];
    const LIGHT: [(u8, u8, u8); 8] = [
        (0, 102, 204),
        (199, 96, 0),
        (22, 128, 72),
        (191, 38, 104),
        (110, 64, 201),
        (150, 110, 0),
        (0, 120, 138),
        (196, 50, 38),
    ];
    let (r, g, b) = if dark { DARK[lane % 8] } else { LIGHT[lane % 8] };
    Color32::from_rgb(r, g, b)
}

/// (añadidos, eliminados, modificados) de los conteos de un archivo.
fn tally(files: &[FileSummary]) -> (i64, i64, i64) {
    let (mut a, mut r, mut m) = (0, 0, 0);
    for f in files {
        for (k, v) in &f.counts {
            if k.ends_with("_added") {
                a += v;
            } else if k.ends_with("_removed") {
                r += v;
            } else {
                m += v;
            }
        }
    }
    (a, r, m)
}

/// Resumen de un commit a la derecha de la fila: formatos ("sch · gds") y
/// +añadidos −eliminados ~modificados en los colores de cambio. El color
/// nunca va solo: los signos dicen lo mismo.
#[allow(clippy::too_many_arguments)]
fn paint_badge(painter: &egui::Painter, right: f32, mid: f32, width: f32, files: &[FileSummary], dark: bool, fade: f32, weak: Color32) {
    if files.is_empty() {
        return;
    }
    use viewer_core::diff::ChangeKind as K;
    let (a, r, m) = tally(files);
    let mut parts: Vec<(String, Color32)> = Vec::new();
    if files.iter().all(|f| f.category == SummaryCategory::Cosmetic) {
        parts.push((tr!("history.cosmetic"), weak));
    } else {
        for (n, sign, kind) in [(a, "+", K::Added), (r, "−", K::Removed), (m, "~", K::Modified)] {
            if n > 0 {
                parts.push((format!("{sign}{n}"), theme::change_color(kind, dark)));
            }
        }
    }
    let mut exts: Vec<String> = files
        .iter()
        .filter_map(|f| Path::new(&f.path).extension().map(|e| e.to_string_lossy().to_lowercase()))
        .collect();
    exts.dedup();
    exts.truncate(3);
    let mut x = right;
    for (text, color) in parts.iter().rev() {
        let g = text_galley(painter, text, egui::FontId::monospace(11.0), color.gamma_multiply(fade), width);
        x -= g.size().x;
        painter.galley(Pos2::new(x, mid - g.size().y / 2.0), g, *color);
        x -= space::XS;
    }
    if !exts.is_empty() {
        let g = text_galley(painter, &exts.join(" · "), egui::FontId::proportional(11.0), weak.gamma_multiply(fade), (x - (right - width)).max(0.0));
        x -= g.size().x + space::XS;
        painter.galley(Pos2::new(x, mid - g.size().y / 2.0), g, weak);
    }
}

/// Conteos de un archivo en palabras, en el idioma del visor.
fn counts_text(s: &FileSummary) -> String {
    if s.category == SummaryCategory::Cosmetic {
        return tr!("history.cosmetic");
    }
    s.counts
        .iter()
        .map(|(k, v)| {
            let key = format!("history.count.{k}");
            let label = tr!(&key);
            // Una clave sin texto corto vuelve tal cual: se usa el de la CLI
            // (cubre los conteos nuevos, como las señales) y, si tampoco
            // hay, la clave.
            let label = if label.contains("history.count.") {
                crate::core::analysis::summary::label_for(k, *v).unwrap_or_else(|| k.clone())
            } else {
                label
            };
            format!("{v} {label}")
        })
        .collect::<Vec<_>>()
        .join(", ")
}

/// "ahora", "5 min", "3 h", "ayer", "4 d"; más de una semana, la fecha.
fn relative_time(ts: i64, now: i64) -> String {
    let d = now - ts;
    if ts <= 0 {
        return "—".into();
    }
    match d {
        ..60 => tr!("time.now"),
        60..3600 => tr!("time.minutes", n = d / 60),
        3600..86_400 => tr!("time.hours", n = d / 3600),
        86_400..172_800 => tr!("time.yesterday"),
        172_800..604_800 => tr!("time.days", n = d / 86_400),
        _ => full_date(ts).split(' ').next().unwrap_or("").to_string(),
    }
}

fn full_date(ts: i64) -> String {
    crate::cli::format::log_text::format_timestamp(ts)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lane_colors_contrast_with_the_panel() {
        let bg_dark = Color32::from_rgb(27, 27, 31);
        let bg_light = Color32::from_rgb(242, 242, 238);
        for l in 0..8 {
            assert!(theme::contrast(lane_color(l, true), bg_dark) >= 3.0, "oscuro {l}");
            assert!(theme::contrast(lane_color(l, false), bg_light) >= 3.0, "claro {l}");
        }
    }

    #[test]
    fn counts_without_a_short_text_use_the_cli_label() {
        // Las señales no tienen texto corto en History: sale el de la CLI
        // (en el idioma que esté activo), no la clave cruda.
        let mut f = FileSummary::unknown("tb.raw");
        f.category = SummaryCategory::Semantic;
        f.counts.insert("signals_added".into(), 3);
        let cli = crate::core::analysis::summary::label_for("signals_added", 3).unwrap();
        assert_eq!(counts_text(&f), format!("3 {cli}"));
        assert!(!counts_text(&f).contains("signals_added"));
    }

    #[test]
    fn tally_groups_counts_by_kind() {
        let mut f = FileSummary::unknown("a.sch");
        f.counts.insert("components_added".into(), 2);
        f.counts.insert("nets_removed".into(), 1);
        f.counts.insert("components_modified".into(), 3);
        f.counts.insert("signals".into(), 1);
        assert_eq!(tally(&[f]), (2, 1, 4));
    }
}
