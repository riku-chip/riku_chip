//! LVS: el layout contra el esquemático, en una versión (ver `docs/lvs.md`).
//!
//! 1. La versión en el disco: el working tree, o los archivos de un commit en
//!    una carpeta temporal (Xschem y Netgen leen del disco).
//! 2. La netlist del esquemático con el netlister de `xschem-viewer-rust`
//!    (`spice::netlist`, modo LVS), con los símbolos del PDK y del proyecto
//!    de esa misma versión: no hace falta Xschem.
//! 3. La del layout con `riku_mod_layout::nets::layout_spice`, sin
//!    herramientas externas.
//! 4. Netgen con el `setup.tcl` de ese PDK; su `comp.json` se lee en un
//!    [`Report`].
//!
//! Qué esquemático va con qué layout: `[[lvs]]` en `.riku.toml` o, si no hay,
//! los de igual nombre (`ota-5t.sch` ↔ `ota-5t.gds`).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::i18n::tr;
use crate::modules::xschem_pdk::{symbol_source_for, PdkSource};

pub const SCHEMA: &str = "riku-lvs/v1";

/// Un esquemático y su layout (rutas relativas a la raíz de la versión).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Pair {
    pub schematic: String,
    pub layout: String,
    /// Celda del layout; `None`: la top.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
}

// Los tipos que también ven `log --lvs` y `status --lvs` (sin estas features).
pub use crate::core::analysis::lvs_types::{Delta, Discrepancy, LvsState, Side, Transition, Verdict};

/// Lo mismo visto del lado del layout y del esquemático.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Sides<T> {
    pub layout: T,
    pub schematic: T,
}

/// Un dispositivo emparejado con un parámetro distinto.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyError {
    pub model: String,
    /// Nombres de instancia (`M1` en el esquemático, `19` en el layout).
    pub layout: String,
    pub schematic: String,
    pub values: Vec<PropertyValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PropertyValue {
    pub name: String,
    pub layout: String,
    pub schematic: String,
}

/// Lo que dijo Netgen, sin los datos del par.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Comparison {
    pub result: Verdict,
    /// Las últimas líneas de Netgen ("Final result: …").
    pub summary: Vec<String>,
    pub devices: Sides<BTreeMap<String, u64>>,
    pub nets: Sides<u64>,
    pub pins: Sides<Vec<String>>,
    /// Grupos de redes que no se pudieron emparejar.
    pub unmatched_nets: Vec<Sides<Vec<String>>>,
    /// Grupos de dispositivos que no se pudieron emparejar.
    pub unmatched_devices: Vec<Sides<Vec<String>>>,
    pub properties: Vec<PropertyError>,
}

impl Comparison {
    /// Lo que no coincide, cada cosa con su identidad (ver [`Discrepancy::key`]):
    /// cada parámetro distinto, cada grupo de redes o dispositivos sin pareja
    /// y cada pin de un solo lado.
    pub fn discrepancies(&self) -> Vec<Discrepancy> {
        let mut out: Vec<Discrepancy> = self
            .properties
            .iter()
            .flat_map(|p| {
                p.values.iter().filter(|v| v.layout != v.schematic).map(|v| Discrepancy::Property {
                    instance: p.schematic.clone(),
                    layout_instance: p.layout.clone(),
                    model: p.model.clone(),
                    param: v.name.clone(),
                    schematic: v.schematic.clone(),
                    layout: v.layout.clone(),
                })
            })
            .collect();
        let group = |g: &Sides<Vec<String>>| (g.schematic.clone(), g.layout.clone());
        out.extend(self.unmatched_nets.iter().map(group).map(|(schematic, layout)| Discrepancy::Nets { schematic, layout }));
        out.extend(
            self.unmatched_devices.iter().map(group).map(|(schematic, layout)| Discrepancy::Devices { schematic, layout }),
        );
        let only = |a: &[String], b: &[String], side: Side| -> Vec<Discrepancy> {
            a.iter().filter(|p| !b.contains(p)).map(|p| Discrepancy::Pin { name: p.clone(), only_in: side }).collect()
        };
        out.extend(only(&self.pins.schematic, &self.pins.layout, Side::Schematic));
        out.extend(only(&self.pins.layout, &self.pins.schematic, Side::Layout));
        out
    }
}

/// El LVS de un par en una versión.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Report {
    #[serde(flatten)]
    pub pair: Pair,
    /// La celda comparada del layout.
    pub layout_cell: String,
    pub pdk: String,
    #[serde(flatten)]
    pub comparison: Comparison,
    pub warnings: Vec<String>,
    /// Dónde está en el esquemático cada net y dispositivo de su netlist
    /// (para ir de lo que dice Netgen al dibujo). No va en el JSON.
    #[serde(skip)]
    pub places: xschem_viewer::spice::Places,
}

// ─── Herramientas ────────────────────────────────────────────────────────────

/// `netgen`: en el `PATH` o en `/foss/tools/bin` (iic-osic-tools). Es la
/// única herramienta externa del LVS.
pub struct Tools {
    pub netgen: PathBuf,
}

