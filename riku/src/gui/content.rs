//! Qué muestra el lienzo: la pantalla de inicio, una escena de un backend
//! (esquemático, layout) o una vista de formas de onda. Un solo `enum`: antes eran dos
//! `Option` independientes y podían quedar los dos llenos (una vista de ondas
//! tapando el diff pedido, B7).

use crate::gui::tr;
use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use viewer_core::{backend::ViewerBackend, bbox::BoundingBox, element::Layer, scene::SceneHandle, viewport::Viewport, DiffFiles};

use crate::gui::loader::LoadedScene;
use crate::gui::motion::{Inertia, ViewAnimation};
#[cfg(feature = "spice")]
use crate::gui::wave_view::WaveView;

/// Vista de un diff: la diferencia o una de las dos versiones.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DiffTab {
    Before,
    After,
    Diff,
}

impl DiffTab {
    /// Nombre visible de la vista, en el idioma de la interfaz.
    pub(crate) fn label(self) -> String {
        match self {
            DiffTab::Diff => tr!("tab.diff"),
            DiffTab::Before => tr!("tab.before"),
            DiffTab::After => tr!("tab.after"),
        }
    }
}

/// De qué diff viene lo que se ve (la ruta sobre el lienzo y el panel de
/// vistas). Viaja con la carga: se fija cuando llega, junto con lo que se
/// muestra.
#[derive(Clone, Debug)]
pub(crate) struct DiffContext {
    /// Vacío: el commit inicial, que se compara contra nada.
    pub commit_a: String,
    pub commit_b: String,
    pub file: PathBuf,
    /// Se abrió desde el panel History (la ruta sobre el lienzo lo dice).
    pub from_history: bool,
}

/// Qué produce una carga via backend. En modo diff, `source` (en
/// [`SceneState`]) es la versión "después" y `before` la "antes"; `files`
/// da los otros archivos de cada commit (las sub-celdas de un `.mag`).
#[derive(Clone)]
pub(crate) enum LoadKind {
    Single,
    /// `renamed`: entradas renombradas (antes, después), para abrir la misma
    /// en cada pestaña; las da la escena de diff.
    Diff {
        before: Arc<Vec<u8>>,
        files: DiffFiles,
        tab: DiffTab,
        renamed: Arc<[(String, String)]>,
    },
}

/// Un paso atrás de la navegación dentro de un archivo: de qué entrada
/// (celda, sub-esquemático) se vino y con qué vista.
#[derive(Clone, Debug)]
pub(crate) struct BackStep {
    /// `None`: la entrada por defecto (la raíz).
    pub entry: Option<String>,
    pub viewport: Viewport,
}

