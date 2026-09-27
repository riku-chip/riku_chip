//! Vista de formas de onda (`.raw` de ngspice): un archivo suelto o la
//! comparación de dos versiones (A antes, B después).
//!
//! No usa `ViewerBackend`/`Scene`, que describen planos (esquemáticos,
//! layouts): una curva necesita ejes con unidades, escala logarítmica en
//! frecuencia y zoom independiente en X e Y, que da `egui_plot`.
//!
//! Mismas vistas que los demás formatos: **Diff** (B continua sobre A
//! punteada, y debajo el error B − A con el eje X enlazado), **Before** (solo
//! A) y **After** (solo B). Las tres encuadran igual y conservan el zoom, así
//! que alternar Before/After sobre un tramo muestra cómo "salta" la curva.

use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui::{self, Color32, RichText};
use egui_plot::{GridMark, Legend, Line, LineStyle, Plot, PlotPoints};

use crate::cli::format::diff_text::eng;
use crate::gui::app::DiffTab;
use crate::gui::theme::space;
use crate::gui::tr;
use crate::modules::spice::compare::{self, interp, PlotDiff, SignalDiff, Status, Tolerance};
use crate::modules::spice::raw::{self, RawFile};

/// Puntos por curva que se mandan a dibujar: con más, egui_plot se vuelve
/// lento y en pantalla no se distinguen (se conserva el mín/máx por tramo).
const MAX_POINTS: usize = 4000;

/// Señales que se muestran al abrir.
const INITIAL_SIGNALS: usize = 3;

pub struct WaveView {
    /// B (o el archivo suelto).
    pub after: Arc<RawFile>,
    /// A, si se compara.
    pub before: Option<Arc<RawFile>>,
    /// Nombres de A y B para la leyenda (`abc1234`, `sim_v1.raw`).
    pub label_a: String,
    pub label_b: String,
    /// Archivo abierto (modo suelto), para "comparar con" y recargar.
    pub path: Option<PathBuf>,
    /// Análisis emparejados (índice en A, índice en B).
    pairs: Vec<(String, Option<usize>, Option<usize>)>,
    /// Comparación por análisis (mismo orden que `pairs`), si hay A.
    diffs: Vec<PlotDiff>,
    plot: usize,
    /// Señales visibles (en minúsculas), por análisis.
    selected: HashMap<usize, BTreeSet<String>>,
    filter: String,
    only_changed: bool,
    hide_internal: bool,
    show_error: bool,
    /// Curvas ya reducidas para dibujar: (lado B?, análisis, señal).
    cache: HashMap<(bool, usize, String), Arc<Vec<[f64; 2]>>>,
    /// Pedido de la vista: otro `.raw` para comparar, o quitar la comparación.
    pub request: Option<Request>,
    /// Vista del diff (Diff / Before / After). Sin comparación no se usa.
    pub tab: DiffTab,
    /// Encuadrar todo en el próximo cuadro. egui guarda el zoom de cada
    /// gráfico entre sesiones; sin esto, un archivo nuevo heredaría el zoom
    /// del anterior con el mismo análisis.
    reset_bounds: bool,
}

pub enum Request {
    CompareWith(PathBuf),
    StopComparing,
}

impl WaveView {
    /// Un archivo suelto.
    pub fn single(file: RawFile, path: PathBuf) -> Self {
        let label = file_label(&path);
        Self::build(None, file, String::new(), label, Some(path))
    }

    /// Dos versiones: `before` (A) y `after` (B).
    pub fn compare(before: RawFile, after: RawFile, label_a: String, label_b: String, path: Option<PathBuf>) -> Self {
        Self::build(Some(before), after, label_a, label_b, path)
    }

