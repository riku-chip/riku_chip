use crate::style::{Color, LayerStyle, Pdk};
use gdstk_rs::GdsTag;

pub fn color_for_tag(tag: GdsTag, pdk: Pdk) -> Color {
    if let Some((_, color)) = named_layer(tag, pdk) {
        return color;
    }
    let palette = match pdk {
        Pdk::Sky130 => SKY130_PALETTE,
        Pdk::Gf180 => GF180_PALETTE,
        Pdk::Ihp => IHP_PALETTE,
        Pdk::Generic => GENERIC_PALETTE,
    };
    let index = ((tag.layer as usize) * 31 + tag.datatype as usize) % palette.len();
    palette[index]
}

pub fn default_layer_style(tag: GdsTag, order: u32, fill: Color) -> LayerStyle {
    LayerStyle {
        tag,
        name: format!("layer_{}_{}", tag.layer, tag.datatype),
        fill,
        stroke: fill,
        opacity: 1.0,
        visible: true,
        order,
        hatch: None,
    }
}

pub fn highlight_style(kind: &str) -> (Color, Color) {
    match kind {
        "added" => (Color::rgba(0, 200, 0, 255), Color::rgba(0, 120, 0, 255)),
        "removed" => (Color::rgba(200, 0, 0, 255), Color::rgba(120, 0, 0, 255)),
        "modified" => (Color::rgba(255, 180, 0, 255), Color::rgba(180, 120, 0, 255)),
        _ => (Color::rgba(255, 0, 0, 255), Color::rgba(180, 0, 0, 255)),
    }
}

const GENERIC_PALETTE: [Color; 12] = [
    Color::rgba(76, 175, 80, 255),
    Color::rgba(33, 150, 243, 255),
    Color::rgba(255, 193, 7, 255),
    Color::rgba(244, 67, 54, 255),
    Color::rgba(0, 188, 212, 255),
    Color::rgba(156, 39, 176, 255),
    Color::rgba(255, 152, 0, 255),
    Color::rgba(63, 81, 181, 255),
    Color::rgba(205, 220, 57, 255),
    Color::rgba(121, 85, 72, 255),
    Color::rgba(96, 125, 139, 255),
    Color::rgba(233, 30, 99, 255),
];

const SKY130_PALETTE: [Color; 12] = GENERIC_PALETTE;
const GF180_PALETTE: [Color; 12] = GENERIC_PALETTE;
const IHP_PALETTE: [Color; 12] = GENERIC_PALETTE;

/// Como se pinta una capa, segun su funcion fisica.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LayerRole {
    /// Geometria de dispositivo/interconexion (diff, poly, li1, metales,
    /// contactos): relleno visible.
    Device,
    /// Pozos: cubren celdas enteras; relleno muy tenue para no tapar.
    Well,
    /// Implantes, marcadores, boundaries, pines y labels: solo contorno.
    /// Suelen ser rectangulos del tamano de la celda y, rellenos, ensucian
    /// toda la vista.
    Outline,
}

/// Estilo resuelto de una capa para un PDK.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LayerSpec {
    /// Nombre del PDK (`"met1"`), si la capa es conocida.
    pub name: Option<&'static str>,
    pub color: Color,
    pub role: LayerRole,
    /// Orden de apilado (menor = se dibuja antes, queda abajo).
    pub rank: u32,
}

/// Nombre y color convencional de una capa conocida del PDK. `None` si la capa
/// no esta en el mapa (el caller cae a la paleta generica por hash).
///
/// Solo SKY130 tiene mapa real por ahora; GF180 e IHP usan la paleta generica.
pub fn named_layer(tag: GdsTag, pdk: Pdk) -> Option<(&'static str, Color)> {
    find_pdk_layer(tag, pdk).map(|(_, l)| (l.name, l.color))
}