pub fn tools() -> Result<Tools, String> {
    Ok(Tools { netgen: find_tool("netgen").ok_or_else(|| tr!("lvs.no_tool", tool = "netgen"))? })
}

/// Dónde está una herramienta: en el `PATH` o en `/foss/tools/bin`.
pub fn find_tool(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain([PathBuf::from("/foss/tools/bin")])
        .map(|d| d.join(name))
        .find(|p| p.is_file())
}

// ─── Versión en el disco ─────────────────────────────────────────────────────

/// Una carpeta temporal que se borra al soltarla.
struct TempDir(PathBuf);

impl TempDir {
    fn new(what: &str) -> Result<Self, String> {
        let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!("riku-{what}-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        Ok(Self(dir))
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Los archivos de una versión en el disco.
pub struct Tree {
    pub root: PathBuf,
    _temp: Option<TempDir>,
}

impl Tree {
    /// El working tree (o cualquier carpeta).
    pub fn disk(root: &Path) -> Self {
        Self { root: root.to_path_buf(), _temp: None }
    }

    /// Los archivos de `rev` en una carpeta temporal. Los de más de
    /// [`LARGE_BLOB_THRESHOLD`](crate::core::domain::git_types::LARGE_BLOB_THRESHOLD)
    /// se saltean (no son esquemáticos ni layouts que se puedan comparar).
    pub fn commit(repo: &Path, rev: &str) -> Result<Self, String> {
        let r = git2::Repository::discover(repo).map_err(|e| e.message().to_string())?;
        let tree =
            r.revparse_single(rev).and_then(|o| o.peel_to_tree()).map_err(|_| tr!("git.commit_not_found", commit = rev))?;
        let temp = TempDir::new("lvs-tree")?;
        let limit = crate::core::domain::git_types::LARGE_BLOB_THRESHOLD;
        let mut failed = None;
        tree.walk(git2::TreeWalkMode::PreOrder, |dir, entry| {
            if entry.kind() != Some(git2::ObjectType::Blob) {
                return git2::TreeWalkResult::Ok;
            }
            let Ok(blob) = r.find_blob(entry.id()) else { return git2::TreeWalkResult::Ok };
            if blob.size() > limit {
                return git2::TreeWalkResult::Ok;
            }
            let path = temp.0.join(dir).join(entry.name().unwrap_or_default());
            let written =
                path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, blob.content()));
            if let Err(e) = written {
                failed = Some(format!("{}: {e}", path.display()));
                return git2::TreeWalkResult::Abort;
            }
            git2::TreeWalkResult::Ok
        })
        .map_err(|e| e.message().to_string())?;
        if let Some(e) = failed {
            return Err(e);
        }
        Ok(Self { root: temp.0.clone(), _temp: Some(temp) })
    }
}

// ─── Pares ───────────────────────────────────────────────────────────────────

/// Los pares a comparar en `root`: los de `.riku.toml` o, si no hay, cada
/// esquemático de Xschem con el layout de igual nombre (`.gds`, `.oas` o
/// `.mag`, en ese orden si hay más de uno).
pub fn pairs(root: &Path, configured: &[Pair]) -> Vec<Pair> {
    if !configured.is_empty() {
        return configured.to_vec();
    }
    let mut sch: Vec<String> = Vec::new();
    let mut layouts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    walk(root, root, &mut |rel| {
        let p = Path::new(rel);
        let stem = p.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
        match p.extension().and_then(|e| e.to_str()).map(str::to_ascii_lowercase).as_deref() {
            Some("sch") => sch.push(rel.to_string()),
            Some("gds" | "oas" | "mag") => layouts.entry(stem).or_default().push(rel.to_string()),
            _ => {}
        }
    });
    let rank = |p: &str| ["gds", "oas", "mag"].iter().position(|e| p.to_ascii_lowercase().ends_with(e)).unwrap_or(9);
    sch.sort();
    sch.into_iter()
        .filter(|s| std::fs::read(root.join(s)).is_ok_and(|b| crate::modules::xschem::is_xschem(&b)))
        .filter_map(|s| {
            let stem = Path::new(&s).file_stem()?.to_string_lossy().to_string();
            let layout = layouts.get(&stem)?.iter().min_by_key(|l| (rank(l), (*l).clone()))?.clone();
            Some(Pair { schematic: s, layout, cell: None })
        })
        .collect()
}

/// El par al que pertenece `file` (su esquemático o su layout) y la raíz del
/// proyecto (la del repositorio, o su carpeta si no hay): los de
/// `.riku.toml` o, si no hay, por nombre.
pub fn pair_for(file: &Path) -> Option<(PathBuf, Pair)> {
    let dir = file.parent()?;
    let root = git2::Repository::discover(dir)
        .ok()
        .and_then(|r| r.workdir().map(Path::to_path_buf))
        .unwrap_or_else(|| dir.to_path_buf());
    let configured: Vec<Pair> = crate::core::config::load(Some(&root))
        .map(|c| c.lvs.into_iter().map(|c| Pair { schematic: c.schematic, layout: c.layout, cell: c.cell }).collect())
        .unwrap_or_default();
    let target = std::fs::canonicalize(file).ok()?;
    let same = |rel: &str| std::fs::canonicalize(root.join(rel)).is_ok_and(|p| p == target);
    pairs(&root, &configured).into_iter().find(|p| same(&p.schematic) || same(&p.layout)).map(|p| (root, p))
}

/// Archivos bajo `dir` (sin carpetas ocultas ni `target`), relativos a `root`.
fn walk(root: &Path, dir: &Path, f: &mut dyn FnMut(&str)) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let path = e.path();
        let name = e.file_name().to_string_lossy().to_string();
        if name.starts_with('.') || name == "target" {
            continue;
        }
        if path.is_dir() {
            walk(root, &path, f);
        } else if let Ok(rel) = path.strip_prefix(root) {
            f(&rel.to_string_lossy().replace('\\', "/"));
        }
    }
}

