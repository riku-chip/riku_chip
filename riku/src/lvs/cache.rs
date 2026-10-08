//! La caché de resultados del LVS, por dependencias: un resultado se reusa
//! para una versión (un commit o el working tree) solo si cada archivo que
//! leyó la corrida tiene el mismo contenido en esa versión, los que buscó y
//! no encontró siguen sin estar, y el entorno (PDK, Netgen, versiones) es el
//! mismo. Ver `docs/dev/design-notes.md` (D10).
//!
//! Los archivos del proyecto pasan todos por un [`FileSource`] (también los
//! que pide el netlister de `xschem-viewer-rust`), así que se registran con
//! [`RecordingFiles`]. Lo que no pasa por ahí (los símbolos del PDK, el
//! `setup.tcl`, Netgen) va en la huella del entorno ([`env_print`]).

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use git2::{ObjectType, Oid};
use serde::{Deserialize, Serialize};
use viewer_core::FileSource;

use super::{Pair, Report, Tools};

/// Lo que leyó una corrida: ruta (relativa a la raíz de la versión, o
/// absoluta) → id de Git de su contenido; `None` si se buscó y no estaba.
pub type Deps = BTreeMap<String, Option<String>>;

/// Entradas guardadas por par como mucho; al pasar, se borran las más viejas.
const MAX_PER_PAIR: usize = 200;

/// Un [`FileSource`] que anota todo lo que se le pide.
pub struct RecordingFiles {
    inner: Arc<dyn FileSource>,
    seen: Mutex<Deps>,
}

impl RecordingFiles {
    pub fn new(inner: Arc<dyn FileSource>) -> Self {
        Self { inner, seen: Mutex::new(Deps::new()) }
    }

    pub fn deps(&self) -> Deps {
        self.seen.lock().map(|d| d.clone()).unwrap_or_default()
    }
}

impl FileSource for RecordingFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let got = self.inner.read(path);
        if let Ok(mut seen) = self.seen.lock() {
            seen.insert(normalize(path), got.as_deref().map(blob_id));
        }
        got
    }

    fn describe(&self) -> String {
        self.inner.describe()
    }
}

fn normalize(path: &str) -> String {
    let p = path.replace('\\', "/");
    p.trim_start_matches("./").to_string()
}

/// El id que Git le da a estos bytes como blob.
pub fn blob_id(bytes: &[u8]) -> String {
    Oid::hash_object(ObjectType::Blob, bytes).map(|o| o.to_string()).unwrap_or_default()
}

/// Una versión del proyecto: un commit o el working tree.
pub trait Version {
    fn read(&self, path: &str) -> Option<Vec<u8>>;
    /// El id del contenido de `path`; `None` si no está.
    fn blob_id(&self, path: &str) -> Option<String>;
}

/// Una ruta absoluta (fuera del proyecto) se lee siempre del disco.
fn absolute(path: &str) -> Option<&Path> {
    Some(Path::new(path)).filter(|p| p.is_absolute())
}

/// Un commit: los ids salen del árbol, sin escribir nada.
pub struct CommitVersion<'r> {
    pub repo: &'r git2::Repository,
    pub tree: git2::Tree<'r>,
}

impl Version for CommitVersion<'_> {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        if let Some(p) = absolute(path) {
            return std::fs::read(p).ok();
        }
        let entry = self.tree.get_path(Path::new(path)).ok()?;
        Some(self.repo.find_blob(entry.id()).ok()?.content().to_vec())
    }

    fn blob_id(&self, path: &str) -> Option<String> {
        if let Some(p) = absolute(path) {
            return std::fs::read(p).ok().map(|b| blob_id(&b));
        }
        let entry = self.tree.get_path(Path::new(path)).ok()?;
        (entry.kind() == Some(ObjectType::Blob)).then(|| entry.id().to_string())
    }
}

/// El working tree (o cualquier carpeta). Con `core.autocrlf` el id de un
/// archivo puede no ser el de su blob: eso solo hace que se recalcule.
pub struct DiskVersion {
    pub root: PathBuf,
}

impl Version for DiskVersion {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(absolute(path).map_or_else(|| self.root.join(path), Path::to_path_buf)).ok()
    }

    fn blob_id(&self, path: &str) -> Option<String> {
        self.read(path).map(|b| blob_id(&b))
    }
}

