//! La vista de LVS: el esquemático y el layout lado a lado, la lista de lo
//! que no coincide y, al elegir algo, dónde está en cada lado (el resto se
//! atenúa, como en el diff).
//!
//! El esquemático sabe dónde está cada net y cada dispositivo de su netlist
//! (`Report::places`, del mismo netlister que vio Netgen). El layout lo dice
//! su `NetProbe` (`net_named` / `device_named`).
//!
//! La pestaña «Vínculos» es el LVS manual (`lvs::manual`): cada transistor
//! coloreado según su estado, clic en uno de cada lado para vincularlos, y
//! el archivo `lvs/<celda>.toml` al día con cada cambio.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, RichText};
use poll_promise::Promise;
use viewer_core::{BoundingBox, NetProbe, ViewerBackend, ViewerError};
use xschem_viewer::spice::Places;

use crate::gui::canvas::{self, CanvasOptions, Readout};
use crate::gui::content::{Mark, SceneState, Tag};
use crate::gui::loader::{LoadedScene, Loader};
use crate::gui::theme::space;
use crate::gui::tr;
use crate::lvs::manual::{self, Check, Session};
use crate::lvs::{Comparison, Pair, Report, Sides, Verdict};

/// Qué es algo de la lista.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ItemKind {
    /// Un dispositivo con los dos lados, pero con parámetros distintos.
    Property,
    /// Redes sin pareja.
    Net,
    /// Dispositivos sin pareja.
    Device,
}

/// Algo que no coincide, con sus nombres en cada lado.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Item {
    pub kind: ItemKind,
    pub title: String,
    pub detail: String,
    pub schematic: Vec<String>,
    pub layout: Vec<String>,
}

/// Lo que no coincide, en el orden en que se lee: parámetros, redes y
/// dispositivos.
pub(crate) fn items(c: &Comparison) -> Vec<Item> {
    let mut out: Vec<Item> = c
        .properties
        .iter()
        .map(|p| Item {
            kind: ItemKind::Property,
            title: format!("{} ↔ {}", p.schematic, p.layout),
            detail: format!(
                "{} · {}",
                p.model,
                p.values.iter().map(|v| format!("{} {} ≠ {}", v.name, v.schematic, v.layout)).collect::<Vec<_>>().join(", ")
            ),
            schematic: vec![p.schematic.clone()],
            layout: vec![p.layout.clone()],
        })
        .collect();
    out.extend(c.unmatched_nets.iter().map(|g| group(ItemKind::Net, g)));
    out.extend(c.unmatched_devices.iter().map(|g| group(ItemKind::Device, g)));
    out
}

/// Un grupo de Netgen: lo que quedó sin pareja de cada lado.
fn group(kind: ItemKind, g: &Sides<Vec<String>>) -> Item {
    let names = |v: &[String]| if v.is_empty() { "—".to_string() } else { v.join(", ") };
    Item {
        kind,
        title: names(&g.schematic),
        detail: format!("{} {}", tr!("lvs.side_layout"), names(&g.layout)),
        schematic: g.schematic.clone(),
        layout: g.layout.clone(),
    }
}

/// Medio ancho del trazo de un wire resaltado y medio lado de un pin, en
/// unidades del esquemático (la grilla de Xschem es de 10).
const WIRE_HALF: f64 = 2.0;
const PIN_HALF: f64 = 3.5;

/// Dónde está `item` en el esquemático.
pub(crate) fn schematic_mark(places: &Places, item: &Item) -> Mark {
    let mut mark = Mark::default();
    for name in &item.schematic {
        if item.kind == ItemKind::Net {
            let Some(net) = places.nets.get(name) else { continue };
            mark.fills.extend(net.wires.iter().map(|&(x1, y1, x2, y2)| segment(x1, y1, x2, y2)));
            mark.fills.extend(net.pins.iter().map(|&(x, y)| square(x, y, PIN_HALF)));
        } else if let Some(&(x1, y1, x2, y2)) = places.instance(name).and_then(|i| places.instances.get(i)) {
            mark.boxes.push(BoundingBox::from_points((x1, y1), (x2, y2)));
        }
    }
    mark
}

/// Dónde está `item` en el layout, si su escena lo sabe.
pub(crate) fn layout_mark(probe: Option<&dyn NetProbe>, item: &Item) -> Mark {
    let mut mark = Mark::default();
    let Some(probe) = probe else { return mark };
    for name in &item.layout {
        let hit = if item.kind == ItemKind::Net { probe.net_named(name) } else { probe.device_named(name) };
        if let Some(hit) = hit {
            mark.fills.extend(hit.outline);
        }
    }
    mark
}

