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
use crate::diff::{Annotation, ChangeItem, ChangeKind};
use crate::element::{DrawElement, Layer};
use crate::index::SceneIndex;
use crate::paint::LayerPaint;
use crate::viewport::YAxis;

/// Sub-vista navegable de un archivo: una celda de un GDS, a futuro una página
/// o un nivel de jerarquía. Un backend que las soporte las lista en la escena
/// y carga una concreta con `ViewerBackend::load_entry`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ViewEntry {
    /// Identificador estable dentro del archivo (en GDS, el nombre de celda).
    pub id: String,
    /// Raíz de la jerarquía (top cell): no la referencia ninguna otra entrada.
    pub is_root: bool,
    /// Ancho × alto en unidades de mundo, si se conoce.
    pub size: Option<(f64, f64)>,
    /// En escenas de diff: cómo cambió esta entrada entre las dos versiones
    /// (`None` = sin cambios o escena que no es diff).
    pub change: Option<ChangeKind>,
    /// En escenas de diff: el id que tenía en la versión "antes", si se
    /// renombró (así se abre la misma entrada en la otra versión).
    pub renamed_from: Option<String>,
}

/// Cómo se dibujan los `DrawElement::Text` de una escena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextStyle {
    /// Etiquetas: tamaño fijo en pantalla, con pastilla y sin solaparse
    /// (nombres de pines en un layout).
    #[default]
    Labels,
    /// Texto que es parte del dibujo: escala con el zoom y respeta ángulo y
    /// alineación (textos de un esquemático).
    Drawn,
}

/// Una red bajo un punto de la escena: su nombre y sus polígonos (en
/// coordenadas de mundo), para mostrarla y resaltarla entera.
#[derive(Debug, Clone, PartialEq)]
pub struct NetHit {
    pub name: String,
    pub outline: Vec<Vec<(f64, f64)>>,
}

