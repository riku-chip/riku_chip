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
use crate::modules::spice::derived::{self, Derived};
use crate::modules::spice::expr::{self, Evaluated};
use crate::modules::spice::raw::{self, RawFile, Variable};

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
    /// Por análisis: nombres del archivo e índices por nombre, armados una
    /// vez. Con miles de señales (post-layout) buscar recorriendo la lista
    /// hacía cada cuadro O(n²): 3,8 s con 10 000.
    lookup: Vec<PairLookup>,
    plot: usize,
    /// Señales visibles (en minúsculas), por análisis.
    selected: HashMap<usize, BTreeSet<String>>,
    filter: String,
    only_changed: bool,
    hide_internal: bool,
    show_error: bool,
    /// Curvas ya reducidas para dibujar: (lado B?, análisis, señal).
    cache: HashMap<(bool, usize, String), Arc<Curve>>,
    /// Pedido de la vista: otro `.raw` para comparar, o quitar la comparación.
    pub request: Option<Request>,
    /// Vista del diff (Diff / Before / After). Sin comparación no se usa.
    pub tab: DiffTab,
    /// Encuadrar todo en el próximo cuadro. egui guarda el zoom de cada
    /// gráfico entre sesiones; sin esto, un archivo nuevo heredaría el zoom
    /// del anterior con el mismo análisis.
    reset_bounds: bool,
    /// Expresiones del usuario (`gain = v(out)/v(in)`) y su resultado en cada
    /// análisis: las señales se suman a la lista; los escalares, a Mediciones.
    expr_texts: Vec<String>,
    derived: Vec<Derived>,
    expr_warnings: Vec<String>,
    expr_input: String,
    expr_error: Option<String>,
    exprs_changed: bool,
}

pub enum Request {
    CompareWith(PathBuf),
    StopComparing,
}

impl WaveView {
    /// Un archivo suelto.
    pub fn single(file: RawFile, path: PathBuf, exprs: &[String]) -> Self {
        let label = file_label(&path);
        Self::build(None, file, String::new(), label, Some(path), exprs)
    }

    /// Dos versiones: `before` (A) y `after` (B).
    pub fn compare(
        before: RawFile,
        after: RawFile,
        label_a: String,
        label_b: String,
        path: Option<PathBuf>,
        exprs: &[String],
    ) -> Self {
        Self::build(Some(before), after, label_a, label_b, path, exprs)
    }

    fn build(
        before: Option<RawFile>,
        after: RawFile,
        label_a: String,
        label_b: String,
        path: Option<PathBuf>,
        exprs: &[String],
    ) -> Self {
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
        let lookup = (0..pairs.len())
            .map(|i| {
                let side = |p: Option<&raw::Plot>| p.map(|p| p.signals().iter().map(|s| s.name.clone()).collect::<Vec<_>>());
                let (_, ia, ib) = &pairs[i];
                let b = side(ib.map(|j| &after.plots[j]));
                let a = side(ia.and_then(|j| before.as_ref().map(|f| &f.plots[j])));
                PairLookup::new(b.into_iter().chain(a).flatten(), diffs.get(i))
            })
            .collect();
        let mut view = Self {
            lookup,
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
            expr_texts: Vec::new(),
            derived: Vec::new(),
            expr_warnings: Vec::new(),
            expr_input: String::new(),
            expr_error: None,
            exprs_changed: false,
        };
        view.set_expressions(exprs.to_vec());
        view.exprs_changed = false;
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
        let file = self
            .lookup
            .get(idx)
            .and_then(|l| l.diff.get(&name.to_ascii_lowercase()))
            .and_then(|&i| self.diffs.get(idx)?.signals.get(i));
        file.or_else(|| {
            let d = self.derived.iter().find(|d| d.pair == idx && d.diff.name.eq_ignore_ascii_case(name))?;
            self.is_diff().then_some(&d.diff)
        })
    }

    /// Señal por nombre en un lado: del archivo o calculada.
    fn var(&self, side_b: bool, idx: usize, name: &str) -> Option<&Variable> {
        if let Some(v) = self.plot_of(side_b, idx).and_then(|p| p.signal(name)) {
            return Some(v);
        }
        self.derived.iter().filter(|d| d.pair == idx).find_map(|d| match if side_b { &d.b } else { &d.a } {
            Some(Evaluated::Signal(v)) if v.name.eq_ignore_ascii_case(name) => Some(v),
            _ => None,
        })
    }

