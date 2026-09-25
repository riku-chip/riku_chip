//! Adaptador `gds-renderer` ↔ `viewer-core`.
//!
//! Expone `GdsBackend`, que implementa `ViewerBackend` para que `riku-gui`
//! abra archivos `.gds` por la ruta neutra: `Library::from_bytes` →
//! `scene_from_cell` → conversión `DrawCommand → DrawElement` → `Scene`.
//!
//! La escena resultante es Y-up y trae un `LayerPaint` por cada
//! (layer, datatype): el campo `layer` de cada `DrawElement` es la clave de
//! ese estilo, no el numero de layer GDS crudo.

use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

use gdstk_rs::{Anchor, GdsTag, Library, Point2D};
use viewer_core::{
    backend::{BackendInfo, ViewerBackend},
    bbox::BoundingBox as VcBBox,
    element::{DrawElement, HAlign, Layer, VAlign},
    error::{Result as VcResult, ViewerError},
    paint::{LayerPaint, Rgba},
    scene::{Scene as VcScene, SceneHandle},
    viewport::YAxis,
    CancellationToken,
};

use crate::palette::{detect_pdk, layer_spec, LayerRole};
use crate::scene::DrawCommand;
use crate::style::{Pdk, RenderConfig};

pub struct GdsBackend;

impl GdsBackend {
    pub fn new() -> Self {
        Self
    }
}

impl Default for GdsBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Opacidad del relleno segun el rol de la capa. Dispositivo: se ve el color
/// y tambien lo que queda debajo. Pozo: apenas un tinte (cubre media celda).
/// Contorno: sin relleno (implantes, marcadores, pines, boundary).
fn fill_alpha(role: LayerRole) -> u8 {
    match role {
        LayerRole::Device => 90,
        LayerRole::Well => 28,
        LayerRole::Outline => 0,
    }
}

/// Asigna una clave `Layer` (u16) por cada (layer, datatype) distinto, en
/// orden de apilado del PDK (y luego (layer, datatype) para desempatar), asi
/// la lista de capas de la UI sale de abajo hacia arriba. `68/20` (met1) y
/// `68/16` (met1.pin) tienen claves y estilos distintos.
///
/// Se indexa por tupla porque `GdsTag` (struct compartido de cxx) no es `Ord`.
struct LayerKeys {
    keys: BTreeMap<(u32, u32), Layer>,
    pdk: Pdk,
}

impl LayerKeys {
    fn new(commands: &[DrawCommand], path_hint: Option<&str>) -> Self {
        let tags: BTreeSet<(u32, u32)> = commands.iter().map(|c| tag_tuple(command_tag(c))).collect();
        let as_gds: Vec<GdsTag> = tags.iter().map(|&t| gds_tag(t)).collect();
        let pdk = detect_pdk(path_hint, &as_gds);

        let mut ordered: Vec<(u32, u32)> = tags.into_iter().collect();
        ordered.sort_by_key(|&t| (layer_spec(gds_tag(t), pdk).rank, t));
        let keys = ordered
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, i.min(u16::MAX as usize) as Layer))
            .collect();
        Self { keys, pdk }
    }

    fn key(&self, tag: GdsTag) -> Layer {
        self.keys.get(&tag_tuple(tag)).copied().unwrap_or(u16::MAX)
    }

    fn paints(&self) -> BTreeMap<Layer, LayerPaint> {
        self.keys
            .iter()
            .map(|(&t, key)| (*key, layer_paint(gds_tag(t), self.pdk)))
            .collect()
    }
}

fn tag_tuple(tag: GdsTag) -> (u32, u32) {
    (tag.layer, tag.datatype)
}

fn gds_tag((layer, datatype): (u32, u32)) -> GdsTag {
    GdsTag { layer, datatype }
}

fn command_tag(cmd: &DrawCommand) -> GdsTag {
    match cmd {
        DrawCommand::Polygon { tag, .. } | DrawCommand::Label { tag, .. } => *tag,
    }
}

