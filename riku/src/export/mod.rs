//! Imágenes sin ventana: `riku render` (una versión) y `riku diff -f png|svg`
//! (dos versiones). Sirven en CI, por SSH y para agentes de IA, que pueden
//! mirar la imagen: no hace falta pantalla ni GPU.
//!
//! Los esquemáticos y layouts salen del backend del visor de su formato (la
//! misma escena que dibuja la GUI) y se dibujan como SVG ([`svg`]); las
//! formas de onda, con su propio gráfico ([`wave`]). El PNG se genera desde
//! el SVG con `resvg`.

pub mod svg;
#[cfg(feature = "spice")]
pub mod wave;

use std::path::{Path, PathBuf};

use riku_kernel::{DiffFiles, DiffOptions, Registry};
use viewer_core::CancellationToken;

use crate::i18n::tr;

/// Qué imagen generar y dónde.
#[derive(Clone, Debug)]
pub struct Request {
    pub png: bool,
    /// Archivo de salida; `None` = la carpeta temporal de riku.
    pub output: Option<PathBuf>,
    pub width: u32,
    pub height: u32,
    pub dark: bool,
    /// Sub-vista (celda de un layout); `None` = la que elija el formato.
    pub cell: Option<String>,
}

/// Carpeta donde van las imágenes si no se pide otra: `<temp>/riku`.
pub fn default_dir() -> PathBuf {
    std::env::temp_dir().join("riku")
}

/// `2x` o `1600x1000` → (ancho, alto).
pub fn parse_size(s: &str) -> Result<(u32, u32), String> {
    let (w, h) = s.split_once(['x', 'X']).ok_or_else(|| tr!("err.size", size = s))?;
    let parse = |v: &str| v.trim().parse::<u32>().ok().filter(|n| (64..=16384).contains(n));
    match (parse(w), parse(h)) {
        (Some(w), Some(h)) => Ok((w, h)),
        _ => Err(tr!("err.size", size = s)),
    }
}

/// Nombre de archivo sin caracteres raros (`HEAD~1` → `HEAD_1`).
fn slug(s: &str) -> String {
    let s = s.replace(" → ", "-");
    let raw: String = s.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '.' { c } else { '_' }).collect();
    // Sin `__` repetidos ni en los bordes.
    raw.split('_').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("_")
}

/// Genera la imagen de `path`: de una versión (`before = None`) o del diff.
/// `label` va en el nombre del archivo por defecto y en el título.
#[allow(clippy::too_many_arguments)]
pub fn image(
    modules: &Registry,
    path: &str,
    before: Option<Vec<u8>>,
    after: Vec<u8>,
    files: DiffFiles,
    label: &str,
    req: &Request,
    diff: &DiffOptions,
) -> Result<PathBuf, String> {
    let name = Path::new(path).file_name().map_or_else(|| path.to_string(), |n| n.to_string_lossy().to_string());
    let style = svg::Style { width: req.width, height: req.height, dark: req.dark, caption: format!("{name}  ·  {label}") };
    let svg_text = if is_raw(path) {
        raw_svg(path, before.as_deref(), &after, diff, &style)?
    } else {
        let module =
            modules.for_path(path).or_else(|| modules.detect(&after)).ok_or_else(|| tr!("err.no_module", file = path))?;
        let backend = module.viewer().ok_or_else(|| tr!("err.no_image", file = path))?;
        let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().map_err(|e| e.to_string())?;
        let token = CancellationToken::new();
        let hint = Some(path.to_string());
        let scene = rt
            .block_on(async {
                match before {
                    Some(b) => backend.load_diff_with(b, after, hint, req.cell.clone(), files, token).await,
                    None => backend.load_with(after, hint, req.cell.clone(), files.after.clone(), token).await,
                }
            })
            .map_err(|e| e.to_string())?;
        svg::scene_svg(scene.as_ref(), &style)
    };

    let out = match &req.output {
        Some(p) => p.clone(),
        None => {
            let stem = Path::new(&name).file_stem().map_or_else(|| name.clone(), |s| s.to_string_lossy().to_string());
            let cell = req.cell.as_deref().map(|c| format!("-{}", slug(c))).unwrap_or_default();
            default_dir().join(format!("{}{cell}-{}.{}", slug(&stem), slug(label), if req.png { "png" } else { "svg" }))
        }
    };
    if let Some(dir) = out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let bytes = if req.png { svg::to_png(&svg_text)? } else { svg_text.into_bytes() };
    std::fs::write(&out, bytes).map_err(|e| format!("{}: {e}", out.display()))?;
    Ok(std::path::absolute(&out).unwrap_or(out))
}

fn is_raw(path: &str) -> bool {
    Path::new(path).extension().and_then(|e| e.to_str()).is_some_and(|e| e.eq_ignore_ascii_case("raw"))
}

#[cfg(feature = "spice")]
fn raw_svg(path: &str, before: Option<&[u8]>, after: &[u8], diff: &DiffOptions, style: &svg::Style) -> Result<String, String> {
    use crate::modules::spice::{compare::Tolerance, raw};
    let parse = |b: &[u8]| raw::parse(b).map_err(|e| format!("{path}: {e}"));
    let before = match before {
        Some(b) if !b.is_empty() => Some(parse(b)?),
        _ => None,
    };
    let after = parse(after)?;
    let tol = diff.tolerance.map_or_else(Tolerance::default, |rel| Tolerance { rel, ..Tolerance::default() });
    Ok(wave::wave_svg(before.as_ref(), &after, &diff.expressions, tol, style))
}

#[cfg(not(feature = "spice"))]
fn raw_svg(path: &str, _: Option<&[u8]>, _: &[u8], _: &DiffOptions, _: &svg::Style) -> Result<String, String> {
    Err(tr!("err.no_image", file = path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sizes_and_slugs() {
        assert_eq!(parse_size("1600x1000"), Ok((1600, 1000)));
        assert_eq!(parse_size("800X600"), Ok((800, 600)));
        assert!(parse_size("10x10").is_err() && parse_size("grande").is_err());
        assert_eq!(slug("HEAD~1 → worktree"), "HEAD_1-worktree");
        assert_eq!(slug("a b  c"), "a_b_c");
    }
}
