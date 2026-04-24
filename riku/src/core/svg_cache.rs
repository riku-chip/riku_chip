//! Caché en disco de SVGs renderizados, indexado por hash del contenido.
//!
//! Es una política propia de riku (la librería de render no decide dónde
//! ni cómo cachear). Se aísla aquí para que cualquier driver que quiera
//! cacheo pueda componerlo, sin duplicar la lógica de SHA256 + I/O.

use std::fs;
use std::path::PathBuf;

use dirs::cache_dir;
use sha2::{Digest, Sha256};

/// Directorio base donde se guardan los SVGs cacheados.
pub fn cache_root() -> PathBuf {
    cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("riku")
        .join("ops")
}

/// Ruta del SVG cacheado para un contenido dado. No comprueba existencia.
pub fn path_for(content: &[u8]) -> PathBuf {
    let digest = Sha256::digest(content);
    let key: String = digest.iter().map(|b| format!("{:02x}", b)).collect();
    cache_root().join(key).join("render.svg")
}

/// Intenta obtener el SVG cacheado sin renderizar.
/// Devuelve `Some(path)` si ya existe, `None` si no hay entrada.
pub fn lookup(content: &[u8]) -> Option<PathBuf> {
    let p = path_for(content);
    if p.exists() { Some(p) } else { None }
}

/// Guarda un SVG en el caché y devuelve la ruta donde quedó escrito.
/// Silenciosamente devuelve `None` si falla la escritura (el caller no
/// depende del cacheo para funcionar correctamente).
pub fn store(content: &[u8], svg: &str) -> Option<PathBuf> {
    let path = path_for(content);
    fs::create_dir_all(path.parent()?).ok()?;
    fs::write(&path, svg).ok()?;
    Some(path)
}

/// Obtiene el SVG del caché o lo produce con `render_fn` y lo almacena.
pub fn get_or_render<F>(content: &[u8], render_fn: F) -> Option<PathBuf>
where
    F: FnOnce() -> Option<String>,
{
    if let Some(p) = lookup(content) { return Some(p); }
    let svg = render_fn()?;
    store(content, &svg)
}
