//! Jerarquía de un esquemático de Xschem dentro de una versión.
//!
//! Una instancia `C {amp.sym} … {name=x1}` baja a un sub-esquemático si el
//! proyecto tiene su `.sch`: el del atributo `schematic=` si lo da, o el del
//! símbolo con otra extensión (`amp.sym` → `amp.sch`), como hace Xschem al
//! descender. Se busca junto al esquemático que la usa y desde la raíz de la
//! versión, en los mismos archivos (el mismo commit, o el disco): así un
//! diff ve el sub-esquemático de cada versión. Los símbolos del PDK no
//! tienen `.sch` en el proyecto: son hojas.
//!
//! Con la jerarquía de las dos versiones se sabe qué sub-esquemático cambió,
//! incluso por dentro (un cambio en `mirror.sch` también cambia a `amp.sch`
//! que la usa, y a `top.sch`): lo usan la lista de jerarquía del visor y el
//! diff (una instancia cuyo sub-esquemático cambió).

use std::collections::{BTreeMap, BTreeSet};

use viewer_core::files::{join_relative, FileSource};

use super::xschem::is_xschem;
use crate::core::domain::models::ChangeKind;

/// Tope de sub-esquemáticos por jerarquía (un proyecto real tiene decenas).
const MAX_NODES: usize = 500;

/// Una instancia que baja a un sub-esquemático del proyecto.
#[derive(Clone, Debug, PartialEq)]
pub struct SubInstance {
    /// Nombre de la instancia (`x1`).
    pub instance: String,
    /// Ruta del `.sch` (relativa a la raíz de la versión).
    pub schematic: String,
    /// Ruta del `.sym`, si está en el proyecto.
    pub symbol: Option<String>,
}

/// Un esquemático de la jerarquía, leído de una versión.
#[derive(Clone, Debug, Default)]
pub struct Node {
    pub bytes: Vec<u8>,
    /// El `.sym` por el que se llega (sus pines también son parte del
    /// sub-circuito). Vacío en la raíz o sin símbolo en el proyecto.
    pub symbol_bytes: Vec<u8>,
    pub children: Vec<SubInstance>,
}

/// La jerarquía de una versión: cada `.sch` alcanzable desde la raíz.
#[derive(Clone, Debug, Default)]
pub struct Hierarchy {
    pub top: String,
    pub nodes: BTreeMap<String, Node>,
}

/// Instancias de `text` (el esquemático `path`) con sub-esquemático en `files`.
pub fn sub_instances(text: &str, path: &str, files: &dyn FileSource) -> Vec<SubInstance> {
    let Ok(sch) = xschem_viewer::parser::parse(text) else { return Vec::new() };
    let mut out = Vec::new();
    for c in sch.components() {
        let name = c.properties.get("name").cloned().unwrap_or_default();
        let symbol = c.symbol_reference.trim();
        let wanted = match c.properties.get("schematic").map(|s| s.trim()).filter(|s| !s.is_empty()) {
            Some(s) => s.to_string(),
            None => match symbol.strip_suffix(".sym") {
                Some(stem) => format!("{stem}.sch"),
                None => continue,
            },
        };
        let Some(schematic) = find(&wanted, path, files, true) else { continue };
        let symbol = find(symbol, path, files, false);
        out.push(SubInstance { instance: name, schematic, symbol });
    }
    out
}

/// `rel` junto a `from` o desde la raíz de la versión, si existe (y es un
/// esquemático de Xschem, si `xschem`).
pub(crate) fn find(rel: &str, from: &str, files: &dyn FileSource, xschem: bool) -> Option<String> {
    [join_relative(from, rel), join_relative("", rel)].into_iter().find(|p| {
        files.read(p).is_some_and(|b| !xschem || is_xschem(&b))
    })
}

/// La jerarquía de `top` (con su contenido `top_bytes`) en `files`. Sin
/// `files`, solo la raíz.
pub fn collect(top_bytes: &[u8], top: &str, files: Option<&dyn FileSource>) -> Hierarchy {
    let mut h = Hierarchy { top: top.to_string(), nodes: BTreeMap::new() };
    let mut queue = vec![(top.to_string(), top_bytes.to_vec(), Vec::new())];
    while let Some((path, bytes, symbol_bytes)) = queue.pop() {
        if h.nodes.contains_key(&path) || h.nodes.len() >= MAX_NODES {
            continue;
        }
        let children = match (files, std::str::from_utf8(&bytes)) {
            (Some(f), Ok(text)) => sub_instances(text, &path, f),
            _ => Vec::new(),
        };
        if let Some(f) = files {
            for c in &children {
                if !h.nodes.contains_key(&c.schematic) {
                    let sub = f.read(&c.schematic).unwrap_or_default();
                    let sym = c.symbol.as_deref().and_then(|s| f.read(s)).unwrap_or_default();
                    queue.push((c.schematic.clone(), sub, sym));
                }
            }
        }
        h.nodes.insert(path, Node { bytes, symbol_bytes, children });
    }
    h
}

