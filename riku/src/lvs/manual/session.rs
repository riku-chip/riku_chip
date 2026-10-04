//! Un par en una versión (el disco o un commit): las extracciones con su caché, el archivo de vínculos, el historial y el JSON.

use super::*;
use std::collections::{BTreeMap};
use serde::{Deserialize, Serialize};
use crate::i18n::tr;

/// El archivo de vínculos de una celda, relativo a la raíz del proyecto.
pub fn map_path(cell: &str) -> String {
    format!("lvs/{cell}.toml")
}

/// Lo caro de una sesión: las dos extracciones (el esquemático pasado a
/// netlist y el layout a transistores y redes). Va a la caché por
/// dependencias: se reusa si nada de lo que leyó cambió.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Sides {
    /// La celda comparada del layout.
    pub cell: String,
    pub schematic: Vec<SchDevice>,
    pub layout: Vec<LayDevice>,
    pub sch_ports: Vec<String>,
    pub lay_ports: Vec<String>,
    /// Lo que no se revisa, de los dos lados.
    pub unchecked: Vec<String>,
    /// µm por unidad de la escena del layout (para dibujar sobre ella).
    pub unit_um: f64,
    /// El recuadro de cada instancia del esquemático (para dibujar sobre él).
    pub boxes: BTreeMap<String, (f64, f64, f64, f64)>,
    pub warnings: Vec<String>,
}

/// Los dos lados de un par en una versión y su archivo de vínculos.
pub struct Session {
    pub cell: String,
    /// Dónde va el archivo (relativo a la raíz).
    pub map_path: String,
    /// El archivo (uno vacío si todavía no existe).
    pub map: MapFile,
    pub exists: bool,
    /// El archivo vino del disco porque la versión pedida no lo tiene.
    pub from_disk: bool,
    /// Las extracciones salieron de la caché.
    pub cached: bool,
    pub schematic: Vec<SchDevice>,
    pub layout: Vec<LayDevice>,
    pub warnings: Vec<String>,
    pub unit_um: f64,
    pub boxes: BTreeMap<String, (f64, f64, f64, f64)>,
    pub sch_ports: Vec<String>,
    pub lay_ports: Vec<String>,
    pub unchecked: Vec<String>,
}

/// Las dos extracciones de `pair`, con los archivos de `files`.
pub(super) fn extract(pair: &crate::lvs::Pair, files: std::sync::Arc<dyn viewer_core::FileSource>) -> Result<Sides, String> {
    let s = crate::lvs::schematic_netlist(pair, files.clone())?;
    let places = &s.netlist.places;
    let schematic = schematic_devices(&s.netlist.text, &s.stem, &|n| places.instance(n).map(str::to_string));
    let (sch_ports, mut unchecked) = schematic_extras(&s.netlist.text, &s.stem, &|n| places.instance(n).map(str::to_string));
    let bytes = files.read(&pair.layout).ok_or_else(|| format!("{}: {}", pair.layout, tr!("lvs.cannot_read")))?;
    let ln = riku_mod_layout::nets::layout_netlist(&bytes, &pair.layout, Some(files.as_ref()), pair.cell.as_deref())?;
    let layout = layout_devices(&ln);
    let lay_ports = ln.netlist.ports();
    if !ln.netlist.resistors.is_empty() {
        unchecked.push(tr!("lvs_map.unchecked_lay_res", count = ln.netlist.resistors.len()));
    }
    let mut warnings: Vec<String> = s.netlist.warnings.clone();
    warnings.extend(ln.netlist.warnings.iter().cloned());
    Ok(Sides { cell: ln.cell, schematic, layout, sch_ports, lay_ports, unchecked, unit_um: ln.unit_um, boxes: places.instances.clone(), warnings })
}

/// Cambia cuando cambia lo que guarda [`Sides`] o cómo se extrae.
pub(super) const SIDES_VERSION: u32 = 1;