// ─── Correr ──────────────────────────────────────────────────────────────────

/// El PDK de los dispositivos del esquemático y su carpeta: el que tiene sus
/// símbolos (si el activo, `$PDK`, no los tiene, el que sí).
fn pdk_of(schematic: &str) -> Result<(String, PathBuf), String> {
    // `…/<pdk>/libs.tech/xschem` → (`<pdk>`, `…/<pdk>`).
    let dir = |xschem: &Path| {
        let pdk = xschem.parent()?.parent()?;
        Some((pdk.file_name()?.to_string_lossy().to_string(), pdk.to_path_buf()))
    };
    match symbol_source_for(schematic) {
        PdkSource::Env { path, extra } => match extra.first() {
            Some((_, p)) => dir(p),
            None => dir(&path),
        },
        PdkSource::Detected(found) => found.first().and_then(|(_, p)| dir(p)),
        PdkSource::Missing(reason) => return Err(tr!("lvs.no_pdk", reason = reason)),
    }
    .ok_or_else(|| tr!("lvs.no_pdk", reason = "?"))
}

/// El LVS de `pair` en `tree`.
pub fn run(tree: &Tree, pair: &Pair, tools: &Tools) -> Result<Report, String> {
    let sch_path = tree.root.join(&pair.schematic);
    let text = std::fs::read_to_string(&sch_path).map_err(|e| format!("{}: {e}", pair.schematic))?;
    let (pdk, pdk_dir) = pdk_of(&text)?;
    let work = TempDir::new("lvs")?;
    let mut warnings = Vec::new();

    // Esquemático: el netlister propio (modo LVS, con su `.subckt`). Los
    // símbolos y sub-esquemáticos del proyecto salen de esta misma versión;
    // los del PDK, de donde los busca el visor.
    let stem = sch_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let files: std::sync::Arc<dyn viewer_core::FileSource> = std::sync::Arc::new(viewer_core::DiskFiles::new(tree.root.clone()));
    let (mut opts, _) = crate::modules::xschem::render_options_for(&text);
    let (from, symbols) = (pair.schematic.clone(), files.clone());
    opts = opts.with_symbol_lookup(std::sync::Arc::new(move |sym: &str| {
        let found = crate::modules::xschem_hier::find(sym, &from, symbols.as_ref(), false)?;
        symbols.read(&found).and_then(|b| String::from_utf8(b).ok())
    }));
    let lookup = |reference: &str, parent: &str| {
        let found = crate::modules::xschem_hier::find(reference, parent, files.as_ref(), true)?;
        Some((found.clone(), String::from_utf8(files.read(&found)?).ok()?))
    };
    // Las variables de los bloques de código (`.include $::SKYWATER_MODELS/…`),
    // del `xschemrc` del PDK. `PDK_ROOT` y `PDK`, los que fija `sak-pdk`; si
    // el esquemático es de otro PDK (o no hay entorno), los de la carpeta
    // del PDK de sus símbolos.
    let rc = std::fs::read_to_string(pdk_dir.join("libs.tech/xschem/xschemrc")).unwrap_or_default();
    let active = std::env::var("PDK").is_ok_and(|p| p == pdk);
    let root = std::env::var("PDK_ROOT")
        .ok()
        .filter(|_| active)
        .unwrap_or_else(|| pdk_dir.parent().map(|p| p.display().to_string()).unwrap_or_default());
    let pdk_name = pdk.clone();
    let env = move |n: &str| match n.trim_start_matches("env(").trim_end_matches(')') {
        "PDK_ROOT" => Some(root.clone()),
        "PDK" => Some(pdk_name.clone()),
        other => std::env::var(other).ok(),
    };
    let vars = xschem_viewer::tcleval::rc_vars(&rc, &env);
    let lvs_mode = xschem_viewer::spice::SpiceOptions {
        lvs: true,
        top_subckt: true,
        vars: Some(std::sync::Arc::new(move |n: &str| vars.get(n).cloned().or_else(|| env(n)))),
    };
    let netlist = xschem_viewer::spice::netlist(&text, &pair.schematic, &stem, &opts, lvs_mode, &lookup)?;
    warnings.extend(netlist.warnings.iter().map(|w| tr!("lvs.missing_symbol", line = w)));
    std::fs::write(work.0.join("schematic.spice"), &netlist.text).map_err(|e| e.to_string())?;

    // Layout: la netlist que extrae riku. SKY130 usa `.option scale=1e-6`:
    // W y L sin sufijo, como su netlist de Xschem.
    let bytes = std::fs::read(tree.root.join(&pair.layout)).map_err(|e| format!("{}: {e}", pair.layout))?;
    let unit = if pdk.starts_with("sky130") { "" } else { "u" };
    let files = viewer_core::DiskFiles::new(tree.root.clone());
    let layout = riku_mod_layout::nets::layout_spice(&bytes, &pair.layout, Some(&files), pair.cell.as_deref(), unit)?;
    warnings.extend(layout.warnings.iter().cloned());
    std::fs::write(work.0.join("layout.spice"), &layout.spice).map_err(|e| e.to_string())?;

    // Netgen con el setup del PDK: el layout es el circuito 1.
    let setup = pdk_dir.join(format!("libs.tech/netgen/{pdk}_setup.tcl"));
    if !setup.is_file() {
        return Err(tr!("lvs.no_setup", path = setup.display()));
    }
    let out = Command::new(&tools.netgen)
        .args(["-batch", "lvs", &format!("layout.spice {}", layout.cell), &format!("schematic.spice {stem}")])
        .arg(&setup)
        .args(["comp.out", "-json"])
        .current_dir(&work.0)
        .output()
        .map_err(|e| tr!("lvs.tool_failed", tool = "netgen", error = e))?;
    // `RIKU_LVS_KEEP=<carpeta>`: las netlists y el reporte de Netgen, para
    // revisarlos a mano.
    if let Some(keep) = std::env::var_os("RIKU_LVS_KEEP").map(PathBuf::from) {
        let _ = std::fs::create_dir_all(&keep);
        for f in ["schematic.spice", "layout.spice", "comp.out", "comp.json"] {
            let _ = std::fs::copy(work.0.join(f), keep.join(f));
        }
    }
    let (json, text) = (std::fs::read_to_string(work.0.join("comp.json")), std::fs::read_to_string(work.0.join("comp.out")));
    let comparison = match (json, text) {
        (Ok(json), Ok(text)) => parse_netgen(&json, &text)?,
        // Sin JSON (p. ej. las celdas de arriba no se pudieron emparejar):
        // el veredicto del texto, que es "no coinciden".
        (Err(_), Ok(text)) if text.contains("Final result") => from_text(&text),
        _ => return Err(tr!("lvs.tool_failed", tool = "netgen", error = tail(&out.stdout))),
    };
    Ok(Report { pair: pair.clone(), layout_cell: layout.cell, pdk, comparison, warnings, places: netlist.places })
}

