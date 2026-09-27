//! Cómo se dibujan las capas de un layout: [`Process`], un solo modelo que
//! junta lo que riku trae compilado (`palette`: SKY130, GF180, IHP) con lo
//! leído del PDK instalado (`pdk_tech`: cualquier PDK). La escena y el diff
//! le preguntan solo a él.
//!
//! Qué manda, de más a menos:
//! 1. las capas curadas a mano (rol y apilado pensados para cada PDK);
//! 2. el `.lyp` del PDK instalado; sin él, la tabla generada del `.lyp`;
//! 3. para lo que nadie conoce, un color de la paleta genérica, arriba de
//!    todo, y contorno si su datatype es de pin, label o marcador.
//!
//! Las capas de Magic van a su capa GDS (la equivalente curada, o la que
//! escribe el `cifoutput` del `.tech` instalado) y toman su estilo.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

use gdstk_rs::GdsTag;

use crate::palette::{self, LayerRole, LayerSpec};
use crate::pdk_tech::{self, Tech};
use crate::style::{Color, Pdk};

/// Una capa conocida.
struct Entry {
    tag: (u32, u32),
    name: String,
    color: Color,
    role: LayerRole,
}

/// Capas y tipos de Magic de un proceso, ya resueltos.
pub(crate) struct Process {
    /// Nombre para mostrar (`sky130A`, o `SKY130` sin PDK instalado).
    pub name: String,
    pdk: Pdk,
    /// En orden de apilado: de abajo hacia arriba.
    layers: Vec<Entry>,
    index: HashMap<(u32, u32), usize>,
    /// Tipo de Magic → capa GDS y si va solo con contorno.
    magic: HashMap<String, ((u32, u32), bool)>,
}

impl Process {
    /// El proceso de `pdk` con el PDK instalado `tech`. Se arma una vez por
    /// combinación y vive lo que el programa: son unas pocas.
    pub fn get(pdk: Pdk, tech: Option<&'static Tech>) -> &'static Process {
        type Key = (Pdk, Option<usize>);
        static ALL: OnceLock<Mutex<HashMap<Key, &'static Process>>> = OnceLock::new();
        let key = (pdk, tech.map(|t| t as *const Tech as usize));
        let mut all = ALL.get_or_init(Default::default).lock().unwrap_or_else(|e| e.into_inner());
        all.entry(key).or_insert_with(|| Box::leak(Box::new(Process::build(pdk, tech))))
    }

    /// Solo lo compilado, sin mirar el disco.
    #[cfg(test)]
    pub fn compiled(pdk: Pdk) -> &'static Process {
        Process::get(pdk, None)
    }

