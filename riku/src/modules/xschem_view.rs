//! Visor de esquemáticos Xschem para `riku gui`.
//!
//! Convierte la escena resuelta del motor (`xschem_viewer::ResolvedScene`) a
//! la escena neutra de `viewer-core`, así el visor dibuja `.sch` y `.gds`
//! por la misma ruta (zoom, tooltip, atajos, lista de cambios). En los diffs
//! agrega lo que antes solo tenía la ruta propia de esquemáticos: fantasmas
//! de la versión anterior, recuadros por componente cambiado y nets
//! resaltadas.
//!
//! Reemplaza al adaptador `viewer-core-compat` del crate de Xschem: vive del
//! lado de Riku, el motor no depende de nada de Riku.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use async_trait::async_trait;
use viewer_core::{
    Annotation, AnnotationShape, BackendInfo, BoundingBox, CancellationToken, ChangeItem, ChangeKind as VcKind,
    DrawElement as Vc, EntryLink, HAlign, Layer, LayerPaint, Rgba, Scene, SceneHandle, TextStyle, VAlign, ViewEntry,
    ViewerBackend, ViewerError, YAxis,
};
use xschem_viewer::{DrawElement as X, HAlign as XH, LineDirection, ResolvedScene, VBaseline};

use super::xschem::{is_xschem, render_options_for, XschemModule};
use super::xschem_hier as hier;
use super::xschem_pdk::{installed_pdks, pdk_root, PdkSource};
use crate::core::domain::models::{Change, ChangeKind, Element, FileChange};
use crate::i18n::tr;
use riku_kernel::{DiffFiles, DiffOptions, DiskFiles, FileSource, FormatModule};

/// Capa sintética de los marcadores de símbolo faltante.
const MISSING_LAYER: Layer = 100;
/// En la vista Diff, lo que cambió se copia a la capa `capa + HIGHLIGHT`,
/// con el color pleno, sobre el resto atenuado (como en los layouts).
const HIGHLIGHT: Layer = 200;
/// Opacidad del contorno de lo que no cambió en la vista Diff (de 255): el
/// contexto se lee (qué se conecta con qué), pero no compite con el cambio.
const DIM_STROKE: u8 = 105;
/// Altura de una línea de texto por unidad de `v_size` (convención de Xschem).
const TEXT_SCALE: f64 = 50.0;

pub struct XschemViewer;

#[async_trait]
impl ViewerBackend for XschemViewer {
    fn info(&self) -> BackendInfo {
        BackendInfo { name: "xschem", version: env!("CARGO_PKG_VERSION"), extensions: &["sch", "sym"] }
    }

    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool {
        let by_ext = path_hint.is_some_and(|p| {
            let p = p.to_ascii_lowercase();
            p.ends_with(".sch") || p.ends_with(".sym")
        });
        by_ext || is_xschem(content)
    }

    async fn load(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        self.load_entry(content, path_hint, None, token).await
    }

    /// Un archivo suelto: sus sub-esquemáticos salen del disco, junto a él.
    async fn load_entry(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        self.load_with(content, path_hint, entry, None, token).await
    }

    /// `entry`: un sub-esquemático de la jerarquía (su ruta), o `None` para
    /// la raíz. `files`: los archivos de la misma versión; sin ellos, el disco.
    async fn load_with(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        files: Option<Arc<dyn FileSource>>,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        run_blocking(token, move || {
            let (top, files) = version(path_hint.as_deref(), files);
            let h = hier::collect(&content, &top, files.as_deref());
            let current = pick(entry, &h, None)?;
            let (rs, pdk) = resolve_in(&h.nodes[&current].bytes, &current, files.as_ref())?;
            let mut scene = scene_from(&rs, &pdk);
            add_hierarchy(&mut scene, None, &h, None, &current, &rs);
            Ok(scene)
        })
        .await
    }

    async fn load_diff(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        self.load_diff_with(before, after, path_hint, entry, DiffFiles::default(), token).await
    }

