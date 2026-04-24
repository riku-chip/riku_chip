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

use std::sync::Arc;

use crate::bbox::BoundingBox;
use crate::element::DrawElement;

/// Implementación trivial y eager: todos los elementos materializados en memoria.
#[derive(Debug, Clone)]
pub struct Scene {
    pub elements: Vec<DrawElement>,
    pub bbox: BoundingBox,
}

impl Default for Scene {
    fn default() -> Self {
        Self::new()
    }
}

impl Scene {
    pub fn new() -> Self {
        Self { elements: Vec::new(), bbox: BoundingBox::empty() }
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

    fn visit<'a>(&'a self, viewport_bbox: &BoundingBox, visitor: &mut dyn FnMut(&'a DrawElement) -> bool) {
        for el in &self.elements {
            let eb = el.bounding_box();
            if viewport_bbox.is_empty() || !intersects(&eb, viewport_bbox) {
                continue;
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
