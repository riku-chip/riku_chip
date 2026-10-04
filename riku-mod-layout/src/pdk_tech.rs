//! Capas y escala leídas del PDK instalado, al ejecutar: riku no necesita
//! conocer un PDK de antemano para dibujar sus layouts.
//!
//! Por cada PDK de `$PDK_ROOT` (o `/foss/pdks`) y `$PDKPATH` se lee:
//! - el `.lyp` de KLayout (`libs.tech/klayout[/tech]/*.lyp`): nombre, color
//!   y estilo de cada capa GDS;
//! - el `.tech` de Magic (`libs.tech/magic/<pdk>.tech`, con sus `include`):
//!   lambda (`scalefactor` de `cifoutput`) y la capa GDS en que se escribe
//!   cada tipo de Magic (`layer MET1 *metal1` + `calma 68 20`).
//!
//! Qué PDK corresponde a un layout lo deciden sus capas (el que más conoce;
//! en empate, `$PDK`), no su nombre. [`crate::process::Process`] lo junta
//! con las tablas compiladas de `palette.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::devices::DeviceRules;
use crate::palette::LayerRole;
use crate::style::Color;

/// Una capa del `.lyp`.
#[derive(Clone, Debug, PartialEq)]
pub struct TechLayer {
    pub tag: (u32, u32),
    pub name: String,
    pub color: Color,
    pub role: LayerRole,
}

/// Un tipo de capa de Magic: la capa GDS en que se escribe y si se dibuja
/// solo con contorno (obstrucciones, bloqueos, comentarios).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MagicType {
    pub tag: (u32, u32),
    pub outline: bool,
}

/// Lo que riku sabe de un PDK instalado.
#[derive(Debug, Default)]
pub struct Tech {
    /// Nombre de la carpeta (`sky130A`, `ihp-sg13g2`).
    pub name: String,
    /// Capas del `.lyp`, en su orden (que se usa como apilado).
    pub layers: Vec<TechLayer>,
    index: HashMap<(u32, u32), usize>,
    /// Tipo de Magic (y sus alias) → capa GDS y si va solo con contorno.
    magic: HashMap<String, MagicType>,
    /// Lambda de Magic en µm.
    pub lambda_um: Option<f64>,
    /// Carpeta del PDK (vacía en los armados a mano).
    dir: PathBuf,
    /// Reglas de transistores del `.tech`: se leen la primera vez que se piden.
    devices: OnceLock<Option<DeviceRules>>,
}

impl Tech {
    pub(crate) fn new(name: String, layers: Vec<TechLayer>, magic: HashMap<String, MagicType>, lambda_um: Option<f64>) -> Self {
        let mut index = HashMap::new();
        for (i, l) in layers.iter().enumerate() {
            index.entry(l.tag).or_insert(i);
        }
        Self { name, layers, index, magic, lambda_um, ..Default::default() }
    }

    /// Reglas de transistores del `.tech` de Magic del PDK (ver
    /// [`crate::devices`]); `None` si no hay `.tech` o no define ninguno.
    pub fn device_rules(&self) -> Option<&DeviceRules> {
        self.devices
            .get_or_init(|| {
                let magic_dir = self.dir.join("libs.tech").join("magic");
                read_tech(&magic_dir, &self.name, 0).and_then(|t| DeviceRules::parse(&t))
            })
            .as_ref()
    }

    /// Capa del `.lyp` y su posición.
    pub fn layer(&self, tag: (u32, u32)) -> Option<(usize, &TechLayer)> {
        self.index.get(&tag).map(|&i| (i, &self.layers[i]))
    }

    /// Capa GDS en que Magic escribe el tipo `name`.
    pub fn magic_type(&self, name: &str) -> Option<MagicType> {
        self.magic.get(name).copied()
    }

    /// Todos los tipos de Magic que conoce.
    pub fn magic_types(&self) -> impl Iterator<Item = (&str, MagicType)> {
        self.magic.iter().map(|(n, t)| (n.as_str(), *t))
    }
}

/// PDK instalados, `$PDK` primero. Se leen una vez por proceso.
pub fn installed() -> &'static [Tech] {
    static ALL: OnceLock<Vec<Tech>> = OnceLock::new();
    ALL.get_or_init(|| {
        let mut dirs: Vec<PathBuf> = viewer_core::files::pdk_root()
            .and_then(|r| std::fs::read_dir(r).ok())
            .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.join("libs.tech").is_dir()).collect())
            .unwrap_or_default();
        if let Some(p) = std::env::var_os("PDKPATH").map(PathBuf::from).filter(|p| p.join("libs.tech").is_dir()) {
            if !dirs.iter().any(|d| d.file_name() == p.file_name()) {
                dirs.push(p);
            }
        }
        let active = std::env::var("PDK").unwrap_or_default();
        dirs.sort_by_key(|d| {
            let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            (name != active, name)
        });
        dirs.iter().filter_map(|d| load(d)).collect()
    })
}