/// Estilo completo de una capa: nombre, color, rol y orden de apilado.
///
/// Capas fuera del mapa del PDK: en SKY130 el rol se infiere del datatype
/// (16 = pin, 5/59 = label, 4 = marcador → contorno) y se apilan encima de
/// todo; en `Generic` todas son `Device` con el mismo rank, de modo que un
/// sort estable conserva el orden del archivo.
pub fn layer_spec(tag: GdsTag, pdk: Pdk) -> LayerSpec {
    if let Some((rank, l)) = find_pdk_layer(tag, pdk) {
        return LayerSpec { name: Some(l.name), color: l.color, role: l.role, rank: rank as u32 };
    }
    let color = color_for_tag(tag, pdk);
    match pdk {
        Pdk::Sky130 => {
            let role = match tag.datatype {
                4 | 5 | 16 | 59 => LayerRole::Outline,
                _ => LayerRole::Device,
            };
            LayerSpec { name: None, color, role, rank: SKY130_LAYERS.len() as u32 }
        }
        Pdk::Gf180 | Pdk::Ihp | Pdk::Generic => {
            LayerSpec { name: None, color, role: LayerRole::Device, rank: 0 }
        }
    }
}

fn find_pdk_layer(tag: GdsTag, pdk: Pdk) -> Option<(usize, &'static PdkLayer)> {
    let table: &'static [PdkLayer] = match pdk {
        Pdk::Sky130 => SKY130_LAYERS,
        Pdk::Gf180 | Pdk::Ihp | Pdk::Generic => return None,
    };
    table
        .iter()
        .enumerate()
        .find(|(_, l)| l.tag == (tag.layer, tag.datatype))
}

/// Infere el PDK de un layout. Primero por el path (los PDK de iic-osic-tools
/// y volare viven en carpetas `sky130*`, `gf180*`, `ihp-sg13g2`), luego por
/// las capas de dibujo presentes. `Generic` si no hay evidencia.
pub fn detect_pdk(path_hint: Option<&str>, tags: &[GdsTag]) -> Pdk {
    if let Some(p) = path_hint {
        let p = p.to_ascii_lowercase();
        if p.contains("sky130") {
            return Pdk::Sky130;
        }
        if p.contains("gf180") {
            return Pdk::Gf180;
        }
        if p.contains("ihp") || p.contains("sg13g2") {
            return Pdk::Ihp;
        }
    }
    // poly 66/20, li1 67/20, met1 68/20: combinacion propia de SKY130.
    const SKY130_MARKERS: [(u32, u32); 3] = [(66, 20), (67, 20), (68, 20)];
    let hits = SKY130_MARKERS
        .iter()
        .filter(|(l, d)| tags.iter().any(|t| t.layer == *l && t.datatype == *d))
        .count();
    if hits >= 2 {
        Pdk::Sky130
    } else {
        Pdk::Generic
    }
}

struct PdkLayer {
    tag: (u32, u32),
    name: &'static str,
    color: Color,
    role: LayerRole,
}

const fn pl(layer: u32, datatype: u32, name: &'static str, color: Color, role: LayerRole) -> PdkLayer {
    PdkLayer { tag: (layer, datatype), name, color, role }
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::rgba(r, g, b, 255)
}

use LayerRole::{Device as D, Outline as O, Well as W};

