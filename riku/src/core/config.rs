//! Configuración del proyecto: `.riku.toml` en la raíz del repositorio.
//!
//! Fija las opciones de diff del proyecto para que todos (personas, CI,
//! agentes) comparen igual sin repetir flags:
//!
//! ```toml
//! [layout]
//! cosmetic_threshold_um2 = 0.01      # µm²: debajo, un cambio es cosmético
//!
//! [waveform]
//! tolerance = "0.5%"                 # o 0.005: fracción del rango de cada señal
//! expressions = [                    # señales calculadas a comparar siempre
//!   "gain = v(out)/v(in)",
//!   "tran: vpk = max(v(out))",
//! ]
//! ```
//!
//! Los flags de la línea de comandos ganan sobre el archivo; las expresiones
//! de `--expr` se suman a las del archivo.

use std::path::{Path, PathBuf};

use crate::i18n::tr;
use riku_kernel::DiffOptions;
use serde::Deserialize;

/// Nombre del archivo, en la raíz del repositorio.
pub const FILE: &str = ".riku.toml";

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ProjectConfig {
    #[serde(default)]
    pub layout: LayoutConfig,
    #[serde(default)]
    pub waveform: WaveformConfig,
    /// Qué esquemático va con qué layout para `riku lvs`.
    #[serde(default)]
    pub lvs: Vec<LvsConfig>,
}

/// Un par de `riku lvs` (rutas relativas a la raíz del repositorio).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LvsConfig {
    pub schematic: String,
    pub layout: String,
    /// Celda del layout; sin ella, la top.
    pub cell: Option<String>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct LayoutConfig {
    pub cosmetic_threshold_um2: Option<f64>,
}

#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct WaveformConfig {
    /// Número (`0.005`) o porcentaje (`"0.5%"`).
    pub tolerance: Option<Fraction>,
    #[serde(default)]
    pub expressions: Vec<String>,
}

/// Una fracción escrita como número o como porcentaje en texto.
#[derive(Clone, Debug, Deserialize, PartialEq)]
#[serde(untagged)]
pub enum Fraction {
    Number(f64),
    Text(String),
}

impl Fraction {
    fn value(&self) -> Result<f64, String> {
        match self {
            Fraction::Number(v) => check_fraction(*v, &v.to_string()),
            Fraction::Text(s) => parse_fraction(s),
        }
    }
}

/// `0.005` o `0.5%` → 0.005. Para `--tolerance` y el archivo.
pub fn parse_fraction(s: &str) -> Result<f64, String> {
    let t = s.trim();
    let (num, percent) = match t.strip_suffix('%') {
        Some(n) => (n.trim(), true),
        None => (t, false),
    };
    let v: f64 = num.parse().map_err(|_| tr!("config.not_number", value = s))?;
    check_fraction(if percent { v / 100.0 } else { v }, s)
}

fn check_fraction(v: f64, original: &str) -> Result<f64, String> {
    if v > 0.0 && v < 1.0 {
        Ok(v)
    } else {
        Err(tr!("config.tolerance_range", value = original))
    }
}

/// Flags de la línea de comandos que ganan sobre el archivo.
#[derive(Clone, Debug, Default)]
pub struct Overrides {
    pub cosmetic_threshold_um2: Option<f64>,
    pub tolerance: Option<f64>,
    pub expressions: Vec<String>,
    pub no_cache: bool,
}

impl ProjectConfig {
    /// Opciones de diff: flags > archivo > valores de cada módulo.
    pub fn diff_options(&self, o: Overrides) -> Result<DiffOptions, String> {
        let tolerance = match (o.tolerance, &self.waveform.tolerance) {
            (Some(t), _) => Some(t),
            (None, Some(f)) => Some(f.value().map_err(|e| format!("{FILE}: waveform.tolerance: {e}"))?),
            (None, None) => None,
        };
        let mut expressions = self.waveform.expressions.clone();
        for e in o.expressions {
            if !expressions.contains(&e) {
                expressions.push(e);
            }
        }
        Ok(DiffOptions {
            cosmetic_threshold: o.cosmetic_threshold_um2.or(self.layout.cosmetic_threshold_um2),
            tolerance,
            use_cache: !o.no_cache,
            expressions,
        })
    }
}

/// Lee `.riku.toml` de `root`. Sin archivo, la configuración vacía; con un
/// archivo mal escrito, un error que dice dónde.
pub fn load(root: Option<&Path>) -> Result<ProjectConfig, String> {
    let Some(path) = root.map(|r| r.join(FILE)).filter(|p| p.is_file()) else {
        return Ok(ProjectConfig::default());
    };
    let text = std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let cfg: ProjectConfig = toml::from_str(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if let Some(f) = &cfg.waveform.tolerance {
        f.value().map_err(|e| format!("{}: waveform.tolerance: {e}", path.display()))?;
    }
    Ok(cfg)
}

/// Raíz del repositorio que contiene `start` (para buscar el archivo).
pub fn repo_root(start: &Path) -> Option<PathBuf> {
    git2::Repository::discover(start).ok()?.workdir().map(Path::to_path_buf)
}

/// Opciones de diff del repo en `start`, con los flags dados.
pub fn options_for(start: &Path, o: Overrides) -> Result<DiffOptions, String> {
    load(repo_root(start).as_deref())?.diff_options(o)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fractions_as_number_or_percent() {
        assert_eq!(parse_fraction("0.005"), Ok(0.005));
        assert!((parse_fraction("0.5%").unwrap() - 0.005).abs() < 1e-15);
        assert!(parse_fraction("150%").is_err() && parse_fraction("abc").is_err() && parse_fraction("0").is_err());
    }

    #[test]
    fn file_then_flags() {
        let cfg: ProjectConfig = toml::from_str(
            "[layout]\ncosmetic_threshold_um2 = 0.05\n[waveform]\ntolerance = \"1%\"\nexpressions = [\"gain = v(out)/v(in)\"]\n",
        )
        .unwrap();
        let o = cfg.diff_options(Overrides::default()).unwrap();
        assert_eq!((o.cosmetic_threshold, o.tolerance, o.expressions.len()), (Some(0.05), Some(0.01), 1));
        let o = cfg
            .diff_options(Overrides {
                tolerance: Some(0.002),
                expressions: vec!["max(v(out))".into(), "gain = v(out)/v(in)".into()],
                ..Default::default()
            })
            .unwrap();
        assert_eq!(o.tolerance, Some(0.002));
        assert_eq!(o.expressions, vec!["gain = v(out)/v(in)".to_string(), "max(v(out))".to_string()]);
    }

    #[test]
    fn unknown_keys_and_missing_file() {
        assert!(toml::from_str::<ProjectConfig>("[waveform]\ntolerancia = 1\n").is_err());
        assert_eq!(load(Some(Path::new("/no/existe"))), Ok(ProjectConfig::default()));
    }
}