    /// Diff de la raíz o de un sub-esquemático (`entry`), cada versión con
    /// sus archivos. La lista de jerarquía marca qué cambió, también por dentro.
    async fn load_diff_with(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        files: DiffFiles,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        run_blocking(token, move || {
            let top = path_hint.unwrap_or_else(|| "archivo.sch".into());
            let ha = hier::collect(&before, &top, files.before.as_deref());
            let hb = hier::collect(&after, &top, files.after.as_deref());
            let changed = hier::changes(&ha, &hb);
            let current = pick(entry, &hb, Some(&ha))?;
            let bytes = |h: &hier::Hierarchy| h.nodes.get(&current).map(|n| n.bytes.clone()).unwrap_or_default();
            let (a, b) = (bytes(&ha), bytes(&hb));
            let report = XschemModule::new().diff_with(&a, &b, &current, &DiffOptions::default(), &files);
            let mut scene = if b.is_empty() {
                // Un sub-esquemático que ya no está: cómo era, con aviso.
                let (ra, pdk) = resolve_in(&a, &current, files.before.as_ref())?;
                let mut scene = scene_from(&ra, &pdk);
                scene.notices.push(tr!("xschem.not_in_new", path = current));
                add_hierarchy(&mut scene, Some(&ha), &hb, Some(&changed), &current, &ra);
                return Ok(scene);
            } else {
                let (rb, pdk) = resolve_in(&b, &current, files.after.as_ref())?;
                let ra = if a.is_empty() { None } else { Some(resolve_in(&a, &current, files.before.as_ref())?.0) };
                let mut scene = diff_scene(ra.as_ref(), &rb, &pdk, &report);
                add_hierarchy(&mut scene, Some(&ha), &hb, Some(&changed), &current, &rb);
                scene
            };
            scene.current_entry = Some(current);
            Ok(scene)
        })
        .await
    }
}

/// La raíz de la jerarquía y de dónde se leen sus sub-esquemáticos. Sin
/// archivos de la versión, el disco: la carpeta del archivo, si existe.
fn version(path_hint: Option<&str>, files: Option<Arc<dyn FileSource>>) -> (String, Option<Arc<dyn FileSource>>) {
    let hint = path_hint.unwrap_or("archivo.sch");
    if files.is_some() {
        return (hint.to_string(), files);
    }
    let path = std::path::Path::new(hint);
    match (path.parent().filter(|d| path.is_file() && d.is_dir()), path.file_name()) {
        (Some(dir), Some(name)) => {
            let disk: Arc<dyn FileSource> = Arc::new(DiskFiles::new(dir.to_path_buf()));
            (name.to_string_lossy().into_owned(), Some(disk))
        }
        _ => (hint.to_string(), None),
    }
}

/// El esquemático a mostrar: `entry` si está en la jerarquía (de alguna de
/// las dos versiones), o la raíz.
fn pick(entry: Option<String>, h: &hier::Hierarchy, other: Option<&hier::Hierarchy>) -> viewer_core::Result<String> {
    match entry {
        None => Ok(h.top.clone()),
        Some(e) if h.nodes.contains_key(&e) || other.is_some_and(|o| o.nodes.contains_key(&e)) => Ok(e),
        Some(e) => Err(ViewerError::Backend(tr!("xschem.not_in_hierarchy", entry = e, top = h.top))),
    }
}

/// Lista de jerarquía (raíz primero, marcada con lo que cambió) y los
/// vínculos de las instancias de `current` a sus sub-esquemáticos (doble
/// clic para entrar). `a`: la jerarquía anterior, en un diff.
fn add_hierarchy(
    scene: &mut Scene,
    a: Option<&hier::Hierarchy>,
    b: &hier::Hierarchy,
    changed: Option<&BTreeMap<String, ChangeKind>>,
    current: &str,
    rs: &ResolvedScene,
) {
    let mut paths: BTreeSet<&String> = b.nodes.keys().collect();
    paths.extend(a.iter().flat_map(|a| a.nodes.keys()));
    if paths.len() > 1 {
        let kind = |p: &str| {
            changed.and_then(|c| c.get(p)).map(|k| match k {
                ChangeKind::Added => VcKind::Added,
                ChangeKind::Removed => VcKind::Removed,
                ChangeKind::Modified | ChangeKind::Renamed => VcKind::Modified,
            })
        };
        let mut entries: Vec<ViewEntry> = paths
            .into_iter()
            .map(|p| ViewEntry { id: p.clone(), is_root: *p == b.top, size: None, change: kind(p), renamed_from: None })
            .collect();
        entries.sort_by(|x, y| y.is_root.cmp(&x.is_root).then_with(|| x.id.cmp(&y.id)));
        scene.entries = entries;
        scene.metadata.push((tr!("meta.hierarchy"), tr!("meta.schematics", count = scene.entries.len())));
    }
    scene.current_entry = Some(current.to_string());
    let node = b.nodes.get(current).or_else(|| a.and_then(|a| a.nodes.get(current)));
    for sub in node.iter().flat_map(|n| n.children.iter()) {
        let mut bbox = BoundingBox::empty();
        for vc in rs.elements_of(&sub.instance).flat_map(convert) {
            bbox.expand(&vc.bounding_box());
        }
        if bbox.width().is_finite() && bbox.width() >= 0.0 {
            scene.links.push(EntryLink {
                bbox,
                entry: sub.schematic.clone(),
                label: format!("{} ({})", sub.instance, sub.schematic),
            });
        }
    }
}

