//! Qué muestra el lienzo: la pantalla de inicio, una escena de un backend
//! (esquemático, layout) o una vista de formas de onda. Un solo `enum`: antes eran dos
//! `Option` independientes y podían quedar los dos llenos (una vista de ondas
//! tapando el diff pedido, B7).

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Arc;

use eframe::egui;
use viewer_core::{
    backend::ViewerBackend, bbox::BoundingBox, element::Layer, scene::SceneHandle, viewport::Viewport, DiffFiles,
};

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
    /// Nombre visible de la vista.
    pub(crate) fn label(self) -> &'static str {
        match self {
            DiffTab::Diff => "Diff",
            DiffTab::Before => "Before",
            DiffTab::After => "After",
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
    Diff { before: Arc<Vec<u8>>, files: DiffFiles, tab: DiffTab, renamed: Arc<[(String, String)]> },
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
    /// Inercia del pan tras soltar un arrastre rápido.
    pub inertia: Option<Inertia>,
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
                ..p
            },
            None => SceneState {
                scene: loaded.scene,
                viewport: Viewport::default(),
                backend: loaded.backend,
                source: loaded.source,
                path: loaded.path,
                needs_fit: true,
                fitted_size: None,
                hidden_layers: HashSet::new(),
                layer_focus: None,
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
            },
        }
    }

    /// Claves de capa de la escena actual que están ocultas.
    pub(crate) fn hidden_keys(&self) -> HashSet<Layer> {
        self.scene
            .layer_list()
            .into_iter()
            .filter(|(_, p)| self.hidden_layers.contains(&p.name))
            .map(|(k, _)| k)
            .collect()
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
}

impl Content {
    pub(crate) fn scene(&self) -> Option<&SceneState> {
        match self {
            Content::Scene(s) => Some(s),
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