fn layer_paint(tag: GdsTag, pdk: Pdk) -> LayerPaint {
    let spec = layer_spec(tag, pdk);
    let c = spec.color;
    let stroke = Rgba::new(c.r, c.g, c.b, 255);
    let name = match spec.name {
        Some(name) => format!("{name} {}/{}", tag.layer, tag.datatype),
        None => format!("{}/{}", tag.layer, tag.datatype),
    };
    LayerPaint { name, fill: stroke.with_alpha(fill_alpha(spec.role)), stroke }
}

/// Alto de las etiquetas como fraccion del lado mayor de la cell. GDS no
/// guarda tamano de texto util (los LABEL suelen ser puntos), asi que se
/// escala con la geometria para que no tapen el layout.
const LABEL_FRACTION: f64 = 0.03;

fn label_size(bbox: &VcBBox) -> f64 {
    let side = bbox.width().max(bbox.height());
    if side.is_finite() && side > 0.0 {
        side * LABEL_FRACTION
    } else {
        1.0
    }
}

/// Anchor GDS → alineacion neutra. En Y-up "N" (norte) es el borde superior
/// del texto, que en pantalla sigue siendo `Top`.
fn anchor_align(anchor: Anchor) -> (HAlign, VAlign) {
    let h = match anchor {
        Anchor::NW | Anchor::W | Anchor::SW => HAlign::Start,
        Anchor::N | Anchor::O | Anchor::S => HAlign::Middle,
        Anchor::NE | Anchor::E | Anchor::SE => HAlign::End,
    };
    // El anchor nombra el punto del texto que cae en `origin`: NW = la esquina
    // superior izquierda del texto esta en origin.
    let v = match anchor {
        Anchor::NW | Anchor::N | Anchor::NE => VAlign::Top,
        Anchor::W | Anchor::O | Anchor::E => VAlign::Middle,
        Anchor::SW | Anchor::S | Anchor::SE => VAlign::Bottom,
    };
    (h, v)
}

fn command_to_element(cmd: &DrawCommand, keys: &LayerKeys, text_size: f64) -> Option<DrawElement> {
    match cmd {
        DrawCommand::Polygon { tag, points } => Some(DrawElement::Polygon {
            points: points.iter().map(|p: &Point2D| (p.x, p.y)).collect(),
            layer: keys.key(*tag),
            filled: true,
        }),
        DrawCommand::Label { tag, text, origin, anchor } => {
            let (h_align, v_align) = anchor_align(*anchor);
            Some(DrawElement::Text {
                x: origin.x,
                y: origin.y,
                content: text.clone(),
                size: text_size,
                angle_deg: 0.0,
                h_align,
                v_align,
                layer: keys.key(*tag),
            })
        }
    }
}

fn vc_scene_from_cell(cell: &gdstk_rs::Cell<'_>, path_hint: Option<&str>) -> VcScene {
    let cfg = RenderConfig::default();
    let render_scene = crate::compat::scene_from_cell(cell, &cfg);
    let keys = LayerKeys::new(&render_scene.commands, path_hint);

    let mut scene = VcScene::new();
    // GDS usa la convencion matematica: Y crece hacia arriba.
    scene.y_axis = YAxis::Up;
    scene.layers = keys.paints();
    // Sembrar el bbox con el de la cell aunque algún DrawCommand no contribuya
    // (Scene::push lo expandirá igualmente con cada elemento).
    let cb = cell.bbox();
    if cb.min_x.is_finite()
        && cb.min_y.is_finite()
        && cb.max_x.is_finite()
        && cb.max_y.is_finite()
    {
        scene.bbox.expand(&VcBBox {
            min_x: cb.min_x,
            min_y: cb.min_y,
            max_x: cb.max_x,
            max_y: cb.max_y,
        });
    }

    // Orden de pintado: poligonos por apilado (las claves ya siguen el rank)
    // y los textos al final para que queden encima. Sort estable: dentro de
    // una capa se conserva el orden del archivo.
    let mut commands: Vec<&DrawCommand> = render_scene.commands.iter().collect();
    commands.sort_by_key(|c| (matches!(c, DrawCommand::Label { .. }), keys.key(command_tag(c))));

    let text_size = label_size(&scene.bbox);
    let (mut polygons, mut labels) = (0usize, 0usize);
    for cmd in commands {
        match cmd {
            DrawCommand::Polygon { .. } => polygons += 1,
            DrawCommand::Label { .. } => labels += 1,
        }
        if let Some(el) = command_to_element(cmd, &keys, text_size) {
            scene.push(el);
        }
    }

    scene.metadata = vec![
        ("Cell".into(), cell.name().to_string()),
        ("PDK".into(), pdk_name(keys.pdk).into()),
        ("Polígonos".into(), polygons.to_string()),
        ("Labels".into(), labels.to_string()),
        ("Capas".into(), scene.layers.len().to_string()),
        (
            "Tamaño".into(),
            format!("{:.3} × {:.3} µm", scene.bbox.width(), scene.bbox.height()),
        ),
    ];
    scene
}