    /// `true` si la señal es una expresión del usuario.
    fn is_derived(&self, idx: usize, name: &str) -> bool {
        self.derived.iter().any(|d| d.pair == idx && d.diff.name.eq_ignore_ascii_case(name))
    }

    /// Resultados escalares del análisis (Mediciones).
    fn measures(&self, idx: usize) -> Vec<&Derived> {
        self.derived.iter().filter(|d| d.pair == idx && d.diff.scalar.is_some()).collect()
    }

    pub fn expr_texts(&self) -> &[String] {
        &self.expr_texts
    }

    /// `true` una vez después de que el usuario agregó o quitó una expresión.
    pub fn take_exprs_changed(&mut self) -> bool {
        std::mem::take(&mut self.exprs_changed)
    }

    /// Reemplaza las expresiones y las evalúa en A y B.
    fn set_expressions(&mut self, texts: Vec<String>) {
        let mut parsed = Vec::new();
        self.expr_warnings.clear();
        for t in &texts {
            match expr::parse(t) {
                Ok(e) => parsed.push(e),
                Err(e) => self.expr_warnings.push(format!("{t}: {e}")),
            }
        }
        let empty = RawFile { plots: Vec::new() };
        let before = self.before.as_deref().unwrap_or(&empty);
        let (derived, warnings) = derived::evaluate(&parsed, before, &self.after, Tolerance::default());
        self.derived = derived;
        self.expr_warnings.extend(warnings);
        self.expr_texts = texts;
        self.cache.clear();
        self.exprs_changed = true;
    }

    /// Agrega una expresión escrita por el usuario y muestra sus señales.
    fn add_expression(&mut self) {
        let text = self.expr_input.trim().to_string();
        if text.is_empty() {
            return;
        }
        let parsed = match expr::parse(&text) {
            Ok(e) => e,
            Err(e) => {
                self.expr_error = Some(e.to_string());
                return;
            }
        };
        self.expr_error = None;
        self.expr_input.clear();
        let mut texts = self.expr_texts.clone();
        texts.retain(|t| t != &text);
        texts.push(text);
        self.set_expressions(texts);
        let shown: Vec<(usize, String)> = self
            .derived
            .iter()
            .filter(|d| d.diff.name == parsed.name && d.diff.scalar.is_none())
            .map(|d| (d.pair, d.diff.name.to_lowercase()))
            .collect();
        for (pair, name) in shown {
            self.selection(pair).insert(name);
        }
    }

    fn remove_expression(&mut self, i: usize) {
        let mut texts = self.expr_texts.clone();
        if i < texts.len() {
            let removed = texts.remove(i);
            if let Ok(e) = expr::parse(&removed) {
                let name = e.name.to_lowercase();
                for sel in self.selected.values_mut() {
                    sel.remove(&name);
                }
            }
            self.set_expressions(texts);
        }
    }

    fn changed(d: &SignalDiff) -> bool {
        match d.status {
            Status::Compared => !d.within_tolerance,
            Status::Added | Status::Removed | Status::Incomparable => true,
        }
    }

    fn changed_count(&self, idx: usize) -> usize {
        let file = self.diffs.get(idx).map_or(0, |d| d.signals.iter().filter(|s| Self::changed(s)).count());
        let derived = if self.is_diff() {
            // Solo señales: las mediciones (escalares) muestran su Δ aparte.
            self.derived.iter().filter(|d| d.pair == idx && d.diff.scalar.is_none() && Self::changed(&d.diff)).count()
        } else {
            0
        };
        file + derived
    }

    /// Nombres de señales del análisis (unión de A y B), en orden de B.
    fn signal_names(&self, idx: usize) -> Vec<String> {
        let Some(l) = self.lookup.get(idx) else { return Vec::new() };
        let mut names = l.names.clone();
        // Las expresiones que dan una señal, al final (son pocas).
        for d in self.derived.iter().filter(|d| d.pair == idx && d.diff.scalar.is_none()) {
            let lower = d.diff.name.to_ascii_lowercase();
            if !l.position.contains_key(&lower) && !names[l.names.len()..].iter().any(|n| n.eq_ignore_ascii_case(&lower)) {
                names.push(d.diff.name.clone());
            }
        }
        names
    }