/// Una escena cargada via `ViewerBackend` (todos los formatos salvo las
/// ondas) con su vista y lo que el usuario eligió sobre ella.
pub(crate) struct SceneState {
    pub scene: SceneHandle,
    pub viewport: Viewport,
    /// Backend que produjo la escena y bytes/ruta de origen: permiten cargar
    /// otra sub-vista (celda) sin volver a leer el disco.
    pub backend: Arc<dyn ViewerBackend>,
    pub source: Arc<Vec<u8>>,
    pub path: String,
    /// Encuadrar la escena en el próximo frame (al cargar o con "Fit"). El
    /// fit necesita el tamaño real del panel, que solo se conoce al pintar.
    pub needs_fit: bool,
    /// Tamaño del lienzo en el último encuadre automático; `None` si el
    /// usuario movió la vista a mano (entonces no se re-encuadra solo).
    pub fitted_size: Option<egui::Vec2>,
    /// Capas ocultas desde el panel de detalles, por **nombre** (`"met1 68/20"`):
    /// las claves numéricas cambian entre celdas, el nombre no.
    pub hidden_layers: HashSet<String>,
    /// Capa resaltada fija (clic en su nombre), también por nombre.
    pub layer_focus: Option<String>,
    /// Red resaltada (clic en un polígono de un layout): su nombre y sus
    /// polígonos. Se suelta con Esc o con otro clic.
    pub net_focus: Option<viewer_core::NetHit>,
    /// Capa bajo el puntero en el panel Capas y en la leyenda: resalta
    /// mientras el puntero está encima. Cada uno pone la suya en cada cuadro.
    pub layer_hover_panel: Option<String>,
    pub layer_hover_legend: Option<String>,
    /// Buscador y filtro del selector de celdas.
    pub entry_query: String,
    pub only_roots: bool,
    /// En diff: listar solo celdas con cambios.
    pub only_changed: bool,
    /// Qué se carga: un archivo suelto o un diff entre dos versiones.
    pub kind: LoadKind,
    /// Zona a encuadrar en el próximo frame (clic en un cambio).
    pub focus: Option<BoundingBox>,
    /// El próximo encuadre lo pidió el usuario (Encuadrar / F): se anima.
    /// Los automáticos (al cargar, al redimensionar) son inmediatos.
    pub animate_fit: bool,
    /// Zoom pedido por teclado (+/−), aplicado sobre el centro del lienzo.
    pub pending_zoom: Option<f64>,
    /// Transición de vista en curso (spring interrumpible).
    pub anim: Option<ViewAnimation>,
    /// De dónde se vino al entrar a una sub-celda o un sub-esquemático:
    /// "Volver" desapila (como el "atrás" de un navegador).
    pub back: Vec<BackStep>,
    /// Inercia del pan tras soltar un arrastre rápido.
    pub inertia: Option<Inertia>,
    /// Lo que se resalta de un resultado del LVS (una red, un dispositivo):
    /// el resto se atenúa. Se suelta con Esc.
    pub mark: Option<Mark>,
    /// Recuadros de color sobre la escena, sin atenuar el resto (el estado
    /// de cada transistor en el LVS manual).
    pub tags: Vec<Tag>,
}

/// Un recuadro de color sobre la escena (coordenadas de mundo).
#[derive(Clone, Debug, PartialEq)]
#[cfg_attr(not(all(feature = "xschem", feature = "layout")), allow(dead_code))]
pub(crate) struct Tag {
    pub bbox: BoundingBox,
    pub color: egui::Color32,
    /// Lo elegido: borde más grueso, para encontrarlo de lejos.
    pub strong: bool,
}

/// Algo resaltado sobre la escena, en coordenadas de mundo: polígonos
/// rellenos (una red) y recuadros (un dispositivo).
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Mark {
    pub fills: Vec<Vec<(f64, f64)>>,
    pub boxes: Vec<BoundingBox>,
}

// Las marcas las pone la vista de LVS (con los módulos de esquemático y layout).
#[cfg_attr(not(all(feature = "xschem", feature = "layout")), allow(dead_code))]
impl Mark {
    pub(crate) fn is_empty(&self) -> bool {
        self.fills.is_empty() && self.boxes.is_empty()
    }

    /// Lo que ocupa, para encuadrarlo.
    pub(crate) fn bbox(&self) -> Option<BoundingBox> {
        let mut points =
            self.fills.iter().flatten().copied().chain(self.boxes.iter().flat_map(|b| [(b.min_x, b.min_y), (b.max_x, b.max_y)]));
        let first = points.next()?;
        let mut bb = BoundingBox::from_points(first, first);
        for (x, y) in points {
            bb.expand_point(x, y);
        }
        Some(bb)
    }
}

impl SceneState {
    /// Estado para una carga que llegó. Si es el mismo archivo (otra celda,
    /// otra pestaña o una recarga) se conservan capas ocultas, buscador y
    /// vista; una celda nueva tiene otro tamaño → re-encuadre, otra pestaña no.
    pub(crate) fn from_loaded(loaded: LoadedScene, prev: Option<SceneState>) -> Self {
        match prev.filter(|p| p.path == loaded.path) {
            Some(p) => SceneState {
                scene: loaded.scene,
                backend: loaded.backend,
                source: loaded.source,
                kind: loaded.kind,
                needs_fit: loaded.refit || p.needs_fit,
                // Otra celda u otra versión: la red resaltada ya no es la misma.
                net_focus: None,
                mark: None,
                tags: Vec::new(),
                ..p
            },
            None => SceneState {
                // Las capas de ayuda (transistores) empiezan ocultas.
                hidden_layers: loaded
                    .scene
                    .layer_list()
                    .into_iter()
                    .filter(|(_, p)| p.hidden)
                    .map(|(_, p)| p.name.clone())
                    .collect(),
                scene: loaded.scene,
                viewport: Viewport::default(),
                backend: loaded.backend,
                source: loaded.source,
                path: loaded.path,
                needs_fit: true,
                fitted_size: None,
                layer_focus: None,
                net_focus: None,
                layer_hover_panel: None,
                layer_hover_legend: None,
                entry_query: String::new(),
                only_roots: true,
                only_changed: true,
                kind: loaded.kind,
                focus: None,
                animate_fit: false,
                pending_zoom: None,
                anim: None,
                inertia: None,
                back: Vec::new(),
                mark: None,
                tags: Vec::new(),
            },
        }
    }

