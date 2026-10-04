//! Lo que riku trae compilado de SKY130, GF180MCU e IHP SG13G2: capas
//! curadas a mano (rol y apilado), las generadas de sus `.lyp` (respaldo
//! sin PDK instalado), las capas de Magic de sus `.tech` y cómo reconocer
//! cada PDK. [`crate::process::Process`] lo junta con lo leído del PDK
//! instalado; este módulo no decide nada por sí solo.

use gdstk_rs::GdsTag;

use crate::style::{Color, Pdk};

/// Color de una capa que nadie conoce: de una paleta fija, por su número.
pub(crate) fn generic_color(tag: GdsTag) -> Color {
    GENERIC_PALETTE[((tag.layer as usize) * 31 + tag.datatype as usize) % GENERIC_PALETTE.len()]
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

/// Capas curadas a mano de un PDK, en orden de apilado.
pub(crate) fn curated(pdk: Pdk) -> &'static [PdkLayer] {
    match pdk {
        Pdk::Sky130 => SKY130_LAYERS,
        Pdk::Gf180 => GF180_LAYERS,
        Pdk::Ihp => IHP_LAYERS,
        Pdk::Generic => &[],
    }
}

/// Tabla completa generada del `.lyp` oficial (ver `palette_generated.rs`):
/// el respaldo cuando el PDK no está instalado.
pub(crate) fn generated(pdk: Pdk) -> &'static [PdkLayer] {
    match pdk {
        Pdk::Gf180 => crate::palette_generated::GF180_LYP,
        Pdk::Ihp => crate::palette_generated::IHP_LYP,
        Pdk::Sky130 | Pdk::Generic => &[],
    }
}

/// Si una capa que no está en ninguna tabla va solo con contorno, por la
/// convención de datatypes del PDK (pines, labels, marcadores). En
/// `Generic`, nunca.
pub(crate) fn outline_datatype(pdk: Pdk, datatype: u32) -> bool {
    match pdk {
        // SKY130: 16 pin, 5/59 label, 4 boundary/marcador.
        Pdk::Sky130 => matches!(datatype, 4 | 5 | 16 | 59),
        // GF180: 0 drawing, 4 dummy fill; el resto son labels (10),
        // marcadores (5, 17…) y slots (3).
        Pdk::Gf180 => !matches!(datatype, 0 | 4),
        // IHP: 0 drawing, 20 mask, 22 filler; el resto son labels (1),
        // pines (2), boundaries (4), textos (25)…
        Pdk::Ihp => !matches!(datatype, 0 | 20 | 22),
        Pdk::Generic => false,
    }
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
    // Tres capas de dibujo caracteristicas de cada PDK (difusion/poly/metal1).
    // Hacen falta al menos dos para decidir: una sola capa (p.ej. 1/0) es
    // demasiado comun como para identificar un proceso.
    const MARKERS: [(Pdk, [(u32, u32); 3]); 3] = [
        (Pdk::Sky130, [(66, 20), (67, 20), (68, 20)]), // poly, li1, met1
        (Pdk::Gf180, [(22, 0), (30, 0), (34, 0)]),     // COMP, Poly2, Metal1
        (Pdk::Ihp, [(1, 0), (5, 0), (8, 0)]),          // Activ, GatPoly, Metal1
    ];
    let hits =
        |markers: &[(u32, u32)]| markers.iter().filter(|(l, d)| tags.iter().any(|t| t.layer == *l && t.datatype == *d)).count();
    MARKERS
        .iter()
        .map(|(pdk, m)| (hits(m), *pdk))
        .filter(|(n, _)| *n >= 2)
        .max_by_key(|(n, _)| *n)
        .map_or(Pdk::Generic, |(_, pdk)| pdk)
}

// ---- Capas de Magic ----

/// Plano de una capa de Magic y, si es un contacto, el plano de su residuo
/// de arriba (tabla generada de los `.tech`, ver `magic_layers_generated.rs`).
fn magic_row(pdk: Pdk, name: &str) -> Option<(&'static str, &'static str)> {
    let table = magic_rows(pdk);
    let i = table.binary_search_by(|r| r.0.cmp(name)).ok()?;
    Some((table[i].1, table[i].2))
}