/// Lo que se encuadra al elegir algo: al menos un tercio del dibujo, para
/// ver a qué está conectado (un transistor solo llenaría el lienzo).
fn with_context(b: BoundingBox, scene: &BoundingBox) -> BoundingBox {
    let min = scene.width().max(scene.height()) / 3.0;
    let (cx, cy) = ((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0);
    let (hw, hh) = (b.width().max(min) / 2.0, b.height().max(min) / 2.0);
    BoundingBox::from_points((cx - hw, cy - hh), (cx + hw, cy + hh))
}

/// Un wire como un rectángulo fino a lo largo del segmento.
fn segment(x1: f64, y1: f64, x2: f64, y2: f64) -> Vec<(f64, f64)> {
    let (dx, dy) = (x2 - x1, y2 - y1);
    let len = (dx * dx + dy * dy).sqrt();
    if len < f64::EPSILON {
        return square(x1, y1, WIRE_HALF);
    }
    // Normal y extensión en la dirección del wire (que se vean los extremos).
    let (nx, ny) = (-dy / len * WIRE_HALF, dx / len * WIRE_HALF);
    let (ex, ey) = (dx / len * WIRE_HALF, dy / len * WIRE_HALF);
    vec![(x1 - ex + nx, y1 - ey + ny), (x2 + ex + nx, y2 + ey + ny), (x2 + ex - nx, y2 + ey - ny), (x1 - ex - nx, y1 - ey - ny)]
}

fn square(x: f64, y: f64, h: f64) -> Vec<(f64, f64)> {
    vec![(x - h, y - h), (x + h, y - h), (x + h, y + h), (x - h, y + h)]
}

// ─── Estado ──────────────────────────────────────────────────────────────────

/// Un lado de la vista: su escena, cuando llega.
pub(crate) struct Side {
    pub path: String,
    pub scene: Option<SceneState>,
    pub error: Option<String>,
    job: Option<Promise<Result<LoadedScene, ViewerError>>>,
}

impl Side {
    fn load(loader: &Loader, backends: &[Arc<dyn ViewerBackend>], path: &Path, entry: Option<String>) -> Self {
        let path_str = path.to_string_lossy().to_string();
        let mut side = Side { path: path_str.clone(), scene: None, error: None, job: None };
        match std::fs::read(path) {
            Ok(bytes) => match backends.iter().find(|b| b.accepts(&bytes, Some(&path_str))) {
                Some(backend) => side.job = Some(loader.load_detached(backend.clone(), Arc::new(bytes), path_str, entry)),
                None => side.error = Some(tr!("status.unsupported", file = path.display())),
            },
            Err(e) => side.error = Some(format!("{}: {e}", path.display())),
        }
        side
    }

    /// `true` si llegó la escena en este cuadro.
    fn poll(&mut self) -> bool {
        if !self.job.as_ref().is_some_and(|j| j.ready().is_some()) {
            return false;
        }
        match self.job.take().map(Promise::block_and_take) {
            Some(Ok(loaded)) => {
                self.scene = Some(SceneState::from_loaded(loaded, None));
                true
            }
            Some(Err(e)) => {
                self.error = Some(e.to_string());
                false
            }
            None => false,
        }
    }
}

/// Qué muestra el panel: el resultado de Netgen o los vínculos (LVS manual).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tab {
    Netgen,
    Manual,
}

/// Lo que se puede hacer en la pestaña de vínculos.
enum Action {
    Bind,
    Unbind,
    Suggest,
    SavePositions,
    Select(String),
}

/// El LVS de un par, con sus dos escenas.
pub(crate) struct LvsState {
    pub root: PathBuf,
    pub pair: Pair,
    pub schematic: Side,
    pub layout: Side,
    pub report: Option<Result<Report, String>>,
    pub items: Vec<Item>,
    pub selected: Option<usize>,
    job: Option<Promise<Result<Report, String>>>,
    pub tab: Tab,
    /// El usuario eligió la pestaña (no se cambia sola al llegar el archivo).
    tab_chosen: bool,
    /// Los vínculos y los transistores de cada lado.
    pub manual: Option<Result<Session, String>>,
    manual_job: Option<Promise<Result<Session, String>>>,
    pub check: Option<Check>,
    /// Lo elegido para vincular: un transistor del esquemático y dedos del
    /// layout (índices en `Session::layout`).
    pub sel_sch: Option<String>,
    pub sel_lay: Vec<usize>,
    /// Lo último que pasó (guardado, un error al escribir).
    pub message: Option<String>,
}

impl LvsState {
    /// Arranca el LVS (Netgen, en otro hilo) y la carga de las dos escenas.
    pub(crate) fn start(root: PathBuf, pair: Pair, backends: &[Arc<dyn ViewerBackend>], loader: &Loader) -> Self {
        let (r, p) = (root.clone(), pair.clone());
        let job = Promise::spawn_thread("riku-lvs", move || {
            let tools = crate::lvs::tools()?;
            crate::lvs::run(&crate::lvs::Tree::disk(&r), &p, &tools)
        });
        let (r, p) = (root.clone(), pair.clone());
        let manual_job = Promise::spawn_thread("riku-lvs-map", move || manual::load(&crate::lvs::Tree::disk(&r), &p, None));
        let schematic = Side::load(loader, backends, &root.join(&pair.schematic), None);
        let layout = Side::load(loader, backends, &root.join(&pair.layout), pair.cell.clone());
        LvsState {
            root,
            pair,
            schematic,
            layout,
            report: None,
            items: Vec::new(),
            selected: None,
            job: Some(job),
            tab: Tab::Netgen,
            tab_chosen: false,
            manual: None,
            manual_job: Some(manual_job),
            check: None,
            sel_sch: None,
            sel_lay: Vec::new(),
            message: None,
        }
    }

