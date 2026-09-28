//! Estilo de las capas de una escena: una clave `Layer` por (layer,
//! datatype), en orden de apilado, y su pintura, según el [`Process`] del
//! layout.

use std::collections::{BTreeMap, BTreeSet, HashMap};

use gdstk_rs::{GdsTag, Library};
use viewer_core::element::Layer;
use viewer_core::paint::{LayerPaint, Rgba};

use crate::palette::{LayerRole, LayerSpec};
use crate::process::Process;

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
///
/// Las capas con nombre de Magic (`names`) toman el estilo de su equivalente
/// GDS en el proceso ([`Process::magic_spec`]) y se muestran con su nombre.
pub(crate) struct LayerKeys {
    keys: BTreeMap<(u32, u32), Layer>,
    pub process: &'static Process,
    names: LayerNames,
}

/// Nombre de cada capa con nombre (Magic) de una `Library`.
pub(crate) type LayerNames = HashMap<(u32, u32), String>;

pub(crate) fn layer_names(lib: &Library) -> LayerNames {
    lib.layer_names().into_iter().map(|(t, n)| (tag_tuple(t), n)).collect()
}

impl LayerKeys {
    pub fn new(tags: BTreeSet<(u32, u32)>, path_hint: Option<&str>, names: LayerNames) -> Self {
        let as_gds: Vec<GdsTag> = tags.iter().map(|&t| gds_tag(t)).collect();
        let magic: Vec<&str> = names.values().map(String::as_str).collect();
        let process = Process::for_layout(&magic, path_hint, &as_gds);
        let mut this = Self { keys: BTreeMap::new(), process, names };

        let mut ordered: Vec<(u32, u32)> = tags.into_iter().collect();
        ordered.sort_by_key(|&t| (this.spec(gds_tag(t)).rank, t));
        this.keys = ordered
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, i.min(u16::MAX as usize) as Layer))
            .collect();
        this
    }

    fn spec(&self, tag: GdsTag) -> LayerSpec {
        match self.names.get(&tag_tuple(tag)) {
            Some(name) => self.process.magic_spec(name, tag),
            None => self.process.layer_spec(tag),
        }
    }

    pub fn key(&self, tag: GdsTag) -> Layer {
        self.keys.get(&tag_tuple(tag)).copied().unwrap_or(u16::MAX)
    }

    pub fn paints(&self) -> BTreeMap<Layer, LayerPaint> {
        self.keys
            .iter()
            .map(|(&t, key)| {
                let tag = gds_tag(t);
                (*key, layer_paint(tag, self.spec(tag), self.names.get(&t).map(String::as_str)))
            })
            .collect()
    }
}

pub(crate) fn tag_tuple(tag: GdsTag) -> (u32, u32) {
    (tag.layer, tag.datatype)
}

fn gds_tag((layer, datatype): (u32, u32)) -> GdsTag {
    GdsTag { layer, datatype }
}

/// Estilo de una capa. `magic_name`: el nombre de una capa de Magic, que se
/// muestra solo (sin número: el número es interno).
fn layer_paint(tag: GdsTag, spec: LayerSpec, magic_name: Option<&str>) -> LayerPaint {
    let c = spec.color;
    let stroke = Rgba::new(c.r, c.g, c.b, 255);
    LayerPaint { name: layer_label(tag, &spec, magic_name), fill: stroke.with_alpha(fill_alpha(spec.role)), stroke, hidden: false }
}

pub(crate) fn layer_label(tag: GdsTag, spec: &LayerSpec, magic_name: Option<&str>) -> String {
    match (magic_name, spec.name) {
        (Some(name), _) => name.to_string(),
        (None, Some(name)) => format!("{name} {}/{}", tag.layer, tag.datatype),
        (None, None) => format!("{}/{}", tag.layer, tag.datatype),
    }
}

