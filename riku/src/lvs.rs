//! LVS: el layout contra el esquemático, en una versión (ver `docs/lvs.md`).
//!
//! 1. La versión en el disco: el working tree, o los archivos de un commit en
//!    una carpeta temporal (Xschem y Netgen leen del disco).
//! 2. La netlist del esquemático con `xschem --netlist`, con el `xschemrc`
//!    del PDK de sus símbolos (el mismo que elige el visor).
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

use serde::Serialize;
use serde_json::Value;

use crate::i18n::tr;
use crate::modules::xschem_pdk::{symbol_source_for, PdkSource};

pub const SCHEMA: &str = "riku-lvs/v1";

/// Un esquemático y su layout (rutas relativas a la raíz de la versión).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Pair {
    pub schematic: String,
    pub layout: String,
    /// Celda del layout; `None`: la top.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Coinciden conectividad y parámetros.
    Match,
    /// La conectividad coincide; algún parámetro (W, L…) no.
    PropertyErrors,
    /// No coinciden: redes, dispositivos o pines.
    Mismatch,
}

/// Lo mismo visto del lado del layout y del esquemático.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Sides<T> {
    pub layout: T,
    pub schematic: T,
}

/// Un dispositivo emparejado con un parámetro distinto.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyError {
    pub model: String,
    /// Nombres de instancia (`M1` en el esquemático, `19` en el layout).
    pub layout: String,
    pub schematic: String,
    pub values: Vec<PropertyValue>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct PropertyValue {
    pub name: String,
    pub layout: String,
    pub schematic: String,
}

/// Lo que dijo Netgen, sin los datos del par.
#[derive(Clone, Debug, PartialEq, Serialize)]
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

/// El LVS de un par en una versión.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Report {
    #[serde(flatten)]
    pub pair: Pair,
    /// La celda comparada del layout.
    pub layout_cell: String,
    pub pdk: String,
    #[serde(flatten)]
    pub comparison: Comparison,
    pub warnings: Vec<String>,
}

// ─── Herramientas ────────────────────────────────────────────────────────────

/// `xschem` y `netgen`: en el `PATH` o en `/foss/tools/bin` (iic-osic-tools).
pub struct Tools {
    pub xschem: PathBuf,
    pub netgen: PathBuf,
}

pub fn tools() -> Result<Tools, String> {
    let find = |name: &str| {
        std::env::var_os("PATH")
            .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
            .unwrap_or_default()
            .into_iter()
            .chain([PathBuf::from("/foss/tools/bin")])
            .map(|d| d.join(name))
            .find(|p| p.is_file())
            .ok_or_else(|| tr!("lvs.no_tool", tool = name))
    };
    Ok(Tools { xschem: find("xschem")?, netgen: find("netgen")? })
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
        let tree = r
            .revparse_single(rev)
            .and_then(|o| o.peel_to_tree())
            .map_err(|_| tr!("git.commit_not_found", commit = rev))?;
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
            let written = path.parent().map_or(Ok(()), std::fs::create_dir_all).and_then(|_| std::fs::write(&path, blob.content()));
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

    // Esquemático: `xschem --netlist` en su carpeta, con el PDK de sus símbolos.
    let stem = sch_path.file_stem().map(|s| s.to_string_lossy().to_string()).unwrap_or_default();
    let rc = pdk_dir.join("libs.tech/xschem/xschemrc");
    let sch_dir = sch_path.parent().unwrap_or(&tree.root);
    let out = Command::new(&tools.xschem)
        .arg("--rcfile")
        .arg(&rc)
        .args(["--tcl", "set lvs_netlist 1; set top_subckt 1", "-n", "-s", "-q", "-x", "--no_x", "-o"])
        .arg(&work.0)
        .args(["-N", "schematic.spice"])
        .arg(sch_path.file_name().unwrap_or_default())
        .current_dir(sch_dir)
        .env("PDK_ROOT", pdk_dir.parent().unwrap_or(&pdk_dir))
        .env("PDK", &pdk)
        .output()
        .map_err(|e| tr!("lvs.tool_failed", tool = "xschem", error = e))?;
    let sch_spice = std::fs::read_to_string(work.0.join("schematic.spice"))
        .map_err(|_| tr!("lvs.tool_failed", tool = "xschem", error = tail(&out.stderr)))?;
    warnings.extend(sch_spice.lines().filter(|l| l.contains("IS MISSING")).map(|l| tr!("lvs.missing_symbol", line = l.trim_start_matches('*').trim())));

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
    let (json, text) = (std::fs::read_to_string(work.0.join("comp.json")), std::fs::read_to_string(work.0.join("comp.out")));
    let comparison = match (json, text) {
        (Ok(json), Ok(text)) => parse_netgen(&json, &text)?,
        // Sin JSON (p. ej. las celdas de arriba no se pudieron emparejar):
        // el veredicto del texto, que es "no coinciden".
        (Err(_), Ok(text)) if text.contains("Final result") => from_text(&text),
        _ => return Err(tr!("lvs.tool_failed", tool = "netgen", error = tail(&out.stdout))),
    };
    Ok(Report { pair: pair.clone(), layout_cell: layout.cell, pdk, comparison, warnings })
}

/// Las últimas líneas de la salida de una herramienta, para un error.
fn tail(bytes: &[u8]) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    lines[lines.len().saturating_sub(6)..].join(" | ")
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
        v.as_array()
            .into_iter()
            .flatten()
            .filter_map(|d| Some((d.get(0)?.as_str()?.to_string(), d.get(1)?.as_u64()?)))
            .collect()
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
    let failed = out.contains("Netlists do not match") || out.contains("failed") || !unmatched_nets.is_empty() || !unmatched_devices.is_empty();
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
        devices: Sides { layout: devices(&side(top.get("devices").unwrap_or(&Value::Null), 0)), schematic: devices(&side(top.get("devices").unwrap_or(&Value::Null), 1)) },
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
        let c = from_text("Final result: 
Top level cell failed pin matching.

LVS Done.
");
        assert_eq!((c.result, c.summary), (Verdict::Mismatch, vec!["Top level cell failed pin matching.".to_string()]));
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
}