    fn build(before: Option<RawFile>, after: RawFile, label_a: String, label_b: String, path: Option<PathBuf>) -> Self {
        let empty = RawFile { plots: Vec::new() };
        let pairs: Vec<_> = compare::pair_plots(before.as_ref().unwrap_or(&empty), &after)
            .into_iter()
            .map(|(a, b)| {
                let name = b.map(|i| after.plots[i].name.clone())
                    .or_else(|| a.and_then(|i| before.as_ref().map(|f| f.plots[i].name.clone())))
                    .unwrap_or_default();
                (name, a, b)
            })
            .collect();
        let diffs = match &before {
            Some(a) => pairs
                .iter()
                .map(|(_, ia, ib)| {
                    compare::compare_plot(ia.map(|i| &a.plots[i]), ib.map(|i| &after.plots[i]), Tolerance::default())
                })
                .collect(),
            None => Vec::new(),
        };
        // Primer análisis con curvas (el punto de operación tiene un solo punto).
        let plot = pairs
            .iter()
            .position(|(_, _, b)| b.is_some_and(|i| after.plots[i].points() > 1))
            .unwrap_or(0);
        let mut view = Self {
            after: Arc::new(after),
            before: before.map(Arc::new),
            label_a,
            label_b,
            path,
            pairs,
            diffs,
            plot,
            selected: HashMap::new(),
            filter: String::new(),
            only_changed: false,
            hide_internal: true,
            show_error: false,
            cache: HashMap::new(),
            request: None,
            tab: DiffTab::Diff,
            reset_bounds: true,
        };
        view.only_changed = view.is_diff() && view.changed_count(view.plot) > 0;
        view.show_error = view.only_changed;
        view
    }

    pub fn is_diff(&self) -> bool {
        self.before.is_some()
    }

    /// Resumen para la barra de estado.
    pub fn summary(&self) -> String {
        if !self.is_diff() {
            let n: usize = self.after.plots.iter().map(|p| p.signals().len()).sum();
            return tr!("wave.summary_single", plots = self.after.plots.len(), signals = n);
        }
        let changed: usize = (0..self.pairs.len()).map(|i| self.changed_count(i)).sum();
        match changed {
            0 => tr!("wave.summary_equal"),
            n => tr!("wave.summary_changed", count = n),
        }
    }

    fn plot_of(&self, side_b: bool, idx: usize) -> Option<&raw::Plot> {
        let (_, a, b) = self.pairs.get(idx)?;
        if side_b {
            b.map(|i| &self.after.plots[i])
        } else {
            a.and_then(|i| self.before.as_ref().map(|f| &f.plots[i]))
        }
    }