/// PDK instalado de nombre `name` (la línea `tech` de un `.mag`).
pub fn by_name(name: &str) -> Option<&'static Tech> {
    installed().iter().find(|t| t.name == name)
}

/// El PDK instalado (entre los que acepta `fits`) cuyo `.lyp` conoce más
/// capas del layout. Hace falta que conozca al menos dos y un tercio de ellas.
pub fn for_tags(tags: &[(u32, u32)], fits: impl Fn(&Tech) -> bool) -> Option<&'static Tech> {
    let hits = |t: &Tech| tags.iter().filter(|&&tag| t.layer(tag).is_some()).count();
    best(installed(), fits, hits, |n| n >= 2 && n * 3 >= tags.len())
}

/// El PDK instalado (entre los que acepta `fits`) cuyo `.tech` de Magic
/// conoce más tipos del layout.
pub fn for_magic(names: &[&str], fits: impl Fn(&Tech) -> bool) -> Option<&'static Tech> {
    let hits = |t: &Tech| names.iter().filter(|n| t.magic.contains_key(**n)).count();
    best(installed(), fits, hits, |n| n >= 1)
}

/// El de más aciertos; en empate, el primero (`$PDK` va primero).
fn best<'t>(
    techs: &'t [Tech],
    fits: impl Fn(&Tech) -> bool,
    hits: impl Fn(&Tech) -> usize,
    enough: impl Fn(usize) -> bool,
) -> Option<&'t Tech> {
    let mut found: Option<(usize, &Tech)> = None;
    for t in techs.iter().filter(|t| fits(t)) {
        let n = hits(t);
        if enough(n) && found.is_none_or(|(m, _)| n > m) {
            found = Some((n, t));
        }
    }
    found.map(|(_, t)| t)
}

/// Lee el `.lyp` y el `.tech` de un PDK. `None` si no tiene ninguno.
pub fn load(dir: &Path) -> Option<Tech> {
    let name = dir.file_name()?.to_string_lossy().to_string();
    let layers = find_lyp(dir).and_then(|p| std::fs::read_to_string(p).ok()).map(|t| parse_lyp(&t)).unwrap_or_default();
    let magic_dir = dir.join("libs.tech").join("magic");
    let (magic, lambda) = read_tech(&magic_dir, &name, 0).map(|t| parse_magic_tech(&t)).unwrap_or_default();
    if layers.is_empty() && magic.is_empty() && lambda.is_none() {
        return None;
    }
    let mut tech = Tech::new(name, layers, magic, lambda);
    tech.dir = dir.to_path_buf();
    Some(tech)
}

/// El `.lyp` del PDK (no los de `xsect`, que son de cortes transversales).
fn find_lyp(dir: &Path) -> Option<PathBuf> {
    let klayout = dir.join("libs.tech").join("klayout");
    [klayout.join("tech"), klayout].iter().find_map(|d| {
        let mut found: Vec<PathBuf> = std::fs::read_dir(d)
            .ok()?
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "lyp") && p.is_file())
            .collect();
        found.sort();
        found.into_iter().next()
    })
}

/// Texto del `.tech` de Magic con sus `include` ya puestos en su lugar.
fn read_tech(dir: &Path, name: &str, depth: u32) -> Option<String> {
    let path = [dir.join(format!("{name}.tech")), dir.join(name)].into_iter().find(|p| p.is_file())?;
    let text = std::fs::read_to_string(path).ok()?;
    if depth > 4 {
        return Some(text);
    }
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        match line.trim().strip_prefix("include ") {
            Some(inc) => out.push_str(&read_tech(dir, inc.trim(), depth + 1).unwrap_or_default()),
            None => out.push_str(line),
        }
        out.push('\n');
    }
    Some(out)
}

// ---- .lyp ----