/// Las últimas líneas de la salida de una herramienta, para un error.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(6)..].join(" | ")
}

// ─── Historial ───────────────────────────────────────────────────────────────

/// Cómo quedó el LVS de un par en un commit.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum StepResult {
    /// El esquemático o el layout no están en ese commit.
    Missing,
    Error {
        error: String,
    },
    Done {
        #[serde(flatten)]
        report: Box<Report>,
        /// El par no cambió desde otro commit ya comparado: su resultado.
        reused: bool,
    },
}

impl StepResult {
    pub fn verdict(&self) -> Option<Verdict> {
        match self {
            StepResult::Done { report, .. } => Some(report.comparison.result),
            _ => None,
        }
    }
}

/// Un commit del historial.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Step {
    pub commit: String,
    pub summary: String,
    pub author: String,
    pub time: i64,
    #[serde(flatten)]
    pub result: StepResult,
    /// Respecto del commit anterior (más viejo) con resultado: ver
    /// [`Transition`].
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// Qué apareció, se arregló o cambió respecto de ese mismo commit.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
}

/// Qué pasó en cada commit (del más nuevo al más viejo) respecto del
/// anterior con resultado: dónde dejó de coincidir o volvió a coincidir, y
/// qué discrepancias aparecieron, se arreglaron o cambiaron. Los commits sin
/// resultado (sin el par, o con error) no cortan la comparación.
pub fn mark_transitions(steps: &mut [Step]) {
    for i in 0..steps.len() {
        let before = steps[i + 1..].iter().find_map(|s| match &s.result {
            StepResult::Done { report, .. } => Some(report.comparison.clone()),
            _ => None,
        });
        let (transition, delta) = match (&before, &steps[i].result) {
            (Some(b), StepResult::Done { report, .. }) => {
                let now = &report.comparison;
                (Transition::between(b.result, now.result), Some(Delta::between(&b.discrepancies(), &now.discrepancies())))
            }
            _ => (None, None),
        };
        steps[i].transition = transition;
        steps[i].delta = delta.filter(|d| !d.is_empty());
    }
}

/// Lo que determina el LVS de un par en un commit: las carpetas del
/// esquemático y del layout (Git da un id por carpeta que cambia si cambia
/// cualquier archivo adentro; ahí están sus sub-esquemáticos, símbolos y
/// sub-celdas). `None` si falta alguno de los dos archivos.
pub fn signature(tree: &git2::Tree<'_>, pair: &Pair) -> Option<String> {
    let dir_id = |file: &str| -> Option<String> {
        tree.get_path(Path::new(file)).ok()?;
        match Path::new(file).parent().filter(|d| !d.as_os_str().is_empty()) {
            Some(d) => Some(tree.get_path(d).ok()?.id().to_string()),
            None => Some(tree.id().to_string()),
        }
    };
    let (s, l) = (dir_id(&pair.schematic)?, dir_id(&pair.layout)?);
    Some(format!("{}|{}|{}|{s}|{l}", pair.schematic, pair.layout, pair.cell.as_deref().unwrap_or("")))
}