    /// Selección del análisis; la primera vez, las que más cambiaron (o las
    /// primeras señales visibles si no hay comparación).
    fn selection(&mut self, idx: usize) -> &mut BTreeSet<String> {
        if !self.selected.contains_key(&idx) {
            let mut names = self.signal_names(idx);
            names.retain(|n| !is_internal(n) || self.is_derived(idx, n));
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
    fn curve(&mut self, side_b: bool, idx: usize, name: &str) -> Option<Arc<Curve>> {
        let key = (side_b, idx, name.to_lowercase());
        if let Some(c) = self.cache.get(&key) {
            return Some(c.clone());
        }
        let p = self.plot_of(side_b, idx)?;
        let (x, y) = (p.x()?, self.var(side_b, idx, name)?);
        let log = p.log_x();
        // Sin los puntos fuera de un tramo o no calculables (NaN).
        let (xs, ys): (Vec<f64>, Vec<f64>) = x
            .values
            .iter()
            .zip(&y.values)
            .filter(|(_, v)| v.is_finite())
            .map(|(&xv, &yv)| (if log { xv.max(1e-300).log10() } else { xv }, yv))
            .unzip();
        let pts = Arc::new(Curve::new(xs, ys));
        self.cache.insert(key, pts.clone());
        Some(pts)
    }

    /// B − A sobre la unión de las dos grillas.
    fn error_curve(&mut self, idx: usize, name: &str) -> Option<Arc<Curve>> {
        let key = (false, usize::MAX - idx, name.to_lowercase());
        if let Some(c) = self.cache.get(&key) {
            return Some(c.clone());
        }
        let (pa, pb) = (self.plot_of(false, idx)?, self.plot_of(true, idx)?);
        let (xa, xb) = (&pa.x()?.values, &pb.x()?.values);
        let (ya, yb) = (&self.var(false, idx, name)?.values, &self.var(true, idx, name)?.values);
        if xa.len() < 2 || xb.len() < 2 {
            return None;
        }
        let (lo, hi) = (xa[0].max(xb[0]), xa[xa.len() - 1].min(xb[xb.len() - 1]));
        let mut grid: Vec<f64> = xa.iter().chain(xb).copied().filter(|&x| x >= lo && x <= hi).collect();
        grid.sort_by(f64::total_cmp);
        grid.dedup();
        let log = pb.log_x();
        let (xs, err): (Vec<f64>, Vec<f64>) = grid
            .iter()
            .map(|&x| (if log { x.max(1e-300).log10() } else { x }, interp(xb, yb, x) - interp(xa, ya, x)))
            .filter(|(_, e)| e.is_finite())
            .unzip();
        let pts = Arc::new(Curve::new(xs, err));
        self.cache.insert(key, pts.clone());
        Some(pts)
    }

    /// Suelta las curvas que ya no se muestran: cada una guarda la serie
    /// completa (para redibujar con detalle al acercarse).
    fn keep_curves(&mut self, idx: usize, shown: &BTreeSet<String>) {
        self.cache.retain(|(_, i, name), _| (*i == idx || *i == usize::MAX - idx) && shown.contains(name));
    }
}

/// Una curva lista para dibujar: la serie completa (X creciente) y su
/// versión reducida a [`MAX_POINTS`] para verla entera.
pub struct Curve {
    xs: Vec<f64>,
    ys: Vec<f64>,
    full: Vec<[f64; 2]>,
    /// X no decreciente: se puede recortar el tramo visible por búsqueda
    /// binaria (un barrido DC puede ir hacia atrás).
    sorted: bool,
}

impl Curve {
    pub fn new(xs: Vec<f64>, ys: Vec<f64>) -> Self {
        let full = decimate(&xs, &ys, MAX_POINTS);
        let sorted = xs.windows(2).all(|w| w[0] <= w[1]);
        Self { xs, ys, full, sorted }
    }

    /// Los puntos a dibujar para la ventana `[x0, x1]` con `px` píxeles de
    /// ancho. Con la curva entera (o casi) a la vista, la versión reducida;
    /// al acercarse, el tramo visible reducido a dos puntos (mínimo y
    /// máximo) por píxel: con zoom ×100 la versión reducida dejaba ~40
    /// puntos en pantalla y la forma salía mal.
    pub fn points(&self, x0: f64, x1: f64, px: f32) -> Vec<[f64; 2]> {
        let n = self.xs.len();
        if n <= self.full.len() || !self.sorted || !(x1 > x0) {
            return self.full.clone();
        }
        let (lo, hi) = (self.xs[0], self.xs[n - 1]);
        if x1 - x0 >= 0.5 * (hi - lo) {
            return self.full.clone();
        }
        // Un punto de cada lado de la ventana, para que la línea llegue al borde.
        let a = self.xs.partition_point(|&x| x < x0).saturating_sub(1);
        let b = (self.xs.partition_point(|&x| x <= x1) + 1).min(n);
        let max = ((px.max(1.0) as usize) * 2).max(200);
        decimate(&self.xs[a..b], &self.ys[a..b], max)
    }
}

/// Una línea del gráfico; los puntos se eligen al dibujar, según la zona a
/// la vista.
struct Trace {
    label: String,
    curve: Arc<Curve>,
    color: Color32,
    width: f32,
    dashed: bool,
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
        view.var(true, idx, n)
            .or_else(|| view.var(false, idx, n))
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

    let keep: BTreeSet<String> = groups.iter().flat_map(|(_, s)| s.iter().map(|(_, n)| n.to_lowercase())).collect();
    view.keep_curves(idx, &keep);
    let mut row = 0;
    for (unit, sigs) in &groups {
        let mut lines: Vec<Trace> = Vec::new();
        // Extensión de A y B juntas: las tres vistas encuadran igual.
        let mut extent = Extent::default();
        for (i, name) in sigs {
            let color = color_for(*i);
            let (a, b) = (if diff { view.curve(false, idx, name) } else { None }, view.curve(true, idx, name));
            for c in [&a, &b].into_iter().flatten() {
                extent.add(&c.full);
            }
            if let Some(b) = b.filter(|_| tab != DiffTab::Before) {
                let label = if diff { format!("{name} · {}", view.label_b) } else { name.clone() };
                lines.push(Trace { label, curve: b, color, width: 1.6, dashed: false });
            }
            if let Some(a) = a.filter(|_| tab != DiffTab::After) {
                let label = format!("{name} · {}", view.label_a);
                // En Diff, A va detrás de B (punteada); en Before es la única.
                lines.push(match tab {
                    DiffTab::Diff => Trace { label, curve: a, color: color.gamma_multiply(0.75), width: 1.2, dashed: true },
                    _ => Trace { label, curve: a, color, width: 1.6, dashed: false },
                });
            }
        }
        if row > 0 {
            ui.add_space(gap);
        }
        wave_plot(ui, ("riku_wave", idx, unit.as_str()), row_h, link, reset, extent, log_x, &x_unit, &x_label, unit, unit, lines);
        row += 1;

        if show_error {
            let mut err: Vec<Trace> = Vec::new();
            for (i, name) in sigs {
                if let Some(e) = view.error_curve(idx, name) {
                    err.push(Trace { label: format!("Δ {name}"), curve: e, color: color_for(*i), width: 1.4, dashed: false });
                }
            }
            ui.add_space(gap);
            let y_label = if unit.is_empty() { "B − A".to_string() } else { format!("B − A [{unit}]") };
            wave_plot(ui, ("riku_wave_err", idx, unit.as_str()), row_h, link, reset, Extent::default(), log_x, &x_unit, &x_label, &y_label, unit, err);
            row += 1;
        }
    }
}

/// Nombres de un análisis y dónde buscarlos (sin distinguir mayúsculas,
/// como SPICE): la unión de A y B en orden de B, y la señal comparada.
struct PairLookup {
    names: Vec<String>,
    position: HashMap<String, usize>,
    diff: HashMap<String, usize>,
}

impl PairLookup {
    fn new(names: impl Iterator<Item = String>, diff: Option<&PlotDiff>) -> Self {
        let (mut out, mut position) = (Vec::new(), HashMap::new());
        for n in names {
            if let std::collections::hash_map::Entry::Vacant(e) = position.entry(n.to_ascii_lowercase()) {
                e.insert(out.len());
                out.push(n);
            }
        }
        let diff = diff
            .map(|d| d.signals.iter().enumerate().rev().map(|(i, s)| (s.name.to_ascii_lowercase(), i)).collect())
            .unwrap_or_default();
        Self { names: out, position, diff }
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
    lines: Vec<Trace>,
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
            // La ventana del cuadro anterior (la de este se conoce al final):
            // al acercarse, cada curva se redibuja con el detalle de la zona.
            let b = plot_ui.plot_bounds();
            let px = plot_ui.transform().frame().width();
            for t in lines {
                let pts = t.curve.points(b.min()[0], b.max()[0], px);
                let line = Line::new(t.label, PlotPoints::new(pts)).color(t.color).width(t.width);
                plot_ui.line(if t.dashed { line.style(LineStyle::dashed_loose()) } else { line });
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
    let value = |side: bool, n: &str| view.var(side, idx, n).and_then(|s| s.values.first().map(|v| eng(*v, s.unit(complex))));
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
            for n in names.iter().filter(|n| !view.hide_internal || !is_internal(n) || view.is_derived(idx, n)) {
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

/// Expresiones del usuario y los resultados escalares del análisis actual.
fn show_expressions(ui: &mut egui::Ui, view: &mut WaveView) {
    ui.separator();
    ui.label(RichText::new(tr!("wave.expressions")).strong()).on_hover_text(tr!("wave.expr_help"));
    ui.horizontal(|ui| {
        let resp = ui.add(
            egui::TextEdit::singleline(&mut view.expr_input)
                .hint_text(tr!("wave.expr_hint"))
                .desired_width(ui.available_width() - 34.0),
        );
        let enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if ui.button("+").on_hover_text(tr!("wave.expr_add")).clicked() || enter {
            view.add_expression();
        }
    });
    if let Some(err) = &view.expr_error {
        ui.label(RichText::new(err).small().color(ui.visuals().error_fg_color));
    }
    let mut remove = None;
    for (i, t) in view.expr_texts.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.small_button("✕").on_hover_text(tr!("wave.expr_remove")).clicked() {
                remove = Some(i);
            }
            ui.add(egui::Label::new(RichText::new(format!("ƒ {t}")).monospace().small()).truncate());
        });
    }
    if let Some(i) = remove {
        view.remove_expression(i);
    }
    for w in &view.expr_warnings {
        ui.label(RichText::new(w).small().color(ui.visuals().warn_fg_color));
    }

    // Mediciones: escalares del análisis actual.
    let measures = view.measures(view.plot);
    if measures.is_empty() {
        return;
    }
    ui.add_space(space::XS);
    ui.label(RichText::new(tr!("wave.measurements")).strong());
    let diff = view.is_diff();
    egui::Grid::new("riku_measures").striped(true).num_columns(if diff { 4 } else { 2 }).show(ui, |ui| {
        for d in measures {
            let (a, b) = d.diff.scalar.unwrap_or((None, None));
            let fmt = |v: Option<f64>| v.map_or_else(|| "—".to_string(), |v| eng(v, d.diff.unit));
            ui.label(RichText::new(&d.diff.name).monospace())
                .on_hover_text(d.diff.expression.as_deref().map(|e| format!("= {e}")).unwrap_or_default());
            if diff {
                ui.label(fmt(a));
                ui.label(fmt(b));
                let delta = if d.diff.status == Status::Compared { eng(d.diff.max_abs, d.diff.unit) } else { "—".into() };
                let t = RichText::new(delta).small();
                ui.label(if WaveView::changed(&d.diff) { t.color(ui.visuals().warn_fg_color) } else { t.weak() });
            } else {
                ui.label(fmt(b));
            }
            ui.end_row();
        }
    });
}

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
    show_expressions(ui, view);
    ui.add_space(space::XS);

    ui.checkbox(&mut view.hide_internal, tr!("wave.hide_internal"))
        .on_hover_text(tr!("wave.hide_internal_hint"));
    ui.add(egui::TextEdit::singleline(&mut view.filter).hint_text(tr!("wave.filter")));

    let mut names = view.signal_names(idx);
    let filter = view.filter.to_lowercase();
    names.retain(|n| {
        (filter.is_empty() || n.to_lowercase().contains(&filter))
            && (!view.hide_internal || !is_internal(n) || view.is_derived(idx, n))
            && (!view.only_changed || view.diff_of(idx, n).is_some_and(WaveView::changed))
    });
    if view.is_diff() {
        let key = |n: &String| view.diff_of(idx, n).map_or(0.0, |d| if WaveView::changed(d) { d.rel().max(1e-9) } else { 0.0 });
        names.sort_by(|a, b| key(b).total_cmp(&key(a)));
    }
    // Color de cada señal: su lugar en la lista completa.
    let all_names = view.signal_names(idx);
    let color_index: HashMap<&str, usize> = all_names.iter().enumerate().map(|(i, n)| (n.as_str(), i)).rev().collect();

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

    if names.is_empty() {
        ui.label(RichText::new(tr!("wave.no_match")).weak());
    }
    // Solo las filas a la vista: con miles de señales, armar todas cada
    // cuadro no tiene sentido.
    let row_h = ui.spacing().interact_size.y;
    egui::ScrollArea::vertical().id_salt("wave_signals").auto_shrink([false, false]).show_rows(ui, row_h, names.len(), |ui, rows| {
        for n in &names[rows] {
            let key = n.to_lowercase();
            let color = color_for(color_index.get(n.as_str()).copied().unwrap_or(0));
            let mut on = view.selection(idx).contains(&key);
            let diff = view.diff_of(idx, n).cloned();
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                ui.painter().rect_filled(rect, 2.0, if on { color } else { color.gamma_multiply(0.25) });
                let label = if view.is_derived(idx, n) { format!("ƒ {n}") } else { n.clone() };
                let mut resp = ui.checkbox(&mut on, label);
                if let Some(text) = view.derived.iter().find(|d| d.pair == idx && d.diff.name == *n).and_then(|d| d.diff.expression.as_ref()) {
                    resp = resp.on_hover_text(format!("= {text}"));
                }
                if resp.changed() {
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
    fn zooming_in_redraws_the_visible_part_with_detail() {
        // Un millón de puntos: una senoidal y un pico angosto en x = 0,5.
        let n = 1_000_000;
        let xs: Vec<f64> = (0..n).map(|i| i as f64 / n as f64).collect();
        let ys: Vec<f64> = xs.iter().map(|&x| (x * 2000.0).sin() + if (x - 0.5).abs() < 1e-6 { 5.0 } else { 0.0 }).collect();
        let c = Curve::new(xs, ys);
        // Entera: la versión reducida.
        assert_eq!(c.points(0.0, 1.0, 800.0).len(), c.full.len());
        let in_range = |pts: &[[f64; 2]], x0: f64, x1: f64| pts.iter().filter(|p| p[0] >= x0 && p[0] <= x1).count();
        // Zoom al 1 %: la reducida tenía ~40 puntos ahí; ahora, ~2 por píxel.
        let (x0, x1) = (0.495, 0.505);
        let zoomed = c.points(x0, x1, 800.0);
        assert!(in_range(&c.full, x0, x1) < 60, "{}", in_range(&c.full, x0, x1));
        assert!(in_range(&zoomed, x0, x1) >= 1500, "{}", in_range(&zoomed, x0, x1));
        assert!(zoomed.len() <= 1600 + 4);
        // El pico sigue ahí, y la línea llega a los bordes de la ventana.
        assert!(zoomed.iter().any(|p| p[1] > 4.0));
        assert!(zoomed.first().unwrap()[0] <= x0 && zoomed.last().unwrap()[0] >= x1);
        // Un barrido que va hacia atrás no se recorta (usaría la búsqueda binaria mal).
        let back = Curve::new((0..10_000).rev().map(f64::from).collect(), vec![0.0; 10_000]);
        assert_eq!(back.points(10.0, 20.0, 800.0).len(), back.full.len());
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

    /// Tiempo de un cuadro (panel de señales + gráfico) con `n` señales
    /// comparadas, como una simulación post-layout.
    #[test]
    #[ignore = "medición"]
    fn frame_time_with_many_signals() {
        use crate::modules::spice::raw::tests::binary_raw;
        for n in [1_000usize, 10_000] {
            let mut vars = vec![("time".to_string(), "time")];
            vars.extend((0..n).map(|i| (format!("v(net{i})"), "voltage")));
            let vars: Vec<(&str, &str)> = vars.iter().map(|(a, b)| (a.as_str(), *b)).collect();
            let points = 200;
            let cols = |k: f64| -> Vec<Vec<f64>> {
                (0..=n).map(|c| (0..points).map(|p| if c == 0 { p as f64 } else { (p as f64 * 0.01 + c as f64 * k).sin() }).collect()).collect()
            };
            let a = raw::parse(&binary_raw("Transient Analysis", &vars, &cols(0.0))).unwrap();
            let b = raw::parse(&binary_raw("Transient Analysis", &vars, &cols(1e-3))).unwrap();
            let mut view = WaveView::compare(a, b, "A".into(), "B".into(), None, &[]);
            let ctx = egui::Context::default();
            let rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1600.0, 1000.0));
            let mut times = Vec::new();
            for _ in 0..8 {
                let input = egui::RawInput { screen_rect: Some(rect), ..Default::default() };
                let _ = ctx.run_ui(input, |ui| {
                    let t = std::time::Instant::now();
                    show_details(ui, &mut view, &[]);
                    show_plot(ui, &mut view);
                    times.push(t.elapsed());
                });
            }
            times.sort();
            eprintln!("[ondas] {n} señales: mediana {:?}", times[times.len() / 2]);
        }
    }
}
