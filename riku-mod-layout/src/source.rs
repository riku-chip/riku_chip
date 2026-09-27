//! Lectura de un layout: GDSII/OASIS de sus bytes, o Magic con las
//! sub-celdas de su jerarquía. La usan el diff de la CLI
//! ([`crate::diff_layout_sides`]) y el visor ([`crate::GdsBackend`]): las
//! mismas reglas, los mismos avisos y la misma clave de cache.
//!
//! Leer va en dos pasos. [`collect`] junta los archivos (barato: con eso ya
//! se sabe la clave de la cache) y [`Raw::read`] arma la `Library` (caro:
//! solo si la cache no tiene el resultado).

use std::sync::Arc;

use gdstk_rs::magic::{MagInfo, MagSources};
use gdstk_rs::Library;
use viewer_core::FileSource;

/// Los archivos de un lado, antes de leerlos.
pub(crate) enum Raw<'a> {
    Bytes(&'a [u8]),
    Magic(MagSources),
}

/// Un lado leído: la `Library`, los avisos para el usuario y, si es Magic,
/// lo que trae además de la geometría (puertos).
pub(crate) struct Side {
    pub lib: Arc<Library>,
    pub notices: Vec<String>,
    pub info: Option<MagInfo>,
}

#[derive(Debug)]
pub(crate) enum ReadError {
    /// No es GDSII, OASIS ni Magic.
    NotLayout,
    Parse(String),
}

/// Los archivos de un layout. `path` es su ruta relativa a la raíz de
/// `files` (o absoluta en el disco); un `.mag` busca ahí sus sub-celdas,
/// y en el PDK.
pub(crate) fn collect<'a>(bytes: &'a [u8], path: Option<&str>, files: Option<&dyn FileSource>) -> Result<Raw<'a>, ReadError> {
    if crate::mag::is_magic(bytes) {
        crate::mag::collect(bytes, path.unwrap_or("layout.mag"), files).map(Raw::Magic).map_err(ReadError::Parse)
    } else if crate::is_layout(bytes) {
        Ok(Raw::Bytes(bytes))
    } else {
        Err(ReadError::NotLayout)
    }
}

impl Raw<'_> {
    /// Lo que determina el resultado de leerlo: el archivo, o todos los de
    /// la jerarquía Magic (editar una sub-celda cambia el resultado).
    fn inputs(&self) -> Vec<&[u8]> {
        match self {
            Raw::Bytes(b) => vec![b],
            Raw::Magic(s) => crate::mag::cache_inputs(s),
        }
    }

    /// Lambda con que se lee un `.mag` (sale del PDK o de `RIKU_MAG_LAMBDA`).
    fn lambda(&self) -> Option<f64> {
        match self {
            Raw::Bytes(_) => None,
            Raw::Magic(s) => Some(crate::mag::lambda_um(s.tech()).0),
        }
    }

    /// Arma la `Library`. `libs`: GDSII/OASIS ya leídos (el visor).
    pub fn read(&self, libs: Option<&LibCache>) -> Result<Side, ReadError> {
        match self {
            Raw::Magic(s) => {
                let (lib, info) = crate::mag::build(s);
                Ok(Side { lib: Arc::new(lib), notices: crate::mag::notices(&info), info: Some(info) })
            }
            Raw::Bytes(b) => {
                let read = || {
                    let lib = Library::from_bytes_any(b).map_err(|e| ReadError::Parse(e.to_string()))?;
                    crate::gds_diff::check_acyclic(&lib).map_err(ReadError::Parse)?;
                    let notices = crate::gds_diff::read_notes(&lib);
                    Ok((lib, notices))
                };
                let (lib, notices) = match libs {
                    Some(cache) => cache.get_or_read(b, read)?,
                    None => read().map(|(lib, n)| (Arc::new(lib), n))?,
                };
                Ok(Side { lib, notices, info: None })
            }
        }
    }
}

/// Clave de cache de un par de lados (`None` = el archivo no existía ahí):
/// las entradas de cada lado, marcadas, y los parámetros de lectura que no
/// están en los bytes (lambda de Magic).
pub(crate) fn pair_key<'a>(a: Option<&'a Raw<'_>>, b: Option<&'a Raw<'_>>) -> (Vec<&'a [u8]>, String) {
    let mut inputs: Vec<&[u8]> = vec![b"A"];
    inputs.extend(a.iter().flat_map(|r| r.inputs()));
    inputs.push(b"B");
    inputs.extend(b.iter().flat_map(|r| r.inputs()));
    let lambda = |r: Option<&Raw<'_>>| r.and_then(Raw::lambda).map_or(String::new(), |l| l.to_string());
    let params = match (a.and_then(Raw::lambda), b.and_then(Raw::lambda)) {
        (None, None) => String::new(),
        _ => format!("lambda={},{};", lambda(a), lambda(b)),
    };
    (inputs, params)
}

/// Bibliotecas GDSII/OASIS ya leídas, por contenido (hash y largo), las
/// [`LIB_CACHE`] más recientes: un par de diff. Leer el chip de 42 MB lleva
/// 1–3 s y antes se repetía en cada clic de celda. Magic no entra: sus
/// sub-celdas pueden cambiar sin que cambie el archivo principal.
#[derive(Default)]
pub(crate) struct LibCache(std::sync::Mutex<std::collections::VecDeque<(u64, usize, Arc<Library>, Vec<String>)>>);

const LIB_CACHE: usize = 2;

impl LibCache {
    pub fn get_or_read<E>(
        &self,
        bytes: &[u8],
        read: impl FnOnce() -> Result<(Library, Vec<String>), E>,
    ) -> Result<(Arc<Library>, Vec<String>), E> {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut h);
        let key = (h.finish(), bytes.len());
        {
            let mut libs = self.0.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(i) = libs.iter().position(|(k, n, ..)| (*k, *n) == key) {
                let hit = libs.remove(i).expect("índice válido");
                let out = (hit.2.clone(), hit.3.clone());
                libs.push_front(hit);
                return Ok(out);
            }
        }
        // Se lee sin el candado: otra carga puede seguir mientras tanto.
        let (lib, notes) = read()?;
        let lib = Arc::new(lib);
        let mut libs = self.0.lock().unwrap_or_else(|e| e.into_inner());
        libs.push_front((key.0, key.1, lib.clone(), notes.clone()));
        libs.truncate(LIB_CACHE);
        Ok((lib, notes))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn not_a_layout_is_rejected_before_reading() {
        assert!(matches!(collect(b"hola", None, None), Err(ReadError::NotLayout)));
    }

    #[test]
    fn key_marks_sides_and_magic_lambda() {
        let gds = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/hier_inv_a.gds")).unwrap();
        let a = collect(&gds, None, None).unwrap();
        let (inputs, params) = pair_key(Some(&a), None);
        assert_eq!(inputs, vec![b"A".as_slice(), gds.as_slice(), b"B".as_slice()]);
        assert!(params.is_empty(), "GDSII no depende de lambda");
        let (swapped, _) = pair_key(None, Some(&a));
        assert_ne!(inputs, swapped, "el mismo archivo de un lado u otro es otro diff");

        let mag = b"magic\ntech sky130A\nmagscale 1 2\n<< metal1 >>\nrect 0 0 10 10\n<< end >>\n";
        let m = collect(mag, Some("x.mag"), None).unwrap();
        let (_, params) = pair_key(Some(&m), Some(&m));
        assert!(params.starts_with("lambda="), "{params}");
    }
}