    /// Claves de capa de la escena actual que están ocultas.
    pub(crate) fn hidden_keys(&self) -> HashSet<Layer> {
        self.scene.layer_list().into_iter().filter(|(_, p)| self.hidden_layers.contains(&p.name)).map(|(k, _)| k).collect()
    }

    /// Nombre de la capa resaltada: la del puntero, o la fija.
    pub(crate) fn focused_layer(&self) -> Option<&str> {
        self.layer_hover_panel.as_deref().or(self.layer_hover_legend.as_deref()).or(self.layer_focus.as_deref())
    }

    /// Clave de la capa resaltada en la escena actual (una capa oculta no
    /// se resalta: no hay nada que ver).
    pub(crate) fn focus_key(&self) -> Option<Layer> {
        let name = self.focused_layer()?;
        if self.hidden_layers.contains(name) {
            return None;
        }
        self.scene.layer_list().into_iter().find(|(_, p)| p.name == name).map(|(k, _)| k)
    }

    /// Clic en el nombre de una capa: la fija como resaltada, o la suelta.
    pub(crate) fn toggle_layer_focus(&mut self, name: &str) {
        self.layer_focus = if self.layer_focus.as_deref() == Some(name) { None } else { Some(name.to_string()) };
    }

    /// Pestaña actual si es un diff.
    pub(crate) fn diff_tab(&self) -> Option<DiffTab> {
        match &self.kind {
            LoadKind::Diff { tab, .. } => Some(*tab),
            LoadKind::Single => None,
        }
    }
}

/// Lo que ocupa el lienzo.
#[derive(Default)]
pub(crate) enum Content {
    /// Nada abierto: la pantalla de inicio (ver `home`).
    #[default]
    Home,
    Scene(SceneState),
    /// Formas de onda (`.raw`): no pasan por `ViewerBackend`, tienen su vista.
    #[cfg(feature = "spice")]
    Wave(WaveView),
    /// El LVS de un par: esquemático y layout lado a lado.
    #[cfg(all(feature = "xschem", feature = "layout"))]
    Lvs(Box<crate::gui::lvs_view::LvsState>),
}

impl Content {
    pub(crate) fn scene(&self) -> Option<&SceneState> {
        match self {
            Content::Scene(s) => Some(s),
            _ => None,
        }
    }

    #[cfg(all(feature = "xschem", feature = "layout"))]
    pub(crate) fn lvs_mut(&mut self) -> Option<&mut crate::gui::lvs_view::LvsState> {
        match self {
            Content::Lvs(s) => Some(s),
            _ => None,
        }
    }

    pub(crate) fn scene_mut(&mut self) -> Option<&mut SceneState> {
        match self {
            Content::Scene(s) => Some(s),
            _ => None,
        }
    }

    /// La escena, si la hay, dejando el lienzo vacío.
    pub(crate) fn take_scene(&mut self) -> Option<SceneState> {
        match std::mem::take(self) {
            Content::Scene(s) => Some(s),
            other => {
                *self = other;
                None
            }
        }
    }

    #[cfg(feature = "spice")]
    pub(crate) fn wave(&self) -> Option<&WaveView> {
        match self {
            Content::Wave(w) => Some(w),
            _ => None,
        }
    }

    #[cfg(feature = "spice")]
    pub(crate) fn wave_mut(&mut self) -> Option<&mut WaveView> {
        match self {
            Content::Wave(w) => Some(w),
            _ => None,
        }
    }
}
