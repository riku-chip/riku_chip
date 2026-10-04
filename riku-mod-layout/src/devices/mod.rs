//! Transistores de un layout: su modelo, W y L, reconocidos con las reglas
//! del `.tech` de Magic del PDK (ver `docs/formatos.md`, «Transistores y redes»).

mod devices_generated;
mod diff;
pub(crate) mod extract;
mod regions;
pub(crate) mod rules;

pub use diff::{cell_device_changes, device_changes, flat_polygon_estimate, DeviceChange, DeviceDesc};
pub use extract::{extract, extract_magic, Device, LayerPolys};
pub use regions::RegionEval;
pub use rules::{Cond, CondOp, DeviceRules, DeviceType, GdsLayer, ResistorType};

use std::collections::HashMap;
use std::sync::OnceLock;

use gdstk_rs::{Cell, GdsTag, Library, OwnedPolygon};

use crate::style::Pdk;

/// Hasta cuántos polígonos (aplanados) se reconocen transistores en una
/// celda: una más grande (un chip entero) tardaría segundos y mucha memoria
/// en aplanar difusión, poly y marcadores; se reconocen en sus sub-celdas.
pub const MAX_POLYGONS: u64 = 2_000_000;

/// Las reglas para un layout: las del PDK instalado que reconoce sus capas
/// o, si no hay, las de la tabla compilada del PDK que se detecta por la
/// ruta y las capas (`devices_generated.rs`). `None` si no es de un PDK
/// conocido.
pub fn rules_for(path_hint: Option<&str>, tags: &[(u32, u32)]) -> Option<&'static DeviceRules> {
    if let Some(tech) = crate::pdk_tech::for_tags(tags, |t| t.device_rules().is_some()) {
        return tech.device_rules();
    }
    let gds: Vec<GdsTag> = tags.iter().map(|&(layer, datatype)| GdsTag { layer, datatype }).collect();
    compiled(crate::palette::detect_pdk(path_hint, &gds))
}

/// Las reglas de la tabla compilada de un PDK (se leen una vez).
pub fn compiled(pdk: Pdk) -> Option<&'static DeviceRules> {
    static RULES: [OnceLock<Option<DeviceRules>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    let (i, text) = match pdk {
        Pdk::Sky130 => (0, devices_generated::SKY130),
        Pdk::Gf180 => (1, devices_generated::GF180),
        Pdk::Ihp => (2, devices_generated::IHP),
        Pdk::Generic => return None,
    };
    RULES[i].get_or_init(|| DeviceRules::parse(text)).as_ref()
}

/// Las reglas para una librería: si viene de Magic (sus capas tienen nombre
/// de tipo de Magic), las del PDK instalado que conoce esos tipos.
pub fn rules_for_library(lib: &Library, path_hint: Option<&str>) -> Option<&'static DeviceRules> {
    let names = lib.layer_names();
    let tags: Vec<(u32, u32)> = lib.layers().into_iter().map(|t| (t.layer, t.datatype)).collect();
    if !names.is_empty() {
        let names: Vec<&str> = names.iter().map(|(_, n)| n.as_str()).collect();
        if let Some(tech) = crate::pdk_tech::for_magic(&names, |t| t.device_rules().is_some()) {
            return tech.device_rules();
        }
    }
    rules_for(path_hint, &tags)
}

/// Los transistores de `cell` (aplanada) según `rules`. Un layout de Magic
/// ya trae los transistores pintados como capas; un GDS/OASIS se reconoce
/// con las reglas de `cifinput`.
pub fn cell_devices(lib: &Library, cell: &Cell<'_>, rules: &DeviceRules) -> Vec<Device> {
    let names: HashMap<(u32, u32), String> = lib.layer_names().into_iter().map(|(t, n)| ((t.layer, t.datatype), n)).collect();
    if names.values().any(|n| rules.device_type(n).is_some()) {
        let wanted: Vec<(u32, u32)> = names
            .iter()
            .filter(|(_, n)| rules.device_type(n).is_some() || rules.devices.iter().any(|(_, t)| rules.is_sd_of(t, n)))
            .map(|(&t, _)| t)
            .collect();
        return extract_magic(rules, flatten(cell, &wanted), &names, lib.unit() / 1e-6);
    }
    let wanted = rules.used_layers();
    let tags: Vec<(u32, u32)> = lib
        .layers()
        .into_iter()
        .map(|t| (t.layer, t.datatype))
        .filter(|&(l, d)| wanted.iter().any(|&(wl, wd)| wl == l && wd.is_none_or(|wd| wd == d)))
        .collect();
    extract(rules, &LayerPolys::new(flatten(cell, &tags)), lib.unit() / 1e-6)
}

/// La celda aplanada, solo en esas capas.
pub(crate) fn flatten(cell: &Cell<'_>, tags: &[(u32, u32)]) -> Vec<OwnedPolygon> {
    tags.iter()
        .flat_map(|&(l, d)| {
            cell.get_polygons()
                .with_filter(l, d)
                .build()
                .polygons()
                .map(|p| OwnedPolygon { layer: p.layer(), datatype: p.datatype(), points: p.points().collect() })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El pozo P de SKY130 se cierra con `grow 420` / `shrink 420`: une
    /// pozos a menos de 0,84 µm. El metal no tiene cierre.
    #[test]
    fn the_sky130_pwell_bridges_gaps_and_metal_does_not() {
        let rules = compiled(Pdk::Sky130).expect("sky130");
        assert!((rules.bridge("pwell") - 0.84).abs() < 1e-9, "{}", rules.bridge("pwell"));
        assert_eq!(rules.bridge("metal1"), 0.0);
        assert_ne!(rules.fingerprint, compiled(Pdk::Gf180).expect("gf180").fingerprint);
    }

    #[test]
    fn the_compiled_tables_have_the_transistors_of_each_pdk() {
        for (pdk, model) in [(Pdk::Sky130, "sky130_fd_pr__nfet_01v8"), (Pdk::Gf180, "nfet_03v3"), (Pdk::Ihp, "sg13_lv_nmos")] {
            let rules = compiled(pdk).unwrap_or_else(|| panic!("{pdk:?}"));
            assert!(
                rules.devices.iter().any(|(_, t)| t.models.iter().any(|m| m.name.ends_with(model))),
                "{pdk:?}: {:?}",
                rules.devices.iter().map(|(_, t)| &t.magic).collect::<Vec<_>>()
            );
        }
        assert!(compiled(Pdk::Generic).is_none());
    }

    /// Sin el PDK instalado (la CI): la SRAM de SKY130 del repo se reconoce
    /// con la tabla compilada.
    #[test]
    fn the_sram_example_has_its_transistors_with_the_compiled_rules() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/GDS/sram_16x8_sky130.gds");
        let lib = Library::from_bytes(&std::fs::read(&path).expect("sram")).expect("gds");
        let rules = compiled(Pdk::Sky130).expect("sky130");
        let top = crate::select_top_cell(&lib).expect("top");
        let devs = cell_devices(&lib, &top, rules);
        assert!(devs.len() > 500, "{} transistores", devs.len());
        // SKY130: L mínimo 0,15 µm (en el SRAM hay compuertas de 0,15 y más).
        assert!(devs.iter().all(|d| d.w_um > 0.1 && d.l_um > 0.15 - 1e-6), "{:?}", devs.iter().find(|d| d.l_um <= 0.15 - 1e-6));
        let models: std::collections::BTreeSet<&str> = devs.iter().map(|d| d.model.as_str()).collect();
        assert!(models.iter().any(|m| m.contains("nfet")) && models.iter().any(|m| m.contains("pfet")), "{models:?}");
    }
}
