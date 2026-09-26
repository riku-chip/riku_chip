//! PDK para el módulo Xschem: dónde están los símbolos del PDK.
//!
//! La ruta de símbolos es `<PDK_ROOT>/<PDK>/libs.tech/xschem`. Si `$PDK` no
//! está definida (pasa seguido: el contenedor iic-osic-tools define
//! `PDK_ROOT` pero no siempre `PDK`), se **detecta por los símbolos del
//! esquemático**: gana el PDK instalado que tiene más de los `.sym` que el
//! archivo referencia (`sky130_fd_pr/nfet_01v8.sym` → `sky130A`).
//!
//! Consumido por `cli::doctor`, el shell y `modules::xschem` (diff y visor).
//! Solo usa std.

use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdkStatus {
    /// `PDK_ROOT` o `PDK` no están en el entorno.
    NotConfigured,
    /// Ambos configurados, pero la ruta `<PDK_ROOT>/<PDK>/libs.tech/xschem`
    /// no existe en disco.
    Misconfigured(PathBuf),
    /// Ruta encontrada.
    Found(PathBuf),
}

/// De dónde salió la ruta de símbolos de un esquemático.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PdkSource {
    /// `$PDK_ROOT/$PDK`.
    Env(PathBuf),
    /// `$PDK` no está definida y se eligieron los PDKs por los símbolos del
    /// archivo: el que tiene más primero y, si el diseño mezcla, los que
    /// aportan los símbolos que faltan.
    Detected(Vec<(String, PathBuf)>),
    /// No hay ruta del PDK (sin `$PDK` y sin un PDK instalado que coincida, o
    /// `$PDK` apunta a algo que no existe). El texto dice qué falta.
    Missing(String),
}

impl PdkSource {
    /// Rutas de símbolos a agregar, en orden de prioridad.
    pub fn paths(&self) -> Vec<&Path> {
        match self {
            Self::Env(p) => vec![p.as_path()],
            Self::Detected(v) => v.iter().map(|(_, p)| p.as_path()).collect(),
            Self::Missing(_) => Vec::new(),
        }
    }
}

/// PDKs que se prefieren si dos tienen los mismos símbolos (`gf180mcuC` y
/// `gf180mcuD`, `sky130A` y `sky130B`).
const PREFERRED: &[&str] = &["sky130A", "gf180mcuD", "ihp-sg13g2"];

pub fn pdk_status() -> PdkStatus {
    let (Some(root), Some(name)) = (std::env::var("PDK_ROOT").ok(), std::env::var("PDK").ok().filter(|p| !p.is_empty()))
    else {
        return PdkStatus::NotConfigured;
    };
    let path = Path::new(&root).join(&name).join("libs.tech/xschem");
    if path.exists() {
        PdkStatus::Found(path)
    } else {
        PdkStatus::Misconfigured(path)
    }
}

/// Carpeta con los PDKs instalados: `$PDK_ROOT`, o `/foss/pdks` (iic-osic-tools)
/// si existe.
pub fn pdk_root() -> Option<PathBuf> {
    std::env::var_os("PDK_ROOT")
        .map(PathBuf::from)
        .or_else(|| Some(PathBuf::from("/foss/pdks")).filter(|p| p.is_dir()))
}

/// PDKs instalados en `root` que tienen símbolos de Xschem, ordenados.
pub fn installed_pdks(root: &Path) -> Vec<String> {
    let Ok(rd) = std::fs::read_dir(root) else { return Vec::new() };
    let mut v: Vec<String> = rd
        .filter_map(Result::ok)
        .filter(|e| e.path().join("libs.tech/xschem").is_dir())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .collect();
    v.sort();
    v
}

/// Ruta de símbolos para un esquemático concreto: la de `$PDK` si está, si no
/// la del PDK instalado que tiene sus símbolos.
pub fn symbol_source_for(content: &str) -> PdkSource {
    match pdk_status() {
        PdkStatus::Found(p) => PdkSource::Env(p),
        PdkStatus::Misconfigured(p) => {
            PdkSource::Missing(format!("$PDK_ROOT/$PDK apunta a {}, que no existe", p.display()))
        }
        PdkStatus::NotConfigured => match pdk_root() {
            Some(root) => match detect_pdks(&root, content) {
                found if !found.is_empty() => PdkSource::Detected(found),
                _ => PdkSource::Missing(format!(
                    "$PDK no está definida y ningún PDK de {} tiene los símbolos de este esquemático",
                    root.display()
                )),
            },
            None => PdkSource::Missing("$PDK_ROOT y $PDK no están definidas".into()),
        },
    }
}