/// Lo que determina la extracción y no pasa por los archivos del proyecto:
/// las versiones (de Riku, del netlister, de [`Sides`]) y el PDK del
/// esquemático. `None` si no se sabe el PDK (la extracción falla igual).
pub(super) fn sides_env(schematic: &str) -> Option<String> {
    use std::hash::{Hash, Hasher};
    let (pdk, dir) = super::pdk_of(schematic).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (env!("CARGO_PKG_VERSION"), xschem_viewer::spice::VERSION, SIDES_VERSION, &pdk, &dir).hash(&mut h);
    for f in [".config/nodeinfo.json", "libs.tech/xschem/xschemrc"] {
        std::fs::read(dir.join(f)).ok().hash(&mut h);
    }
    format!("{:?}", crate::modules::xschem_pdk::symbol_source_for(schematic).paths()).hash(&mut h);
    Some(format!("{:016x}", h.finish()))
}

/// Una entrada de la caché: el entorno, lo que se leyó y las extracciones.
#[derive(Serialize, Deserialize)]
pub(super) struct CacheEntry {
    env: String,
    deps: super::cache::Deps,
    sides: Sides,
}

/// `<caché de LVS>/manual-v1/<par>`; `None` con `RIKU_NO_CACHE`.
pub(super) fn sides_dir(pair: &crate::lvs::Pair) -> Option<std::path::PathBuf> {
    Some(super::cache::cache_dir()?.join("manual-v1").join(super::cache::pair_key(pair)))
}

/// Unas extracciones guardadas que valen para `v`.
pub(super) fn lookup(pair: &crate::lvs::Pair, env: &str, v: &dyn super::cache::Version) -> Option<Sides> {
    let dir = sides_dir(pair)?;
    let mut files: Vec<(std::time::SystemTime, std::path::PathBuf)> =
        std::fs::read_dir(&dir).ok()?.flatten().filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path()))).collect();
    files.sort_by(|a, b| b.0.cmp(&a.0));
    files.into_iter().find_map(|(_, f)| {
        let entry: CacheEntry = serde_json::from_str(&std::fs::read_to_string(f).ok()?).ok()?;
        (entry.env == env && super::cache::valid(&entry.deps, v)).then_some(entry.sides)
    })
}

pub(super) fn store(pair: &crate::lvs::Pair, env: &str, deps: super::cache::Deps, sides: &Sides) {
    use std::hash::{Hash, Hasher};
    let Some(dir) = sides_dir(pair) else { return };
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (env, &deps).hash(&mut h);
    let entry = CacheEntry { env: env.to_string(), deps, sides: sides.clone() };
    if std::fs::create_dir_all(&dir).is_ok() {
        let _ = serde_json::to_string(&entry).map(|t| std::fs::write(dir.join(format!("{:016x}.json", h.finish())), t));
        super::cache::prune(&dir);
    }
}

/// Las extracciones de `pair` en la versión `v`: de la caché si nada de lo
/// que leyeron cambió; si no, se extraen de los archivos que da `tree` (y
/// se guardan). `true` si vinieron de la caché.
pub(super) fn sides(pair: &crate::lvs::Pair, v: &dyn super::cache::Version, tree: &dyn Fn() -> Result<crate::lvs::Tree, String>) -> Result<(Sides, bool), String> {
    use std::sync::Arc;
    let text = v
        .read(&pair.schematic)
        .and_then(|b| String::from_utf8(b).ok())
        .ok_or_else(|| format!("{}: {}", pair.schematic, tr!("lvs.cannot_read")))?;
    let env = sides_env(&text);
    if let Some(found) = env.as_deref().and_then(|e| lookup(pair, e, v)) {
        return Ok((found, true));
    }
    let t = tree()?;
    let rec = Arc::new(super::cache::RecordingFiles::new(Arc::new(viewer_core::DiskFiles::new(t.root.clone()))));
    let extracted = extract(pair, rec.clone())?;
    if let Some(e) = env {
        store(pair, &e, rec.deps(), &extracted);
    }
    Ok((extracted, false))
}

/// [`Session`] de `pair` con los archivos de `tree` (el disco, o un commit
/// ya extraído). Si esa versión no tiene el archivo de vínculos y se da
/// `disk` (la raíz del proyecto en el disco), se usa el de ahí: los
/// vínculos de hoy sirven para revisar un commit anterior.
pub fn load(tree: &crate::lvs::Tree, pair: &crate::lvs::Pair, disk: Option<&std::path::Path>) -> Result<Session, String> {
    let v = super::cache::DiskVersion { root: tree.root.clone() };
    let root = tree.root.clone();
    let (sides, cached) = sides(pair, &v, &|| Ok(crate::lvs::Tree::disk(&root)))?;
    session(sides, cached, pair, &v, disk)
}