async fn run_blocking(
    token: CancellationToken,
    f: impl FnOnce() -> viewer_core::Result<Scene> + Send + 'static,
) -> viewer_core::Result<SceneHandle> {
    if token.is_cancelled() {
        return Err(ViewerError::Cancelled);
    }
    // Índice espacial (culling, picking) en el mismo hilo de carga.
    let scene = tokio::task::spawn_blocking(move || {
        f().map(|mut s| {
            s.build_index();
            s
        })
    })
    .await??;
    if token.is_cancelled() {
        return Err(ViewerError::Cancelled);
    }
    Ok(Arc::new(scene) as SceneHandle)
}

/// Parsea y resuelve un esquemático (símbolos de `.xschemrc` y del PDK, que
/// se detecta por sus símbolos si `$PDK` no está definida).
#[cfg(test)]
fn resolve(content: &[u8]) -> viewer_core::Result<(ResolvedScene, PdkSource)> {
    resolve_in(content, "", None)
}

/// Como [`resolve`]; los símbolos del proyecto (`amp.sym` junto al
/// esquemático `path`) salen primero de `files`, la misma versión: en el
/// diff de dos commits, cada lado con sus propios símbolos.
fn resolve_in(
    content: &[u8],
    path: &str,
    files: Option<&Arc<dyn FileSource>>,
) -> viewer_core::Result<(ResolvedScene, PdkSource)> {
    let text = std::str::from_utf8(content).map_err(|e| ViewerError::Parse(tr!("err.not_utf8", error = e)))?;
    let parsed = xschem_viewer::parser::parse(text).map_err(|e| ViewerError::Parse(e.to_string()))?;
    let (mut opts, pdk) = render_options_for(text);
    if let Some(files) = files.cloned() {
        let from = path.to_string();
        opts = opts.with_symbol_lookup(Arc::new(move |sym: &str| {
            let found = hier::find(sym, &from, files.as_ref(), false)?;
            files.read(&found).and_then(|b| String::from_utf8(b).ok())
        }));
    }
    Ok((xschem_viewer::SceneBuilder::new(&opts).build(&parsed), pdk))
}

// ─── Escena ──────────────────────────────────────────────────────────────────

fn scene_from(rs: &ResolvedScene, pdk: &PdkSource) -> Scene {
    let mut scene = Scene::new();
    scene.y_axis = YAxis::Down;
    scene.text_style = TextStyle::Drawn;
    let mut layers = BTreeSet::new();
    for el in &rs.elements {
        for vc in convert(el) {
            layers.insert(vc.layer());
            scene.push(vc);
        }
    }
    scene.layers = layers.into_iter().map(|l| (l, layer_paint(l))).collect();
    scene.metadata = vec![(tr!("meta.elements"), rs.elements.len().to_string()), (tr!("meta.wires"), rs.wires.len().to_string())];
    match pdk {
        PdkSource::Env { path, extra } if extra.is_empty() => scene.metadata.push((tr!("meta.pdk"), pdk_name(path))),
        PdkSource::Env { path, extra } => {
            let active = pdk_name(path);
            let others: Vec<&str> = extra.iter().map(|(n, _)| n.as_str()).collect();
            scene.metadata.push((tr!("meta.pdk"), format!("{active} + {}", others.join(" + "))));
            scene.notices.push(tr!("xschem.pdk_other", active = active, others = others.join(", "), first = others[0]));
        }
        PdkSource::Detected(found) => {
            let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
            let main = names[0];
            scene.metadata.push((tr!("meta.pdk"), tr!("meta.detected", names = names.join(" + "))));
            let which = if names.len() == 1 {
                tr!("xschem.pdk_used_one", main = main)
            } else {
                tr!("xschem.pdk_used_many", names = names.join(", "))
            };
            scene.notices.push(tr!("xschem.pdk_unset", which = which, main = main));
        }
        PdkSource::Missing(_) => {}
    }
    if !rs.missing_symbols.is_empty() {
        scene.metadata.push((tr!("meta.unresolved_symbols"), rs.missing_symbols.len().to_string()));
        scene.notices.push(missing_notice(&rs.missing_symbols, pdk));
    }
    scene
}

/// Nombre del PDK a partir de su ruta de símbolos (`…/sky130A/libs.tech/xschem`).
fn pdk_name(symbols: &std::path::Path) -> String {
    symbols
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.file_name())
        .map_or_else(|| symbols.display().to_string(), |n| n.to_string_lossy().to_string())
}

/// Aviso de símbolos faltantes que dice la causa y qué hacer.
fn missing_notice(missing: &[String], pdk: &PdkSource) -> String {
    let list = missing.join(", ");
    let cause = match pdk {
        PdkSource::Missing(reason) => {
            let installed = pdk_root().map(|r| installed_pdks(&r)).unwrap_or_default();
            let hint = match installed.first() {
                Some(first) => tr!("xschem.missing_hint_pdk", first = first, installed = installed.join(", ")),
                None => tr!("xschem.missing_hint_env"),
            };
            format!("{reason}. {hint}")
        }
        found => tr!(
            "xschem.missing_not_in",
            paths = found.paths().iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ),
    };
    tr!("xschem.missing", count = missing.len(), list = list, cause = cause)
}