    /// El proceso de un layout: el PDK compilado que reconocen sus capas (o
    /// su ruta) y el instalado que más capas suyas conoce, de la misma
    /// familia si hay una. `magic_names`: capas con nombre de un `.mag`.
    pub fn for_layout(magic_names: &[&str], path_hint: Option<&str>, tags: &[GdsTag]) -> &'static Process {
        let mut pdk = match magic_names {
            [] => Pdk::Generic,
            names => palette::magic_pdk(names.iter().copied()),
        };
        if pdk == Pdk::Generic {
            pdk = palette::detect_pdk(path_hint, tags);
        }
        let family = |t: &Tech| palette::detect_pdk(Some(&t.name), &[]);
        let fits = |t: &Tech| pdk == Pdk::Generic || family(t) == pdk;
        let tech = if magic_names.is_empty() {
            let tuples: Vec<(u32, u32)> = tags.iter().map(|t| (t.layer, t.datatype)).collect();
            pdk_tech::for_tags(&tuples, fits)
        } else {
            pdk_tech::for_magic(magic_names, fits)
        };
        // Las capas no alcanzaron para reconocerlo, pero el PDK instalado
        // que las conoce es de una familia compilada: sus tablas curadas.
        if pdk == Pdk::Generic {
            pdk = tech.map_or(Pdk::Generic, family);
        }
        Process::get(pdk, tech)
    }

    pub(crate) fn build(pdk: Pdk, tech: Option<&Tech>) -> Process {
        let mut layers: Vec<Entry> = palette::curated(pdk)
            .iter()
            .map(|l| Entry { tag: l.tag, name: l.name.to_string(), color: l.color, role: l.role })
            .collect();
        match tech.filter(|t| !t.layers.is_empty()) {
            Some(t) => layers.extend(
                t.layers.iter().map(|l| Entry { tag: l.tag, name: l.name.clone(), color: l.color, role: l.role }),
            ),
            None => layers.extend(
                palette::generated(pdk)
                    .iter()
                    .map(|l| Entry { tag: l.tag, name: l.name.to_string(), color: l.color, role: l.role }),
            ),
        }
        let mut index = HashMap::new();
        for (i, l) in layers.iter().enumerate() {
            index.entry(l.tag).or_insert(i);
        }

        let mut magic: HashMap<String, ((u32, u32), bool)> =
            palette::magic_to_gds(pdk).map(|(name, tag, outline)| (name.to_string(), (tag, outline))).collect();
        for (name, t) in tech.iter().flat_map(|t| t.magic_types()) {
            magic.entry(name.to_string()).or_insert((t.tag, t.outline));
        }

        let name = match tech {
            Some(t) => t.name.clone(),
            None => pdk_label(pdk).to_string(),
        };
        Process { name, pdk, layers, index, magic }
    }

    /// Estilo de una capa GDS.
    pub fn layer_spec(&'static self, tag: GdsTag) -> LayerSpec {
        match self.index.get(&(tag.layer, tag.datatype)) {
            Some(&i) => {
                let l = &self.layers[i];
                LayerSpec { name: Some(l.name.as_str()), color: l.color, role: l.role, rank: i as u32 }
            }
            None => {
                let outline = palette::outline_datatype(self.pdk, tag.datatype);
                LayerSpec {
                    name: None,
                    color: palette::generic_color(tag),
                    role: if outline { LayerRole::Outline } else { LayerRole::Device },
                    rank: self.layers.len() as u32,
                }
            }
        }
    }

    /// Estilo de una capa de Magic (`tag`: su número en la `Library`): el de
    /// su capa GDS. `name` de la spec queda en `None`: se muestra el nombre
    /// de Magic.
    pub fn magic_spec(&'static self, name: &str, tag: GdsTag) -> LayerSpec {
        match self.magic.get(name) {
            Some(&((layer, datatype), outline)) => {
                let spec = self.layer_spec(GdsTag { layer, datatype });
                let role = if outline { LayerRole::Outline } else { spec.role };
                LayerSpec { name: None, role, ..spec }
            }
            None => LayerSpec {
                name: None,
                color: palette::generic_color(tag),
                role: LayerRole::Device,
                rank: self.layers.len() as u32,
            },
        }
    }
}

fn pdk_label(pdk: Pdk) -> &'static str {
    match pdk {
        Pdk::Sky130 => "SKY130",
        Pdk::Gf180 => "GF180MCU",
        Pdk::Ihp => "IHP SG13G2",
        Pdk::Generic => "genérico",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_combination_is_built_once() {
        assert!(std::ptr::eq(Process::compiled(Pdk::Gf180), Process::compiled(Pdk::Gf180)));
        assert_eq!(Process::compiled(Pdk::Sky130).name, "SKY130");
    }

    #[test]
    fn installed_pdk_names_the_process_and_fills_missing_layers() {
        // Con los PDK de iic-osic-tools instalados.
        let Some(sky) = pdk_tech::by_name("sky130A") else { return };
        let p = Process::get(Pdk::Sky130, Some(sky));
        assert_eq!(p.name, "sky130A");
        // Curada: manda sobre el .lyp.
        assert_eq!(p.layer_spec(GdsTag { layer: 68, datatype: 20 }).name, Some("met1"));
        // Fuera de la tabla curada de SKY130, que no trae tabla generada.
        let pin = p.layer_spec(GdsTag { layer: 122, datatype: 16 });
        assert!(pin.name.is_some(), "{pin:?}");
        let tags = [GdsTag { layer: 67, datatype: 20 }, GdsTag { layer: 68, datatype: 20 }, GdsTag { layer: 66, datatype: 20 }];
        assert_eq!(Process::for_layout(&[], None, &tags).pdk, Pdk::Sky130);
    }
}
