//! La vista de LVS: el esquemático y el layout lado a lado, la lista de lo
//! que no coincide y, al elegir algo, dónde está en cada lado (el resto se
//! atenúa, como en el diff).
//!
//! El esquemático sabe dónde está cada net y cada dispositivo de su netlist
//! (`Report::places`, del mismo netlister que vio Netgen). El layout lo dice
//! su `NetProbe` (`net_named` / `device_named`); mientras no lo implemente,
//! su lado se ve y se navega pero no resalta.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use eframe::egui::{self, RichText};
use poll_promise::Promise;
use viewer_core::{BoundingBox, NetProbe, ViewerBackend, ViewerError};
use xschem_viewer::spice::Places;

use crate::gui::canvas::{self, CanvasOptions};
use crate::gui::content::{Mark, SceneState};
use crate::gui::loader::{LoadedScene, Loader};
use crate::gui::theme::space;
use crate::gui::tr;
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
}

impl LvsState {
    /// Arranca el LVS (Netgen, en otro hilo) y la carga de las dos escenas.
    pub(crate) fn start(root: PathBuf, pair: Pair, backends: &[Arc<dyn ViewerBackend>], loader: &Loader) -> Self {
        let (r, p) = (root.clone(), pair.clone());
        let job = Promise::spawn_thread("riku-lvs", move || {
            let tools = crate::lvs::tools()?;
            crate::lvs::run(&crate::lvs::Tree::disk(&r), &p, &tools)
        });
        let schematic = Side::load(loader, backends, &root.join(&pair.schematic), None);
        let layout = Side::load(loader, backends, &root.join(&pair.layout), pair.cell.clone());
        LvsState { root, pair, schematic, layout, report: None, items: Vec::new(), selected: None, job: Some(job) }
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
        // Lo elegido antes de que llegara una escena: marcarlo en ella.
        if arrived {
            if let Some(i) = self.selected {
                self.apply(i, false);
            }
        }
        self.job.is_some() || self.schematic.job.is_some() || self.layout.job.is_some()
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
        for bs in [self.schematic.scene.as_mut(), self.layout.scene.as_mut()].into_iter().flatten() {
            bs.mark = None;
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

// ─── Dibujo ──────────────────────────────────────────────────────────────────

/// El esquemático y el layout, uno al lado del otro.
pub(crate) fn show_central(ui: &mut egui::Ui, st: &mut LvsState, opts: CanvasOptions) {
    // Dos lienzos: la leyenda (un área fija) se pisaría entre ellos.
    let opts = CanvasOptions { legend: false, ..opts };
    let root = st.root.clone();
    ui.columns(2, |cols| {
        side(&mut cols[0], &mut st.schematic, &tr!("lvs_view.schematic"), &root, opts);
        side(&mut cols[1], &mut st.layout, &tr!("lvs_view.layout"), &root, opts);
    });
}

fn side(ui: &mut egui::Ui, s: &mut Side, title: &str, root: &Path, opts: CanvasOptions) {
    let file = Path::new(&s.path).strip_prefix(root).map_or_else(|_| s.path.clone(), |p| p.display().to_string());
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong());
        ui.add(egui::Label::new(RichText::new(file).weak()).truncate());
    });
    match (&mut s.scene, &s.error) {
        (Some(bs), _) => {
            egui::Frame::group(ui.style()).inner_margin(0.0).show(ui, |ui| {
                canvas::show(ui, bs, opts);
            });
        }
        (None, Some(e)) => {
            ui.colored_label(ui.visuals().error_fg_color, e);
        }
        (None, None) => {
            ui.centered_and_justified(|ui| {
                ui.spinner();
            });
        }
    }
}

/// El resultado y la lista de lo que no coincide.
pub(crate) fn show_list(ui: &mut egui::Ui, st: &mut LvsState) {
    ui.heading(tr!("lvs_view.title"));
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