    fn diff_of(&self, idx: usize, name: &str) -> Option<&SignalDiff> {
        self.diffs.get(idx)?.signals.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    fn changed(d: &SignalDiff) -> bool {
        match d.status {
            Status::Compared => !d.within_tolerance,
            Status::Added | Status::Removed | Status::Incomparable => true,
        }
    }

    fn changed_count(&self, idx: usize) -> usize {
        self.diffs.get(idx).map_or(0, |d| d.signals.iter().filter(|s| Self::changed(s)).count())
    }

    /// Nombres de señales del análisis (unión de A y B), en orden de B.
    fn signal_names(&self, idx: usize) -> Vec<String> {
        let mut names: Vec<String> = Vec::new();
        for side in [true, false] {
            if let Some(p) = self.plot_of(side, idx) {
                for s in p.signals() {
                    if !names.iter().any(|n| n.eq_ignore_ascii_case(&s.name)) {
                        names.push(s.name.clone());
                    }
                }
            }
        }
        names
    }

    /// Selección del análisis; la primera vez, las que más cambiaron (o las
    /// primeras señales visibles si no hay comparación).
    fn selection(&mut self, idx: usize) -> &mut BTreeSet<String> {
        if !self.selected.contains_key(&idx) {
            let mut names = self.signal_names(idx);
            names.retain(|n| !is_internal(n));
            if self.is_diff() {
                let key = |n: &String| self.diff_of(idx, n).map_or(0.0, |d| if Self::changed(d) { d.rel().max(1e-9) } else { 0.0 });
                names.sort_by(|a, b| key(b).total_cmp(&key(a)));
            }
            if self.is_diff() && self.changed_count(idx) > 0 {
                names.retain(|n| self.diff_of(idx, n).is_some_and(Self::changed));
            }
            let initial = names.into_iter().take(INITIAL_SIGNALS).map(|n| n.to_lowercase()).collect();
            self.selected.insert(idx, initial);
        }
        self.selected.get_mut(&idx).expect("recién insertada")
    }

    /// Curva lista para dibujar (con la X en log10 si el análisis es en frecuencia).
    fn curve(&mut self, side_b: bool, idx: usize, name: &str) -> Option<Arc<Vec<[f64; 2]>>> {
        let key = (side_b, idx, name.to_lowercase());
        if let Some(c) = self.cache.get(&key) {
            return Some(c.clone());
        }
        let p = self.plot_of(side_b, idx)?;
        let (x, y) = (p.x()?, p.signal(name)?);
        let log = p.log_x();
        let xs: Vec<f64> = x.values.iter().map(|&v| if log { v.max(1e-300).log10() } else { v }).collect();
        let pts = Arc::new(decimate(&xs, &y.values, MAX_POINTS));
        self.cache.insert(key, pts.clone());
        Some(pts)
    }

    /// B − A sobre la unión de las dos grillas.
    fn error_curve(&mut self, idx: usize, name: &str) -> Option<Arc<Vec<[f64; 2]>>> {
        let key = (false, usize::MAX - idx, name.to_lowercase());
        if let Some(c) = self.cache.get(&key) {
            return Some(c.clone());
        }
        let (pa, pb) = (self.plot_of(false, idx)?, self.plot_of(true, idx)?);
        let (xa, xb) = (&pa.x()?.values, &pb.x()?.values);
        let (ya, yb) = (&pa.signal(name)?.values, &pb.signal(name)?.values);
        if xa.len() < 2 || xb.len() < 2 {
            return None;
        }
        let (lo, hi) = (xa[0].max(xb[0]), xa[xa.len() - 1].min(xb[xb.len() - 1]));
        let mut grid: Vec<f64> = xa.iter().chain(xb).copied().filter(|&x| x >= lo && x <= hi).collect();
        grid.sort_by(f64::total_cmp);
        grid.dedup();
        let log = pb.log_x();
        let err: Vec<f64> = grid.iter().map(|&x| interp(xb, yb, x) - interp(xa, ya, x)).collect();
        let xs: Vec<f64> = grid.iter().map(|&v| if log { v.max(1e-300).log10() } else { v }).collect();
        let pts = Arc::new(decimate(&xs, &err, MAX_POINTS));
        self.cache.insert(key, pts.clone());
        Some(pts)
    }
}

/// Nodos internos de dispositivos y subcircuitos (`v(m.xm1.m…#body)`,
/// `v(x1.net3)`, `@m1[id]`): ruido para una primera mirada.
fn is_internal(name: &str) -> bool {
    name.contains('#') || name.contains('.') || name.starts_with('@')
}

fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(|| path.display().to_string(), |n| n.to_string_lossy().to_string())
}

/// Reduce una curva a `max` puntos conservando el mínimo y el máximo de
/// cada tramo (un pico angosto no desaparece al alejarse).
pub fn decimate(xs: &[f64], ys: &[f64], max: usize) -> Vec<[f64; 2]> {
    let n = xs.len().min(ys.len());
    if n <= max {
        return (0..n).map(|i| [xs[i], ys[i]]).collect();
    }
    let buckets = (max / 2).max(1);
    let per = n.div_ceil(buckets);
    let mut out = Vec::with_capacity(buckets * 2);
    for start in (0..n).step_by(per) {
        let end = (start + per).min(n);
        let (mut lo, mut hi) = (start, start);
        for i in start..end {
            if ys[i] < ys[lo] {
                lo = i;
            }
            if ys[i] > ys[hi] {
                hi = i;
            }
        }
        let (first, second) = if lo <= hi { (lo, hi) } else { (hi, lo) };
        out.push([xs[first], ys[first]]);
        if second != first {
            out.push([xs[second], ys[second]]);
        }
    }
    out
}

/// Colores de las señales (distinguibles en tema claro y oscuro).
const PALETTE: [Color32; 8] = [
    Color32::from_rgb(0x4e, 0x9c, 0xf5),
    Color32::from_rgb(0xf5, 0x8f, 0x3b),
    Color32::from_rgb(0x3c, 0xc4, 0x7c),
    Color32::from_rgb(0xe0, 0x4f, 0x6a),
    Color32::from_rgb(0xa7, 0x7b, 0xf0),
    Color32::from_rgb(0xd4, 0xb8, 0x3a),
    Color32::from_rgb(0x2f, 0xc2, 0xc9),
    Color32::from_rgb(0xe0, 0x7b, 0xc8),
];

fn color_for(i: usize) -> Color32 {
    PALETTE[i % PALETTE.len()]
}