/// Colores de Xschem (tema oscuro; el visor los adapta al tema claro).
fn layer_paint(layer: Layer) -> LayerPaint {
    let (name, (r, g, b)) = match layer {
        1 => (tr!("layer.wires"), (100, 180, 255)),
        2 => (tr!("layer.components"), (200, 200, 100)),
        3 => (tr!("layer.text"), (180, 180, 180)),
        4 => (tr!("layer.pins"), (100, 220, 100)),
        MISSING_LAYER => (tr!("layer.missing_symbol"), (200, 80, 80)),
        _ => (tr!("layer.n", n = layer), (160, 160, 160)),
    };
    LayerPaint { name, fill: Rgba::new(r, g, b, 77), stroke: Rgba::new(r, g, b, 255), hidden: false }
}

fn layer_of(l: i32) -> Layer {
    l.clamp(0, Layer::MAX as i32) as Layer
}

/// Un primitivo de Xschem → cero o más primitivos neutros. Los arcos pasan a
/// polilíneas abiertas, los textos multilínea a una línea por elemento y los
/// símbolos faltantes a un marcador con su nombre.
fn convert(el: &X) -> Vec<Vc> {
    match el {
        X::Line { x1, y1, x2, y2, layer, .. } => {
            vec![Vc::Line { x1: *x1, y1: *y1, x2: *x2, y2: *y2, layer: layer_of(*layer) }]
        }
        X::Rect { x, y, w, h, layer, filled, .. } => {
            vec![Vc::Rect { x: *x, y: *y, w: *w, h: *h, layer: layer_of(*layer), filled: *filled }]
        }
        X::Circle { cx, cy, r, layer, .. } => {
            vec![Vc::Circle { cx: *cx, cy: *cy, r: *r, layer: layer_of(*layer), filled: false }]
        }
        X::Arc { cx, cy, r, start_angle, sweep_angle, layer, .. } => {
            let steps = (sweep_angle.abs() / 5.0).ceil().max(1.0) as usize;
            let points = (0..=steps)
                .map(|i| {
                    // Ángulos de Xschem en sentido matemático; el eje Y va hacia abajo.
                    let a = -(start_angle + sweep_angle * i as f64 / steps as f64).to_radians();
                    (cx + r * a.cos(), cy + r * a.sin())
                })
                .collect();
            vec![Vc::Polygon { points, layer: layer_of(*layer), filled: false }]
        }
        X::Polygon { points, layer, filled, .. } => {
            vec![Vc::Polygon { points: points.clone(), layer: layer_of(*layer), filled: *filled }]
        }
        X::Text { x, y, content, v_size, rotation, mirror, h_center, v_center, layer, .. } => {
            let layout = xschem_viewer::resolve_text_layout(*rotation, *mirror, *h_center, *v_center);
            let size = v_size * TEXT_SCALE;
            let angle = layout.visual_angle_deg as f64;
            let (sin, cos) = angle.to_radians().sin_cos();
            let h_align = match layout.h_align {
                XH::Start => HAlign::Start,
                XH::Middle => HAlign::Middle,
                XH::End => HAlign::End,
            };
            let v_align = match layout.baseline {
                VBaseline::Top => VAlign::Top,
                VBaseline::Middle => VAlign::Middle,
                VBaseline::Bottom => VAlign::Bottom,
            };
            let lines: Vec<&str> = content.lines().collect();
            let n = lines.len();
            lines
                .into_iter()
                .enumerate()
                .map(|(i, line)| {
                    let idx = match layout.line_direction {
                        LineDirection::Forward => i,
                        LineDirection::Reverse => n - 1 - i,
                    } as f64;
                    // Cada línea baja una altura de texto, en la dirección del texto rotado.
                    let dy = idx * size;
                    Vc::Text {
                        x: x - sin * dy,
                        y: y + cos * dy,
                        content: line.to_string(),
                        size,
                        angle_deg: angle,
                        h_align,
                        v_align,
                        layer: layer_of(*layer),
                    }
                })
                .collect()
        }
        X::MissingSymbol { name, x, y, .. } => vec![
            Vc::Rect { x: x - 10.0, y: y - 10.0, w: 20.0, h: 20.0, layer: MISSING_LAYER, filled: false },
            Vc::Text {
                x: *x,
                y: *y,
                content: format!("?{}", name.rsplit('/').next().unwrap_or(name)),
                size: 18.0,
                angle_deg: 0.0,
                h_align: HAlign::Middle,
                v_align: VAlign::Middle,
                layer: MISSING_LAYER,
            },
        ],
    }
}