    /// Recoge lo que terminó. `true` mientras algo siga en curso.
    pub(crate) fn poll(&mut self) -> bool {
        let mut arrived = self.schematic.poll() | self.layout.poll();
        if self.job.as_ref().is_some_and(|j| j.ready().is_some()) {
            let report = self.job.take().map(Promise::block_and_take);
            self.items = report.as_ref().and_then(|r| r.as_ref().ok()).map(|r| items(&r.comparison)).unwrap_or_default();
            self.report = report;
            arrived = true;
        }
        if self.manual_job.as_ref().is_some_and(|j| j.ready().is_some()) {
            let session = self.manual_job.take().map(Promise::block_and_take);
            // Con un archivo de vínculos, se abre en esa pestaña.
            if !self.tab_chosen && session.as_ref().is_some_and(|s| s.as_ref().is_ok_and(|s| s.exists)) {
                self.tab = Tab::Manual;
            }
            self.manual = session;
            self.recheck();
            arrived = true;
        }
        // Lo elegido antes de que llegara una escena: marcarlo en ella.
        if arrived {
            if let Some(i) = self.selected {
                self.apply(i, false);
            }
            self.refresh_tags();
        }
        self.job.is_some() || self.schematic.job.is_some() || self.layout.job.is_some() || self.manual_job.is_some()
    }

    /// Elegir (o soltar, con otro clic) algo de la lista: resaltarlo y
    /// encuadrarlo en los dos lados.
    pub(crate) fn select(&mut self, i: usize) {
        if self.selected == Some(i) {
            self.clear();
        } else {
            self.selected = Some(i);
            self.apply(i, true);
        }
    }

    /// Soltar lo elegido (Esc).
    pub(crate) fn clear(&mut self) {
        self.selected = None;
        self.sel_sch = None;
        self.sel_lay.clear();
        for bs in [self.schematic.scene.as_mut(), self.layout.scene.as_mut()].into_iter().flatten() {
            bs.mark = None;
        }
        self.refresh_tags();
    }

    /// Cambiar de pestaña: lo resaltado de la otra se suelta.
    pub(crate) fn set_tab(&mut self, tab: Tab) {
        self.tab = tab;
        self.tab_chosen = true;
        self.clear();
    }

    fn session(&self) -> Option<&Session> {
        self.manual.as_ref()?.as_ref().ok()
    }

    /// Volver a deducir todo de los vínculos (es inmediato: no se extrae
    /// nada de nuevo).
    fn recheck(&mut self) {
        self.check = self.session().map(manual::check_session);
        self.refresh_tags();
    }

    /// Escribe el archivo de vínculos y vuelve a deducir.
    fn save(&mut self) {
        let root = self.root.clone();
        if let Some(Ok(s)) = self.manual.as_mut() {
            s.map.sort();
            let path = root.join(&s.map_path);
            let written = s.map.write(&path);
            self.message = Some(match written {
                Ok(()) => {
                    s.exists = true;
                    tr!("lvs_map.saved", file = s.map_path)
                }
                Err(e) => format!("{}: {e}", path.display()),
            });
        }
        self.recheck();
    }

    fn act(&mut self, action: Action) {
        match action {
            Action::Select(name) => self.select_device(&name, true),
            Action::Bind => {
                let (Some(name), Some(Ok(s))) = (self.sel_sch.clone(), self.manual.as_mut()) else { return };
                manual::bind(&mut s.map, &name, &self.sel_lay, &s.layout);
                self.save();
            }
            Action::Unbind => {
                let (Some(name), Some(Ok(s))) = (self.sel_sch.clone(), self.manual.as_mut()) else { return };
                manual::unbind(&mut s.map, &name);
                self.sel_lay.clear();
                self.save();
            }
            Action::Suggest => {
                let Some(Ok(s)) = self.manual.as_mut() else { return };
                let new = manual::suggest(&s.map, &s.schematic, &s.layout);
                let names: Vec<String> = new.iter().map(|b| b.schematic.clone()).collect();
                s.map.binds.extend(new);
                self.save();
                self.message = Some(tr!("lvs_map.suggested", count = names.len(), names = names.join(", ")));
            }
            Action::SavePositions => {
                let Some(updated) = self.check.as_ref().map(|c| c.updated.clone()) else { return };
                if let Some(Ok(s)) = self.manual.as_mut() {
                    s.map = updated;
                }
                self.save();
            }
        }
    }

