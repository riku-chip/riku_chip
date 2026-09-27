//! Layouts de Magic (`.mag`): de dónde salen las sub-celdas y en qué
//! unidades se leen. El lector está en el motor (`gdstk_rs::magic`), que no
//! sabe de Git ni de PDKs; acá se le da la función que encuentra cada celda.
//!
//! Una celda usada (`use inv  inv_0 [dir]`) se busca, en orden:
//! 1. en `dir` si el `use` lo da (relativo al archivo que la usa, o con
//!    `$PDK_ROOT`, `$PDKPATH`, `~`…);
//! 2. junto al archivo que la usa, en la misma versión (el mismo commit, o
//!    el disco);
//! 3. en los directorios de `$RIKU_MAG_PATH` (separados por `:`) y en las
//!    librerías del PDK de la línea `tech`
//!    (`$PDK_ROOT/<tech>/libs.ref/*/mag`), en disco.
//!
//! Una celda que no aparece queda vacía y se avisa.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};

use gdstk_rs::magic::{self, Found, MagInfo, MagOptions, MagSources, Request};
use gdstk_rs::Library;
use viewer_core::files::join_relative;
use viewer_core::FileSource;

/// `true` si el contenido es un `.mag` de Magic.
pub fn is_magic(content: &[u8]) -> bool {
    magic::is_mag(content)
}

/// Lambda (µm) de una tecnología de Magic: el `scalefactor` de la sección
/// `cifoutput` de su archivo `.tech`, leído del PDK instalado. Sin el PDK,
/// los valores conocidos de SKY130, GF180 e IHP. `$RIKU_MAG_LAMBDA` lo fuerza.
pub fn lambda_um(tech: Option<&str>) -> (f64, Option<String>) {
    if let Some(v) = std::env::var("RIKU_MAG_LAMBDA").ok().and_then(|v| v.parse::<f64>().ok()) {
        return (v, None);
    }
    if let Some(v) = tech.and_then(crate::pdk_tech::by_name).and_then(|t| t.lambda_um) {
        return (v, None);
    }
    let t = tech.unwrap_or("").to_ascii_lowercase();
    if t.starts_with("gf180") {
        (0.05, None)
    } else if t.starts_with("sky130") || t.starts_with("ihp") || t.contains("sg13g2") {
        (0.01, None)
    } else {
        let name = tech.unwrap_or("(sin tech)");
        (0.01, Some(format!("tecnología de Magic {name} desconocida y sin su .tech en $PDK_ROOT: se usa lambda = 0.01 µm (RIKU_MAG_LAMBDA lo cambia)")))
    }
}

use viewer_core::files::pdk_root;

/// Tecnologías del PDK con librerías `.mag` (`libs.ref/*/mag`) y cuántas
/// librerías tiene cada una, para `riku doctor`.
pub fn pdk_libraries() -> Vec<(String, usize)> {
    let Some(root) = pdk_root() else { return Vec::new() };
    let Ok(rd) = std::fs::read_dir(&root) else { return Vec::new() };
    let mut out: Vec<(String, usize)> = rd
        .flatten()
        .filter_map(|e| {
            let libs = std::fs::read_dir(e.path().join("libs.ref")).ok()?;
            let n = libs.flatten().filter(|l| l.path().join("mag").is_dir()).count();
            (n > 0).then(|| (e.file_name().to_string_lossy().to_string(), n))
        })
        .collect();
    out.sort();
    out
}

/// Celdas `.mag` de las librerías de una tecnología (y de `$RIKU_MAG_PATH`),
/// por nombre. Se arma una vez por tecnología: son miles de archivos.
fn library_cells(tech: Option<&str>) -> Arc<HashMap<String, PathBuf>> {
    static CACHE: OnceLock<Mutex<HashMap<String, Arc<HashMap<String, PathBuf>>>>> = OnceLock::new();
    let key = tech.unwrap_or("").to_string();
    let cache = CACHE.get_or_init(Default::default);
    if let Some(found) = cache.lock().unwrap().get(&key) {
        return found.clone();
    }
    let mut dirs: Vec<PathBuf> = std::env::var("RIKU_MAG_PATH")
        .unwrap_or_default()
        .split(':')
        .filter(|s| !s.is_empty())
        .map(PathBuf::from)
        .collect();
    if let (Some(root), Some(t)) = (pdk_root(), tech) {
        let libs = root.join(t).join("libs.ref");
        if let Ok(rd) = std::fs::read_dir(&libs) {
            let mut lib_dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path().join("mag")).filter(|p| p.is_dir()).collect();
            lib_dirs.sort();
            dirs.extend(lib_dirs);
        }
    }
    let mut cells = HashMap::new();
    for d in &dirs {
        let Ok(rd) = std::fs::read_dir(d) else { continue };
        let mut files: Vec<PathBuf> = rd.flatten().map(|e| e.path()).collect();
        files.sort();
        for f in files {
            if f.extension().is_some_and(|e| e == "mag") {
                if let Some(stem) = f.file_stem().and_then(|s| s.to_str()) {
                    // El primer directorio gana, como en la ruta de búsqueda de Magic.
                    cells.entry(stem.to_string()).or_insert(f.clone());
                }
            }
        }
    }
    let found = Arc::new(cells);
    cache.lock().unwrap().insert(key, found.clone());
    found
}