/// Carpeta de la caché de resultados (`RIKU_CACHE_DIR/lvs` o
/// `~/.cache/riku/lvs`); `None` con `RIKU_NO_CACHE`.
fn cache_dir() -> Option<PathBuf> {
    if std::env::var("RIKU_NO_CACHE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    std::env::var_os("RIKU_CACHE_DIR")
        .map(|d| PathBuf::from(d).join("lvs"))
        .or_else(|| dirs::cache_dir().map(|d| d.join("riku").join("lvs")))
}

fn cache_file(signature: &str) -> Option<PathBuf> {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    // Con otra netlist (otra versión de Riku o del netlister), otro resultado.
    (env!("CARGO_PKG_VERSION"), xschem_viewer::spice::VERSION, signature).hash(&mut h);
    Some(cache_dir()?.join(format!("{:016x}.json", h.finish())))
}

/// El LVS de cada par en los últimos `limit` commits desde `from` (por el
/// primer padre, del más nuevo al más viejo). Solo compara cuando el par
/// cambió; si no, repite el resultado (de este historial o de la caché).
pub fn history(
    repo_path: &Path,
    from: &str,
    limit: usize,
    pairs: &[Pair],
    tools: &Tools,
) -> Result<Vec<(Pair, Vec<Step>)>, String> {
    let repo = git2::Repository::discover(repo_path).map_err(|e| e.message().to_string())?;
    let start =
        repo.revparse_single(from).and_then(|o| o.peel_to_commit()).map_err(|_| tr!("git.commit_not_found", commit = from))?;
    let mut commits = vec![start];
    while commits.len() < limit {
        let Ok(parent) = commits[commits.len() - 1].parent(0) else { break };
        commits.push(parent);
    }

    let mut out = Vec::new();
    for pair in pairs {
        let mut seen: BTreeMap<String, Result<Report, String>> = BTreeMap::new();
        let mut steps = Vec::new();
        for c in &commits {
            let id = c.id().to_string();
            let short = id[..7.min(id.len())].to_string();
            let sig = c.tree().ok().and_then(|t| signature(&t, pair));
            let result = match sig {
                None => StepResult::Missing,
                Some(sig) => {
                    let reused = seen.contains_key(&sig);
                    if !reused {
                        let cached = cache_file(&sig)
                            .and_then(|f| std::fs::read_to_string(f).ok())
                            .and_then(|t| serde_json::from_str::<Report>(&t).ok());
                        let fresh = match cached {
                            Some(r) => Ok(r),
                            None => Tree::commit(repo_path, &id).and_then(|tree| run(&tree, pair, tools)),
                        };
                        if let (Ok(r), Some(f)) = (&fresh, cache_file(&sig)) {
                            let _ = f.parent().map(std::fs::create_dir_all);
                            let _ = serde_json::to_string(r).map(|t| std::fs::write(f, t));
                        }
                        seen.insert(sig.clone(), fresh);
                    }
                    match &seen[&sig] {
                        Ok(r) => StepResult::Done { report: Box::new(r.clone()), reused },
                        Err(e) => StepResult::Error { error: e.clone() },
                    }
                }
            };
            steps.push(Step {
                commit: short,
                summary: String::from_utf8_lossy(c.summary_bytes().unwrap_or_default()).into_owned(),
                author: String::from_utf8_lossy(c.author().name_bytes()).into_owned(),
                time: c.time().seconds(),
                result,
                transition: None,
                delta: None,
            });
        }
        mark_transitions(&mut steps);
        out.push((pair.clone(), steps));
    }
    Ok(out)
}

// ─── Netgen ──────────────────────────────────────────────────────────────────

/// Lee el `comp.json` de Netgen (circuito 1: el layout; 2: el esquemático) y
/// el veredicto de su `comp.out`.
pub fn parse_netgen(json: &str, out: &str) -> Result<Comparison, String> {
    let all: Value = serde_json::from_str(json).map_err(|e| tr!("lvs.bad_json", error = e))?;
    // Una entrada por celda comparada; la última es la de arriba.
    let top = all.as_array().and_then(|a| a.last()).ok_or_else(|| tr!("lvs.bad_json", error = "[]"))?;
    let side = |v: &Value, i: usize| v.get(i).cloned().unwrap_or(Value::Null);

    let devices = |v: &Value| -> BTreeMap<String, u64> {
        v.as_array().into_iter().flatten().filter_map(|d| Some((d.get(0)?.as_str()?.to_string(), d.get(1)?.as_u64()?))).collect()
    };
    let strings = |v: &Value| -> Vec<String> {
        v.as_array().into_iter().flatten().map(|s| s.as_str().map_or_else(|| s.to_string(), str::to_string)).collect()
    };
    // Una red o un dispositivo sin pareja: `[nombre, conexiones]`.
    let names = |v: &Value| -> Vec<String> {
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|x| x.get(0)?.as_str())
            .filter(|n| !n.starts_with('('))
            .map(instance)
            .filter(|n| !n.is_empty())
            .collect()
    };
    let groups = |key: &str| -> Vec<Sides<Vec<String>>> {
        top.get(key)
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .map(|g| Sides { layout: names(&side(g, 0)), schematic: names(&side(g, 1)) })
            .collect()
    };

    let properties = top
        .get("properties")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|p| {
            let (l, s) = (p.get(0)?, p.get(1)?);
            let (l_name, s_name) = (l.get(0)?.as_str()?, s.get(0)?.as_str()?);
            let props = |v: &Value| -> BTreeMap<String, String> {
                v.get(1)
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                    .filter_map(|kv| Some((kv.get(0)?.as_str()?.to_lowercase(), value_text(kv.get(1)?))))
                    .collect()
            };
            let (lp, sp) = (props(l), props(s));
            let keys: std::collections::BTreeSet<&String> = lp.keys().chain(sp.keys()).collect();
            let values = keys
                .into_iter()
                .map(|k| PropertyValue {
                    name: k.clone(),
                    layout: lp.get(k).cloned().unwrap_or_default(),
                    schematic: sp.get(k).cloned().unwrap_or_default(),
                })
                .collect();
            Some(PropertyError {
                model: l_name.rsplit_once(':').map_or(l_name, |(m, _)| m).to_string(),
                layout: instance(l_name),
                schematic: instance(s_name),
                values,
            })
        })
        .collect::<Vec<_>>();

    let (unmatched_nets, unmatched_devices) = (groups("badnets"), groups("badelements"));
    let failed = out.contains("Netlists do not match")
        || out.contains("failed")
        || !unmatched_nets.is_empty()
        || !unmatched_devices.is_empty();
    let result = if failed {
        Verdict::Mismatch
    } else if !properties.is_empty() || out.contains("Property errors were found") {
        Verdict::PropertyErrors
    } else {
        Verdict::Match
    };
    let summary = summary_of(out);
    Ok(Comparison {
        result,
        summary,
        devices: Sides {
            layout: devices(&side(top.get("devices").unwrap_or(&Value::Null), 0)),
            schematic: devices(&side(top.get("devices").unwrap_or(&Value::Null), 1)),
        },
        nets: Sides {
            layout: top.get("nets").and_then(|n| n.get(0)).and_then(Value::as_u64).unwrap_or(0),
            schematic: top.get("nets").and_then(|n| n.get(1)).and_then(Value::as_u64).unwrap_or(0),
        },
        pins: Sides {
            layout: strings(&side(top.get("pins").unwrap_or(&Value::Null), 0)),
            schematic: strings(&side(top.get("pins").unwrap_or(&Value::Null), 1)),
        },
        unmatched_nets,
        unmatched_devices,
        properties,
    })
}