/// Cómo cambió cada esquemático entre dos versiones de la jerarquía: por
/// sí mismo (su `.sch` o su `.sym`), o por dentro (un sub-esquemático suyo
/// cambió). Los que no cambiaron no están.
pub fn changes(a: &Hierarchy, b: &Hierarchy) -> BTreeMap<String, ChangeKind> {
    let paths: BTreeSet<&String> = a.nodes.keys().chain(b.nodes.keys()).collect();
    let mut own: BTreeMap<String, ChangeKind> = BTreeMap::new();
    for p in &paths {
        let kind = match (a.nodes.get(*p), b.nodes.get(*p)) {
            (None, Some(_)) => Some(ChangeKind::Added),
            (Some(_), None) => Some(ChangeKind::Removed),
            (Some(x), Some(y)) if x.bytes != y.bytes || x.symbol_bytes != y.symbol_bytes => Some(ChangeKind::Modified),
            _ => None,
        };
        if let Some(k) = kind {
            own.insert((*p).clone(), k);
        }
    }
    // Por dentro: un nodo cambia si cambió alguno de sus hijos (en cualquiera
    // de las dos versiones). Punto fijo: la jerarquía es chica.
    let children = |p: &str| -> Vec<&str> {
        [a.nodes.get(p), b.nodes.get(p)]
            .into_iter()
            .flatten()
            .flat_map(|n| n.children.iter().map(|c| c.schematic.as_str()))
            .collect()
    };
    let mut all = own.clone();
    loop {
        let mut grew = false;
        for p in &paths {
            if !all.contains_key(p.as_str()) && children(p).iter().any(|c| all.contains_key(*c)) {
                all.insert((*p).clone(), ChangeKind::Modified);
                grew = true;
            }
        }
        if !grew {
            break;
        }
    }
    all
}

/// Qué cambió dentro de `schematic` para contarlo: él mismo, o el primer
/// sub-esquemático cambiado por debajo (`amp.sch → mirror.sch`).
pub fn inside_path(schematic: &str, a: &Hierarchy, b: &Hierarchy, changed: &BTreeMap<String, ChangeKind>) -> String {
    let own = |p: &str| match (a.nodes.get(p), b.nodes.get(p)) {
        (Some(x), Some(y)) => x.bytes != y.bytes || x.symbol_bytes != y.symbol_bytes,
        _ => true,
    };
    let mut path = vec![schematic.to_string()];
    let mut here = schematic.to_string();
    while !own(&here) && path.len() < 16 {
        let next = [a.nodes.get(&here), b.nodes.get(&here)]
            .into_iter()
            .flatten()
            .flat_map(|n| n.children.iter())
            .map(|c| c.schematic.clone())
            .find(|c| changed.contains_key(c) && !path.contains(c));
        match next {
            Some(n) => {
                path.push(n.clone());
                here = n;
            }
            None => break,
        }
    }
    path.join(" → ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    struct Mem(HashMap<String, Vec<u8>>);

    impl FileSource for Mem {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).cloned()
        }
    }

    fn sch(body: &str) -> Vec<u8> {
        format!("v {{xschem version=3.4.5 file_version=1.2}}\n{body}").into_bytes()
    }

    fn version(amp_r: &str, mirror_w: &str) -> Mem {
        Mem(HashMap::from([
            ("top.sch".to_string(), sch("C {amp.sym} 0 0 0 0 {name=x1}\nC {res.sym} 100 0 0 0 {name=R1 value=1k}\n")),
            ("amp.sym".to_string(), sch("B 5 -2.5 -2.5 2.5 2.5 {name=in dir=in}\n")),
            ("amp.sch".to_string(), sch(&format!("C {{lib/mirror.sym}} 0 0 0 0 {{name=x2}}\nC {{res.sym}} 0 0 0 0 {{name=R5 value={amp_r}}}\n"))),
            ("lib/mirror.sym".to_string(), sch("")),
            ("lib/mirror.sch".to_string(), sch(&format!("C {{nfet.sym}} 0 0 0 0 {{name=M1 W={mirror_w}}}\n"))),
        ]))
    }

    fn hier(files: &Mem) -> Hierarchy {
        collect(&files.read("top.sch").unwrap(), "top.sch", Some(files))
    }

    #[test]
    fn baja_a_los_sub_esquematicos_del_proyecto() {
        let v = version("1k", "1");
        let h = hier(&v);
        assert_eq!(h.nodes.keys().collect::<Vec<_>>(), ["amp.sch", "lib/mirror.sch", "top.sch"]);
        let top = &h.nodes["top.sch"];
        assert_eq!(top.children.len(), 1, "res.sym es del PDK: no baja");
        assert_eq!(top.children[0].instance, "x1");
        assert_eq!(top.children[0].symbol.as_deref(), Some("amp.sym"));
        assert!(!h.nodes["amp.sch"].symbol_bytes.is_empty());
    }

    #[test]
    fn un_cambio_abajo_sube_por_la_jerarquia() {
        let (a, b) = (version("1k", "1"), version("1k", "2"));
        let (ha, hb) = (hier(&a), hier(&b));
        let c = changes(&ha, &hb);
        assert_eq!(c.get("lib/mirror.sch"), Some(&ChangeKind::Modified));
        assert_eq!(c.get("amp.sch"), Some(&ChangeKind::Modified), "por dentro");
        assert_eq!(c.get("top.sch"), Some(&ChangeKind::Modified), "por dentro");
        assert_eq!(inside_path("amp.sch", &ha, &hb, &c), "amp.sch → lib/mirror.sch");

        let same = changes(&ha, &hier(&version("1k", "1")));
        assert!(same.is_empty());
        let own = changes(&ha, &hier(&version("2k", "1")));
        assert_eq!(inside_path("amp.sch", &ha, &hier(&version("2k", "1")), &own), "amp.sch");
        assert!(!own.contains_key("lib/mirror.sch"));
    }

    #[test]
    fn un_ciclo_no_cuelga() {
        let v = Mem(HashMap::from([
            ("a.sch".to_string(), sch("C {b.sym} 0 0 0 0 {name=x1}\n")),
            ("b.sch".to_string(), sch("C {a.sym} 0 0 0 0 {name=x2}\n")),
        ]));
        let h = collect(&v.read("a.sch").unwrap(), "a.sch", Some(&v));
        assert_eq!(h.nodes.len(), 2);
    }
}
