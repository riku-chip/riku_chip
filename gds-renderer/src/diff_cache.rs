//! Cache en disco de resultados de diff de layouts grandes.
//!
//! El XOR de un layout de decenas de MB puede tardar minutos. El resultado
//! depende solo de los bytes de cada lado y de los parametros, asi que se
//! guarda con esa clave y la segunda vez (otro `riku diff`, `riku log`, volver
//! a abrir el diff en la GUI) sale del disco.
//!
//! - **Clave:** hash de 128 bits de (version de gds-renderer, tipo de
//!   resultado, bytes de cada lado, parametros). Cambiar de version invalida
//!   todo; una clave que no coincide solo cuesta recalcular.
//! - **Cuando:** solo si las entradas suman mas de [`MIN_BYTES`]: los diffs
//!   chicos tardan milisegundos y no justifican tocar el disco.
//! - **Donde:** `$RIKU_CACHE_DIR`, o `<cache del sistema>/riku/diff`
//!   (`~/.cache` en Linux, `%LOCALAPPDATA%` en Windows).
//! - **Desactivar:** `RIKU_NO_CACHE=1` o `riku diff --no-cache`.
//! - **Limite:** [`MAX_TOTAL_BYTES`]; al escribir se borran las entradas mas
//!   viejas (por fecha de modificacion) hasta quedar debajo.
//! - **Corrupcion:** una entrada ilegible se borra y se recalcula; nunca es
//!   un error para el usuario.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use gdstk_rs::{OwnedPolygon, Point2D};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

use crate::gds_diff::{CellChange, CellDiff, GdsGeomDiff, LayerKey, LayerPolygons};

/// Tamano minimo (suma de las entradas) para usar la cache.
pub const MIN_BYTES: usize = 1 << 20;
/// Tope del directorio de cache.
pub const MAX_TOTAL_BYTES: u64 = 512 << 20;

/// Cache de diffs. `dir = None` = desactivada.
#[derive(Clone, Debug)]
pub struct DiffCache {
    dir: Option<PathBuf>,
    min_bytes: usize,
    max_total: u64,
}

impl DiffCache {
    /// Cache segun el entorno (ver doc del modulo).
    pub fn from_env() -> Self {
        let off = std::env::var("RIKU_NO_CACHE").is_ok_and(|v| !v.is_empty() && v != "0");
        let dir = if off {
            None
        } else {
            std::env::var_os("RIKU_CACHE_DIR")
                .map(PathBuf::from)
                .or_else(|| dirs::cache_dir().map(|d| d.join("riku").join("diff")))
        };
        Self { dir, min_bytes: MIN_BYTES, max_total: MAX_TOTAL_BYTES }
    }

    pub fn disabled() -> Self {
        Self { dir: None, min_bytes: MIN_BYTES, max_total: MAX_TOTAL_BYTES }
    }

    /// Cache en `dir` que guarda cualquier tamano (para tests).
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self { dir: Some(dir.into()), min_bytes: 0, max_total: MAX_TOTAL_BYTES }
    }

    pub fn with_max_total(mut self, bytes: u64) -> Self {
        self.max_total = bytes;
        self
    }

    pub fn is_enabled(&self) -> bool {
        self.dir.is_some()
    }

    /// Resultado cacheado de `compute` para (`kind`, `inputs`, `params`), o lo
    /// calcula y lo guarda. El `bool` dice si salio de la cache.
    pub fn get_or_compute<T, E>(
        &self,
        kind: &str,
        inputs: &[&[u8]],
        params: &str,
        compute: impl FnOnce() -> Result<T, E>,
    ) -> Result<(T, bool), E>
    where
        T: Serialize + DeserializeOwned,
    {
        let size: usize = inputs.iter().map(|b| b.len()).sum();
        let Some(dir) = self.dir.as_ref().filter(|_| size >= self.min_bytes) else {
            return compute().map(|v| (v, false));
        };
        let path = dir.join(format!("{}.json", key(kind, inputs, params)));
        if let Ok(bytes) = std::fs::read(&path) {
            match serde_json::from_slice::<T>(&bytes) {
                Ok(v) => {
                    touch(&path);
                    return Ok((v, true));
                }
                Err(_) => {
                    let _ = std::fs::remove_file(&path);
                }
            }
        }
        let v = compute()?;
        self.store(dir, &path, &v);
        Ok((v, false))
    }

    fn store<T: Serialize>(&self, dir: &Path, path: &Path, v: &T) {
        let Ok(json) = serde_json::to_vec(v) else { return };
        if std::fs::create_dir_all(dir).is_err() {
            return;
        }
        // Escribir aparte y renombrar: otro proceso nunca lee una entrada a medias.
        let tmp = path.with_extension(format!("tmp{}", std::process::id()));
        if std::fs::write(&tmp, json).is_ok() && std::fs::rename(&tmp, path).is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        prune(dir, self.max_total);
    }
}

fn key(kind: &str, inputs: &[&[u8]], params: &str) -> String {
    let half = |seed: u64| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (seed, env!("CARGO_PKG_VERSION"), kind, params).hash(&mut h);
        for b in inputs {
            b.hash(&mut h);
        }
        h.finish()
    };
    format!("{kind}-{:016x}{:016x}", half(0x5249_4b55), half(0x6764_7374))
}

/// Marca la entrada como usada (la limpieza borra primero las menos usadas).
fn touch(path: &Path) {
    if let Ok(f) = std::fs::File::options().append(true).open(path) {
        let _ = f.set_modified(std::time::SystemTime::now());
    }
}