/// `$VAR`, `${VAR}` y `~` al principio de un directorio de `use`. `$PDKPATH`
/// y `$PDK_PATH` sin definir valen `$PDK_ROOT/<tech>`.
fn expand(dir: &str, tech: Option<&str>) -> String {
    if let Some(rest) = dir.strip_prefix('~') {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return format!("{}{rest}", home.to_string_lossy());
        }
    }
    let Some(rest) = dir.strip_prefix('$') else { return dir.to_string() };
    let braced = rest.starts_with('{');
    let rest = rest.trim_start_matches('{');
    let end = rest.find(|c: char| !(c.is_ascii_alphanumeric() || c == '_')).unwrap_or(rest.len());
    let (var, tail) = rest.split_at(end);
    let tail = if braced { tail.strip_prefix('}').unwrap_or(tail) } else { tail };
    let value = std::env::var(var).ok().or_else(|| match var {
        "PDKPATH" | "PDK_PATH" => pdk_root().zip(tech).map(|(r, t)| r.join(t).display().to_string()),
        "PDK_ROOT" | "PDKROOT" => pdk_root().map(|r| r.display().to_string()),
        _ => None,
    });
    match value {
        Some(v) => format!("{v}{tail}"),
        None => dir.to_string(),
    }
}

/// Lee `path`: una ruta absoluta del disco, o relativa a la raíz de `files`.
fn read(path: &str, files: Option<&dyn FileSource>) -> Option<Vec<u8>> {
    if Path::new(path).is_absolute() {
        std::fs::read(path).ok()
    } else {
        files?.read(path)
    }
}

/// Encuentra la celda de un `use` (ver el orden en la doc del módulo).
fn resolve(req: &Request<'_>, files: Option<&dyn FileSource>, tech: Option<&str>) -> Option<Found> {
    let file = format!("{}.mag", req.cell);
    let mut candidates = Vec::new();
    if let Some(d) = req.dir {
        let d = expand(d, tech);
        let p = format!("{}/{file}", d.trim_end_matches('/'));
        candidates.push(if Path::new(&d).is_absolute() { p } else { join_relative(req.parent, &p) });
    }
    candidates.push(join_relative(req.parent, &file));
    for c in candidates {
        if let Some(bytes) = read(&c, files) {
            return Some(Found { path: c, bytes });
        }
    }
    let p = library_cells(tech).get(req.cell)?.clone();
    let bytes = std::fs::read(&p).ok()?;
    Some(Found { path: p.display().to_string(), bytes })
}

/// Nombre de la celda de un archivo: `dir/inv.mag` → `inv`.
fn cell_name(path: &str) -> String {
    let base = path.rsplit(['/', '\\']).next().unwrap_or(path);
    base.strip_suffix(".mag").unwrap_or(base).to_string()
}

/// Todos los archivos de la jerarquía de un `.mag`. `path` es su ruta
/// relativa a la raíz de `files` (o absoluta en el disco).
pub fn collect(bytes: &[u8], path: &str, files: Option<&dyn FileSource>) -> Result<MagSources, String> {
    let tech = magic::parse(bytes).map_err(|e| e.to_string())?.tech;
    let name = cell_name(path);
    magic::collect(&name, path, bytes.to_vec(), |req| resolve(req, files, tech.as_deref())).map_err(|e| e.to_string())
}

/// `Library` de una jerarquía ya reunida, con lambda según su tecnología.
/// Cada capa de Magic toma el número estable de su nombre
/// ([`magic::default_layer_of`]): los dos lados de un diff coinciden sin
/// compartir nada, y los reportes muestran el nombre. Los avisos (celdas
/// que faltan, tecnología desconocida…) van en `MagInfo`.
pub fn build(sources: &MagSources) -> (Library, MagInfo) {
    let (lambda, warn) = lambda_um(sources.tech());
    let opts = MagOptions { lambda_um: lambda, layer_of: None, keep_hint_layers: false };
    let (lib, mut info) = Library::from_mag(sources, &opts);
    info.warnings.extend(warn);
    (lib, info)
}