/// PDKs de `root` que resuelven los símbolos del esquemático, el mejor
/// primero. Elige de a uno el que cubre más símbolos todavía sin resolver
/// (a igualdad, el preferido), hasta que no quede ninguno que aporte: un
/// diseño de un solo PDK da uno; uno que mezcla, varios. Vacío si ninguno
/// resuelve nada.
pub fn detect_pdks(root: &Path, content: &str) -> Vec<(String, PathBuf)> {
    let mut pending = referenced_symbols(content);
    let rank = |name: &str| PREFERRED.iter().position(|p| *p == name).unwrap_or(PREFERRED.len());
    let mut candidates: Vec<(String, PathBuf)> = installed_pdks(root)
        .into_iter()
        .map(|name| {
            let dir = root.join(&name).join("libs.tech/xschem");
            (name, dir)
        })
        .collect();
    let mut chosen = Vec::new();
    while !pending.is_empty() {
        let best = candidates
            .iter()
            .enumerate()
            .map(|(i, (name, dir))| (pending.iter().filter(|s| dir.join(s).is_file()).count(), i, name))
            .filter(|(hits, ..)| *hits > 0)
            .min_by(|a, b| b.0.cmp(&a.0).then(rank(a.2).cmp(&rank(b.2))).then(a.2.cmp(b.2)))
            .map(|(_, i, _)| i);
        let Some(i) = best else { break };
        let (name, dir) = candidates.remove(i);
        pending.retain(|s| !dir.join(s).is_file());
        chosen.push((name, dir));
    }
    chosen
}

/// Símbolos con carpeta (`sky130_fd_pr/nfet_01v8.sym`) que referencia un
/// esquemático en sus instancias `C {…}`. Los de la librería estándar de
/// Xschem (`res.sym`, sin carpeta) no identifican a ningún PDK.
fn referenced_symbols(content: &str) -> Vec<String> {
    let mut v: Vec<String> = content
        .lines()
        .filter_map(|l| l.strip_prefix("C {"))
        .filter_map(|rest| rest.split('}').next())
        .filter(|s| s.contains('/') && !s.starts_with('/'))
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    v
}

/// Ruta de símbolos del PDK según el entorno (`$PDK_ROOT/$PDK`), sin mirar
/// ningún archivo. Para quienes no tienen un esquemático a mano.
pub fn pdk_symbol_path() -> Option<PathBuf> {
    match pdk_status() {
        PdkStatus::Found(p) => Some(p),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_root(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("riku-pdk-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for (pdk, sym) in [
            ("sky130A", "sky130_fd_pr/nfet_01v8.sym"),
            ("sky130A", "sky130_fd_pr/pfet_01v8.sym"),
            ("sky130B", "sky130_fd_pr/nfet_01v8.sym"),
            ("sky130B", "sky130_fd_pr/pfet_01v8.sym"),
            ("ihp-sg13g2", "sg13g2_pr/sg13_lv_nmos.sym"),
        ] {
            let p = root.join(pdk).join("libs.tech/xschem").join(sym);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"v {xschem version=3.4.5}").unwrap();
        }
        std::fs::create_dir_all(root.join("sin_xschem")).unwrap();
        root
    }

    fn names(v: &[(String, PathBuf)]) -> Vec<&str> {
        v.iter().map(|(n, _)| n.as_str()).collect()
    }

    const SKY: &str = "v {xschem version=3.4.5 file_version=1.2}\n\
C {sky130_fd_pr/nfet_01v8.sym} 0 0 0 0 {name=M1}\n\
C {sky130_fd_pr/pfet_01v8.sym} 0 0 0 0 {name=M2}\n\
C {res.sym} 0 0 0 0 {name=R1}\n";

    #[test]
    fn detects_the_pdk_that_has_the_symbols() {
        let root = fake_root("detect");
        assert_eq!(installed_pdks(&root), vec!["ihp-sg13g2", "sky130A", "sky130B"]);
        // sky130A y sky130B empatan: gana el preferido, y sky130B no aporta nada más.
        let found = detect_pdks(&root, SKY);
        assert_eq!(found, vec![("sky130A".to_string(), root.join("sky130A/libs.tech/xschem"))]);
        let ihp = "C {sg13g2_pr/sg13_lv_nmos.sym} 0 0 0 0 {name=M1}\n";
        assert_eq!(names(&detect_pdks(&root, ihp)), vec!["ihp-sg13g2"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_design_that_mixes_pdks_gets_all_of_them() {
        let root = fake_root("mix");
        let mixed = format!("{SKY}C {{sg13g2_pr/sg13_lv_nmos.sym}} 0 0 0 0 {{name=M9}}\n");
        // sky130A cubre 2 símbolos; ihp-sg13g2, el que falta.
        assert_eq!(names(&detect_pdks(&root, &mixed)), vec!["sky130A", "ihp-sg13g2"]);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn generic_symbols_do_not_pick_a_pdk() {
        let root = fake_root("generic");
        assert!(detect_pdks(&root, "C {res.sym} 0 0 0 0 {name=R1}\nC {devices/vsource.sym} 0 0 0 0 {}\n").is_empty());
        assert_eq!(referenced_symbols(SKY), vec!["sky130_fd_pr/nfet_01v8.sym", "sky130_fd_pr/pfet_01v8.sym"]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