// ─── Diff ────────────────────────────────────────────────────────────────────

/// Escena de diff: la versión nueva, los fantasmas de la anterior (lo que se
/// movió o se eliminó), una marca por cambio y la lista de cambios.
fn diff_scene(a: Option<&ResolvedScene>, b: &ResolvedScene, pdk: &PdkSource, report: &FileChange) -> Scene {
    let mut scene = scene_from(b, pdk);
    highlight_changes(&mut scene, b, report);

    if let Some(a) = a {
        // Componentes movidos o eliminados: cómo estaban antes.
        for c in &report.changes {
            let Element::Component { name } = &c.element else { continue };
            if c.position_changed || c.kind == ChangeKind::Removed {
                let before = c.renamed_from.as_deref().unwrap_or(name);
                scene.ghost.extend(a.elements_of(before).flat_map(convert));
            }
        }
        // Wires que ya no están (en cualquier sentido). Los de B se buscan
        // por sus extremos en una grilla de celdas de `NEAR`: comparar cada
        // wire de A con todos los de B era O(n²).
        let cell = |x: f64, y: f64| ((x / NEAR).floor() as i64, (y / NEAR).floor() as i64);
        let mut by_end: HashMap<(i64, i64), Vec<usize>> = HashMap::new();
        for (i, &(bx1, by1, bx2, by2, _)) in b.wires.iter().enumerate() {
            by_end.entry(cell(bx1, by1)).or_default().push(i);
            by_end.entry(cell(bx2, by2)).or_default().push(i);
        }
        for &(x1, y1, x2, y2, _) in &a.wires {
            let same = |&(bx1, by1, bx2, by2, _): &(f64, f64, f64, f64, Option<String>)| {
                (near(x1, bx1) && near(y1, by1) && near(x2, bx2) && near(y2, by2))
                    || (near(x1, bx2) && near(y1, by2) && near(x2, bx1) && near(y2, by1))
            };
            // Un extremo de B a menos de NEAR de (x1, y1) cae en su celda o
            // en una vecina.
            let (cx, cy) = cell(x1, y1);
            let found = (cx - 1..=cx + 1)
                .flat_map(|x| (cy - 1..=cy + 1).map(move |y| (x, y)))
                .filter_map(|k| by_end.get(&k))
                .flatten()
                .any(|&i| same(&b.wires[i]));
            if !found {
                scene.ghost.push(Vc::Line { x1, y1, x2, y2, layer: 1 });
            }
        }
    }

    let mut functional = 0;
    let mut cosmetic = 0;
    for c in &report.changes {
        if c.cosmetic {
            cosmetic += 1
        } else {
            functional += 1
        }
        let Some((annotation, item)) = mark(a, b, c) else { continue };
        if let Some(an) = annotation {
            scene.annotations.push(an);
        }
        scene.changes.push(item);
    }
    let summary = if functional + cosmetic == 0 {
        tr!("meta.no_changes")
    } else {
        tr!("diff.summary", functional = functional, cosmetic = cosmetic)
    };
    scene.metadata.insert(0, (tr!("meta.diff"), summary));
    scene
}

/// Atenúa todo el esquemático y vuelve a pintar a color pleno lo que cambió
/// funcionalmente: los componentes añadidos, modificados o renombrados y los
/// wires de las nets nuevas. Lo cosmético (solo movido) queda atenuado, con
/// su recuadro; lo eliminado es el fantasma de la versión anterior.
fn highlight_changes(scene: &mut Scene, b: &ResolvedScene, report: &FileChange) {
    let full: Vec<(Layer, LayerPaint)> = scene.layers.iter().map(|(k, p)| (*k, p.clone())).collect();
    for paint in scene.layers.values_mut() {
        paint.stroke.a = paint.stroke.a.min(DIM_STROKE);
        paint.fill.a /= 3;
    }
    let mut changed: Vec<Vc> = Vec::new();
    for c in report.changes.iter().filter(|c| !c.cosmetic && c.kind != ChangeKind::Removed) {
        match &c.element {
            Element::Component { name } => changed.extend(b.elements_of(name).flat_map(convert)),
            Element::Net { name } if c.kind == ChangeKind::Added => {
                changed.extend(
                    b.wires.iter().filter(|w| w.4.as_deref() == Some(name.as_str())).map(|&(x1, y1, x2, y2, _)| Vc::Line {
                        x1,
                        y1,
                        x2,
                        y2,
                        layer: 1,
                    }),
                );
            }
            _ => {}
        }
    }
    for mut el in changed {
        let base = el.layer();
        let to = base.saturating_add(HIGHLIGHT);
        if let Some((_, paint)) = full.iter().find(|(k, _)| *k == base) {
            scene.layers.entry(to).or_insert_with(|| paint.clone());
        }
        el.set_layer(to);
        scene.push(el);
    }
}