/// Un puerto de una celda de Magic (`port 1 nsew signal input` bajo una
/// etiqueta), con todas las etiquetas que lo forman.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PortDesc {
    pub index: i64,
    /// Lados por los que se conecta (`nsew`).
    pub sides: String,
    /// signal, analog, power, ground, clock.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<String>,
    /// input, output, inout, tristate, feedthrough.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub class: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub shape: Option<String>,
    /// Capas de sus etiquetas, sin repetir.
    pub layers: Vec<String>,
    /// Rectángulos de sus etiquetas en µm, ordenados.
    pub rects_um: Vec<[f64; 4]>,
}

impl PortDesc {
    /// Lo que cambia la función del puerto (no su posición).
    fn functional(&self) -> (i64, &str, &Option<String>, &Option<String>, &Option<String>, &[String]) {
        (self.index, &self.sides, &self.usage, &self.class, &self.shape, &self.layers)
    }
}

/// Un puerto que aparece, desaparece o cambia entre dos versiones.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct PortChange {
    pub cell: String,
    pub name: String,
    pub before: Option<PortDesc>,
    pub after: Option<PortDesc>,
    /// Solo se movió (misma función, otra posición).
    pub cosmetic: bool,
}

/// Puertos de cada celda, por nombre.
fn ports_by_cell(info: &MagInfo) -> std::collections::BTreeMap<&str, std::collections::BTreeMap<&str, PortDesc>> {
    let mut cells = std::collections::BTreeMap::new();
    for c in &info.cells {
        let mut ports: std::collections::BTreeMap<&str, PortDesc> = std::collections::BTreeMap::new();
        for p in &c.ports {
            let round = |r: [f64; 4]| r.map(|v| (v * 1e6).round() / 1e6);
            let d = ports.entry(p.name.as_str()).or_insert_with(|| PortDesc {
                index: p.port.index,
                sides: p.port.sides.clone(),
                usage: p.port.usage.clone(),
                class: p.port.class.clone(),
                shape: p.port.shape.clone(),
                layers: Vec::new(),
                rects_um: Vec::new(),
            });
            d.layers.push(p.layer.clone());
            d.rects_um.push(round(p.rect_um));
        }
        for d in ports.values_mut() {
            d.layers.sort();
            d.layers.dedup();
            d.rects_um.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        }
        cells.insert(c.name.as_str(), ports);
    }
    cells
}

/// Puertos que cambiaron en las celdas que existen en las dos versiones
/// (una celda nueva o borrada ya se reporta como celda). Ordenados por celda
/// y nombre.
pub fn port_changes(a: Option<&MagInfo>, b: Option<&MagInfo>) -> Vec<PortChange> {
    let (Some(a), Some(b)) = (a, b) else { return Vec::new() };
    let (pa, pb) = (ports_by_cell(a), ports_by_cell(b));
    let mut out = Vec::new();
    for (cell, before) in &pa {
        let Some(after) = pb.get(cell) else { continue };
        let names: std::collections::BTreeSet<&&str> = before.keys().chain(after.keys()).collect();
        for name in names {
            let (x, y) = (before.get(*name), after.get(*name));
            let cosmetic = match (x, y) {
                (Some(x), Some(y)) if x == y => continue,
                (Some(x), Some(y)) => x.functional() == y.functional(),
                _ => false,
            };
            out.push(PortChange { cell: cell.to_string(), name: name.to_string(), before: x.cloned(), after: y.cloned(), cosmetic });
        }
    }
    out
}

/// Avisos para el usuario de una jerarquía leída.
pub fn notices(info: &MagInfo) -> Vec<String> {
    let mut out = Vec::new();
    if !info.missing.is_empty() {
        let mut m = info.missing.clone();
        m.sort();
        m.dedup();
        out.push(format!(
            "{} celda(s) de Magic sin archivo, comparadas vacías: {}",
            m.len(),
            m.join(", ")
        ));
    }
    out.extend(info.warnings.iter().cloned());
    out
}