    /// Elegir un transistor del esquemático (y, si está vinculado, sus dedos
    /// del layout), y encuadrarlo en los dos lados.
    fn select_device(&mut self, name: &str, frame: bool) {
        self.sel_sch = Some(name.to_string());
        self.sel_lay = self.check.as_ref().and_then(|c| c.bound.iter().find(|(n, _)| n == name)).map(|(_, f)| f.clone()).unwrap_or_default();
        self.refresh_tags();
        if !frame {
            return;
        }
        let Some(s) = self.session() else { return };
        let sch_box = s.boxes.get(name).map(|&(x1, y1, x2, y2)| BoundingBox::from_points((x1, y1), (x2, y2)));
        let lay_box = gates_box(s, &self.sel_lay);
        for (bs, b) in [(self.schematic.scene.as_mut(), sch_box), (self.layout.scene.as_mut(), lay_box)] {
            if let (Some(bs), Some(b)) = (bs, b) {
                bs.focus = Some(with_context(b, &bs.scene.bbox()));
            }
        }
    }

    /// Clic en el esquemático: el transistor bajo el puntero (el de recuadro
    /// más chico). Con sus dedos si está vinculado; si no, se conservan los
    /// dedos elegidos que estén libres (para vincularlos con él).
    fn pick_schematic(&mut self, (x, y): (f64, f64)) {
        let Some(s) = self.session() else { return };
        let hit = s
            .schematic
            .iter()
            .filter_map(|d| s.boxes.get(&d.name).map(|b| (d.name.clone(), *b)))
            .filter(|&(_, (x1, y1, x2, y2))| x >= x1 && x <= x2 && y >= y1 && y <= y2)
            .min_by(|a, b| area(a.1).total_cmp(&area(b.1)))
            .map(|(n, _)| n);
        match hit {
            Some(n) if self.sel_sch.as_deref() == Some(&n) => {
                self.sel_sch = None;
                self.sel_lay.clear();
            }
            Some(n) => {
                let bound = self.check.as_ref().and_then(|c| c.bound.iter().find(|(b, _)| *b == n)).map(|(_, f)| f.clone());
                match bound {
                    Some(f) => self.sel_lay = f,
                    None => self.sel_lay.retain(|i| !self.check.as_ref().is_some_and(|c| c.bound.iter().any(|(_, f)| f.contains(i)))),
                }
                self.sel_sch = Some(n);
            }
            None => return,
        }
        self.refresh_tags();
    }

    /// Clic en el layout: el dedo bajo el puntero. Con Shift se suma (o se
    /// quita) de lo elegido; sin Shift, si está vinculado se elige su
    /// vínculo entero.
    fn pick_layout(&mut self, (x, y): (f64, f64), add: bool) {
        let Some(s) = self.session() else { return };
        let k = 1.0 / s.unit_um;
        let pad = 0.05 * k;
        let hit = s.layout.iter().position(|d| x >= d.gate[0] * k - pad && x <= d.gate[2] * k + pad && y >= d.gate[1] * k - pad && y <= d.gate[3] * k + pad);
        let Some(i) = hit else { return };
        let owner = self.check.as_ref().and_then(|c| c.bound.iter().find(|(_, f)| f.contains(&i))).cloned();
        if add {
            match self.sel_lay.iter().position(|&j| j == i) {
                Some(k) => {
                    self.sel_lay.remove(k);
                }
                None => self.sel_lay.push(i),
            }
        } else if self.sel_lay == [i] {
            self.sel_lay.clear();
        } else if let Some((name, fingers)) = owner {
            self.sel_sch = Some(name);
            self.sel_lay = fingers;
        } else {
            self.sel_lay = vec![i];
            let sch_bound = self.sel_sch.as_ref().is_some_and(|n| self.check.as_ref().is_some_and(|c| c.bound.iter().any(|(b, _)| b == n)));
            if sch_bound {
                self.sel_sch = None;
            }
        }
        self.refresh_tags();
    }

    /// El color de cada transistor en los dos lienzos (solo en la pestaña
    /// de vínculos).
    fn refresh_tags(&mut self) {
        let (mut sch_tags, mut lay_tags) = (Vec::new(), Vec::new());
        if let (Tab::Manual, Some(s), Some(c)) = (self.tab, self.session(), self.check.as_ref()) {
            let issues: HashSet<&str> = c.params.iter().chain(c.models.iter()).map(|(d, _)| d.as_str()).collect();
            let owner: HashMap<usize, &str> = c.bound.iter().flat_map(|(n, f)| f.iter().map(move |&i| (i, n.as_str()))).collect();
            let color = |name: Option<&str>| match name {
                Some(n) if issues.contains(n) => ISSUE,
                Some(_) => BOUND,
                None => FREE,
            };
            let bound_names: HashSet<&str> = c.bound.iter().map(|(n, _)| n.as_str()).collect();
            for d in &s.schematic {
                let Some(&(x1, y1, x2, y2)) = s.boxes.get(&d.name) else { continue };
                let bound = bound_names.contains(d.name.as_str()).then_some(d.name.as_str());
                let picked = self.sel_sch.as_deref() == Some(d.name.as_str());
                let col = if picked { PICKED } else { color(bound) };
                sch_tags.push(Tag { bbox: BoundingBox::from_points((x1, y1), (x2, y2)), color: col, strong: picked });
            }
            let k = 1.0 / s.unit_um;
            for (i, d) in s.layout.iter().enumerate() {
                let picked = self.sel_lay.contains(&i);
                let col = if picked { PICKED } else { color(owner.get(&i).copied()) };
                let bbox = BoundingBox::from_points((d.gate[0] * k, d.gate[1] * k), (d.gate[2] * k, d.gate[3] * k));
                lay_tags.push(Tag { bbox, color: col, strong: picked });
            }
        }
        if let Some(bs) = self.schematic.scene.as_mut() {
            bs.tags = sch_tags;
        }
        if let Some(bs) = self.layout.scene.as_mut() {
            bs.tags = lay_tags;
        }
    }

