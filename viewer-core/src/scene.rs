//! Escena renderizable neutra.
//!
//! Dos tipos complementarios:
//!
//! - [`Scene`] — struct concreto que un backend puede retornar directamente.
//!   Simple, útil para casos donde no hace falta perezoso.
//! - [`RenderableScene`] — trait que permite implementaciones perezosas o
//!   streaming (un backend GDS con millones de polígonos puede generarlos bajo
//!   demanda por ventana visible sin materializar todo el `Vec`).
//!
//! Los consumidores (riku-gui) deben aceptar `Arc<dyn RenderableScene>` para
//! permitir ambos modos sin ramificar el código de UI.

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::bbox::BoundingBox;
use crate::element::{DrawElement, Layer};
use crate::paint::LayerPaint;
use crate::viewport::YAxis;

/// Sub-vista navegable de un archivo: una celda de un GDS, a futuro una página
/// o un nivel de jerarquía. Un backend que las soporte las lista en la escena
/// y carga una concreta con `ViewerBackend::load_entry`.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewEntry {
    /// Identificador estable dentro del archivo (en GDS, el nombre de celda).
    pub id: String,
    /// Raíz de la jerarquía (top cell): no la referencia ninguna otra entrada.
    pub is_root: bool,
    /// Ancho × alto en unidades de mundo, si se conoce.
    pub size: Option<(f64, f64)>,
}

/// Implementación trivial y eager: todos los elementos materializados en memoria.
#[derive(Debug, Clone)]
pub struct Scene {
    pub elements: Vec<DrawElement>,
    pub bbox: BoundingBox,
    /// Sentido del eje Y de las coordenadas de `elements`.
    pub y_axis: YAxis,
    /// Estilo por capa provisto por el backend. Capas ausentes usan la paleta
    /// neutral del consumidor.
    pub layers: BTreeMap<Layer, LayerPaint>,
    /// Resumen legible (clave, valor) para paneles de detalle: celda, PDK,
    /// conteos… El orden es el de presentación.
    pub metadata: Vec<(String, String)>,
    /// Sub-vistas del archivo de origen (vacío si no tiene). Van con la escena
    /// para no parsear el archivo dos veces.
    pub entries: Vec<ViewEntry>,
    /// Entrada que representa esta escena, si el archivo tiene varias.
    pub current_entry: Option<String>,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    pub fn new() -> Self {
        Self {
            elements: Vec::new(),
            bbox: BoundingBox::empty(),
            y_axis: YAxis::Down,
            layers: BTreeMap::new(),
            metadata: Vec::new(),
            entries: Vec::new(),
            current_entry: None,
        }
    }

    pub fn push(&mut self, el: DrawElement) {
        self.bbox.expand(&el.bounding_box());
        self.elements.push(el);
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }
}

/// Trait para escenas renderizables. `Send + Sync` permite pasarla entre hilos
/// (Tokio, rayon) y compartirla con el renderer de egui sin locks.
pub trait RenderableScene: Send + Sync {
    /// Bounding box global de la escena en coordenadas de mundo.
    fn bbox(&self) -> BoundingBox;

    /// Total de elementos (puede ser una estimación para escenas perezosas).
    fn len(&self) -> usize;

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Sentido del eje Y de las coordenadas de mundo. Por defecto Y-down.
    fn y_axis(&self) -> YAxis {
        YAxis::Down
    }

    /// Estilo de una capa, si el backend lo provee. Por defecto ninguno.
    fn layer_paint(&self, _layer: Layer) -> Option<&LayerPaint> {
        None
    }

    /// Capas con estilo propio, en el orden en que la UI debe listarlas.
    /// Por defecto ninguna.
    fn layer_list(&self) -> Vec<(Layer, &LayerPaint)> {
        Vec::new()
    }

    /// Resumen (clave, valor) para paneles de detalle. Por defecto vacío.
    fn metadata(&self) -> &[(String, String)] {
        &[]
    }

    /// Sub-vistas del archivo (celdas, páginas…). Por defecto ninguna.
    fn entries(&self) -> &[ViewEntry] {
        &[]
    }

    /// Sub-vista que muestra esta escena. Por defecto ninguna.
    fn current_entry(&self) -> Option<&str> {
        None
    }

    /// Enumera elementos visibles dentro de `viewport_bbox`. Los backends que
    /// quieran culling granular implementan esto; por defecto entrega todos.
    ///
    /// El callback debe retornar `true` para continuar o `false` para detener
    /// la iteración (útil para cancelación cooperativa desde el renderer).
    fn visit<'a>(&'a self, viewport_bbox: &BoundingBox, visitor: &mut dyn FnMut(&'a DrawElement) -> bool);
}

impl RenderableScene for Scene {
    fn bbox(&self) -> BoundingBox {
        self.bbox
    }

    fn len(&self) -> usize {
        self.elements.len()
    }

    fn y_axis(&self) -> YAxis {
        self.y_axis
    }

    fn layer_paint(&self, layer: Layer) -> Option<&LayerPaint> {
        self.layers.get(&layer)
    }

    fn layer_list(&self) -> Vec<(Layer, &LayerPaint)> {
        self.layers.iter().map(|(k, p)| (*k, p)).collect()
    }

    fn metadata(&self) -> &[(String, String)] {
        &self.metadata
    }

    fn entries(&self) -> &[ViewEntry] {
        &self.entries
    }

    fn current_entry(&self) -> Option<&str> {
        self.current_entry.as_deref()
    }

    fn visit<'a>(&'a self, viewport_bbox: &BoundingBox, visitor: &mut dyn FnMut(&'a DrawElement) -> bool) {
        // Si viewport_bbox esta vacio (sin info de culling, p.ej. primer
        // frame antes del auto-fit), entregamos TODOS los elementos en vez
        // de descartar silenciosamente — la condicion antigua hacia lo opuesto
        // y dejaba la escena en blanco hasta que el viewport se inicializaba.
        let cull = !viewport_bbox.is_empty();
        for el in &self.elements {
            if cull {
                let eb = el.bounding_box();
                if !intersects(&eb, viewport_bbox) {
                    continue;
                }
            }
            if !visitor(el) {
                return;
            }
        }
    }
}

fn intersects(a: &BoundingBox, b: &BoundingBox) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    a.min_x <= b.max_x && a.max_x >= b.min_x && a.min_y <= b.max_y && a.max_y >= b.min_y
}

/// Alias conveniente: handle compartido y mutable-safe para pasar escenas entre
/// tareas async y el renderer de UI.
pub type SceneHandle = Arc<dyn RenderableScene>;