/// [`Session`] de `pair` en el commit `rev`. Si la caché vale (se comprueba
/// leyendo de Git), no se escribe el commit a ningún lado.
pub fn load_commit(repo: &std::path::Path, rev: &str, pair: &crate::lvs::Pair, disk: Option<&std::path::Path>) -> Result<Session, String> {
    let git = git2::Repository::discover(repo).map_err(|e| e.message().to_string())?;
    let tree = git.revparse_single(rev).and_then(|o| o.peel_to_tree()).map_err(|_| tr!("git.commit_not_found", commit = rev))?;
    let v = super::cache::CommitVersion { repo: &git, tree };
    let (sides, cached) = sides(pair, &v, &|| crate::lvs::Tree::commit(repo, rev))?;
    session(sides, cached, pair, &v, disk)
}

/// La sesión: las extracciones y el archivo de vínculos de esa versión (o
/// el del disco).
pub(super) fn session(sides: Sides, cached: bool, pair: &crate::lvs::Pair, v: &dyn super::cache::Version, disk: Option<&std::path::Path>) -> Result<Session, String> {
    let map_path = map_path(&sides.cell);
    let in_version = v.read(&map_path).and_then(|b| String::from_utf8(b).ok());
    let (text, from_disk) = match in_version {
        Some(t) => (Some(t), false),
        None => match disk.and_then(|d| std::fs::read_to_string(d.join(&map_path)).ok()) {
            Some(t) => (Some(t), true),
            None => (None, false),
        },
    };
    let (map, exists) = match text {
        Some(text) => (MapFile::parse(&text).map_err(|e| format!("{map_path}: {e}"))?, true),
        None => (MapFile::new(&pair.schematic, &pair.layout, pair.cell.as_deref()), false),
    };
    let Sides { cell, schematic, layout, sch_ports, lay_ports, unchecked, unit_um, boxes, warnings } = sides;
    Ok(Session { cell, map_path, map, exists, from_disk, cached, schematic, layout, warnings, unit_um, boxes, sch_ports, lay_ports, unchecked })
}

/// Todo lo que se deduce de una sesión: [`check`] más los pines y lo que no
/// se revisa.
pub fn check_session(s: &Session) -> Check {
    let mut c = check(&s.map, &s.schematic, &s.layout);
    c.pins = check_pins(&c, &s.sch_ports, &s.lay_ports);
    c.unchecked = s.unchecked.clone();
    c
}

/// Vincula el transistor `schematic` con los dedos `fingers` del layout. Lo
/// que ya hubiera de los dos (otro vínculo de ese transistor, o esos dedos
/// en otro vínculo) se reemplaza.
pub fn bind(map: &mut MapFile, schematic: &str, fingers: &[usize], lay: &[LayDevice]) {
    let refs: Vec<LayoutRef> = fingers.iter().map(|&i| LayoutRef::of(&lay[i])).collect();
    let taken = |r: &LayoutRef| refs.iter().any(|n| same_model(&n.model, &r.model) && (n.at[0] - r.at[0]).hypot(n.at[1] - r.at[1]) <= TOL);
    map.binds.retain(|b| b.schematic != schematic);
    for b in &mut map.binds {
        b.layout.retain(|r| !taken(r));
    }
    map.binds.retain(|b| !b.layout.is_empty());
    if !refs.is_empty() {
        map.binds.push(Bind { schematic: schematic.into(), layout: refs });
    }
}

/// Quita el vínculo de `schematic`.
pub fn unbind(map: &mut MapFile, schematic: &str) {
    map.binds.retain(|b| b.schematic != schematic);
}

/// El estado de un chequeo en pocos números (para el historial).
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Summary {
    pub bound: usize,
    pub total: usize,
    pub fingers: usize,
    pub fingers_total: usize,
    /// Parámetros o modelos distintos.
    pub differences: usize,
    pub shorts: usize,
    pub opens: usize,
    pub lost: usize,
    pub pins: usize,
    /// Lo que no se revisa.
    pub unchecked: usize,
    pub clean: bool,
}