    fn apply(&mut self, i: usize, frame: bool) {
        let Some(item) = self.items.get(i) else { return };
        let places = self.report.as_ref().and_then(|r| r.as_ref().ok()).map(|r| &r.places);
        let marks = [
            places.map(|p| schematic_mark(p, item)).unwrap_or_default(),
            self.layout.scene.as_ref().map(|bs| layout_mark(bs.scene.net_probe().as_deref(), item)).unwrap_or_default(),
        ];
        for (bs, mark) in [self.schematic.scene.as_mut(), self.layout.scene.as_mut()].into_iter().zip(marks) {
            let Some(bs) = bs else { continue };
            if frame {
                bs.focus = mark.bbox().map(|b| with_context(b, &bs.scene.bbox()));
            }
            bs.mark = (!mark.is_empty()).then_some(mark);
        }
    }

    /// Lo elegido no se encontró en el layout (todavía no sabe ubicarlo).
    pub(crate) fn layout_unplaced(&self) -> bool {
        self.selected.is_some() && self.layout.scene.as_ref().is_some_and(|bs| bs.mark.is_none())
    }
}

/// Colores del LVS manual: vinculado, con diferencias, sin vincular y
/// elegido.
const BOUND: egui::Color32 = egui::Color32::from_rgb(80, 190, 110);
const ISSUE: egui::Color32 = egui::Color32::from_rgb(235, 140, 40);
const FREE: egui::Color32 = egui::Color32::from_rgb(140, 140, 150);
const PICKED: egui::Color32 = egui::Color32::from_rgb(255, 205, 40);

fn area((x1, y1, x2, y2): (f64, f64, f64, f64)) -> f64 {
    (x2 - x1).abs() * (y2 - y1).abs()
}

/// El recuadro de unos dedos del layout, en coordenadas de su escena.
fn gates_box(s: &Session, fingers: &[usize]) -> Option<BoundingBox> {
    let k = 1.0 / s.unit_um;
    let mut it = fingers.iter().map(|&i| &s.layout[i]);
    let first = it.next()?;
    let mut b = BoundingBox::from_points((first.gate[0] * k, first.gate[1] * k), (first.gate[2] * k, first.gate[3] * k));
    for d in it {
        b.expand(&BoundingBox::from_points((d.gate[0] * k, d.gate[1] * k), (d.gate[2] * k, d.gate[3] * k)));
    }
    Some(b)
}

// ─── Dibujo ──────────────────────────────────────────────────────────────────

/// El esquemático y el layout, uno al lado del otro.
pub(crate) fn show_central(ui: &mut egui::Ui, st: &mut LvsState, opts: CanvasOptions) {
    // Dos lienzos: la leyenda (un área fija) se pisaría entre ellos.
    let opts = CanvasOptions { legend: false, ..opts };
    let root = st.root.clone();
    let mut read: (Option<Readout>, Option<Readout>) = (None, None);
    ui.columns(2, |cols| {
        read.0 = side(&mut cols[0], &mut st.schematic, &tr!("lvs_view.schematic"), &root, opts);
        read.1 = side(&mut cols[1], &mut st.layout, &tr!("lvs_view.layout"), &root, opts);
    });
    // En la pestaña de vínculos, un clic elige un transistor (no una red).
    if st.tab != Tab::Manual {
        return;
    }
    let shift = ui.input(|i| i.modifiers.shift);
    if let Some(p) = read.0.and_then(|r| r.clicked) {
        st.pick_schematic(p);
        if let Some(bs) = st.schematic.scene.as_mut() {
            bs.net_focus = None;
        }
    }
    if let Some(p) = read.1.and_then(|r| r.clicked) {
        st.pick_layout(p, shift);
        if let Some(bs) = st.layout.scene.as_mut() {
            bs.net_focus = None;
        }
    }
}