// ─── Lienzo ──────────────────────────────────────────────────────────────────

pub fn show_plot(ui: &mut egui::Ui, view: &mut WaveView) {
    let idx = view.plot;
    let Some(pb) = view.plot_of(true, idx).or_else(|| view.plot_of(false, idx)) else {
        ui.centered_and_justified(|ui| ui.label(tr!("wave.no_analyses")));
        return;
    };
    let complex = pb.complex;
    let log_x = pb.log_x();
    let x_unit = pb.x().map_or("", |x| x.unit(complex)).to_string();
    let x_name = pb.x().map_or(String::new(), |x| x.name.clone());
    let single_point = pb.points() <= 1;

    if single_point {
        show_operating_point(ui, view);
        return;
    }

    let names = view.signal_names(idx);
    let selected = view.selection(idx).clone();
    let shown: Vec<(usize, String)> =
        names.iter().enumerate().filter(|(_, n)| selected.contains(&n.to_lowercase())).map(|(i, n)| (i, n.clone())).collect();
    if shown.is_empty() {
        ui.centered_and_justified(|ui| ui.label(RichText::new(tr!("wave.pick_signals")).weak()));
        return;
    }
    // Un gráfico por unidad (V, A, dB…): mezclar voltios con microamperios
    // en el mismo eje aplana la curva chica.
    let unit_of = |view: &WaveView, n: &str| {
        view.plot_of(true, idx)
            .and_then(|p| p.signal(n))
            .or_else(|| view.plot_of(false, idx).and_then(|p| p.signal(n)))
            .map_or(String::new(), |s| s.unit(complex).to_string())
    };
    let mut groups: Vec<(String, Vec<(usize, String)>)> = Vec::new();
    for (i, n) in shown {
        let u = unit_of(view, &n);
        match groups.iter_mut().find(|(gu, _)| *gu == u) {
            Some((_, v)) => v.push((i, n)),
            None => groups.push((u, vec![(i, n)])),
        }
    }

    let diff = view.is_diff();
    let tab = if diff { view.tab } else { DiffTab::After };
    let show_error = diff && tab == DiffTab::Diff && view.show_error;
    let reset = std::mem::take(&mut view.reset_bounds);
    let rows = groups.len() * if show_error { 2 } else { 1 };
    let gap = space::XS;
    let row_h = ((ui.available_height() - gap * (rows as f32 - 1.0)) / rows as f32).max(80.0);
    let link = egui::Id::new(("riku_wave_x", idx));
    let x_label = format!("{x_name} [{x_unit}]");

    let mut row = 0;
    for (unit, sigs) in &groups {
        let mut lines: Vec<Line<'static>> = Vec::new();
        // Extensión de A y B juntas: las tres vistas encuadran igual.
        let mut extent = Extent::default();
        for (i, name) in sigs {
            let color = color_for(*i);
            let (a, b) = (if diff { view.curve(false, idx, name) } else { None }, view.curve(true, idx, name));
            for c in [&a, &b].into_iter().flatten() {
                extent.add(c);
            }
            if let Some(b) = b.filter(|_| tab != DiffTab::Before) {
                let label = if diff { format!("{name} · {}", view.label_b) } else { name.clone() };
                lines.push(Line::new(label, PlotPoints::new(b.to_vec())).color(color).width(1.6_f32));
            }
            if let Some(a) = a.filter(|_| tab != DiffTab::After) {
                let line = Line::new(format!("{name} · {}", view.label_a), PlotPoints::new(a.to_vec()));
                // En Diff, A va detrás de B (punteada); en Before es la única.
                lines.push(match tab {
                    DiffTab::Diff => line.color(color.gamma_multiply(0.75)).style(LineStyle::dashed_loose()).width(1.2_f32),
                    _ => line.color(color).width(1.6_f32),
                });
            }
        }
        if row > 0 {
            ui.add_space(gap);
        }
        wave_plot(ui, ("riku_wave", idx, unit.as_str()), row_h, link, reset, extent, log_x, &x_unit, &x_label, unit, unit, lines);
        row += 1;

        if show_error {
            let mut err: Vec<Line<'static>> = Vec::new();
            for (i, name) in sigs {
                if let Some(e) = view.error_curve(idx, name) {
                    err.push(Line::new(format!("Δ {name}"), PlotPoints::new(e.to_vec())).color(color_for(*i)).width(1.4_f32));
                }
            }
            ui.add_space(gap);
            let y_label = format!("B − A [{unit}]");
            wave_plot(ui, ("riku_wave_err", idx, unit.as_str()), row_h, link, reset, Extent::default(), log_x, &x_unit, &x_label, &y_label, unit, err);
            row += 1;
        }
    }
}