fn magic_rows(pdk: Pdk) -> &'static [(&'static str, &'static str, &'static str)] {
    match pdk {
        Pdk::Sky130 => crate::magic_layers_generated::SKY130_MAGIC,
        Pdk::Gf180 => crate::magic_layers_generated::GF180_MAGIC,
        Pdk::Ihp => crate::magic_layers_generated::IHP_MAGIC,
        Pdk::Generic => &[],
    }
}

/// Capas de Magic que el PDK conoce, con la capa GDS curada que les da
/// color y apilado (`metal1` → met1, `viali` → mcon, `ndiffc` → licon1) y
/// si van solo con contorno (obstrucciones, bloqueos, comentarios).
pub(crate) fn magic_to_gds(pdk: Pdk) -> impl Iterator<Item = (&'static str, (u32, u32), bool)> {
    magic_rows(pdk).iter().filter_map(move |&(name, plane, upper)| {
        let equivalent = magic_equivalent(pdk, name, plane, upper)?;
        let layer = curated(pdk).iter().find(|l| l.name == equivalent)?;
        let outline = name.starts_with("obs") || matches!(plane, "block" | "comment");
        Some((name, layer.tag, outline))
    })
}

/// PDK de un layout de Magic: el que conoce más nombres de sus capas.
/// `Generic` si ninguno conoce alguno.
pub fn magic_pdk<'a>(names: impl IntoIterator<Item = &'a str>) -> Pdk {
    let names: Vec<&str> = names.into_iter().collect();
    [Pdk::Sky130, Pdk::Gf180, Pdk::Ihp]
        .into_iter()
        .map(|pdk| (names.iter().filter(|n| magic_row(pdk, n).is_some()).count(), pdk))
        .filter(|(n, _)| *n > 0)
        // En empate gana el primero (SKY130), que tiene la mayoría de los `.mag`.
        .fold(None, |best: Option<(usize, Pdk)>, c| match best {
            Some(b) if b.0 >= c.0 => Some(b),
            _ => Some(c),
        })
        .map_or(Pdk::Generic, |(_, pdk)| pdk)
}

/// Capa GDS del PDK que se dibuja como la capa de Magic `name` (en el plano
/// `plane`; `upper` = plano de arriba si es un contacto): da su color y su
/// lugar en el apilado.
fn magic_equivalent(pdk: Pdk, name: &str, plane: &str, upper: &str) -> Option<&'static str> {
    let poly = name.contains("poly") || name.contains("pres");
    let gate = ["fet", "mos", "transistor", "var"].iter().any(|k| name.contains(k));
    let tap = ["sub", "nsd", "psd", "tap"].iter().any(|k| name.contains(k));
    let nwell = name.starts_with("nw") || name.starts_with("nwell");
    let metal = |p: &str| p.strip_prefix("metal").and_then(|n| n.parse::<u32>().ok());
    match pdk {
        Pdk::Sky130 => Some(match (upper, plane) {
            ("locali", _) => "licon1",
            ("metal1", _) => "mcon",
            ("metal2", _) => "via",
            ("metal3", _) => "via2",
            ("metal4", _) => "via3",
            ("metal5", _) => "via4",
            (_, "dwell") => "dnwell",
            (_, "well") => {
                if nwell {
                    "nwell"
                } else {
                    "pwell"
                }
            }
            (_, "active") if poly || gate => "poly",
            (_, "active") if tap => "tap",
            (_, "active") => "diff",
            (_, "locali") => "li1",
            (_, "cap1") => "met3",
            (_, "cap2") => "met4",
            (_, p) => match metal(p)? {
                1 => "met1",
                2 => "met2",
                3 => "met3",
                4 => "met4",
                _ => "met5",
            },
        }),
        Pdk::Gf180 => Some(match (upper, plane) {
            ("metal1", _) => "Contact",
            ("metal2", _) => "Via1",
            ("metal3", _) => "Via2",
            ("metal4", _) => "Via3",
            ("metal5", _) => "Via4",
            (_, "dwell") => "DNWELL",
            (_, "well") => {
                if nwell {
                    "Nwell"
                } else {
                    "LVPWELL"
                }
            }
            (_, "active") if poly || gate => "Poly2",
            (_, "active") => "COMP",
            (_, p) => match metal(p)? {
                1 => "Metal1",
                2 => "Metal2",
                3 => "Metal3",
                4 => "Metal4",
                _ => "Metal5",
            },
        }),
        Pdk::Ihp => Some(match (upper, plane) {
            ("metal1", _) => "Cont",
            ("metal2", _) => "Via1",
            ("metal3", _) => "Via2",
            ("metal4", _) => "Via3",
            ("metal5", _) => "Via4",
            ("metal6", _) => "TopVia1",
            ("metal7", _) => "TopVia2",
            (_, "dwell") => "nBuLay",
            (_, "well") => {
                if nwell {
                    "NWell"
                } else {
                    "PWell"
                }
            }
            (_, "active") if poly || gate => "GatPoly",
            (_, "active") => "Activ",
            (_, "mimcap") => "MIM",
            (_, p) => match metal(p)? {
                1 => "Metal1",
                2 => "Metal2",
                3 => "Metal3",
                4 => "Metal4",
                5 => "Metal5",
                6 => "TopMetal1",
                _ => "TopMetal2",
            },
        }),
        Pdk::Generic => None,
    }
}