impl Summary {
    pub fn of(c: &Check, s: &Session) -> Self {
        Summary {
            bound: c.bound.len(),
            total: s.schematic.len(),
            fingers: c.bound.iter().map(|(_, f)| f.len()).sum(),
            fingers_total: s.layout.len(),
            differences: c.params.len() + c.models.len(),
            shorts: c.shorts.len(),
            opens: c.opens.len(),
            lost: c.lost.len(),
            pins: c.pins.len(),
            unchecked: c.unchecked.len(),
            clean: c.clean(),
        }
    }
}

/// Qué cambió de un commit (`older`) al siguiente (`newer`), lo que vale la
/// pena marcar en el historial.
pub fn transitions(older: &Summary, newer: &Summary) -> Vec<Transition> {
    let mut out = Vec::new();
    if newer.clean && !older.clean {
        out.push(Transition::Clean);
    } else if older.clean && !newer.clean {
        out.push(Transition::Broke);
    }
    if newer.shorts > older.shorts {
        out.push(Transition::NewShort);
    } else if newer.shorts < older.shorts {
        out.push(Transition::ShortFixed);
    }
    if newer.opens > older.opens {
        out.push(Transition::NewOpen);
    } else if newer.opens < older.opens {
        out.push(Transition::OpenFixed);
    }
    if newer.bound > older.bound {
        out.push(Transition::Linked(newer.bound - older.bound));
    }
    out
}

/// Un cambio de estado del LVS manual entre dos commits.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    Clean,
    /// Estaba limpio y dejó de estarlo.
    Broke,
    NewShort,
    ShortFixed,
    NewOpen,
    OpenFixed,
    Linked(usize),
}

/// Esquema del resultado en JSON.
pub const CHECK_SCHEMA: &str = "riku-lvs-check/v1";

/// El resultado en JSON: los transistores del layout como en el archivo
/// (modelo y posición).
pub fn check_json(c: &Check, s: &Session) -> serde_json::Value {
    let refs = |idx: &[usize]| -> Vec<serde_json::Value> {
        idx.iter().map(|&i| serde_json::json!({ "model": s.layout[i].model, "at": [s.layout[i].at.0, s.layout[i].at.1] })).collect()
    };
    let pairs = |v: &[(String, String)]| -> Vec<serde_json::Value> { v.iter().map(|(d, w)| serde_json::json!({ "device": d, "what": w })).collect() };
    let groups = |v: &[(String, Vec<String>)]| -> Vec<serde_json::Value> { v.iter().map(|(n, ns)| serde_json::json!({ "net": n, "nets": ns })).collect() };
    serde_json::json!({
        "cell": s.cell,
        "map": s.map_path,
        "map_from_disk": s.from_disk,
        "cached": s.cached,
        "clean": c.clean(),
        "complete": c.complete(),
        "pins": pairs(&c.pins),
        "unchecked": c.unchecked,
        "moved_ambiguous": c.moved_ambiguous,
        "schematic_devices": s.schematic.len(),
        "layout_devices": s.layout.len(),
        "bound": c.bound.iter().map(|(n, f)| serde_json::json!({ "schematic": n, "layout": refs(f) })).collect::<Vec<_>>(),
        "moved": c.moved.map(|m| serde_json::json!({ "angle": (m.orient % 4) as u32 * 90, "mirrored": m.orient >= 4, "dx": m.dx, "dy": m.dy, "count": m.count })),
        "by_connectivity": c.by_connectivity,
        "by_cell": c.by_cell,
        "models": pairs(&c.models),
        "params": pairs(&c.params),
        "shorts": groups(&c.shorts),
        "opens": groups(&c.opens),
        "lost": c.lost.iter().map(|(n, r)| serde_json::json!({ "schematic": n, "model": r.model, "at": r.at })).collect::<Vec<_>>(),
        "unknown": c.unknown,
        "unbound_schematic": c.unbound_schematic,
        "unbound_layout": refs(&c.unbound_layout),
        "nets": c.nets,
        "warnings": s.warnings,
    })
}