fn side(ui: &mut egui::Ui, s: &mut Side, title: &str, root: &Path, opts: CanvasOptions) -> Option<Readout> {
    let file = Path::new(&s.path).strip_prefix(root).map_or_else(|_| s.path.clone(), |p| p.display().to_string());
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.add(egui::Label::new(RichText::new(file).weak()).truncate());
    });
    match (&mut s.scene, &s.error) {
        (Some(bs), _) => Some(egui::Frame::group(ui.style()).inner_margin(0.0).show(ui, |ui| canvas::show(ui, bs, opts)).inner),
        (None, Some(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
            None
        }
        (None, None) => {
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
            None
        }
    }
}

/// El panel: el resultado de Netgen o los vínculos.
pub(crate) fn show_list(ui: &mut egui::Ui, st: &mut LvsState) {
    ui.heading(tr!("lvs_view.title"));
    let mut tab = st.tab;
    ui.horizontal(|ui| {
        ui.selectable_value(&mut tab, Tab::Netgen, tr!("lvs_view.tab_netgen"));
        ui.selectable_value(&mut tab, Tab::Manual, tr!("lvs_view.tab_manual"));
    });
    if tab != st.tab {
        st.set_tab(tab);
    }
    ui.add_space(space::XS);
    match st.tab {
        Tab::Netgen => show_netgen(ui, st),
        Tab::Manual => show_manual(ui, st),
    }
}

/// Los vínculos: avance, lo elegido, las acciones y lo que no cuadra.
fn show_manual(ui: &mut egui::Ui, st: &mut LvsState) {
    let (s, c) = match (&st.manual, &st.check) {
        (None, _) => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr!("lvs_view.manual_loading"));
            });
            return;
        }
        (Some(Err(e)), _) => {
            ui.colored_label(ui.visuals().error_fg_color, tr!("lvs.error", error = e));
            return;
        }
        (Some(Ok(s)), Some(c)) => (s, c),
        (Some(Ok(_)), None) => return,
    };
    let file = if s.exists { s.map_path.clone() } else { tr!("lvs_view.map_new", file = s.map_path) };
    ui.label(RichText::new(file).weak().small());
    let fingers: usize = c.bound.iter().map(|(_, f)| f.len()).sum();
    ui.label(tr!("lvs_map.progress", sch = c.bound.len(), sch_total = s.schematic.len(), lay = fingers, lay_total = s.layout.len()));
    let (verdict, color) = if c.complete() {
        (tr!("lvs_map.clean"), BOUND)
    } else if c.clean() {
        (tr!("lvs_map.clean_partial"), BOUND)
    } else {
        (tr!("lvs_map.pending"), ISSUE)
    };
    ui.label(RichText::new(verdict).color(color).strong());
    if let Some(m) = c.moved {
        let mirror = if m.orient >= 4 { tr!("lvs_map.mirrored") } else { String::new() };
        ui.label(RichText::new(tr!("lvs_map.moved", angle = (m.orient % 4) as u32 * 90, mirror = mirror, dx = format!("{:.3}", m.dx), dy = format!("{:.3}", m.dy), count = m.count)).small());
    }
    if !c.by_cell.is_empty() {
        ui.label(RichText::new(tr!("lvs_map.by_cell", names = c.by_cell.join(", "))).small());
    }
    ui.add_space(space::S);

    // Lo elegido, con su W de cada lado.
    let sch = st.sel_sch.as_ref().and_then(|n| s.schematic.iter().find(|d| &d.name == n));
    let w_lay: f64 = st.sel_lay.iter().map(|&i| s.layout[i].w).sum();
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_width(ui.available_width());
        match sch {
            Some(d) => ui.label(tr!("lvs_view.sel_sch", name = d.name, w = d.w.map_or("?".into(), |w| fmt_um(w * d.m)))),
            None => ui.label(RichText::new(tr!("lvs_view.sel_sch_none")).weak()),
        };
        if st.sel_lay.is_empty() {
            ui.label(RichText::new(tr!("lvs_view.sel_lay_none")).weak());
        } else {
            ui.label(tr!("lvs_view.sel_lay", count = st.sel_lay.len(), w = fmt_um(w_lay)));
        }
    });
    let bound_sel = st.sel_sch.as_ref().is_some_and(|n| c.bound.iter().any(|(b, _)| b == n) || c.lost.iter().any(|(b, _)| b == n));
    let mut action = None;
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(st.sel_sch.is_some() && !st.sel_lay.is_empty(), egui::Button::new(tr!("lvs_view.bind"))).clicked() {
            action = Some(Action::Bind);
        }
        if ui.add_enabled(bound_sel, egui::Button::new(tr!("lvs_view.unbind"))).clicked() {
            action = Some(Action::Unbind);
        }
        if ui.button(tr!("lvs_view.suggest")).on_hover_text(tr!("help.lvs_suggest")).clicked() {
            action = Some(Action::Suggest);
        }
        let moved = c.moved.is_some() || !c.by_connectivity.is_empty() || !c.by_cell.is_empty();
        if ui.add_enabled(moved, egui::Button::new(tr!("lvs_view.save_positions"))).on_hover_text(tr!("help.lvs_update")).clicked() {
            action = Some(Action::SavePositions);
        }
    });
    ui.label(RichText::new(tr!("lvs_view.manual_hint")).weak().small());
    if let Some(m) = &st.message {
        ui.label(RichText::new(m).small());
    }
    ui.add_space(space::S);

    // Lo que no cuadra; un clic en un transistor lo elige.
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let mut section = |ui: &mut egui::Ui, title: String, rows: Vec<(Option<String>, String)>| {
            if rows.is_empty() {
                return;
            }
            ui.add_space(space::XS);
            ui.label(RichText::new(title).strong());
            for (device, text) in rows {
                match device {
                    Some(d) => {
                        let picked = st.sel_sch.as_deref() == Some(d.as_str());
                        if ui.add(egui::Button::selectable(picked, RichText::new(text).small()).truncate()).clicked() {
                            action = Some(Action::Select(d));
                        }
                    }
                    None => {
                        ui.label(RichText::new(text).small());
                    }
                }
            }
        };
        section(ui, tr!("lvs_view.shorts"), c.shorts.iter().map(|(n, ns)| (None, tr!("lvs_map.short", net = n, nets = ns.join(", ")))).collect());
        section(ui, tr!("lvs_view.opens"), c.opens.iter().map(|(n, ns)| (None, tr!("lvs_map.open", net = n, nets = ns.join(", ")))).collect());
        section(ui, tr!("lvs_view.params"), c.params.iter().chain(c.models.iter()).map(|(d, w)| (Some(d.clone()), format!("{d}: {w}"))).collect());
        section(ui, tr!("lvs_view.pins"), c.pins.iter().map(|(p, w)| (None, tr!("lvs_map.pin", pin = p, what = w))).collect());
        if c.moved_ambiguous {
            section(ui, tr!("lvs_view.moved"), vec![(None, tr!("lvs_map.moved_ambiguous"))]);
        }
        section(ui, tr!("lvs_view.lost"), c.lost.iter().map(|(d, r)| (Some(d.clone()), format!("{d}: {} ({:.3}, {:.3})", r.model, r.at[0], r.at[1]))).collect());
        section(ui, tr!("lvs_view.unbound"), c.unbound_schematic.iter().map(|d| (Some(d.clone()), d.clone())).collect());
        if !c.unbound_layout.is_empty() {
            ui.add_space(space::XS);
            ui.label(RichText::new(tr!("lvs_view.unbound_layout", count = c.unbound_layout.len())).strong());
        }
        section(ui, tr!("lvs_view.unchecked"), c.unchecked.iter().map(|u| (None, u.clone())).collect());
        section(ui, tr!("lvs_view.bound"), c.bound.iter().map(|(d, f)| (Some(d.clone()), tr!("lvs_view.bound_row", name = d, count = f.len()))).collect());
    });
    if let Some(a) = action {
        st.act(a);
    }
}