/// Una capa de las tablas compiladas.
pub(crate) struct PdkLayer {
    pub tag: (u32, u32),
    pub name: &'static str,
    pub color: Color,
    pub role: LayerRole,
}

pub(crate) const fn pl(layer: u32, datatype: u32, name: &'static str, color: Color, role: LayerRole) -> PdkLayer {
    PdkLayer { tag: (layer, datatype), name, color, role }
}

pub(crate) const fn rgb(r: u8, g: u8, b: u8) -> Color {
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

/// Capas de GF180MCU. Nombres y colores de relleno del `gf180mcu.lyp`
/// oficial (libs.tech/klayout/tech). Mismo orden de apilado que SKY130.
/// Ojo: el boundary de place&route (`PR_bndry`) vive en la capa 0/0.
const GF180_LAYERS: &[PdkLayer] = &[
    pl(12, 0, "DNWELL", rgb(0x88, 0xcb, 0x1d), W),
    pl(204, 0, "LVPWELL", rgb(0x3a, 0x28, 0x88), W),
    pl(21, 0, "Nwell", rgb(0xf9, 0x94, 0x6b), W),
    pl(5, 0, "NAT", rgb(0x91, 0xd3, 0x39), O),
    pl(55, 0, "Dualgate", rgb(0xc1, 0x6f, 0x5c), O),
    pl(32, 0, "Nplus", rgb(0xbf, 0x3e, 0xfb), O),
    pl(31, 0, "Pplus", rgb(0x34, 0xc5, 0x90), O),
    pl(49, 0, "SAB", rgb(0x67, 0x39, 0x2f), O),
    pl(24, 0, "ESD", rgb(0xd9, 0xef, 0x03), O),
    pl(62, 0, "Resistor", rgb(0x14, 0x37, 0xff), O),
    pl(22, 0, "COMP", rgb(0xbd, 0x74, 0xbd), D),
    pl(30, 0, "Poly2", rgb(0x2e, 0x95, 0x21), D),
    pl(33, 0, "Contact", rgb(0xc8, 0x66, 0x34), D),
    pl(34, 0, "Metal1", rgb(0xed, 0xdd, 0x07), D),
    pl(35, 0, "Via1", rgb(0xf5, 0xe7, 0xf1), D),
    pl(36, 0, "Metal2", rgb(0xcc, 0xf3, 0x38), D),
    pl(38, 0, "Via2", rgb(0xe1, 0xd8, 0xca), D),
    pl(42, 0, "Metal3", rgb(0x97, 0xb9, 0x1b), D),
    pl(40, 0, "Via3", rgb(0x53, 0xe2, 0xe8), D),
    pl(46, 0, "Metal4", rgb(0x80, 0x31, 0x7c), D),
    pl(41, 0, "Via4", rgb(0xa7, 0xb1, 0xd1), D),
    pl(81, 0, "Metal5", rgb(0xcd, 0xc1, 0x6b), D),
    pl(82, 0, "Via5", rgb(0x82, 0x7e, 0x5b), D),
    pl(53, 0, "MetalTop", rgb(0xb1, 0xdd, 0x9c), D),
    pl(37, 0, "Pad", rgb(0x72, 0x34, 0x2b), O),
    pl(112, 1, "V5_XTOR", rgb(0x49, 0xb4, 0x03), O),
    pl(22, 10, "COMP_Label", rgb(0x46, 0x65, 0xc7), O),
    pl(30, 10, "Poly2_Label", rgb(0xfb, 0x18, 0x79), O),
    pl(34, 10, "Metal1_Label", rgb(0xed, 0xdd, 0x07), O),
    pl(36, 10, "Metal2_Label", rgb(0xcc, 0xf3, 0x38), O),
    pl(42, 10, "Metal3_Label", rgb(0x97, 0xb9, 0x1b), O),
    pl(46, 10, "Metal4_Label", rgb(0x80, 0x31, 0x7c), O),
    pl(81, 10, "Metal5_Label", rgb(0xcd, 0xc1, 0x6b), O),
    pl(53, 10, "MetalTop_Label", rgb(0xb1, 0xdd, 0x9c), O),
    pl(0, 0, "PR_bndry", rgb(0xd9, 0xf8, 0x17), O),
    pl(63, 0, "Border", rgb(0xed, 0xeb, 0x06), O),
];

/// Capas de IHP SG13G2. Nombres y colores de relleno del `sg13g2.lyp`
/// oficial (libs.tech/klayout/tech). Mismo orden de apilado que SKY130.
const IHP_LAYERS: &[PdkLayer] = &[
    pl(32, 0, "nBuLay", rgb(0x8c, 0x8c, 0xa6), W),
    pl(46, 0, "PWell", rgb(0xff, 0xff, 0x00), W),
    pl(31, 0, "NWell", rgb(0x26, 0x8c, 0x6b), W),
    pl(44, 0, "ThickGateOx", rgb(0xff, 0xff, 0xcc), O),
    pl(14, 0, "pSD", rgb(0xcc, 0xb8, 0x99), O),
    pl(7, 0, "nSD", rgb(0x00, 0xcc, 0x66), O),
    pl(28, 0, "SalBlock", rgb(0x99, 0x00, 0xe6), O),
    pl(1, 0, "Activ", rgb(0x00, 0xff, 0x00), D),
    pl(5, 0, "GatPoly", rgb(0xbf, 0x40, 0x26), D),
    pl(128, 0, "PolyRes", rgb(0xbf, 0x40, 0x26), D),
    pl(6, 0, "Cont", rgb(0x00, 0xff, 0xff), D),
    pl(8, 0, "Metal1", rgb(0x39, 0xbf, 0xff), D),
    pl(19, 0, "Via1", rgb(0xcc, 0xcc, 0xff), D),
    pl(10, 0, "Metal2", rgb(0xcc, 0xcc, 0xd9), D),
    pl(29, 0, "Via2", rgb(0xff, 0x37, 0x36), D),
    pl(30, 0, "Metal3", rgb(0xd8, 0x00, 0x00), D),
    pl(49, 0, "Via3", rgb(0x9b, 0xa9, 0x40), D),
    pl(50, 0, "Metal4", rgb(0x93, 0xe8, 0x37), D),
    pl(66, 0, "Via4", rgb(0xde, 0xac, 0x5e), D),
    pl(67, 0, "Metal5", rgb(0xdc, 0xd1, 0x46), D),
    pl(36, 0, "MIM", rgb(0x26, 0x8c, 0x6b), D),
    pl(129, 0, "Vmim", rgb(0xff, 0xe6, 0xbf), D),
    pl(125, 0, "TopVia1", rgb(0xff, 0xe6, 0xbf), D),
    pl(126, 0, "TopMetal1", rgb(0xff, 0xe6, 0xbf), D),
    pl(133, 0, "TopVia2", rgb(0xff, 0x80, 0x00), D),
    pl(134, 0, "TopMetal2", rgb(0xff, 0x80, 0x00), D),
    pl(9, 0, "Passiv", rgb(0xe6, 0x1f, 0x0d), O),
    pl(1, 2, "Activ.pin", rgb(0x00, 0xff, 0x00), O),
    pl(5, 2, "GatPoly.pin", rgb(0xbf, 0x40, 0x26), O),
    pl(8, 2, "Metal1.pin", rgb(0x39, 0xbf, 0xff), O),
    pl(10, 2, "Metal2.pin", rgb(0xcc, 0xcc, 0xd9), O),
    pl(30, 2, "Metal3.pin", rgb(0xd8, 0x00, 0x00), O),
    pl(50, 2, "Metal4.pin", rgb(0x93, 0xe8, 0x37), O),
    pl(67, 2, "Metal5.pin", rgb(0xdc, 0xd1, 0x46), O),
    pl(126, 2, "TopMetal1.pin", rgb(0xff, 0xe6, 0xbf), O),
    pl(134, 2, "TopMetal2.pin", rgb(0xff, 0x80, 0x00), O),
    pl(8, 25, "Metal1.text", rgb(0x39, 0xbf, 0xff), O),
    pl(189, 0, "prBoundary", rgb(0x99, 0x00, 0xe6), O),
    pl(189, 4, "prBoundary.boundary", rgb(0x99, 0x00, 0xe6), O),
    pl(63, 0, "TEXT", rgb(0xff, 0xff, 0xff), O),
];

/// Estilos sin PDK instalado, para los tests de las tablas compiladas.
#[cfg(test)]
fn layer_spec(tag: GdsTag, pdk: Pdk) -> LayerSpec {
    crate::process::Process::compiled(pdk).layer_spec(tag)
}

#[cfg(test)]
fn magic_layer_spec(name: &str, tag: GdsTag, pdk: Pdk) -> LayerSpec {
    crate::process::Process::compiled(pdk).magic_spec(name, tag)
}

#[cfg(test)]
mod magic_tests {
    use super::*;

    fn spec(name: &str, pdk: Pdk) -> LayerSpec {
        magic_layer_spec(name, GdsTag { layer: 1 << 30, datatype: 0 }, pdk)
    }

    fn color_of(pdk: Pdk, pdk_name: &str) -> Color {
        curated(pdk).iter().find(|l| l.name == pdk_name).unwrap().color
    }

    #[test]
    fn magic_layers_take_the_color_of_their_gds_equivalent() {
        let s = Pdk::Sky130;
        for (magic, gds) in [
            ("metal1", "met1"),
            ("m1", "met1"),
            ("viali", "mcon"),
            ("ndiffc", "licon1"),
            ("polycont", "licon1"),
            ("locali", "li1"),
            ("poly", "poly"),
            ("nmos", "poly"),
            ("ndiff", "diff"),
            ("psubdiff", "tap"),
            ("nwell", "nwell"),
            ("pwell", "pwell"),
            ("via1", "via"),
            ("metal5", "met5"),
        ] {
            assert_eq!(spec(magic, s).color, color_of(s, gds), "{magic} → {gds}");
        }
        assert_eq!(spec("metal1", Pdk::Gf180).color, color_of(Pdk::Gf180, "Metal1"));
        assert_eq!(spec("ndiffc", Pdk::Gf180).color, color_of(Pdk::Gf180, "Contact"));
        assert_eq!(spec("ndiff", Pdk::Ihp).color, color_of(Pdk::Ihp, "Activ"));
        // Apilado: pozos abajo, metales arriba.
        assert!(spec("nwell", s).rank < spec("ndiff", s).rank);
        assert!(spec("locali", s).rank < spec("metal1", s).rank);
        assert_eq!(spec("nwell", s).role, LayerRole::Well);
        assert_eq!(spec("obsm1", s).role, LayerRole::Outline);
        // Desconocida: color genérico, arriba de todo.
        let unknown = spec("frobnicate", s);
        assert!(unknown.rank >= spec("metal5", s).rank);
    }

    #[test]
    fn the_pdk_is_the_one_that_knows_the_layers() {
        assert_eq!(magic_pdk(["locali", "viali", "metal1"]), Pdk::Sky130);
        assert_eq!(magic_pdk(["metal1", "ndiff"]), Pdk::Sky130, "empate: SKY130");
        assert_eq!(magic_pdk(["frobnicate"]), Pdk::Generic);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(layer: u32, datatype: u32) -> GdsTag {
        GdsTag { layer, datatype }
    }

    #[test]
    fn sky130_named_layers_resolve() {
        assert_eq!(layer_spec(tag(68, 20), Pdk::Sky130).name, Some("met1"));
        assert!(layer_spec(tag(68, 20), Pdk::Generic).name.is_none());
        assert!(layer_spec(tag(999, 0), Pdk::Sky130).name.is_none());
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
        // Capas de gf180mcu_fd_sc_mcu7t5v0__inv_1 y sg13g2_inv_1 (sin path).
        let gf = [tag(0, 0), tag(21, 0), tag(22, 0), tag(30, 0), tag(33, 0), tag(34, 0), tag(34, 10)];
        assert_eq!(detect_pdk(None, &gf), Pdk::Gf180);
        let ihp = [tag(1, 0), tag(5, 0), tag(6, 0), tag(8, 0), tag(8, 2), tag(14, 0), tag(31, 0)];
        assert_eq!(detect_pdk(None, &ihp), Pdk::Ihp);
    }

    #[test]
    fn gf180_and_ihp_tables_and_datatype_fallbacks() {
        let m1 = layer_spec(tag(34, 0), Pdk::Gf180);
        assert_eq!((m1.name, m1.role), (Some("Metal1"), LayerRole::Device));
        assert_eq!(layer_spec(tag(21, 0), Pdk::Gf180).role, LayerRole::Well);
        // PR_bndry en 0/0: contorno, nunca relleno que tape la celda.
        assert_eq!(layer_spec(tag(0, 0), Pdk::Gf180).role, LayerRole::Outline);
        // Fuera de tabla: label (10) contorno, dummy fill (4) dispositivo.
        assert_eq!(layer_spec(tag(21, 10), Pdk::Gf180).role, LayerRole::Outline);
        assert_eq!(layer_spec(tag(99, 4), Pdk::Gf180).role, LayerRole::Device);

        let act = layer_spec(tag(1, 0), Pdk::Ihp);
        assert_eq!((act.name, act.role), (Some("Activ"), LayerRole::Device));
        assert!(act.rank < layer_spec(tag(8, 0), Pdk::Ihp).rank, "Activ debajo de Metal1");
        assert_eq!(layer_spec(tag(10, 1), Pdk::Ihp).role, LayerRole::Outline, "label");
        assert_eq!(layer_spec(tag(8, 22), Pdk::Ihp).role, LayerRole::Device, "filler");
    }

    #[test]
    fn layers_only_in_the_lyp_get_name_and_color() {
        let ihp_curated = curated(Pdk::Ihp).len() as u32;
        let s = layer_spec(tag(32, 21), Pdk::Ihp);
        assert_eq!(s.name, Some("nBuLay.block"));
        assert_eq!(s.color, rgb(0x26, 0x8c, 0x6b));
        assert!(s.rank >= ihp_curated, "las generadas van encima de las curadas");
        // La tabla curada manda sobre la generada.
        assert_eq!(layer_spec(tag(8, 0), Pdk::Ihp).name, Some("Metal1"));
        assert_eq!(layer_spec(tag(34, 0), Pdk::Gf180).name, Some("Metal1"));
    }

    #[test]
    fn generated_tables_have_no_duplicate_layers() {
        for t in [crate::palette_generated::GF180_LYP, crate::palette_generated::IHP_LYP] {
            let tags: std::collections::HashSet<_> = t.iter().map(|l| l.tag).collect();
            assert_eq!(tags.len(), t.len());
        }
        assert!(crate::palette_generated::IHP_LYP.len() > 300);
    }
}
