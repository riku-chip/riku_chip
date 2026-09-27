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
    DrawElement as Vc, HAlign, Layer, LayerPaint, Rgba, Scene, SceneHandle, TextStyle, VAlign, ViewerBackend,
    ViewerError, YAxis,
};
use xschem_viewer::{DrawElement as X, HAlign as XH, LineDirection, ResolvedScene, VBaseline};

use super::xschem::{is_xschem, render_options_for, XschemModule};
use super::xschem_pdk::{installed_pdks, pdk_root, PdkSource};
use crate::core::domain::models::{Change, ChangeKind, Element, FileChange};
use riku_kernel::{DiffOptions, FormatModule};

/// Capa sintética de los marcadores de símbolo faltante.
const MISSING_LAYER: Layer = 100;
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
        _path_hint: Option<String>,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        run_blocking(token, move || {
            let (rs, pdk) = resolve(&content)?;
            Ok(scene_from(&rs, &pdk))
        })
        .await
    }

    async fn load_diff(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        _entry: Option<String>,
        token: CancellationToken,
    ) -> viewer_core::Result<SceneHandle> {
        run_blocking(token, move || {
            let (b, pdk) = resolve(&after)?;
            let a = if before.is_empty() { None } else { Some(resolve(&before)?.0) };
            let path = path_hint.unwrap_or_else(|| "archivo.sch".into());
            let report = XschemModule::new().diff(&before, &after, &path, &DiffOptions::default());
            Ok(diff_scene(a.as_ref(), &b, &pdk, &report))
        })
        .await
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
fn resolve(content: &[u8]) -> viewer_core::Result<(ResolvedScene, PdkSource)> {
    let text = std::str::from_utf8(content).map_err(|e| ViewerError::Parse(format!("no es UTF-8: {e}")))?;
    let parsed = xschem_viewer::parser::parse(text).map_err(|e| ViewerError::Parse(e.to_string()))?;
    let (opts, pdk) = render_options_for(text);
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
    scene.metadata = vec![
        ("Elementos".into(), rs.elements.len().to_string()),
        ("Wires".into(), rs.wires.len().to_string()),
    ];
    match pdk {
        PdkSource::Env { path, extra } if extra.is_empty() => scene.metadata.push(("PDK".into(), pdk_name(path))),
        PdkSource::Env { path, extra } => {
            let active = pdk_name(path);
            let others: Vec<&str> = extra.iter().map(|(n, _)| n.as_str()).collect();
            scene.metadata.push(("PDK".into(), format!("{active} + {}", others.join(" + "))));
            scene.notices.push(format!(
                "El PDK activo es {active}, pero este esquemático usa símbolos de {}: se tomaron de ahí. \
                 Para trabajar con él: sak-pdk {}",
                others.join(", "),
                others[0]
            ));
        }
        PdkSource::Detected(found) => {
            let names: Vec<&str> = found.iter().map(|(n, _)| n.as_str()).collect();
            let main = names[0];
            scene.metadata.push(("PDK".into(), format!("{} (detectado)", names.join(" + "))));
            let which = if names.len() == 1 {
                format!("se usó {main}, que tiene los símbolos de este esquemático")
            } else {
                format!("el esquemático usa símbolos de varios PDKs: {}", names.join(", "))
            };
            scene.notices.push(format!("$PDK no está definida: {which}. Para fijarlo: sak-pdk {main} (o export PDK={main})"));
        }
        PdkSource::Missing(_) => {}
    }
    if !rs.missing_symbols.is_empty() {
        scene.metadata.push(("Símbolos sin resolver".into(), rs.missing_symbols.len().to_string()));
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
                Some(first) => format!(
                    "Elige el PDK del diseño, por ejemplo: sak-pdk {first} (instalados: {}).",
                    installed.join(", ")
                ),
                None => "Define $PDK_ROOT y $PDK (p. ej. PDK_ROOT=/foss/pdks PDK=sky130A).".to_string(),
            };
            format!("{reason}. {hint}")
        }
        found => format!(
            "No están en {} ni en las rutas de .xschemrc.",
            found.paths().iter().map(|p| p.display().to_string()).collect::<Vec<_>>().join(", ")
        ),
    };
    format!(
        "Faltan {} símbolos ({list}); se dibujan como marcadores rojos. {cause} Diagnóstico: riku doctor.",
        missing.len()
    )
}