/// Rectángulo que un gráfico debe incluir al encuadrarse.
#[derive(Clone, Copy)]
struct Extent {
    x: (f64, f64),
    y: (f64, f64),
}

impl Default for Extent {
    fn default() -> Self {
        Self { x: (f64::INFINITY, f64::NEG_INFINITY), y: (f64::INFINITY, f64::NEG_INFINITY) }
    }
}

impl Extent {
    fn add(&mut self, pts: &[[f64; 2]]) {
        for p in pts {
            self.x = (self.x.0.min(p[0]), self.x.1.max(p[0]));
            self.y = (self.y.0.min(p[1]), self.y.1.max(p[1]));
        }
    }

    fn is_valid(&self) -> bool {
        self.x.0 <= self.x.1 && self.y.0 <= self.y.1
    }
}

/// Un gráfico con el eje X enlazado a los demás del mismo análisis.
#[allow(clippy::too_many_arguments)]
fn wave_plot(
    ui: &mut egui::Ui,
    id: impl std::hash::Hash,
    height: f32,
    link: egui::Id,
    reset: bool,
    extent: Extent,
    log_x: bool,
    x_unit: &str,
    x_label: &str,
    y_label: &str,
    y_unit: &str,
    lines: Vec<Line<'static>>,
) {
    let (xu, yu) = (x_unit.to_string(), y_unit.to_string());
    let to_x = move |v: f64| if log_x { 10f64.powf(v) } else { v };
    let plot = Plot::new(id);
    let plot = if reset { plot.reset() } else { plot };
    let plot = if extent.is_valid() {
        plot.include_x(extent.x.0).include_x(extent.x.1).include_y(extent.y.0).include_y(extent.y.1)
    } else {
        plot
    };
    plot.height(height)
        .legend(Legend::default())
        .x_axis_label(x_label.to_string())
        .y_axis_label(y_label.to_string())
        .x_axis_formatter({
            let u = xu.clone();
            move |m: GridMark, _: &std::ops::RangeInclusive<f64>| tick(to_x(m.value), &u)
        })
        .y_axis_formatter({
            let u = yu.clone();
            move |m: GridMark, _: &std::ops::RangeInclusive<f64>| tick(m.value, &u)
        })
        .label_formatter(move |name: &str, p: &egui_plot::PlotPoint| {
            let head = if name.is_empty() { String::new() } else { format!("{name}
") };
            format!("{head}{}
{}", eng(to_x(p.x), &xu), eng(p.y, &yu))
        })
        .link_axis(link, [true, false])
        .link_cursor(link, [true, false])
        .show(ui, |plot_ui| {
            for l in lines {
                plot_ui.line(l);
            }
        });
}

/// Marca de eje corta: `1.500 µs` → `1.5 µs`, `0.000 V` → `0`.
fn tick(v: f64, unit: &str) -> String {
    if v.abs() < 1e-21 {
        return "0".into();
    }
    let full = eng(v, unit);
    let Some((num, rest)) = full.split_once(' ') else { return full };
    let num = if num.contains('.') { num.trim_end_matches('0').trim_end_matches('.') } else { num };
    format!("{num} {rest}")
}

/// Punto de operación: una tabla, no un gráfico.
fn show_operating_point(ui: &mut egui::Ui, view: &mut WaveView) {
    let idx = view.plot;
    let names = view.signal_names(idx);
    let complex = view.plot_of(true, idx).is_some_and(|p| p.complex);
    let value = |side: bool, n: &str| {
        view.plot_of(side, idx).and_then(|p| p.signal(n)).and_then(|s| s.values.first().map(|v| eng(*v, s.unit(complex))))
    };
    let tab = if view.is_diff() { view.tab } else { DiffTab::After };
    let (col_a, col_b, col_d) = (tab != DiffTab::After, tab != DiffTab::Before, tab == DiffTab::Diff);
    egui::ScrollArea::vertical().show(ui, |ui| {
        let columns = 1 + [col_a, col_b, col_d].iter().filter(|c| **c).count();
        egui::Grid::new("riku_op").striped(true).num_columns(columns).show(ui, |ui| {
            ui.label(RichText::new(tr!("wave.signal")).strong());
            if col_a {
                ui.label(RichText::new(&view.label_a).strong());
            }
            if col_b {
                let head = if view.is_diff() { view.label_b.clone() } else { tr!("wave.value") };
                ui.label(RichText::new(head).strong());
            }
            if col_d {
                ui.label(RichText::new("Δ").strong());
            }
            ui.end_row();
            for n in names.iter().filter(|n| !view.hide_internal || !is_internal(n)) {
                let d = view.diff_of(idx, n);
                let changed = d.is_some_and(WaveView::changed);
                let t = RichText::new(n);
                ui.label(if changed { t.strong() } else { t });
                if col_a {
                    ui.label(value(false, n).unwrap_or_else(|| "—".into()));
                }
                if col_b {
                    ui.label(value(true, n).unwrap_or_else(|| "—".into()));
                }
                if let Some(d) = d.filter(|_| col_d) {
                    let text = match d.status {
                        Status::Compared => eng(d.max_abs, d.unit),
                        Status::Added => tr!("wave.new"),
                        Status::Removed => tr!("wave.removed"),
                        Status::Incomparable => "—".into(),
                    };
                    ui.label(if changed { RichText::new(text).color(ui.visuals().warn_fg_color) } else { RichText::new(text).weak() });
                }
                ui.end_row();
            }
        });
    });
}

// ─── Panel de detalles ───────────────────────────────────────────────────────

/// Panel derecho: análisis, comparación y lista de señales. `candidates` son
/// otros `.raw` del proyecto para "comparar con".
pub fn show_details(ui: &mut egui::Ui, view: &mut WaveView, candidates: &[PathBuf]) {
    if view.is_diff() {
        ui.label(RichText::new(format!("A  {}", view.label_a)).small());
        ui.label(RichText::new(format!("B  {}", view.label_b)).small());
        if view.tab == DiffTab::Diff {
            ui.label(RichText::new(tr!("wave.legend")).small().weak());
        }
    }
    if let Some(cmd) = view.after.plots.first().and_then(|p| p.command.as_deref()) {
        ui.label(RichText::new(cmd).small().weak()).on_hover_text(tr!("wave.simulator_hint"));
    }
    ui.add_space(space::XS);

    // Análisis.
    let current = view.pairs.get(view.plot).map(|p| p.0.clone()).unwrap_or_default();
    egui::ComboBox::from_label(tr!("wave.analysis")).selected_text(&current).show_ui(ui, |ui| {
        for i in 0..view.pairs.len() {
            let mut label = view.pairs[i].0.clone();
            let n = view.changed_count(i);
            if n > 0 {
                label = format!("{label}  ({n} Δ)");
            }
            if ui.selectable_value(&mut view.plot, i, label).changed() {
                view.reset_bounds = true;
            }
        }
    });

    // Comparar con otro archivo (solo archivo suelto).
    if let Some(path) = view.path.clone() {
        let others: Vec<&PathBuf> = candidates.iter().filter(|c| **c != path).collect();
        let label = if view.is_diff() { view.label_a.clone() } else { "—".into() };
        egui::ComboBox::from_label(tr!("wave.compare_with")).selected_text(label).show_ui(ui, |ui| {
            if view.is_diff() && ui.selectable_label(false, tr!("wave.none")).clicked() {
                view.request = Some(Request::StopComparing);
            }
            for c in others {
                if ui.selectable_label(false, file_label(c)).on_hover_text(c.display().to_string()).clicked() {
                    view.request = Some(Request::CompareWith(c.clone()));
                }
            }
        });
    }
    ui.add_space(space::XS);

    let idx = view.plot;
    if view.is_diff() {
        let n = view.changed_count(idx);
        let total = view.signal_names(idx).len();
        let text = if n == 0 { tr!("wave.count_equal", total = total) } else { tr!("wave.count_changed", count = n, total = total) };
        ui.label(RichText::new(text).strong());
        ui.checkbox(&mut view.only_changed, tr!("wave.only_changed"));
        if view.tab == DiffTab::Diff {
            ui.checkbox(&mut view.show_error, tr!("wave.show_error"));
        }
    }
    ui.checkbox(&mut view.hide_internal, tr!("wave.hide_internal"))
        .on_hover_text(tr!("wave.hide_internal_hint"));
    ui.add(egui::TextEdit::singleline(&mut view.filter).hint_text(tr!("wave.filter")));

    let mut names = view.signal_names(idx);
    let filter = view.filter.to_lowercase();
    names.retain(|n| {
        (filter.is_empty() || n.to_lowercase().contains(&filter))
            && (!view.hide_internal || !is_internal(n))
            && (!view.only_changed || view.diff_of(idx, n).is_some_and(WaveView::changed))
    });
    if view.is_diff() {
        let key = |n: &String| view.diff_of(idx, n).map_or(0.0, |d| if WaveView::changed(d) { d.rel().max(1e-9) } else { 0.0 });
        names.sort_by(|a, b| key(b).total_cmp(&key(a)));
    }
    let all_names = view.signal_names(idx);

    ui.horizontal(|ui| {
        if ui.small_button(tr!("wave.select_none")).clicked() {
            view.selection(idx).clear();
        }
        if ui.small_button(tr!("wave.select_listed")).on_hover_text(tr!("wave.select_listed_hint")).clicked() {
            let sel = view.selection(idx);
            sel.extend(names.iter().map(|n| n.to_lowercase()));
        }
    });
    ui.separator();

    egui::ScrollArea::vertical().id_salt("wave_signals").auto_shrink([false, false]).show(ui, |ui| {
        if names.is_empty() {
            ui.label(RichText::new(tr!("wave.no_match")).weak());
        }
        for n in &names {
            let key = n.to_lowercase();
            let color = color_for(all_names.iter().position(|m| m == n).unwrap_or(0));
            let mut on = view.selection(idx).contains(&key);
            let diff = view.diff_of(idx, n).cloned();
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, if on { color } else { color.gamma_multiply(0.25) });
                if ui.checkbox(&mut on, n).changed() {
                    let sel = view.selection(idx);
                    if on {
                        sel.insert(key.clone());
                    } else {
                        sel.remove(&key);
                    }
                }
                if let Some(d) = &diff {
                    let (text, hover) = match d.status {
                        Status::Compared => (
                            format!("{:.2} %", d.rel() * 100.0),
                            tr!("wave.diff_hint", max = eng(d.max_abs, d.unit), at = eng(d.at_x, d.x_unit), rms = eng(d.rms, d.unit)),
                        ),
                        Status::Added => (tr!("wave.new"), tr!("wave.only_in_b")),
                        Status::Removed => (tr!("wave.removed"), tr!("wave.only_in_a")),
                        Status::Incomparable => ("?".into(), tr!("wave.no_common_axis")),
                    };
                    let rt = RichText::new(text).small();
                    let rt = if WaveView::changed(d) { rt.color(ui.visuals().warn_fg_color) } else { rt.weak() };
                    ui.label(rt).on_hover_text(hover);
                }
            });
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimate_keeps_peaks() {
        let xs: Vec<f64> = (0..10_000).map(|i| i as f64).collect();
        let mut ys = vec![0.0; 10_000];
        ys[5_123] = 7.0;
        let d = decimate(&xs, &ys, 100);
        assert!(d.len() <= 100);
        assert!(d.iter().any(|p| p[1] == 7.0 && p[0] == 5_123.0));
    }

    #[test]
    fn ticks_are_short() {
        assert_eq!(tick(1.5e-6, "s"), "1.5 µs");
        assert_eq!(tick(1.8, "V"), "1.8 V");
        assert_eq!(tick(2e-6, "s"), "2 µs");
        assert_eq!(tick(1e-30, "V"), "0");
        assert_eq!(tick(-3.0, "dB"), "-3 dB");
    }

    #[test]
    fn internal_nodes() {
        assert!(is_internal("v(m.xm1.msky130_fd_pr__nfet_01v8_lvt#body)"));
        assert!(is_internal("v(x1.net3)"));
        assert!(!is_internal("v(vout)") && !is_internal("i(v1)"));
    }
}