/// Marca en la escena y entrada de la lista para un cambio.
fn mark(a: Option<&ResolvedScene>, b: &ResolvedScene, c: &Change) -> Option<(Option<Annotation>, ChangeItem)> {
    let kind = match c.kind {
        ChangeKind::Added => VcKind::Added,
        ChangeKind::Removed => VcKind::Removed,
        ChangeKind::Modified | ChangeKind::Renamed => VcKind::Modified,
    };
    match &c.element {
        Element::Component { name } => {
            let label = match (&c.renamed_from, c.kind) {
                (Some(from), ChangeKind::Renamed) => format!("{from} → {name}"),
                _ => name.clone(),
            };
            // Un eliminado ya no está en B: se marca donde estaba en A.
            let bbox = match c.kind {
                ChangeKind::Removed => a.and_then(|a| a.component_bbox(name)),
                _ => b.component_bbox(name),
            }
            .filter(|bb| !bb.is_empty())
            .map(|bb| BoundingBox::from_points((bb.min_x, bb.min_y), (bb.max_x, bb.max_y)));
            let moved_only = c.cosmetic && c.position_changed;
            let annotation = bbox.map(|bb| Annotation {
                kind,
                cosmetic: c.cosmetic,
                moved: c.position_changed,
                label: label.clone(),
                shape: AnnotationShape::Box(bb),
            });
            let detail = if moved_only { tr!("change.moved") } else { param_changes(c) };
            let label = if c.kind == ChangeKind::Renamed { tr!("change.renamed", label = label) } else { label };
            Some((annotation, ChangeItem { kind, label, detail, bbox, cosmetic: c.cosmetic, error: false }))
        }
        Element::Net { name } => {
            // Una net añadida se ve en B; una eliminada, en A.
            let wires = match c.kind {
                ChangeKind::Removed => a.map_or(&[][..], |a| a.wires.as_slice()),
                _ => b.wires.as_slice(),
            };
            let segs: Vec<_> = wires
                .iter()
                .filter(|w| w.4.as_deref() == Some(name.as_str()))
                .map(|&(x1, y1, x2, y2, _)| (x1, y1, x2, y2))
                .collect();
            let bbox = segs.iter().fold(None::<BoundingBox>, |acc, &(x1, y1, x2, y2)| {
                let s = BoundingBox::from_points((x1, y1), (x2, y2));
                Some(acc.map_or(s, |mut a| {
                    a.expand(&s);
                    a
                }))
            });
            let annotation = (!segs.is_empty()).then(|| Annotation {
                kind,
                cosmetic: c.cosmetic,
                moved: false,
                label: name.clone(),
                shape: AnnotationShape::Segments(segs),
            });
            let item = ChangeItem {
                kind,
                label: format!("net:{name}"),
                detail: String::new(),
                bbox,
                cosmetic: c.cosmetic,
                error: false,
            };
            Some((annotation, item))
        }
        Element::Whole => Some((
            None,
            ChangeItem {
                kind: VcKind::Modified,
                label: tr!("change.move_all"),
                detail: tr!("change.move_all_detail"),
                bbox: None,
                cosmetic: true,
                error: false,
            },
        )),
        _ => None,
    }
}

/// `W: 1u → 2u · L: …` con los parámetros que cambiaron (sin posición).
fn param_changes(c: &Change) -> String {
    let inside = c.after(super::xschem::INSIDE_KEY).map(|v| tr!("diff.inside", path = v));
    let changed: BTreeMap<&str, String> = c
        .params()
        .filter(|d| d.changed() && d.key != super::xschem::INSIDE_KEY)
        .map(|d| {
            let show = |v: &Option<riku_kernel::Value>| v.as_ref().map_or("—".to_string(), |v| v.to_string());
            (d.key.as_str(), format!("{} → {}", show(&d.before), show(&d.after)))
        })
        .collect();
    inside.into_iter().chain(changed.iter().map(|(k, v)| format!("{k}: {v}"))).collect::<Vec<_>>().join(" · ")
}