/// Lo que determina el resultado y no pasa por un [`FileSource`]: la versión
/// de Riku y del netlister, el PDK del esquemático (carpeta, versión de
/// open_pdks, `setup.tcl` de Netgen, `xschemrc`, dónde se buscan los
/// símbolos) y el ejecutable de Netgen. `None` si no se sabe el PDK (la
/// corrida va a fallar igual).
pub fn env_print(schematic: &str, tools: &Tools) -> Option<String> {
    let (pdk, dir) = super::pdk_of(schematic).ok()?;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (env!("CARGO_PKG_VERSION"), riku_mod_layout::VERSION, xschem_viewer::spice::VERSION, &pdk, &dir).hash(&mut h);
    for f in [".config/nodeinfo.json", &format!("libs.tech/netgen/{pdk}_setup.tcl"), "libs.tech/xschem/xschemrc"] {
        std::fs::read(dir.join(f)).ok().hash(&mut h);
    }
    let symbols = crate::modules::xschem_pdk::symbol_source_for(schematic);
    format!("{:?}", symbols.paths()).hash(&mut h);
    let meta = std::fs::metadata(&tools.netgen).ok();
    let modified = meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok());
    (&tools.netgen, meta.map(|m| m.len()), modified.map(|d| d.as_secs())).hash(&mut h);
    Some(format!("{:016x}", h.finish()))
}

/// Si cada archivo registrado está igual en `v` (y los que faltaban siguen faltando).
pub(super) fn valid(deps: &Deps, v: &dyn Version) -> bool {
    deps.iter().all(|(path, id)| v.blob_id(path) == *id)
}

#[derive(Serialize, Deserialize)]
struct Entry {
    env: String,
    deps: Deps,
    report: Report,
}

/// Una corrida en memoria (también los errores, que no van al disco).
struct MemEntry {
    pair: String,
    env: String,
    deps: Deps,
    result: Result<Report, String>,
}

/// La caché: en memoria por proceso y en `<RIKU_CACHE_DIR o ~/.cache/riku>/lvs/v2`.
pub struct Cache {
    dir: Option<PathBuf>,
    mem: Vec<MemEntry>,
    /// Corridas nuevas (sin caché) en este proceso.
    pub runs: usize,
}

impl Cache {
    /// La de siempre; sin disco con `RIKU_NO_CACHE`.
    pub fn new() -> Self {
        Self::with_dir(cache_dir().map(|d| d.join("v2")))
    }

    pub fn with_dir(dir: Option<PathBuf>) -> Self {
        Self { dir, mem: Vec::new(), runs: 0 }
    }

    fn pair_dir(&self, pair: &Pair) -> Option<PathBuf> {
        Some(self.dir.as_ref()?.join(pair_key(pair)))
    }