/// Capas de un `.lyp` de KLayout: una por `layer/datatype`, la primera que
/// aparece.
pub fn parse_lyp(text: &str) -> Vec<TechLayer> {
    let mut out: Vec<TechLayer> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("<properties>") {
        let body = &rest[start + "<properties>".len()..];
        // Hasta la siguiente capa o el cierre: los grupos anidan `properties`.
        let end = ["</properties>", "<properties>"].iter().filter_map(|t| body.find(t)).min().unwrap_or(body.len());
        let block = &body[..end];
        rest = &body[end..];
        let Some(tag) = field(block, "source").and_then(source_tag) else { continue };
        if out.iter().any(|l| l.tag == tag) {
            continue;
        }
        let suffix = format!(" - {}/{}", tag.0, tag.1);
        let name = field(block, "name")
            .map(|n| unescape(n.strip_suffix(suffix.as_str()).unwrap_or(n).trim()))
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("{}/{}", tag.0, tag.1));
        let color = field(block, "fill-color").or_else(|| field(block, "frame-color")).and_then(hex_color);
        let hollow = field(block, "dither-pattern") == Some("I1") || field(block, "visible") == Some("false");
        let role = role_of(&name, hollow);
        out.push(TechLayer { tag, name, color: color.unwrap_or(Color::rgba(128, 128, 128, 255)), role });
    }
    out
}

fn field<'a>(block: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{tag}>");
    let s = block.find(&open)? + open.len();
    let e = block[s..].find('<')?;
    Some(block[s..s + e].trim())
}

/// `68/20`, `68/20@1` o `met1 68/20@1`.
fn source_tag(src: &str) -> Option<(u32, u32)> {
    let last = src.split_whitespace().last()?;
    let (l, d) = last.split('@').next()?.split_once('/')?;
    Some((l.parse().ok()?, d.parse().ok()?))
}

fn hex_color(s: &str) -> Option<Color> {
    let h = s.trim().trim_start_matches('#');
    let h = h.get(h.len().checked_sub(6)?..)?;
    let v = u32::from_str_radix(h, 16).ok()?;
    Some(Color::rgba((v >> 16) as u8, (v >> 8) as u8, v as u8, 255))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<").replace("&gt;", ">").replace("&quot;", "\"").replace("&apos;", "'").replace("&amp;", "&")
}

/// Rol de una capa por su nombre y estilo: sin tramado o invisible, o con
/// propósito que no es de dibujo (`.pin`, `.label`, `.boundary`…), solo
/// contorno; los pozos, apenas un tinte.
fn role_of(name: &str, hollow: bool) -> LayerRole {
    let lower = name.to_ascii_lowercase();
    let (base, purpose) = match lower.split_once('.') {
        Some((b, p)) => (b, Some(p)),
        None => (lower.as_str(), None),
    };
    const DRAWN: [&str; 5] = ["drawing", "drw", "mask", "filler", "fill"];
    const MARKS: [&str; 8] = ["pin", "label", "text", "txt", "bound", "_mk", "marker", "block"];
    if hollow || purpose.is_some_and(|p| !DRAWN.contains(&p)) || MARKS.iter().any(|m| base.contains(m)) {
        LayerRole::Outline
    } else if base.contains("well") {
        LayerRole::Well
    } else {
        LayerRole::Device
    }
}

// ---- .tech de Magic ----