/// Entradas de una jerarquía para la clave de la cache: nombre y bytes de
/// cada archivo, en orden.
pub fn cache_inputs(sources: &MagSources) -> Vec<&[u8]> {
    let mut v = Vec::with_capacity(2 * sources.files.len() + 1);
    for (name, bytes) in sources.contents() {
        v.push(name.as_bytes());
        v.push(bytes);
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Mem(HashMap<&'static str, &'static str>);
    impl FileSource for Mem {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).map(|s| s.as_bytes().to_vec())
        }
    }

    #[test]
    fn cells_are_found_next_to_the_file_and_in_the_use_dir() {
        let files = Mem(HashMap::from([
            ("chip/top.mag", "magic\ntech sky130A\nuse inv  i0\ntransform 1 0 0 0 1 0\nbox 0 0 1 1\nuse buf  b0 ../lib\ntransform 1 0 0 0 1 0\nbox 0 0 1 1\n"),
            ("chip/inv.mag", "magic\ntech sky130A\n<< metal1 >>\nrect 0 0 2 2\n"),
            ("lib/buf.mag", "magic\ntech sky130A\n<< metal1 >>\nrect 0 0 4 4\n"),
        ]));
        let top = files.read("chip/top.mag").unwrap();
        let s = collect(&top, "chip/top.mag", Some(&files)).unwrap();
        let paths: Vec<&str> = s.files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, ["chip/top.mag", "chip/inv.mag", "lib/buf.mag"]);
        assert!(s.missing.is_empty());
        let (lib, info) = build(&s);
        assert_eq!(lib.cell_count(), 3);
        assert!(notices(&info).is_empty(), "{:?}", notices(&info));
    }

    #[test]
    fn missing_cells_and_unknown_tech_are_reported() {
        let top = b"magic\ntech scmos\nuse ghost  g\ntransform 1 0 0 0 1 0\nbox 0 0 1 1\n";
        let s = collect(top, "top.mag", None).unwrap();
        assert_eq!(s.missing, vec!["ghost".to_string()]);
        let (_, info) = build(&s);
        let n = notices(&info);
        assert!(n[0].contains("ghost"), "{n:?}");
        assert!(n.iter().any(|w| w.contains("scmos")), "{n:?}");
    }

    #[test]
    fn ports_that_change_function_or_only_move() {
        let cell = |ports: &str| format!("magic\ntech sky130A\nmagscale 1 2\n<< labels >>\n{ports}<< end >>\n");
        let read = |text: &str| build(&collect(text.as_bytes(), "inv.mag", None).unwrap()).1;
        let a = read(&cell("rlabel locali s 0 0 10 10 0 A\nport 1 nsew signal input\nrlabel locali s 0 0 10 10 0 Y\nport 2 nsew signal output\nrlabel metal1 s 0 0 10 10 0 VGND\nport 3 nsew ground bidirectional\n"));
        let b = read(&cell("rlabel locali s 0 0 10 10 0 A\nport 1 nsew signal inout\nrlabel locali s 50 0 60 10 0 Y\nport 2 nsew signal output\nrlabel locali s 0 0 10 10 0 EN\nport 4 nsew signal input\n"));
        let changes = port_changes(Some(&a), Some(&b));
        let by = |n: &str| changes.iter().find(|c| c.name == n).unwrap_or_else(|| panic!("{n}: {changes:#?}"));
        // A: input → inout, funcional.
        let c = by("A");
        assert_eq!(c.before.as_ref().unwrap().class.as_deref(), Some("input"));
        assert_eq!(c.after.as_ref().unwrap().class.as_deref(), Some("inout"));
        assert!(!c.cosmetic);
        // Y: solo se movió.
        assert!(by("Y").cosmetic);
        // EN nuevo, VGND quitado.
        assert!(by("EN").before.is_none() && by("EN").after.is_some());
        assert!(by("VGND").after.is_none());
        assert_eq!(changes.len(), 4);
        // Sin la versión anterior no hay comparación de puertos.
        assert!(port_changes(None, Some(&b)).is_empty());
    }

    #[test]
    fn lambda_by_technology() {
        assert_eq!(lambda_um(Some("sky130A")).0, 0.01);
        assert_eq!(lambda_um(Some("gf180mcuD")).0, 0.05);
        assert_eq!(lambda_um(Some("ihp-sg13g2")).0, 0.01);
        assert!(lambda_um(None).1.is_some());
    }

    #[test]
    fn variables_in_use_dirs() {
        assert_eq!(expand("plain/dir", None), "plain/dir");
        // SAFETY: ninguna otra prueba lee esta variable.
        unsafe { std::env::set_var("RIKU_TEST_MAG_DIR", "/x/y") };
        assert_eq!(expand("$RIKU_TEST_MAG_DIR/mag", None), "/x/y/mag");
        assert_eq!(expand("${RIKU_TEST_MAG_DIR}/mag", None), "/x/y/mag");
        assert_eq!(expand("$NO_SUCH_VAR_RIKU/mag", None), "$NO_SUCH_VAR_RIKU/mag");
        assert_eq!(cell_name("a/b/inv_1.mag"), "inv_1");
    }
}