/// Colores de Xschem (tema oscuro; el visor los adapta al tema claro).
fn layer_paint(layer: Layer) -> LayerPaint {
    let (name, (r, g, b)) = match layer {
        1 => ("wires", (100, 180, 255)),
        2 => ("componentes", (200, 200, 100)),
        3 => ("texto", (180, 180, 180)),
        4 => ("pines", (100, 220, 100)),
        MISSING_LAYER => ("símbolo faltante", (200, 80, 80)),
        _ => ("", (160, 160, 160)),
    };
    let name = if name.is_empty() { format!("capa {layer}") } else { name.to_string() };
    LayerPaint { name, fill: Rgba::new(r, g, b, 77), stroke: Rgba::new(r, g, b, 255) }
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
        if c.cosmetic { cosmetic += 1 } else { functional += 1 }
        let Some((annotation, item)) = mark(a, b, c) else { continue };
        if let Some(an) = annotation {
            scene.annotations.push(an);
        }
        scene.changes.push(item);
    }
    let summary = if functional + cosmetic == 0 {
        "sin cambios".to_string()
    } else {
        format!("{functional} funcionales · {cosmetic} cosméticos")
    };
    scene.metadata.insert(0, ("Diff".into(), summary));
    scene
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
            let detail = if moved_only { "trasladado".to_string() } else { param_changes(c) };
            let label = if c.kind == ChangeKind::Renamed { format!("{label} (renombrado)") } else { label };
            Some((annotation, ChangeItem { kind, label, detail, bbox, cosmetic: c.cosmetic }))
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
            let item = ChangeItem { kind, label: format!("net:{name}"), detail: String::new(), bbox, cosmetic: c.cosmetic };
            Some((annotation, item))
        }
        Element::Whole => Some((
            None,
            ChangeItem {
                kind: VcKind::Modified,
                label: "todo el esquemático (Move All)".into(),
                detail: "reorganización cosmética".into(),
                bbox: None,
                cosmetic: true,
            },
        )),
        _ => None,
    }
}

/// `W: 1u → 2u · L: …` con los parámetros que cambiaron (sin posición).
fn param_changes(c: &Change) -> String {
    let changed: BTreeMap<&str, String> = c
        .params()
        .filter(|d| d.changed())
        .map(|d| {
            let show = |v: &Option<riku_kernel::Value>| v.as_ref().map_or("—".to_string(), |v| v.to_string());
            (d.key.as_str(), format!("{} → {}", show(&d.before), show(&d.after)))
        })
        .collect();
    changed.iter().map(|(k, v)| format!("{k}: {v}")).collect::<Vec<_>>().join(" · ")
}

/// Distancia bajo la cual dos coordenadas son la misma.
const NEAR: f64 = 0.001;

fn near(a: f64, b: f64) -> bool {
    (a - b).abs() < NEAR
}

#[cfg(test)]
mod tests {
    use super::*;

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
        let a = format!("{head}N 0 0 100 0 {{lab=x}}
N 0 50 100 50 {{lab=old}}
N 3 7 3 90 {{lab=y}}
");
        // El primero invertido, el tercero corrido menos que la tolerancia.
        let b = format!("{head}N 100 0 0 0 {{lab=x}}
N 3.0004 7 3 90 {{lab=y}}
");
        let (ra, (rb, pdk)) = (resolve(a.as_bytes()).unwrap().0, resolve(b.as_bytes()).unwrap());
        let report = XschemModule::new().diff(a.as_bytes(), b.as_bytes(), "t.sch", &DiffOptions::default());
        let s = diff_scene(Some(&ra), &rb, &pdk, &report);
        let ghosts: Vec<f64> = s.ghost.iter().filter_map(|g| match g { Vc::Line { y1, .. } => Some(*y1), _ => None }).collect();
        assert_eq!(ghosts, vec![50.0], "solo el wire que ya no está");
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
        assert!(s.metadata[0].0 == "Diff" && s.metadata[0].1.contains("funcionales"));
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
        assert!(s.notices.iter().any(|n| n.contains("Faltan") && n.contains("riku doctor")), "{:?}", s.notices);
    }
}
