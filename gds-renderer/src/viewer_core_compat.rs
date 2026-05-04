//! Adaptador `gds-renderer` ↔ `viewer-core`.
//!
//! Expone `GdsBackend`, que implementa `ViewerBackend` para que `riku-gui`
//! abra archivos `.gds` por la ruta neutra: `Library::from_bytes` →
//! `scene_from_cell` → conversión `DrawCommand → DrawElement` → `Scene`.

use async_trait::async_trait;
use std::sync::Arc;

use gdstk_rs::{GdsTag, Library, Point2D};
use viewer_core::{
    backend::{BackendInfo, ViewerBackend},
    bbox::BoundingBox as VcBBox,
    element::{DrawElement, HAlign, Layer, VAlign},
    error::{Result as VcResult, ViewerError},
    scene::{Scene as VcScene, SceneHandle},
    CancellationToken,
};

use crate::scene::DrawCommand;
use crate::style::RenderConfig;

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

/// GDS layer (u32) → viewer-core `Layer` (u16). PDKs estándar usan layers
/// < 256; truncamos con saturación. Polígonos con layer > 65535 caen en
/// `u16::MAX` (preferible a perderlos).
fn tag_to_layer(tag: GdsTag) -> Layer {
    tag.layer.min(u16::MAX as u32) as u16
}

fn command_to_element(cmd: &DrawCommand) -> Option<DrawElement> {
    match cmd {
        DrawCommand::Polygon { tag, points } => Some(DrawElement::Polygon {
            points: points.iter().map(|p: &Point2D| (p.x, p.y)).collect(),
            layer: tag_to_layer(*tag),
            filled: true,
        }),
        DrawCommand::Rect { tag, bbox } => Some(DrawElement::Rect {
            x: bbox.min_x,
            y: bbox.min_y,
            w: bbox.max_x - bbox.min_x,
            h: bbox.max_y - bbox.min_y,
            layer: tag_to_layer(*tag),
            filled: true,
        }),
        DrawCommand::Path {
            tag,
            points,
            closed,
        } => Some(DrawElement::Polygon {
            points: points.iter().map(|p| (p.x, p.y)).collect(),
            layer: tag_to_layer(*tag),
            filled: *closed,
        }),
        DrawCommand::Label { tag, text, origin } => Some(DrawElement::Text {
            x: origin.x,
            y: origin.y,
            content: text.clone(),
            size: 1.0,
            angle_deg: 0.0,
            h_align: HAlign::Start,
            v_align: VAlign::Top,
            layer: tag_to_layer(*tag),
        }),
    }
}

fn vc_scene_from_cell(cell: &gdstk_rs::Cell<'_>) -> VcScene {
    let cfg = RenderConfig::default();
    let render_scene = crate::compat::scene_from_cell(cell, &cfg);
    let mut scene = VcScene::new();
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
    for cmd in &render_scene.commands {
        if let Some(el) = command_to_element(cmd) {
            scene.push(el);
        }
    }
    scene
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
        _path_hint: Option<String>,
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

            Ok(vc_scene_from_cell(&cell))
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
}
