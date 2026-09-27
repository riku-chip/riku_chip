//! Rutas que escribe el usuario → rutas del repo. Como en Git, un archivo
//! se nombra desde donde uno está: `riku diff amp.sch` en `repo/sub` es
//! `sub/amp.sch`. La CLI, el shell y el visor pasan por acá.

use std::path::{Component, Path, PathBuf};

/// `file` (relativo a `base`, o absoluto) como ruta relativa a la raíz
/// del repo `workdir`, con `/`.
///
/// Si desde `base` no existe pero desde la raíz sí, se toma desde la raíz
/// (quien ya escribía rutas del repo sigue funcionando). Un archivo borrado
/// no existe en ningún lado: se nombra desde `base`, como en Git. Fuera del
/// repo, queda tal cual (el error lo dirá quien lo busque).
pub fn to_repo_path(base: &Path, workdir: &Path, file: &str) -> String {
    let path = Path::new(file);
    if !path.is_absolute() && !base.join(path).exists() && workdir.join(path).exists() {
        return slashes(path);
    }
    let base = real(base);
    let workdir = real(workdir);
    let full = normalize(&if path.is_absolute() { path.to_path_buf() } else { base.join(path) });
    match full.strip_prefix(&workdir) {
        Ok(rel) if rel.as_os_str().is_empty() => file.to_string(),
        Ok(rel) => slashes(rel),
        Err(_) => file.to_string(),
    }
}

/// La ruta sin enlaces simbólicos si existe (el workdir de Git viene así);
/// si no, absoluta y normalizada.
fn real(p: &Path) -> PathBuf {
    p.canonicalize()
        .map(strip_verbatim)
        .unwrap_or_else(|_| normalize(&std::path::absolute(p).unwrap_or_else(|_| p.to_path_buf())))
}

/// En Windows `canonicalize` devuelve `\\?\C:\…`; sin el prefijo se puede
/// comparar con las demás rutas.
fn strip_verbatim(p: PathBuf) -> PathBuf {
    let s = p.to_string_lossy();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => p,
    }
}

/// Quita `.` y resuelve `..` sin tocar el disco (el archivo puede no existir).
fn normalize(p: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for c in p.components() {
        match c {
            Component::CurDir => {}
            Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn slashes(p: &Path) -> String {
    p.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn desde_una_subcarpeta_como_git() {
        let repo = tempfile::tempdir().unwrap();
        let root = repo.path();
        std::fs::create_dir_all(root.join("sub/deep")).unwrap();
        std::fs::write(root.join("sub/amp.sch"), b"").unwrap();
        std::fs::write(root.join("top.gds"), b"").unwrap();
        let sub = root.join("sub");

        assert_eq!(to_repo_path(&sub, root, "amp.sch"), "sub/amp.sch");
        assert_eq!(to_repo_path(&root.join("sub/deep"), root, "../amp.sch"), "sub/amp.sch");
        assert_eq!(to_repo_path(&sub, root, "../top.gds"), "top.gds");
        assert_eq!(to_repo_path(root, root, "./sub/amp.sch"), "sub/amp.sch");
        // Absoluta dentro del repo.
        assert_eq!(to_repo_path(root, root, &root.join("top.gds").to_string_lossy()), "top.gds");
        // Ruta del repo escrita desde una subcarpeta: sigue valiendo.
        assert_eq!(to_repo_path(&sub, root, "top.gds"), "top.gds");
        // Borrado (no existe en ningún lado): desde donde uno está.
        assert_eq!(to_repo_path(&sub, root, "viejo.sch"), "sub/viejo.sch");
        // Fuera del repo: tal cual.
        assert_eq!(to_repo_path(root, root, "../../fuera.sch"), "../../fuera.sch");
    }
}