/// Qué red hay en un punto. Un backend que conoce la conectividad de lo que
/// dibuja (un layout con las reglas de su PDK) la pone en la escena.
pub trait NetProbe: Send + Sync + std::fmt::Debug {
    /// La red en `(x, y)` (coordenadas de mundo). `layer`: la capa del
    /// elemento bajo el cursor, para elegir entre capas superpuestas (el
    /// metal, no el pozo de abajo).
    fn at(&self, x: f64, y: f64, layer: Option<Layer>) -> Option<NetHit>;
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
    /// Cambios respecto a otra versión (solo en escenas de diff).
    pub changes: Vec<ChangeItem>,
    /// Unidad de las coordenadas de mundo (`"µm"` en GDS), para mostrar
    /// medidas. `None` = unidades abstractas.
    pub world_unit: Option<String>,
    /// Cómo dibujar los textos de `elements`.
    pub text_style: TextStyle,
    /// Diff: elementos de la versión anterior que ya no están donde estaban
    /// (movidos o eliminados). Se dibujan atenuados debajo de `elements`.
    pub ghost: Vec<DrawElement>,
    /// Diff: marcas de cada cambio (recuadros, nets resaltadas), encima.
    pub annotations: Vec<Annotation>,
    /// Avisos para el usuario sobre esta escena (p. ej. símbolos que no se
    /// pudieron resolver).
    pub notices: Vec<String>,
    /// Índice espacial (culling, nivel de detalle, relleno precalculado).
    /// `None` hasta llamar a [`Scene::build_index`]; `push` lo descarta.
    pub index: Option<Arc<SceneIndex>>,
    /// Redes de lo que se dibuja, si el backend las conoce.
    pub nets: Option<Arc<dyn NetProbe>>,
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
            changes: Vec::new(),
            world_unit: None,
            text_style: TextStyle::Labels,
            ghost: Vec::new(),
            annotations: Vec::new(),
            notices: Vec::new(),
            index: None,
            nets: None,
        }
    }

    pub fn push(&mut self, el: DrawElement) {
        self.index = None;
        self.bbox.expand(&el.bounding_box());
        self.elements.push(el);
    }

    pub fn len(&self) -> usize {
        self.elements.len()
    }

    pub fn is_empty(&self) -> bool {
        self.elements.is_empty()
    }

    /// Arma el índice de los elementos actuales (ver [`SceneIndex`]). Los
    /// backends lo llaman al terminar la escena, fuera del hilo de la UI.
    pub fn build_index(&mut self) {
        // Capas de solo contorno (relleno transparente): la pirámide marca sus bordes.
        let layers = &self.layers;
        let outline = |l: Layer| layers.get(&l).is_some_and(|p| p.fill.a == 0);
        self.index = Some(Arc::new(SceneIndex::build(&self.elements, &self.bbox, &outline)));
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

    /// Cambios respecto a otra versión (escenas de diff). Por defecto ninguno.
    fn changes(&self) -> &[ChangeItem] {
        &[]
    }

    /// Unidad de las coordenadas de mundo, si el backend la conoce.
    fn world_unit(&self) -> Option<&str> {
        None
    }

    /// Cómo dibujar los textos. Por defecto como etiquetas.
    fn text_style(&self) -> TextStyle {
        TextStyle::Labels
    }

    /// Diff: elementos de la versión anterior a dibujar atenuados. Por
    /// defecto ninguno.
    fn ghost(&self) -> &[DrawElement] {
        &[]
    }

    /// Diff: marcas de cada cambio. Por defecto ninguna.
    fn annotations(&self) -> &[Annotation] {
        &[]
    }

    /// Avisos sobre la escena. Por defecto ninguno.
    fn notices(&self) -> &[String] {
        &[]
    }

    /// Enumera elementos visibles dentro de `viewport_bbox`. Los backends que
    /// quieran culling granular implementan esto; por defecto entrega todos.
    ///
    /// El callback debe retornar `true` para continuar o `false` para detener
    /// la iteración (útil para cancelación cooperativa desde el renderer).
    fn visit<'a>(&'a self, viewport_bbox: &BoundingBox, visitor: &mut dyn FnMut(&'a DrawElement) -> bool);

    /// Índice espacial y los elementos a los que se refieren sus índices, si
    /// la escena lo tiene (ver [`Scene::build_index`]). Con él, el consumidor
    /// puede consultar solo lo visible y usar el relleno precalculado. Por
    /// defecto `None`: se usa [`Self::visit`].
    fn indexed(&self) -> Option<(&SceneIndex, &[DrawElement])> {
        None
    }

    /// La red en un punto, si el backend conoce la conectividad (ver
    /// [`NetProbe`]). Por defecto ninguna.
    fn net_at(&self, _x: f64, _y: f64, _layer: Option<Layer>) -> Option<NetHit> {
        None
    }
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

    fn changes(&self) -> &[ChangeItem] {
        &self.changes
    }

    fn world_unit(&self) -> Option<&str> {
        self.world_unit.as_deref()
    }

    fn text_style(&self) -> TextStyle {
        self.text_style
    }

    fn ghost(&self) -> &[DrawElement] {
        &self.ghost
    }

    fn annotations(&self) -> &[Annotation] {
        &self.annotations
    }

    fn notices(&self) -> &[String] {
        &self.notices
    }

    fn indexed(&self) -> Option<(&SceneIndex, &[DrawElement])> {
        self.index.as_deref().map(|i| (i, &self.elements[..]))
    }

    fn net_at(&self, x: f64, y: f64, layer: Option<Layer>) -> Option<NetHit> {
        self.nets.as_ref()?.at(x, y, layer)
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

#[cfg(test)]
mod overlay_tests {
    use super::*;
    use crate::diff::AnnotationShape;

    /// Escena mínima que solo implementa lo obligatorio del trait.
    struct Bare;
    impl RenderableScene for Bare {
        fn bbox(&self) -> BoundingBox {
            BoundingBox::empty()
        }
        fn len(&self) -> usize {
            0
        }
        fn visit<'a>(&'a self, _: &BoundingBox, _: &mut dyn FnMut(&'a DrawElement) -> bool) {}
    }

    #[test]
    fn overlays_are_optional_for_backends() {
        let b = Bare;
        assert_eq!(b.text_style(), TextStyle::Labels);
        assert!(b.ghost().is_empty() && b.annotations().is_empty() && b.notices().is_empty());
    }

    #[test]
    fn scene_exposes_its_overlays() {
        let mut s = Scene::new();
        s.text_style = TextStyle::Drawn;
        s.ghost.push(DrawElement::Line { x1: 0.0, y1: 0.0, x2: 1.0, y2: 1.0, layer: 1 });
        s.annotations.push(Annotation {
            kind: ChangeKind::Added,
            cosmetic: false,
            moved: false,
            label: "R1".into(),
            shape: AnnotationShape::Box(BoundingBox::from_points((0.0, 0.0), (1.0, 1.0))),
        });
        s.notices.push("1 símbolo sin resolver".into());
        let h: &dyn RenderableScene = &s;
        assert_eq!(h.text_style(), TextStyle::Drawn);
        assert_eq!((h.ghost().len(), h.annotations().len(), h.notices().len()), (1, 1, 1));
        // Los fantasmas no cuentan como elementos de la escena.
        assert!(h.is_empty());
    }
}

#[cfg(test)]
mod net_tests {
    use super::*;

    /// Una red cuadrada de 0 a 1, solo en la capa 7.
    #[derive(Debug)]
    struct Square;

    impl NetProbe for Square {
        fn at(&self, x: f64, y: f64, layer: Option<Layer>) -> Option<NetHit> {
            let inside = (0.0..=1.0).contains(&x) && (0.0..=1.0).contains(&y);
            (inside && layer == Some(7)).then(|| NetHit { name: "Y".into(), outline: vec![vec![(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]] })
        }
    }

    #[test]
    fn a_scene_answers_net_at_with_its_probe() {
        let mut scene = Scene::new();
        assert_eq!(scene.net_at(0.5, 0.5, Some(7)), None, "sin sonda, ninguna");
        scene.nets = Some(Arc::new(Square));
        assert_eq!(scene.net_at(0.5, 0.5, Some(7)).map(|h| h.name), Some("Y".into()));
        assert_eq!(scene.net_at(0.5, 0.5, Some(3)), None, "otra capa");
        assert_eq!(scene.net_at(2.0, 0.5, Some(7)), None);
        // Un clon comparte la sonda.
        assert!(scene.clone().net_at(0.5, 0.5, Some(7)).is_some());
    }
}