    /// Un resultado guardado que vale para `v`.
    pub fn lookup(&mut self, pair: &Pair, env: &str, v: &dyn Version) -> Option<Result<Report, String>> {
        let key = pair_key(pair);
        if let Some(m) = self.mem.iter().find(|m| m.pair == key && m.env == env && valid(&m.deps, v)) {
            return Some(m.result.clone());
        }
        let dir = self.pair_dir(pair)?;
        let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&dir)
            .ok()?
            .flatten()
            .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
            .collect();
        files.sort_by(|a, b| b.0.cmp(&a.0));
        for (_, f) in files {
            let Some(entry) = std::fs::read_to_string(&f).ok().and_then(|t| serde_json::from_str::<Entry>(&t).ok()) else {
                continue;
            };
            if entry.env == env && valid(&entry.deps, v) {
                self.mem.push(MemEntry { pair: key, env: entry.env, deps: entry.deps, result: Ok(entry.report.clone()) });
                return Some(Ok(entry.report));
            }
        }
        None
    }

    /// Guarda una corrida: en memoria siempre; en el disco, si salió bien.
    pub fn store(&mut self, pair: &Pair, env: &str, deps: Deps, result: &Result<Report, String>) {
        if let (Ok(report), Some(dir)) = (result, self.pair_dir(pair)) {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            (env, &deps).hash(&mut h);
            let entry = Entry { env: env.to_string(), deps: deps.clone(), report: report.clone() };
            if std::fs::create_dir_all(&dir).is_ok() {
                let _ = serde_json::to_string(&entry).map(|t| std::fs::write(dir.join(format!("{:016x}.json", h.finish())), t));
                prune(&dir);
            }
        }
        self.mem.push(MemEntry { pair: pair_key(pair), env: env.to_string(), deps, result: result.clone() });
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

/// Deja las [`MAX_PER_PAIR`] entradas más nuevas.
pub(super) fn prune(dir: &Path) {
    let mut files: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    if files.len() <= MAX_PER_PAIR {
        return;
    }
    files.sort_by(|a, b| b.0.cmp(&a.0));
    for (_, f) in files.into_iter().skip(MAX_PER_PAIR) {
        let _ = std::fs::remove_file(f);
    }
}

pub(super) fn pair_key(pair: &Pair) -> String {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (&pair.schematic, &pair.layout, &pair.cell).hash(&mut h);
    format!("{:016x}", h.finish())
}

/// `RIKU_CACHE_DIR/lvs` o `~/.cache/riku/lvs`; `None` con `RIKU_NO_CACHE`.
pub(super) fn cache_dir() -> Option<PathBuf> {
    if std::env::var("RIKU_NO_CACHE").is_ok_and(|v| !v.is_empty() && v != "0") {
        return None;
    }
    std::env::var_os("RIKU_CACHE_DIR")
        .map(|d| PathBuf::from(d).join("lvs"))
        .or_else(|| dirs::cache_dir().map(|d| d.join("riku").join("lvs")))
}

#[cfg(test)]
mod tests {
    use super::super::{Comparison, Sides, Verdict};
    use super::*;

    fn report() -> Report {
        Report {
            pair: pair(),
            layout_cell: "amp".into(),
            pdk: "sky130A".into(),
            comparison: Comparison {
                result: Verdict::Match,
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
        }
    }

    fn pair() -> Pair {
        Pair { schematic: "xschem/amp.sch".into(), layout: "layout/amp.gds".into(), cell: None }
    }

    /// Un repo con el esquemático, un símbolo del proyecto en otra carpeta y el layout.
    struct Repo {
        dir: tempfile::TempDir,
        repo: git2::Repository,
    }

    impl Repo {
        fn new() -> Self {
            let dir = tempfile::tempdir().unwrap();
            let repo = git2::Repository::init(dir.path()).unwrap();
            let r = Self { dir, repo };
            r.write("xschem/amp.sch", "v {xschem version=3.4.5}\nC {sym/inv.sym} 0 0 0 0 {name=x1}\n");
            r.write("sym/inv.sym", "v {xschem version=3.4.5}\nK {type=subcircuit}\n");
            r.write("layout/amp.gds", "gds");
            r.commit("inicial");
            r
        }

        fn write(&self, path: &str, text: &str) {
            let p = self.dir.path().join(path);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }

        fn commit(&self, msg: &str) -> git2::Oid {
            let mut index = self.repo.index().unwrap();
            index.add_all(["*"], git2::IndexAddOption::DEFAULT, None).unwrap();
            index.write().unwrap();
            let tree = self.repo.find_tree(index.write_tree().unwrap()).unwrap();
            let sig = git2::Signature::now("t", "t@t").unwrap();
            let parents: Vec<git2::Commit> = self.repo.head().ok().and_then(|h| h.peel_to_commit().ok()).into_iter().collect();
            let parents: Vec<&git2::Commit> = parents.iter().collect();
            self.repo.commit(Some("HEAD"), &sig, &sig, msg, &tree, &parents).unwrap()
        }

        fn at(&self, id: git2::Oid) -> CommitVersion<'_> {
            CommitVersion { repo: &self.repo, tree: self.repo.find_commit(id).unwrap().tree().unwrap() }
        }

        fn disk(&self) -> DiskVersion {
            DiskVersion { root: self.dir.path().to_path_buf() }
        }

        /// Lo que habría registrado una corrida en esta versión: lo leído y
        /// un símbolo que se buscó junto al esquemático y no estaba.
        fn deps(&self, v: &dyn Version) -> Deps {
            let files = Arc::new(RecordingFiles::new(Arc::new(viewer_core::DiskFiles::new(self.dir.path()))));
            for p in ["xschem/amp.sch", "sym/inv.sym", "xschem/sym/inv.sym", "layout/amp.gds"] {
                files.read(p);
            }
            let deps = files.deps();
            assert!(valid(&deps, v), "lo registrado coincide con la versión de la que salió");
            deps
        }
    }

    #[test]
    fn records_what_was_read_and_what_was_missing() {
        let r = Repo::new();
        let deps = r.deps(&r.disk());
        assert_eq!(deps.get("xschem/sym/inv.sym"), Some(&None), "buscado y no encontrado");
        assert!(deps.get("sym/inv.sym").is_some_and(Option::is_some));
        assert_eq!(deps.len(), 4);
    }

    #[test]
    fn an_entry_is_reused_only_if_nothing_it_read_changed() {
        let r = Repo::new();
        let first = r.repo.head().unwrap().target().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let mut cache = Cache::with_dir(Some(cache_dir.path().to_path_buf()));
        cache.store(&pair(), "env", r.deps(&r.at(first)), &Ok(report()));

        // El mismo commit, y el working tree igual: sirve, también desde el disco.
        assert!(cache.lookup(&pair(), "env", &r.at(first)).is_some());
        assert!(Cache::with_dir(Some(cache_dir.path().to_path_buf())).lookup(&pair(), "env", &r.at(first)).is_some());
        assert!(cache.lookup(&pair(), "env", &r.disk()).is_some());
        // Otra huella del entorno (otro PDK, otro Netgen): no.
        assert!(cache.lookup(&pair(), "otro", &r.at(first)).is_none());

        // El símbolo de otra carpeta cambia: no sirve (la caché por carpetas sí lo reusaba).
        r.write("sym/inv.sym", "v {xschem version=3.4.5}\nK {type=subcircuit}\nT {cambio} 0 0 0 0 0.2 0.2 {}\n");
        assert!(cache.lookup(&pair(), "env", &r.disk()).is_none(), "en el disco");
        let second = r.commit("símbolo");
        assert!(cache.lookup(&pair(), "env", &r.at(second)).is_none(), "en el commit");
        assert!(cache.lookup(&pair(), "env", &r.at(first)).is_some(), "el commit viejo sigue valiendo");

        // Aparece un archivo que antes se buscó y no estaba: tampoco.
        let mut c2 = Cache::with_dir(None);
        c2.store(&pair(), "env", r.deps(&r.at(second)), &Ok(report()));
        assert!(c2.lookup(&pair(), "env", &r.at(second)).is_some());
        r.write("xschem/sym/inv.sym", "v {xschem version=3.4.5}\n");
        let third = r.commit("símbolo local");
        assert!(c2.lookup(&pair(), "env", &r.at(third)).is_none());

        // Un cambio en un archivo que la corrida no leyó no importa.
        r.write("README.md", "hola");
        let fourth = r.commit("readme");
        let mut c3 = Cache::with_dir(None);
        c3.store(&pair(), "env", r.deps(&r.at(third)), &Ok(report()));
        assert!(c3.lookup(&pair(), "env", &r.at(fourth)).is_some());
    }

    #[test]
    fn errors_stay_in_memory() {
        let r = Repo::new();
        let head = r.repo.head().unwrap().target().unwrap();
        let cache_dir = tempfile::tempdir().unwrap();
        let mut cache = Cache::with_dir(Some(cache_dir.path().to_path_buf()));
        cache.store(&pair(), "env", r.deps(&r.at(head)), &Err("netgen no está".into()));
        assert_eq!(cache.lookup(&pair(), "env", &r.at(head)), Some(Err("netgen no está".into())));
        assert!(Cache::with_dir(Some(cache_dir.path().to_path_buf())).lookup(&pair(), "env", &r.at(head)).is_none());
    }

    #[test]
    fn keeps_at_most_the_newest_entries() {
        let dir = tempfile::tempdir().unwrap();
        for i in 0..MAX_PER_PAIR + 5 {
            std::fs::write(dir.path().join(format!("{i}.json")), "{}").unwrap();
        }
        prune(dir.path());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), MAX_PER_PAIR);
    }
}