fn pdk_name(pdk: Pdk) -> &'static str {
    match pdk {
        Pdk::Sky130 => "SKY130",
        Pdk::Gf180 => "GF180MCU",
        Pdk::Ihp => "IHP SG13G2",
        Pdk::Generic => "genérico",
    }
}

#[async_trait]
impl ViewerBackend for GdsBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: "gds",
            version: env!("CARGO_PKG_VERSION"),
            extensions: &["gds"],
        }
    }

    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool {
        if let Some(p) = path_hint {
            if p.to_ascii_lowercase().ends_with(".gds") {
                return true;
            }
        }
        // GDSII magic: HEADER record (len=6, type=0x0002) en big-endian.
        content.len() >= 4
            && content[0] == 0x00
            && content[1] == 0x06
            && content[2] == 0x00
            && content[3] == 0x02
    }

    async fn load(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        if token.is_cancelled() {
            return Err(ViewerError::Cancelled);
        }
        let scene = tokio::task::spawn_blocking(move || -> VcResult<VcScene> {
            let lib = Library::from_bytes(&content)
                .map_err(|e| ViewerError::Parse(format!("GDSII parse: {e}")))?;

            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }

            // Selector determinista de top-cell (alfabetico en empate).
            // Library vacia o ciclica -> escena vacia (no es error).
            let Some(cell) = crate::select_top_cell(&lib) else {
                return Ok(VcScene::default());
            };

            Ok(vc_scene_from_cell(&cell, path_hint.as_deref()))
        })
        .await??;

        Ok(Arc::new(scene) as SceneHandle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof_lib_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("external")
            .join("gdstk")
            .join("tests")
            .join("proof_lib.gds");
        std::fs::read(&path)
            .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", path.display()))
    }

    #[test]
    fn accepts_gds_extension() {
        let b = GdsBackend::new();
        assert!(b.accepts(b"", Some("foo.gds")));
        assert!(b.accepts(b"", Some("path/to/Bar.GDS")));
        assert!(!b.accepts(b"", Some("foo.sch")));
    }

    #[test]
    fn accepts_gds_magic_without_hint() {
        let b = GdsBackend::new();
        let bytes = [0x00u8, 0x06, 0x00, 0x02, 0x01, 0x00];
        assert!(b.accepts(&bytes, None));
        assert!(!b.accepts(b"NOT_A_GDS", None));
    }

    #[tokio::test]
    async fn load_proof_lib_returns_nonempty_scene() {
        let bytes = proof_lib_bytes();
        let backend = GdsBackend::new();
        let handle = backend
            .load(bytes, None, CancellationToken::new())
            .await
            .expect("load proof_lib");
        assert!(handle.len() > 0, "esperaba elementos, got {}", handle.len());
        assert!(!handle.bbox().is_empty(), "bbox no debe estar vacío");
    }

    #[tokio::test]
    async fn load_invalid_returns_parse_error() {
        let backend = GdsBackend::new();
        let res = backend
            .load(b"NOT_A_GDS_FILE".to_vec(), None, CancellationToken::new())
            .await;
        match res {
            Err(ViewerError::Parse(_)) => {}
            Err(e) => panic!("esperaba ViewerError::Parse, got {e:?}"),
            Ok(_) => panic!("esperaba error, got Ok"),
        }
    }

    #[tokio::test]
    async fn load_respects_pre_cancelled_token() {
        let backend = GdsBackend::new();
        let token = CancellationToken::new();
        token.cancel();
        let res = backend.load(proof_lib_bytes(), None, token).await;
        match res {
            Err(ViewerError::Cancelled) => {}
            Err(e) => panic!("esperaba ViewerError::Cancelled, got {e:?}"),
            Ok(_) => panic!("esperaba error, got Ok"),
        }
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("leer {}: {e}", path.display()))
    }

    async fn load(bytes: Vec<u8>, hint: Option<&str>) -> SceneHandle {
        GdsBackend::new()
            .load(bytes, hint.map(str::to_string), CancellationToken::new())
            .await
            .expect("load")
    }

    #[tokio::test]
    async fn scene_is_y_up_and_every_element_has_paint() {
        let handle = load(proof_lib_bytes(), None).await;
        assert_eq!(handle.y_axis(), YAxis::Up);
        let mut missing = 0;
        handle.visit(&VcBBox::empty(), &mut |el| {
            match handle.layer_paint(el.layer()) {
                Some(p) => assert!(p.fill.a < 255 && p.stroke.a == 255, "relleno translucido, borde opaco"),
                None => missing += 1,
            }
            true
        });
        assert_eq!(missing, 0, "elementos sin LayerPaint");
    }

    #[tokio::test]
    async fn datatype_is_part_of_layer_style() {
        let a = load(fixture("datatype_a.gds"), None).await;
        let b = load(fixture("datatype_b.gds"), None).await;
        let name = |h: &SceneHandle| {
            let mut n = String::new();
            h.visit(&VcBBox::empty(), &mut |el| {
                n = h.layer_paint(el.layer()).unwrap().name.clone();
                false
            });
            n
        };
        assert_eq!(name(&a), "1/0");
        assert_eq!(name(&b), "1/1");
    }

    #[test]
    fn anchor_maps_to_alignment() {
        assert_eq!(anchor_align(Anchor::O), (HAlign::Middle, VAlign::Middle));
        assert_eq!(anchor_align(Anchor::NW), (HAlign::Start, VAlign::Top));
        assert_eq!(anchor_align(Anchor::SE), (HAlign::End, VAlign::Bottom));
    }

    #[tokio::test]
    async fn labels_are_painted_after_polygons_and_metadata_is_set() {
        let handle = load(proof_lib_bytes(), None).await;
        let mut seen_text = false;
        let mut polygon_after_text = false;
        handle.visit(&VcBBox::empty(), &mut |el| {
            match el {
                DrawElement::Text { .. } => seen_text = true,
                _ if seen_text => polygon_after_text = true,
                _ => {}
            }
            true
        });
        assert!(!polygon_after_text, "los textos deben ir al final");
        let meta = handle.metadata();
        assert!(meta.iter().any(|(k, _)| k == "Cell"));
        let capas: usize = meta.iter().find(|(k, _)| k == "Capas").unwrap().1.parse().unwrap();
        assert_eq!(handle.layer_list().len(), capas);
    }

    #[tokio::test]
    async fn sky130_path_hint_names_layers() {
        // El fixture usa layer 1 (no es de SKY130): sin match cae al nombre crudo,
        // pero el hint no debe romper la carga.
        let h = load(fixture("datatype_a.gds"), Some("/foss/pdks/sky130A/x.gds")).await;
        assert!(!h.is_empty());
    }
}