/// Tipo de Magic → capa GDS, y lambda (µm), de un `.tech`. Solo mira el
/// primer estilo de `cifoutput`, el que escribe el GDS.
pub fn parse_magic_tech(text: &str) -> (HashMap<String, MagicType>, Option<f64>) {
    let sections = sections(text);
    let get = |s: &'static str| sections.iter().filter(move |(n, _)| n == s).flat_map(|(_, l)| l.iter().map(String::as_str));

    // Tipo (o alias de `types`) → nombre canónico; y el grupo de nombres.
    let mut canonical: HashMap<&str, &str> = HashMap::new();
    let mut names_of: HashMap<&str, Vec<&str>> = HashMap::new();
    let mut plane_of: HashMap<&str, &str> = HashMap::new();
    for line in get("types") {
        let mut parts = line.trim_start_matches('-').split_whitespace();
        let (Some(plane), Some(list)) = (parts.next(), parts.next()) else { continue };
        let names: Vec<&str> = list.split(',').filter(|n| !n.is_empty()).collect();
        if let Some(first) = names.first() {
            for n in &names {
                canonical.insert(n, first);
            }
            plane_of.insert(first, plane);
            names_of.insert(first, names);
        }
    }
    let mut aliases: HashMap<&str, &str> = HashMap::new();
    for line in get("aliases") {
        let mut parts = line.split_whitespace();
        if let (Some(a), Some(list)) = (parts.next(), parts.next()) {
            aliases.insert(a, list);
        }
    }

    // Por tipo: la primera capa GDS que lo nombra explícitamente (sin `*`,
    // que también arrastra los contactos); si no, la primera con `*`.
    let mut best: HashMap<&str, ((u32, u32), bool)> = HashMap::new();
    let mut lambda = None;
    let mut styles = 0;
    let mut current: Vec<(&str, bool)> = Vec::new();
    for line in get("cifoutput") {
        let mut words = line.split_whitespace();
        match words.next() {
            Some("style") => {
                styles += 1;
                if styles > 1 {
                    break;
                }
            }
            Some("scalefactor") if lambda.is_none() => lambda = scalefactor_um(words.collect()),
            Some("layer") => {
                current.clear();
                if let (Some(_), Some(list)) = (words.next(), words.next()) {
                    expand(list, &aliases, &canonical, false, 0, &mut current);
                }
            }
            Some("templayer") => current.clear(),
            Some("calma") => {
                let (Some(l), Some(d)) = (words.next().and_then(|w| w.parse().ok()), words.next().and_then(|w| w.parse().ok()))
                else {
                    continue;
                };
                for &(t, explicit) in &current {
                    match best.get(t) {
                        Some((_, true)) => {}
                        Some((_, false)) if !explicit => {}
                        _ => {
                            best.insert(t, ((l, d), explicit));
                        }
                    }
                }
                current.clear();
            }
            _ => {}
        }
    }

    let mut map = HashMap::new();
    for (t, (tag, _)) in best {
        let plane = plane_of.get(t).copied().unwrap_or("");
        let outline = t.starts_with("obs") || matches!(plane, "block" | "comment");
        for n in names_of.get(t).cloned().unwrap_or_else(|| vec![t]) {
            map.insert(n.to_string(), MagicType { tag, outline });
        }
    }
    (map, lambda)
}

/// Tipos de una lista de `cifoutput` (`*metal1,rm1,allnwell`), con los alias
/// expandidos y en su nombre canónico. `bool` = nombrado sin `*`.
fn expand<'a>(
    list: &'a str,
    aliases: &HashMap<&'a str, &'a str>,
    canonical: &HashMap<&'a str, &'a str>,
    starred: bool,
    depth: u32,
    out: &mut Vec<(&'a str, bool)>,
) {
    for token in list.split(',').filter(|t| !t.is_empty()) {
        let star = starred || token.starts_with('*');
        let t = token.trim_start_matches('*');
        match aliases.get(t) {
            Some(sub) if depth < 8 => expand(sub, aliases, canonical, star, depth + 1, out),
            _ => out.push((canonical.get(t).copied().unwrap_or(t), !star)),
        }
    }
}

/// `scalefactor 10 nanometers` → 0.01 µm. Sin unidad, en centimicras;
/// con un segundo número, el divisor (`reducer`).
fn scalefactor_um(words: Vec<&str>) -> Option<f64> {
    let nums: Vec<f64> = words.iter().filter_map(|w| w.parse::<f64>().ok()).collect();
    let value = nums.first()? / nums.get(1).copied().unwrap_or(1.0);
    let unit = if words.contains(&"nanometers") {
        1e-3
    } else if words.contains(&"angstroms") {
        1e-4
    } else {
        1e-2
    };
    Some(value * unit).filter(|v| *v > 0.0)
}