/// Distancia bajo la cual dos coordenadas son la misma.
const NEAR: f64 = 0.001;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < NEAR
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Una versión en memoria: top.sch usa amp.sch (x1), que usa un resistor.
    struct Mem(HashMap<&'static str, Vec<u8>>);

    impl FileSource for Mem {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    const TOP: &str = "v {xschem version=3.4.5 file_version=1.2}
C {amp.sym} 0 0 0 0 {name=x1}
N 0 0 100 0 {lab=out}
";

    fn project(r: &str) -> Arc<dyn FileSource> {
        let amp = format!(
            "v {{xschem version=3.4.5 file_version=1.2}}
C {{res.sym}} 0 0 0 0 {{name=R1 value={r}}}
"
        );
        let sym = "v {xschem version=3.4.5 file_version=1.2}
L 4 -20 0 20 0 {}
B 5 -22.5 -2.5 -17.5 2.5 {name=in dir=in}
";
        Arc::new(Mem(HashMap::from([
            ("top.sch", TOP.as_bytes().to_vec()),
            ("amp.sch", amp.into_bytes()),
            ("amp.sym", sym.as_bytes().to_vec()),
        ])))
    }

    fn block<T>(f: impl std::future::Future<Output = T>) -> T {
        tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap().block_on(f)
    }

    #[test]
    fn la_jerarquia_se_lista_y_se_entra_a_un_sub_esquematico() {
        let v = XschemViewer;
        let top = block(v.load_with(TOP.into(), Some("top.sch".into()), None, Some(project("1k")), Default::default())).unwrap();
        let ids: Vec<&str> = top.entries().iter().map(|e| e.id.as_str()).collect();
        assert_eq!(ids, ["top.sch", "amp.sch"], "la raíz primero");
        assert_eq!(top.current_entry(), Some("top.sch"));
        assert_eq!(
            top.links().iter().map(|l| (l.entry.as_str(), l.label.as_str())).collect::<Vec<_>>(),
            [("amp.sch", "x1 (amp.sch)")]
        );

        let amp = block(v.load_with(
            TOP.into(),
            Some("top.sch".into()),
            Some("amp.sch".into()),
            Some(project("1k")),
            Default::default(),
        ))
        .unwrap();
        assert_eq!(amp.current_entry(), Some("amp.sch"));
        assert!(block(v.load_with(
            TOP.into(),
            Some("top.sch".into()),
            Some("otro.sch".into()),
            Some(project("1k")),
            Default::default()
        ))
        .is_err());
    }

    #[test]
    fn el_diff_marca_el_sub_esquematico_y_su_padre() {
        let v = XschemViewer;
        let files = DiffFiles::new(Some(project("1k")), Some(project("2k")));
        let top =
            block(v.load_diff_with(TOP.into(), TOP.into(), Some("top.sch".into()), None, files.clone(), Default::default()))
                .unwrap();
        let change = |id: &str| top.entries().iter().find(|e| e.id == id).and_then(|e| e.change);
        assert_eq!(change("amp.sch"), Some(VcKind::Modified));
        assert_eq!(change("top.sch"), Some(VcKind::Modified), "por dentro");
        assert!(
            top.changes().iter().any(|c| c.label.contains("x1") && c.detail.contains(&tr!("diff.inside", path = "amp.sch"))),
            "{:?}",
            top.changes()
        );

        let amp = block(v.load_diff_with(
            TOP.into(),
            TOP.into(),
            Some("top.sch".into()),
            Some("amp.sch".into()),
            files,
            Default::default(),
        ))
        .unwrap();
        assert_eq!(amp.current_entry(), Some("amp.sch"));
        assert!(amp.changes().iter().any(|c| c.label.contains("R1")), "el diff de amp.sch: {:?}", amp.changes());
    }

    const A: &str = "v {xschem version=3.4.5 file_version=1.2}\n\
N 0 0 100 0 {lab=vin}\n\
N 0 50 100 50 {lab=old}\n\
T {hola\nmundo} 10 10 0 0 0.4 0.4 {}\n\
C {res.sym} 0 0 0 0 {name=R1 value=1k}\n\
C {res.sym} 200 0 0 0 {name=R2 value=1k}\n";

    const B: &str = "v {xschem version=3.4.5 file_version=1.2}\n\
N 0 0 100 0 {lab=vin}\n\
N 0 80 100 80 {lab=nueva}\n\
T {hola\nmundo} 10 10 0 0 0.4 0.4 {}\n\
C {res.sym} 40 0 0 0 {name=R1 value=2k}\n";

    #[test]
    fn scene_uses_drawn_text_one_element_per_line() {
        let (rs, pdk) = resolve(A.as_bytes()).unwrap();
        let s = scene_from(&rs, &pdk);
        assert_eq!(s.text_style, TextStyle::Drawn);
        let texts: Vec<_> = s
            .elements
            .iter()
            .filter_map(|e| match e {
                Vc::Text { content, y, size, .. } => Some((content.clone(), *y, *size)),
                _ => None,
            })
            .filter(|(c, ..)| c == "hola" || c == "mundo")
            .collect();
        assert_eq!(texts.len(), 2, "{texts:?}");
        // La segunda línea queda una altura de texto más abajo.
        assert!((texts[1].1 - texts[0].1 - texts[0].2).abs() < 1e-9, "{texts:?}");
        assert!(s.layers.contains_key(&1), "capa de wires con su color");
    }

    #[test]
    fn a_wire_kept_reversed_or_within_tolerance_is_not_a_ghost() {
        let head = "v {xschem version=3.0.0 file_version=1.2}
";
        let a = format!(
            "{head}N 0 0 100 0 {{lab=x}}
N 0 50 100 50 {{lab=old}}
N 3 7 3 90 {{lab=y}}
"
        );
        // El primero invertido, el tercero corrido menos que la tolerancia.
        let b = format!(
            "{head}N 100 0 0 0 {{lab=x}}
N 3.0004 7 3 90 {{lab=y}}
"
        );
        let (ra, (rb, pdk)) = (resolve(a.as_bytes()).unwrap().0, resolve(b.as_bytes()).unwrap());
        let report = XschemModule::new().diff(a.as_bytes(), b.as_bytes(), "t.sch", &DiffOptions::default());
        let s = diff_scene(Some(&ra), &rb, &pdk, &report);
        let ghosts: Vec<f64> = s
            .ghost
            .iter()
            .filter_map(|g| match g {
                Vc::Line { y1, .. } => Some(*y1),
                _ => None,
            })
            .collect();
        assert_eq!(ghosts, vec![50.0], "solo el wire que ya no está");
    }

    #[test]
    fn el_diff_atenua_todo_y_resalta_lo_que_cambio() {
        let (a, (b, pdk)) = (resolve(A.as_bytes()).unwrap().0, resolve(B.as_bytes()).unwrap());
        let report = XschemModule::new().diff(A.as_bytes(), B.as_bytes(), "t.sch", &DiffOptions::default());
        let s = diff_scene(Some(&a), &b, &pdk, &report);
        // Las capas de siempre, atenuadas; las del resaltado, con el color pleno.
        let wires = &s.layers[&1];
        assert!(wires.stroke.a <= DIM_STROKE, "{wires:?}");
        let lit: Vec<&LayerPaint> = s.layers.iter().filter(|(k, _)| **k >= HIGHLIGHT).map(|(_, p)| p).collect();
        assert!(!lit.is_empty() && lit.iter().all(|p| p.stroke.a == 255), "{lit:?}");
        // Mismo nombre que la capa de base: se ocultan juntas.
        assert!(lit.iter().all(|p| s.layers.iter().any(|(k, q)| *k < HIGHLIGHT && q.name == p.name)));
        // R1 cambió: sus elementos están en el resaltado.
        let r1 = b.elements_of("R1").flat_map(convert).count();
        let highlighted = s.elements.iter().filter(|e| e.layer() >= HIGHLIGHT).count();
        assert!(r1 > 0 && highlighted >= r1, "{highlighted} de {r1}");
    }

    #[test]
    fn diff_scene_has_ghosts_annotations_and_changes() {
        let (a, (b, pdk)) = (resolve(A.as_bytes()).unwrap().0, resolve(B.as_bytes()).unwrap());
        let report = XschemModule::new().diff(A.as_bytes(), B.as_bytes(), "t.sch", &DiffOptions::default());
        let s = diff_scene(Some(&a), &b, &pdk, &report);
        // El wire "old" ya no está: fantasma.
        assert!(s.ghost.iter().any(|g| matches!(g, Vc::Line { y1, .. } if (*y1 - 50.0).abs() < 1e-9)));
        let labels: Vec<&str> = s.changes.iter().map(|c| c.label.as_str()).collect();
        assert!(labels.contains(&"R1") && labels.contains(&"R2"), "{labels:?}");
        assert!(labels.contains(&"net:nueva"), "{labels:?}");
        // R1 cambió de valor: su detalle lo dice.
        let r1 = s.changes.iter().find(|c| c.label == "R1").unwrap();
        assert!(r1.detail.contains("value: 1k → 2k"), "{}", r1.detail);
        let functional = report.changes.iter().filter(|c| !c.cosmetic).count();
        let cosmetic = report.changes.len() - functional;
        assert_eq!(s.metadata[0], (tr!("meta.diff"), tr!("diff.summary", functional = functional, cosmetic = cosmetic)));
        // Toda marca de componente tiene su recuadro para poder encuadrarla.
        assert!(s.annotations.iter().any(|a| a.label == "net:nueva".trim_start_matches("net:")));
    }

    #[test]
    fn missing_symbols_become_markers_and_a_notice() {
        let (rs, pdk) = resolve(A.as_bytes()).unwrap();
        let s = scene_from(&rs, &pdk);
        if rs.missing_symbols.is_empty() {
            return; // en un entorno con res.sym resuelto no hay nada que marcar
        }
        assert!(s.elements.iter().any(|e| e.layer() == MISSING_LAYER));
        assert!(s.notices.iter().any(|n| n.contains("riku doctor")), "{:?}", s.notices);
    }
}