/// Capas de SKY130 (layer/datatype del tech file) con colores al estilo de
/// los layer properties de KLayout. **El orden es el de apilado**: de abajo
/// (pozos) hacia arriba (metales), y al final contornos de pines/marcadores.
const SKY130_LAYERS: &[PdkLayer] = &[
    pl(64, 18, "dnwell", rgb(110, 110, 70), W),
    pl(64, 44, "pwell", rgb(150, 120, 90), W),
    pl(64, 20, "nwell", rgb(140, 150, 90), W),
    pl(75, 20, "hvi", rgb(170, 140, 90), O),
    pl(78, 44, "hvtp", rgb(200, 170, 110), O),
    pl(93, 44, "nsdm", rgb(110, 150, 220), O),
    pl(94, 20, "psdm", rgb(220, 140, 110), O),
    pl(95, 20, "npc", rgb(140, 200, 140), O),
    pl(65, 20, "diff", rgb(40, 190, 70), D),
    pl(65, 44, "tap", rgb(30, 150, 110), D),
    pl(66, 20, "poly", rgb(230, 50, 50), D),
    pl(66, 44, "licon1", rgb(200, 200, 200), D),
    pl(67, 20, "li1", rgb(160, 110, 230), D),
    pl(67, 44, "mcon", rgb(220, 220, 220), D),
    pl(68, 20, "met1", rgb(60, 130, 240), D),
    pl(68, 44, "via", rgb(230, 230, 230), D),
    pl(69, 20, "met2", rgb(230, 70, 200), D),
    pl(69, 44, "via2", rgb(230, 230, 230), D),
    pl(70, 20, "met3", rgb(40, 200, 210), D),
    pl(70, 44, "via3", rgb(230, 230, 230), D),
    pl(71, 20, "met4", rgb(240, 150, 40), D),
    pl(71, 44, "via4", rgb(230, 230, 230), D),
    pl(72, 20, "met5", rgb(230, 210, 60), D),
    pl(64, 16, "nwell.pin", rgb(140, 150, 90), O),
    pl(122, 16, "pwell.pin", rgb(150, 120, 90), O),
    pl(67, 16, "li1.pin", rgb(160, 110, 230), O),
    pl(68, 16, "met1.pin", rgb(60, 130, 240), O),
    pl(69, 16, "met2.pin", rgb(230, 70, 200), O),
    pl(81, 4, "areaid.sc", rgb(120, 120, 120), O),
    pl(235, 4, "prBoundary", rgb(180, 180, 180), O),
    pl(236, 0, "boundary", rgb(180, 180, 180), O),
    pl(64, 5, "nwell.label", rgb(190, 200, 140), O),
    pl(64, 59, "pwell.label", rgb(200, 170, 140), O),
    pl(67, 5, "li1.label", rgb(200, 170, 250), O),
    pl(68, 5, "met1.label", rgb(140, 190, 255), O),
    pl(69, 5, "met2.label", rgb(250, 150, 230), O),
    pl(83, 44, "text", rgb(220, 220, 160), O),
];

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(layer: u32, datatype: u32) -> GdsTag {
        GdsTag { layer, datatype }
    }

    #[test]
    fn sky130_named_layers_resolve() {
        let (name, _) = named_layer(tag(68, 20), Pdk::Sky130).expect("met1");
        assert_eq!(name, "met1");
        assert!(named_layer(tag(68, 20), Pdk::Generic).is_none());
        assert!(named_layer(tag(999, 0), Pdk::Sky130).is_none());
    }

    #[test]
    fn sky130_roles_and_stacking() {
        let nwell = layer_spec(tag(64, 20), Pdk::Sky130);
        let diff = layer_spec(tag(65, 20), Pdk::Sky130);
        let met1 = layer_spec(tag(68, 20), Pdk::Sky130);
        let nsdm = layer_spec(tag(93, 44), Pdk::Sky130);
        assert_eq!(nwell.role, LayerRole::Well);
        assert_eq!(met1.role, LayerRole::Device);
        assert_eq!(nsdm.role, LayerRole::Outline);
        assert!(nwell.rank < diff.rank && diff.rank < met1.rank);
        // Desconocida con datatype de pin: contorno, encima de todo.
        let pin = layer_spec(tag(999, 16), Pdk::Sky130);
        assert_eq!(pin.role, LayerRole::Outline);
        assert!(pin.rank > met1.rank);
    }

    #[test]
    fn generic_keeps_file_order_and_fills() {
        let a = layer_spec(tag(1, 0), Pdk::Generic);
        let b = layer_spec(tag(50, 3), Pdk::Generic);
        assert_eq!((a.rank, a.role), (b.rank, LayerRole::Device));
    }

    #[test]
    fn detect_pdk_from_path_then_layers() {
        assert_eq!(detect_pdk(Some("/foss/pdks/sky130A/x.gds"), &[]), Pdk::Sky130);
        assert_eq!(detect_pdk(Some("/foss/pdks/gf180mcuD/x.gds"), &[]), Pdk::Gf180);
        assert_eq!(detect_pdk(Some("/foss/pdks/ihp-sg13g2/x.gds"), &[]), Pdk::Ihp);
        assert_eq!(detect_pdk(None, &[tag(67, 20), tag(68, 20)]), Pdk::Sky130);
        assert_eq!(detect_pdk(None, &[tag(1, 0)]), Pdk::Generic);
    }
}