fn fmt_um(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// El resultado de Netgen y la lista de lo que no coincide.
fn show_netgen(ui: &mut egui::Ui, st: &mut LvsState) {
    let report = match &st.report {
        None => {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label(tr!("lvs_view.running"));
            });
            return;
        }
        Some(Err(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, tr!("lvs.error", error = e));
            return;
        }
        Some(Ok(r)) => r,
    };
    let (text, color) = match report.comparison.result {
        Verdict::Match => (tr!("lvs.result_match"), egui::Color32::from_rgb(90, 180, 90)),
        Verdict::PropertyErrors => (tr!("lvs.result_properties"), egui::Color32::from_rgb(220, 170, 40)),
        Verdict::Mismatch => (tr!("lvs.result_mismatch"), ui.visuals().error_fg_color),
    };
    ui.label(RichText::new(text).color(color).strong());
    let c = &report.comparison;
    let total = |m: &std::collections::BTreeMap<String, u64>| m.values().sum::<u64>();
    ui.label(
        RichText::new(tr!(
            "lvs.counts",
            dev_s = total(&c.devices.schematic),
            dev_l = total(&c.devices.layout),
            net_s = c.nets.schematic,
            net_l = c.nets.layout
        ))
        .weak()
        .small(),
    );
    ui.label(RichText::new(format!("{} · {}", report.pdk, report.layout_cell)).weak().small());
    if !report.warnings.is_empty() {
        ui.collapsing(tr!("lvs_view.warnings", count = report.warnings.len()), |ui| {
            for w in &report.warnings {
                ui.label(RichText::new(w).small());
            }
        });
    }
    ui.add_space(space::S);

    if st.items.is_empty() {
        return;
    }
    ui.label(RichText::new(tr!("lvs_view.pick_hint")).weak().small());
    if st.layout_unplaced() {
        ui.label(RichText::new(tr!("lvs_view.layout_unplaced")).weak().small());
    }
    let mut clicked = None;
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        let mut last = None;
        for (i, item) in st.items.iter().enumerate() {
            if last != Some(item.kind) {
                ui.add_space(space::XS);
                let heading = match item.kind {
                    ItemKind::Property => {
                        tr!("lvs_view.properties", count = st.items.iter().filter(|x| x.kind == ItemKind::Property).count())
                    }
                    ItemKind::Net => tr!("lvs.unmatched_nets"),
                    ItemKind::Device => tr!("lvs.unmatched_devices"),
                };
                ui.label(RichText::new(heading).strong());
                last = Some(item.kind);
            }
            let selected = st.selected == Some(i);
            let text = RichText::new(&item.title).monospace();
            let r = ui.add(egui::Button::selectable(selected, text).truncate()).on_hover_text(&item.detail);
            ui.label(RichText::new(&item.detail).weak().small());
            if r.clicked() {
                clicked = Some(i);
            }
        }
    });
    if let Some(i) = clicked {
        st.select(i);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lvs::{PropertyError, PropertyValue};
    use xschem_viewer::spice::NetPlace;

    fn comparison() -> Comparison {
        Comparison {
            result: Verdict::Mismatch,
            summary: vec![],
            devices: Sides::default(),
            nets: Sides::default(),
            pins: Sides::default(),
            unmatched_nets: vec![Sides { layout: vec!["n4".into()], schematic: vec!["net1".into(), "Vout".into()] }],
            unmatched_devices: vec![Sides { layout: vec![], schematic: vec!["M6".into()] }],
            properties: vec![PropertyError {
                model: "sky130_fd_pr__pfet_01v8".into(),
                layout: "19".into(),
                schematic: "M1".into(),
                values: vec![PropertyValue { name: "w".into(), layout: "2".into(), schematic: "4".into() }],
            }],
        }
    }

    #[test]
    fn la_lista_va_por_parametros_redes_y_dispositivos() {
        let it = items(&comparison());
        assert_eq!(it.iter().map(|i| i.kind).collect::<Vec<_>>(), [ItemKind::Property, ItemKind::Net, ItemKind::Device]);
        assert_eq!(it[0].title, "M1 ↔ 19");
        assert!(it[0].detail.contains("w 4 ≠ 2"), "{}", it[0].detail);
        assert_eq!(it[1].title, "net1, Vout");
        assert_eq!(it[2].layout, Vec::<String>::new());
    }

    #[test]
    fn marca_las_redes_con_sus_wires_y_los_dispositivos_con_su_recuadro() {
        let mut places = Places::default();
        places.nets.insert("net1".into(), NetPlace { wires: vec![(0.0, 0.0, 100.0, 0.0)], pins: vec![(0.0, 0.0)] });
        places.devices.insert("XM1".into(), "M1".into());
        places.instances.insert("M1".into(), (10.0, 20.0, 30.0, 60.0));
        let it = items(&comparison());

        // M1 ↔ 19: el recuadro de M1.
        let m = schematic_mark(&places, &it[0]);
        assert_eq!(m.boxes, [BoundingBox::from_points((10.0, 20.0), (30.0, 60.0))]);
        // net1 (Vout no está en el dibujo): un wire y un pin, que se encuadran.
        let m = schematic_mark(&places, &it[1]);
        assert_eq!(m.fills.len(), 2);
        let bb = m.bbox().unwrap();
        assert!(bb.min_x < 0.0 && bb.max_x > 100.0, "{bb:?}");
        // M6 no está: nada que marcar.
        assert!(schematic_mark(&places, &it[2]).is_empty());
    }

    #[test]
    fn sin_ubicacion_en_el_layout_no_se_marca() {
        let it = items(&comparison());
        assert!(layout_mark(None, &it[0]).is_empty());
    }

    #[test]
    fn un_dispositivo_se_encuadra_con_contexto() {
        let scene = BoundingBox::from_points((0.0, 0.0), (900.0, 600.0));
        let b = with_context(BoundingBox::from_points((100.0, 100.0), (130.0, 160.0)), &scene);
        // Al menos un tercio del lado mayor (300), centrado en el dispositivo.
        assert_eq!((b.width(), b.height()), (300.0, 300.0));
        assert_eq!(((b.min_x + b.max_x) / 2.0, (b.min_y + b.max_y) / 2.0), (115.0, 130.0));
        // Algo grande queda como está.
        let big = BoundingBox::from_points((0.0, 0.0), (800.0, 500.0));
        assert_eq!(with_context(big, &scene), big);
    }

    #[test]
    fn un_wire_es_un_rectangulo_a_lo_largo() {
        let p = segment(0.0, 0.0, 10.0, 0.0);
        let xs: Vec<f64> = p.iter().map(|q| q.0).collect();
        let ys: Vec<f64> = p.iter().map(|q| q.1).collect();
        assert_eq!(xs.iter().cloned().fold(f64::INFINITY, f64::min), -WIRE_HALF);
        assert_eq!(xs.iter().cloned().fold(f64::NEG_INFINITY, f64::max), 10.0 + WIRE_HALF);
        assert_eq!(ys.iter().cloned().fold(f64::NEG_INFINITY, f64::max), WIRE_HALF);
    }
}
