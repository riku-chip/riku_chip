//! Otros archivos de la misma versión que el que se abre o compara.
//!
//! Algunos formatos reparten un diseño en varios archivos: un layout de
//! Magic tiene una celda por archivo y sus sub-celdas (`use`) viven en otros.
//! Para leerlos, el backend (o el módulo de formato) recibe un
//! [`FileSource`]: los archivos del mismo commit, o los del disco. Así el
//! diff de un commit viejo usa las sub-celdas de ese commit, no las de hoy.
//!
//! Las rutas son relativas a la raíz de la fuente (la del repo), con `/`,
//! como el `path_hint` del archivo principal.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Archivos de una versión del proyecto.
pub trait FileSource: Send + Sync {
    /// Contenido de `path` (relativo a la raíz, con `/`); `None` si no existe.
    fn read(&self, path: &str) -> Option<Vec<u8>>;

    /// De dónde salen los archivos, para mensajes (`commit abc1234`, `disco`).
    fn describe(&self) -> String {
        String::new()
    }
}

/// Archivos del disco, bajo una raíz. Una ruta absoluta se lee tal cual.
#[derive(Clone, Debug)]
pub struct DiskFiles {
    root: PathBuf,
}

impl DiskFiles {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl FileSource for DiskFiles {
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        let p = Path::new(path);
        let full = if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) };
        std::fs::read(full).ok()
    }

    fn describe(&self) -> String {
        format!("disco ({})", self.root.display())
    }
}

/// Fuentes de los dos lados de un diff. `None` = sin acceso a otros
/// archivos de ese lado (el módulo compara solo lo que recibe).
#[derive(Clone, Default)]
pub struct DiffFiles {
    pub before: Option<Arc<dyn FileSource>>,
    pub after: Option<Arc<dyn FileSource>>,
}

impl DiffFiles {
    pub fn new(before: Option<Arc<dyn FileSource>>, after: Option<Arc<dyn FileSource>>) -> Self {
        Self { before, after }
    }
}

impl std::fmt::Debug for DiffFiles {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let d = |s: &Option<Arc<dyn FileSource>>| s.as_ref().map(|s| s.describe());
        f.debug_struct("DiffFiles").field("before", &d(&self.before)).field("after", &d(&self.after)).finish()
    }
}

/// `rel` (con `/`, quizá con `..`) visto desde el directorio de `from_file`,
/// normalizado. Una ruta absoluta queda igual.
///
/// ```
/// use viewer_core::files::join_relative;
/// assert_eq!(join_relative("lib/top/a.mag", "../cells/b.mag"), "lib/cells/b.mag");
/// assert_eq!(join_relative("a.mag", "b.mag"), "b.mag");
/// ```
pub fn join_relative(from_file: &str, rel: &str) -> String {
    if rel.starts_with('/') || Path::new(rel).is_absolute() {
        return normalize(rel);
    }
    let dir = match from_file.rfind('/') {
        Some(i) => &from_file[..i],
        None => "",
    };
    if dir.is_empty() { normalize(rel) } else { normalize(&format!("{dir}/{rel}")) }
}

/// Carpeta con los PDK instalados: `$PDK_ROOT`, o `/foss/pdks`
/// (iic-osic-tools) si existe. `None` si ninguna es una carpeta. La usan los
/// módulos que buscan archivos del PDK (símbolos de Xschem, capas y celdas
/// de layouts).
pub fn pdk_root() -> Option<PathBuf> {
    std::env::var_os("PDK_ROOT")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| Some(PathBuf::from("/foss/pdks")).filter(|p| p.is_dir()))
}

/// Quita `.` y resuelve `..`. Una ruta absoluta no sube de `/`; en una
/// relativa, los `..` que sobran quedan al principio (`../x` sigue siendo
/// `../x`: quien la use decide si sale de su raíz).
fn normalize(path: &str) -> String {
    let absolute = path.starts_with('/');
    let mut parts: Vec<&str> = Vec::new();
    for p in path.split('/') {
        match p {
            "" | "." => {}
            ".." => {
                if parts.last().is_some_and(|l| *l != "..") {
                    parts.pop();
                } else if !absolute {
                    parts.push("..");
                }
            }
            other => parts.push(other),
        }
    }
    let joined = parts.join("/");
    if absolute { format!("/{joined}") } else { joined }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_paths() {
        assert_eq!(join_relative("a/b/c.mag", "d.mag"), "a/b/d.mag");
        assert_eq!(join_relative("a/b/c.mag", "../../x/d.mag"), "x/d.mag");
        assert_eq!(join_relative("c.mag", "../d.mag"), "../d.mag");
        assert_eq!(join_relative("a/c.mag", "./lib/./d.mag"), "a/lib/d.mag");
        assert_eq!(join_relative("a/c.mag", "/abs/d.mag"), "/abs/d.mag");
        assert_eq!(join_relative("/abs/c.mag", "../d.mag"), "/d.mag");
    }

    #[test]
    fn disk_files_read_relative_and_absolute() {
        let dir = std::env::temp_dir().join(format!("viewer_core_files_{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub/x.txt"), b"hola").unwrap();
        let src = DiskFiles::new(&dir);
        assert_eq!(src.read("sub/x.txt").as_deref(), Some(&b"hola"[..]));
        let abs = dir.join("sub/x.txt");
        assert_eq!(src.read(abs.to_str().unwrap()).as_deref(), Some(&b"hola"[..]));
        assert!(src.read("nada.txt").is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