/// Un resultado de Netgen del que solo hay texto: no coinciden, y por qué.
fn from_text(out: &str) -> Comparison {
    Comparison {
        result: Verdict::Mismatch,
        summary: summary_of(out),
        devices: Sides::default(),
        nets: Sides::default(),
        pins: Sides::default(),
        unmatched_nets: Vec::new(),
        unmatched_devices: Vec::new(),
        properties: Vec::new(),
    }
}

/// "Final result: …" y lo que le sigue hasta la primera línea en blanco.
fn summary_of(out: &str) -> Vec<String> {
    out.lines()
        .skip_while(|l| !l.starts_with("Final result"))
        .take_while(|l| !l.trim().is_empty())
        .map(str::trim)
        .filter(|l| *l != "." && *l != "Final result:")
        .map(str::to_string)
        .collect()
}

/// `sky130_fd_pr__pfet_01v8:M1` → `M1` (el nombre de la instancia).
fn instance(name: &str) -> String {
    name.rsplit_once(':').map_or(name, |(_, n)| n).to_string()
}

/// Un valor de propiedad de Netgen como texto (`"2.0"` → `2`).
fn value_text(v: &Value) -> String {
    let s = v.as_str().map_or_else(|| v.to_string(), str::to_string);
    match s.parse::<f64>() {
        Ok(x) if x.fract() == 0.0 && x.abs() < 1e15 => format!("{}", x as i64),
        Ok(x) => format!("{x}"),
        Err(_) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PROPS: &str = r#"[{"name":["ota","ota"],"devices":[[["sky130_fd_pr__pfet_01v8",2],["sky130_fd_pr__nfet_01v8",7]],[["sky130_fd_pr__pfet_01v8",2],["sky130_fd_pr__nfet_01v8",7]]],"nets":[7,7],"badnets":[],"badelements":[],"properties":[[["sky130_fd_pr__pfet_01v8:19",[["W","2.0"]]],["sky130_fd_pr__pfet_01v8:M1",[["W","4.0"]]]]],"pins":[["Vout","Ib"],["Vout","Ib"]]}]"#;

    #[test]
    fn parametros_distintos() {
        let c = parse_netgen(PROPS, "Final result: Circuits match uniquely.\nProperty errors were found.\n").unwrap();
        assert_eq!(c.result, Verdict::PropertyErrors);
        assert_eq!(c.nets, Sides { layout: 7, schematic: 7 });
        assert_eq!(c.devices.schematic.get("sky130_fd_pr__nfet_01v8"), Some(&7));
        let p = &c.properties[0];
        assert_eq!((p.model.as_str(), p.layout.as_str(), p.schematic.as_str()), ("sky130_fd_pr__pfet_01v8", "19", "M1"));
        assert_eq!(p.values, vec![PropertyValue { name: "w".into(), layout: "2".into(), schematic: "4".into() }]);
        assert_eq!(c.summary, vec!["Final result: Circuits match uniquely.", "Property errors were found."]);
    }

    #[test]
    fn redes_y_dispositivos_sin_pareja() {
        let json = r#"[{"name":["a","a"],"devices":[[],[]],"nets":[8,7],
            "badnets":[[[["n4",[["x","1|3",5]]],["Ib",[]]],[["Vp",[]]]]],
            "badelements":[[[["sky130_fd_pr__nfet_01v8:0",[]]],[["sky130_fd_pr__nfet_01v8:M6",[]],["(no matching instance)",[["",0]]]]]],
            "pins":[[],[]]}]"#;
        let c = parse_netgen(json, "Netlists do not match.\nFinal result: Top level cell failed pin matching.\n").unwrap();
        assert_eq!(c.result, Verdict::Mismatch);
        assert_eq!(c.unmatched_nets, vec![Sides { layout: vec!["n4".into(), "Ib".into()], schematic: vec!["Vp".into()] }]);
        assert_eq!(c.unmatched_devices, vec![Sides { layout: vec!["0".into()], schematic: vec!["M6".into()] }]);
    }

    #[test]
    fn sin_json_es_que_no_coinciden() {
        let c = from_text(
            "Final result: 
Top level cell failed pin matching.

LVS Done.
",
        );
        assert_eq!((c.result, c.summary), (Verdict::Mismatch, vec!["Top level cell failed pin matching.".to_string()]));
    }

    fn step(v: Option<Verdict>) -> Step {
        let result = match v {
            None => StepResult::Missing,
            Some(v) => StepResult::Done {
                report: Box::new(Report {
                    pair: Pair { schematic: "a.sch".into(), layout: "a.gds".into(), cell: None },
                    layout_cell: "a".into(),
                    pdk: "sky130A".into(),
                    comparison: Comparison {
                        result: v,
                        summary: vec![],
                        devices: Sides::default(),
                        nets: Sides::default(),
                        pins: Sides::default(),
                        unmatched_nets: vec![],
                        unmatched_devices: vec![],
                        properties: vec![],
                    },
                    warnings: vec![],
                    places: Default::default(),
                }),
                reused: false,
            },
        };
        Step {
            commit: String::new(),
            summary: String::new(),
            author: String::new(),
            time: 0,
            result,
            transition: None,
            delta: None,
        }
    }

    #[test]
    fn marca_donde_se_rompio_y_donde_se_arreglo() {
        use Verdict::*;
        // Del más nuevo al más viejo.
        let mut steps: Vec<Step> = [Some(Match), Some(PropertyErrors), Some(Mismatch), None, Some(PropertyErrors), Some(Match)]
            .into_iter()
            .map(step)
            .collect();
        mark_transitions(&mut steps);
        let t: Vec<Option<Transition>> = steps.iter().map(|s| s.transition).collect();
        use Transition::*;
        assert_eq!(t, vec![Some(Fixed), Some(Better), Some(Worse), None, Some(Broke), None]);
    }

    fn with_property(mut s: Step, inst: &str, s_val: &str, l_val: &str) -> Step {
        if let StepResult::Done { report, .. } = &mut s.result {
            report.comparison.properties.push(PropertyError {
                model: "pfet".into(),
                layout: "19".into(),
                schematic: inst.into(),
                values: vec![
                    PropertyValue { name: "l".into(), layout: "1".into(), schematic: "1".into() },
                    PropertyValue { name: "w".into(), layout: l_val.into(), schematic: s_val.into() },
                ],
            });
        }
        s
    }

    #[test]
    fn las_discrepancias_tienen_identidad_y_cada_commit_su_delta() {
        let mut c = step(Some(Verdict::PropertyErrors));
        c = with_property(c, "M1", "4", "2");
        let StepResult::Done { report, .. } = &mut c.result else { unreachable!() };
        report.comparison.unmatched_nets.push(Sides { layout: vec!["Vout".into()], schematic: vec!["Vout".into(), "Vp".into()] });
        report.comparison.pins = Sides { layout: vec!["A".into()], schematic: vec!["A".into(), "Ib".into()] };
        let keys: Vec<String> = report.comparison.discrepancies().iter().map(Discrepancy::key).collect();
        assert_eq!(keys, ["P:M1:w", "N:Vout,Vp", "pin:Schematic:Ib"], "l es igual en los dos lados: no es una discrepancia");

        // Del más nuevo al más viejo: coincide, M1 y M3, solo M1.
        let mut steps = vec![
            step(Some(Verdict::Match)),
            with_property(with_property(step(Some(Verdict::PropertyErrors)), "M1", "4", "2"), "M3", "18", "19"),
            with_property(step(Some(Verdict::PropertyErrors)), "M1", "4", "2"),
        ];
        mark_transitions(&mut steps);
        let fixed = steps[0].delta.as_ref().expect("se arreglaron las dos");
        assert_eq!((fixed.fixed.len(), fixed.appeared.len()), (2, 0));
        let appeared = steps[1].delta.as_ref().expect("apareció M3");
        assert_eq!(appeared.appeared.iter().map(Discrepancy::key).collect::<Vec<_>>(), ["P:M3:w"]);
        assert_eq!(steps[1].transition, None, "mismo veredicto, pero con delta");
        assert_eq!(steps[2].delta, None, "el más viejo no tiene con qué comparar");
    }

    #[test]
    fn la_firma_cambia_solo_si_cambia_una_carpeta_del_par() {
        let dir = std::env::temp_dir().join(format!("riku-lvs-sig-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let repo = git2::Repository::init(&dir).unwrap();
        let sig = git2::Signature::now("t", "t@t").unwrap();
        let commit = |files: &[(&str, &str)]| {
            for (p, body) in files {
                let path = dir.join(p);
                std::fs::create_dir_all(path.parent().unwrap()).unwrap();
                std::fs::write(path, body).unwrap();
            }
            let mut index = repo.index().unwrap();
            index.add_all(["*"], git2::IndexAddOption::DEFAULT, None).unwrap();
            index.write().unwrap();
            let tree = repo.find_tree(index.write_tree().unwrap()).unwrap();
            let parents: Vec<git2::Commit> = repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
            let refs: Vec<&git2::Commit> = parents.iter().collect();
            let id = repo.commit(Some("HEAD"), &sig, &sig, "c", &tree, &refs).unwrap();
            repo.find_commit(id).unwrap().tree().unwrap().id()
        };
        let pair = Pair { schematic: "sch/a.sch".into(), layout: "lay/a.gds".into(), cell: None };
        let t1 = commit(&[("sch/a.sch", "1"), ("lay/a.gds", "1"), ("README", "x")]);
        let t2 = commit(&[("README", "y")]);
        let t3 = commit(&[("sch/sub.sch", "2")]);
        let firma = |t| signature(&repo.find_tree(t).unwrap(), &pair);
        assert!(firma(t1).is_some());
        assert_eq!(firma(t1), firma(t2), "otro archivo fuera del par");
        assert_ne!(firma(t2), firma(t3), "un sub-esquemático en su carpeta");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn coincide() {
        let json = r#"[{"name":["a","a"],"devices":[[],[]],"nets":[3,3],"badnets":[],"badelements":[],"pins":[[],[]]}]"#;
        assert_eq!(parse_netgen(json, "Final result: Circuits match uniquely.\n").unwrap().result, Verdict::Match);
    }

    #[test]
    fn empareja_por_nombre() {
        let dir = std::env::temp_dir().join(format!("riku-lvs-pairs-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for (p, body) in [
            ("xschem/ota.sch", "v {xschem version=3.4.5 file_version=1.2}\n"),
            ("xschem/tb.sch", "v {xschem version=3.4.5 file_version=1.2}\n"),
            ("layout/ota.gds", ""),
            ("layout/ota.mag", "magic\n"),
            ("kicad/ota.sch", "EESchema Schematic File Version 4\n"),
        ] {
            let path = dir.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let got = pairs(&dir, &[]);
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(got, vec![Pair { schematic: "xschem/ota.sch".into(), layout: "layout/ota.gds".into(), cell: None }]);
        let cfg = vec![Pair { schematic: "a.sch".into(), layout: "b.mag".into(), cell: Some("c".into()) }];
        assert_eq!(pairs(Path::new("."), &cfg), cfg);
    }

    #[test]
    fn el_par_de_un_archivo_abierto() {
        let dir = std::env::temp_dir().join(format!("riku-lvs-pair-for-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        git2::Repository::init(&dir).unwrap();
        for (p, body) in [
            ("xschem/ota.sch", "v {xschem version=3.4.5 file_version=1.2}\n"),
            ("layout/ota.gds", ""),
            ("xschem/tb.sch", "v {xschem version=3.4.5 file_version=1.2}\n"),
        ] {
            let path = dir.join(p);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(path, body).unwrap();
        }
        let want = Pair { schematic: "xschem/ota.sch".into(), layout: "layout/ota.gds".into(), cell: None };
        // Desde el esquemático o desde el layout, el mismo par; el testbench no tiene.
        let from_sch = pair_for(&dir.join("xschem/ota.sch")).map(|(_, p)| p);
        let from_gds = pair_for(&dir.join("layout/ota.gds")).map(|(_, p)| p);
        let tb = pair_for(&dir.join("xschem/tb.sch"));
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(from_sch.as_ref(), Some(&want));
        assert_eq!(from_gds.as_ref(), Some(&want));
        assert!(tb.is_none());
    }
}