/// Secciones de primer nivel (`types`, `cifoutput`…) con sus líneas, sin
/// comentarios.
fn sections(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut open = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        if !open {
            out.push((line.split_whitespace().next().unwrap_or("").to_string(), Vec::new()));
            open = true;
        } else if line == "end" {
            open = false;
        } else if let Some(last) = out.last_mut() {
            last.1.push(line.to_string());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const LYP: &str = r#"<?xml version="1.0"?>
<layer-properties>
 <properties>
  <fill-color>#39bfff</fill-color>
  <dither-pattern>C1</dither-pattern>
  <name>met1.drawing - 68/20</name>
  <source>68/20@1</source>
 </properties>
 <properties>
  <fill-color>#ff0000</fill-color>
  <dither-pattern>C1</dither-pattern>
  <name>met1.pin - 68/16</name>
  <source>68/16@1</source>
 </properties>
 <properties>
  <fill-color>#00ff00</fill-color>
  <dither-pattern>C2</dither-pattern>
  <name>Nwell</name>
  <source>21/0@1</source>
 </properties>
 <properties>
  <fill-color>#ffffff</fill-color>
  <dither-pattern>I1</dither-pattern>
  <name>Substrate.drawing</name>
  <source>40/0</source>
 </properties>
</layer-properties>"#;

    #[test]
    fn lyp_names_colors_and_roles() {
        let layers = parse_lyp(LYP);
        let t = Tech::new("x".into(), layers, HashMap::new(), None);
        let (i, met1) = t.layer((68, 20)).unwrap();
        assert_eq!(
            (i, met1.name.as_str(), met1.color, met1.role),
            (0, "met1.drawing", Color::rgba(0x39, 0xbf, 0xff, 255), LayerRole::Device)
        );
        assert_eq!(t.layer((68, 16)).unwrap().1.role, LayerRole::Outline);
        assert_eq!(t.layer((21, 0)).unwrap().1.role, LayerRole::Well);
        assert_eq!(t.layer((40, 0)).unwrap().1.role, LayerRole::Outline, "sin tramado");
        assert!(t.layer((1, 0)).is_none());
    }

    const TECH: &str = "
tech
  format 35
  demo
end

types
 active  ndiff,ndf
 active  ndiffc,ndc
 metal1  metal1,m1
 -metal1 obsm1
end

aliases
 alldiff  *ndiff
end

cifoutput
style gdsii
 scalefactor 50 nanometers
 layer DIFF alldiff
    calma 22 0
 templayer TMP ndiff
 layer CONT ndc
    squares 0 170 170
    calma 33 0
 layer MET1 *m1
    calma 34 0
style other
 scalefactor 1
 layer X m1
    calma 99 0
end
";

    #[test]
    fn magic_tech_maps_types_to_gds_and_reads_lambda() {
        let (map, lambda) = parse_magic_tech(TECH);
        assert_eq!(lambda, Some(0.05));
        let tag = |n: &str| map.get(n).map(|t| t.tag);
        assert_eq!(tag("ndiff"), Some((22, 0)));
        assert_eq!(tag("ndf"), Some((22, 0)), "alias de types");
        // El contacto va a su capa explícita, no a la de `*ndiff`.
        assert_eq!(tag("ndiffc"), Some((33, 0)));
        assert_eq!(tag("m1"), Some((34, 0)), "solo el primer estilo");
        assert!(!map.contains_key("obsm1"));
    }

    #[test]
    fn scalefactor_units() {
        assert_eq!(scalefactor_um(vec!["10", "nanometers"]), Some(0.01));
        assert_eq!(scalefactor_um(vec!["100", "2"]), Some(0.5));
        assert_eq!(scalefactor_um(vec!["5"]), Some(0.05));
    }

    /// Un PDK que riku no trae compilado: capas GDS y de Magic salen solo de
    /// su `.lyp` y su `.tech`.
    #[test]
    fn unknown_pdk_is_styled_from_its_own_files() {
        use crate::process::Process;
        use crate::style::Pdk;
        use gdstk_rs::GdsTag;
        let lyp = LYP.replace("68/20", "7/0");
        let tech_text = TECH.replace("calma 34 0", "calma 7 0");
        let (magic, lambda) = parse_magic_tech(&tech_text);
        let tech: &'static Tech = Box::leak(Box::new(Tech::new("acme".into(), parse_lyp(&lyp), magic, lambda)));
        let process: &'static Process = Box::leak(Box::new(Process::build(Pdk::Generic, Some(tech))));
        assert_eq!(process.name, "acme");
        let gds = process.layer_spec(GdsTag { layer: 7, datatype: 0 });
        assert_eq!(
            (gds.name, gds.color, gds.role),
            (Some("met1.drawing"), Color::rgba(0x39, 0xbf, 0xff, 255), LayerRole::Device)
        );
        let mag = process.magic_spec("m1", GdsTag { layer: 1 << 30, datatype: 0 });
        assert_eq!((mag.color, mag.rank), (gds.color, gds.rank));
    }

    /// Con los PDK de iic-osic-tools instalados, lo leído coincide con lo
    /// que ya se sabía de SKY130, GF180 e IHP.
    #[test]
    fn installed_pdks_agree_with_known_values() {
        for (name, lambda, gds) in [("sky130A", 0.01, (68, 20)), ("gf180mcuD", 0.05, (34, 0)), ("ihp-sg13g2", 0.01, (8, 0))] {
            let Some(t) = by_name(name) else { continue };
            assert_eq!(t.lambda_um, Some(lambda), "{name}");
            assert_eq!(t.magic_type("metal1").map(|m| m.tag), Some(gds), "{name}");
            assert!(t.layer(gds).is_some(), "{name}: {gds:?} en el .lyp");
        }
    }
}