/// Borra las entradas mas viejas hasta que el directorio pese `max_total` o menos.
fn prune(dir: &Path, max_total: u64) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    let mut files: Vec<(std::time::SystemTime, u64, PathBuf)> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| {
            let m = e.metadata().ok()?;
            Some((m.modified().ok()?, m.len(), e.path()))
        })
        .collect();
    let mut total: u64 = files.iter().map(|f| f.1).sum();
    files.sort_by_key(|f| f.0);
    for (_, len, path) in files {
        if total <= max_total {
            break;
        }
        if std::fs::remove_file(&path).is_ok() {
            total -= len;
        }
    }
}

// ─── Forma serializable de los resultados ────────────────────────────────────
//
// `OwnedPolygon` y `Point2D` son de gdstk-rs (sin serde): se copian a tipos
// propios para no tocar el binding.

#[derive(Serialize, Deserialize)]
struct PolyDto(u32, u32, Vec<[f64; 2]>);

#[derive(Serialize, Deserialize)]
struct LayerPolygonsDto {
    layer: LayerKey,
    added: Vec<PolyDto>,
    removed: Vec<PolyDto>,
}

/// `CellDiff` en disco.
#[derive(Serialize, Deserialize)]
pub struct CellDiffDto {
    geometry: Vec<GdsGeomDiff>,
    polygons: Vec<LayerPolygonsDto>,
}

fn poly_to_dto(p: &OwnedPolygon) -> PolyDto {
    PolyDto(p.layer, p.datatype, p.points.iter().map(|q| [q.x, q.y]).collect())
}

fn poly_from_dto(p: PolyDto) -> OwnedPolygon {
    OwnedPolygon { layer: p.0, datatype: p.1, points: p.2.into_iter().map(|[x, y]| Point2D { x, y }).collect() }
}

impl From<&CellDiff> for CellDiffDto {
    fn from(d: &CellDiff) -> Self {
        Self {
            geometry: d.geometry.clone(),
            polygons: d
                .polygons
                .iter()
                .map(|l| LayerPolygonsDto {
                    layer: l.layer,
                    added: l.added.iter().map(poly_to_dto).collect(),
                    removed: l.removed.iter().map(poly_to_dto).collect(),
                })
                .collect(),
        }
    }
}

impl From<CellDiffDto> for CellDiff {
    fn from(d: CellDiffDto) -> Self {
        Self {
            geometry: d.geometry,
            polygons: d
                .polygons
                .into_iter()
                .map(|l| LayerPolygons {
                    layer: l.layer,
                    added: l.added.into_iter().map(poly_from_dto).collect(),
                    removed: l.removed.into_iter().map(poly_from_dto).collect(),
                })
                .collect(),
        }
    }
}

/// Mapa de `changed_cells` en disco (mismo tipo, ya serializable).
pub type ChangedCells = BTreeMap<String, CellChange>;

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn second_call_is_a_hit_and_skips_compute() {
        let tmp = tempdir();
        let cache = DiffCache::at(&tmp);
        let calls = Cell::new(0);
        let run = || {
            cache.get_or_compute("t", &[b"a", b"b"], "x", || {
                calls.set(calls.get() + 1);
                Ok::<_, ()>(vec![1.5_f64, -0.0])
            })
        };
        assert_eq!(run().unwrap(), (vec![1.5, -0.0], false));
        assert_eq!(run().unwrap(), (vec![1.5, -0.0], true));
        assert_eq!(calls.get(), 1);
        // Otros bytes u otros parametros: otra clave.
        let other = cache.get_or_compute("t", &[b"a", b"c"], "x", || Ok::<_, ()>(vec![2.0])).unwrap();
        assert_eq!(other, (vec![2.0], false));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn corrupt_entry_is_recomputed_and_errors_are_not_cached() {
        let tmp = tempdir();
        let cache = DiffCache::at(&tmp);
        assert!(cache.get_or_compute("t", &[b"z"], "", || Err::<u8, _>("falla")).is_err());
        assert_eq!(std::fs::read_dir(&tmp).map(|d| d.count()).unwrap_or(0), 0);
        cache.get_or_compute("t", &[b"z"], "", || Ok::<_, ()>(7_u8)).unwrap();
        for e in std::fs::read_dir(&tmp).unwrap() {
            std::fs::write(e.unwrap().path(), b"{no es json").unwrap();
        }
        assert_eq!(cache.get_or_compute("t", &[b"z"], "", || Ok::<_, ()>(8_u8)).unwrap(), (8, false));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn small_inputs_and_disabled_cache_never_touch_disk() {
        let off = DiffCache::disabled();
        assert!(!off.is_enabled());
        assert_eq!(off.get_or_compute("t", &[b"a"], "", || Ok::<_, ()>(1)).unwrap(), (1, false));
        let tmp = tempdir();
        let mut cache = DiffCache::at(&tmp);
        cache.min_bytes = MIN_BYTES;
        cache.get_or_compute("t", &[b"chico"], "", || Ok::<_, ()>(1)).unwrap();
        assert!(!tmp.exists());
    }

    #[test]
    fn prune_keeps_the_directory_under_the_limit() {
        let tmp = tempdir();
        let cache = DiffCache::at(&tmp).with_max_total(1500);
        for i in 0..5_u8 {
            cache.get_or_compute("t", &[&[i]], "", || Ok::<_, ()>(vec![0_u8; 400])).unwrap();
        }
        let total: u64 = std::fs::read_dir(&tmp).unwrap().map(|e| e.unwrap().metadata().unwrap().len()).sum();
        assert!(total <= 1500, "{total}");
        let _ = std::fs::remove_dir_all(&tmp);
    }

    fn tempdir() -> PathBuf {
        use std::sync::atomic::{AtomicU32, Ordering};
        static N: AtomicU32 = AtomicU32::new(0);
        std::env::temp_dir().join(format!(
            "riku-cache-test-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ))
    }
}
