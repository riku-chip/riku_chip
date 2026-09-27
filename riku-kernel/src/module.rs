//! Contrato de los módulos de formato y el registro donde se enchufan.
//!
//! Un módulo trae todo lo que Riku sabe hacer con un formato: reconocerlo
//! ([`FormatModule::detect`]), comparar dos versiones
//! ([`FormatModule::diff`]) y, opcionalmente, mostrarlo en el visor
//! ([`FormatModule::viewer`]). El núcleo, la CLI y el visor solo conocen el
//! [`Registry`]; qué módulos existen se decide en un único lugar del
//! ejecutable.

use std::path::Path;
use std::sync::Arc;

use viewer_core::ViewerBackend;

use crate::{FileChange, FileFormat};

/// Descripción de un módulo (para `riku doctor` y la ayuda).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ModuleInfo {
    /// Nombre corto (`xschem`, `layout`).
    pub name: String,
    /// Versión o motor, texto libre para mostrar.
    pub version: String,
    pub format: FileFormat,
    /// Extensiones con punto (`.sch`, `.gds`).
    pub extensions: Vec<String>,
    /// `false` si el módulo está compilado pero no puede trabajar.
    pub available: bool,
}

/// Opciones de un diff. Cada módulo usa las que le aplican.
#[derive(Clone, Debug, PartialEq)]
pub struct DiffOptions {
    /// Umbral de área bajo el cual un cambio es cosmético, en las unidades
    /// de área del formato (µm² en layouts). `None` = el del módulo.
    pub cosmetic_threshold: Option<f64>,
    /// Permite usar la cache en disco de diffs caros.
    pub use_cache: bool,
    /// Señales calculadas a comparar además de las del archivo
    /// (`v(out)/v(in)`, `gain = db(v(out))`). Las usan los módulos de
    /// simulación; los demás las ignoran.
    pub expressions: Vec<String>,
}

impl Default for DiffOptions {
    fn default() -> Self {
        Self { cosmetic_threshold: None, use_cache: true, expressions: Vec::new() }
    }
}

/// Un formato de archivo de diseño, con todo lo que Riku sabe hacer con él.
pub trait FormatModule: Send + Sync {
    fn info(&self) -> ModuleInfo;

    /// `true` si el contenido es de este formato, por su firma (sin mirar la
    /// extensión).
    fn detect(&self, content: &[u8]) -> bool;

    /// Cambios entre dos versiones. Un lado vacío es un archivo que no
    /// existía en esa versión. Los problemas no fatales van en `warnings`.
    fn diff(&self, before: &[u8], after: &[u8], path_hint: &str, opts: &DiffOptions) -> FileChange;

    /// Backend del visor para este formato; `None` si no se puede mostrar.
    fn viewer(&self) -> Option<Arc<dyn ViewerBackend>> {
        None
    }

    /// `true` si la extensión de `path` es de este módulo.
    fn handles_path(&self, path: &str) -> bool {
        let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("");
        !ext.is_empty()
            && self.info().extensions.iter().any(|e| e.trim_start_matches('.').eq_ignore_ascii_case(ext))
    }
}

/// Módulos disponibles en este ejecutable.
#[derive(Clone, Default)]
pub struct Registry {
    modules: Vec<Arc<dyn FormatModule>>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add(&mut self, module: Arc<dyn FormatModule>) {
        self.modules.push(module);
    }

    pub fn with(mut self, module: Arc<dyn FormatModule>) -> Self {
        self.add(module);
        self
    }

    pub fn modules(&self) -> &[Arc<dyn FormatModule>] {
        &self.modules
    }

    /// Módulo para un archivo según su extensión.
    pub fn for_path(&self, path: &str) -> Option<&Arc<dyn FormatModule>> {
        self.modules.iter().find(|m| m.handles_path(path))
    }

    /// Módulo que reconoce el contenido por su firma.
    pub fn detect(&self, content: &[u8]) -> Option<&Arc<dyn FormatModule>> {
        self.modules.iter().find(|m| m.detect(content))
    }

    /// Formato del contenido, o `Unknown` si ningún módulo lo reconoce.
    pub fn detect_format(&self, content: &[u8]) -> FileFormat {
        self.detect(content).map_or(FileFormat::Unknown, |m| m.info().format)
    }

    /// Backends del visor de todos los módulos que tienen uno.
    pub fn viewers(&self) -> Vec<Arc<dyn ViewerBackend>> {
        self.modules.iter().filter_map(|m| m.viewer()).collect()
    }

    /// Extensiones que Riku sabe abrir (sin punto, en minúsculas).
    pub fn extensions(&self) -> Vec<String> {
        self.modules
            .iter()
            .flat_map(|m| m.info().extensions)
            .map(|e| e.trim_start_matches('.').to_ascii_lowercase())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Change, ChangeKind, Element};

    struct Fake(&'static str, FileFormat, &'static [u8]);

    impl FormatModule for Fake {
        fn info(&self) -> ModuleInfo {
            ModuleInfo {
                name: self.0.into(),
                version: "test".into(),
                format: self.1.clone(),
                extensions: vec![format!(".{}", self.0)],
                available: true,
            }
        }
        fn detect(&self, content: &[u8]) -> bool {
            content.starts_with(self.2)
        }
        fn diff(&self, _: &[u8], _: &[u8], _: &str, opts: &DiffOptions) -> FileChange {
            let mut f = FileChange::new(self.1.clone());
            let c = Change::new(ChangeKind::Added, Element::Whole).cosmetic(opts.cosmetic_threshold.is_some());
            f.changes.push(c);
            f
        }
    }

    fn registry() -> Registry {
        Registry::new()
            .with(Arc::new(Fake("sch", FileFormat::Xschem, b"v {xschem")))
            .with(Arc::new(Fake("gds", FileFormat::Gds, &[0, 6, 0, 2])))
    }

    #[test]
    fn modules_are_found_by_extension_and_by_signature() {
        let r = registry();
        assert_eq!(r.for_path("dir/amp.SCH").map(|m| m.info().name), Some("sch".to_string()));
        assert!(r.for_path("notas.txt").is_none() && r.for_path("sin_extension").is_none());
        assert_eq!(r.detect_format(&[0, 6, 0, 2, 9]), FileFormat::Gds);
        assert_eq!(r.detect_format(b"hola"), FileFormat::Unknown);
        assert_eq!(r.extensions(), vec!["sch", "gds"]);
        assert!(r.viewers().is_empty());
    }

    #[test]
    fn options_reach_the_module() {
        let r = registry();
        let m = r.for_path("a.gds").unwrap();
        let opts = DiffOptions { cosmetic_threshold: Some(1.0), ..Default::default() };
        assert!(m.diff(&[], &[], "a.gds", &opts).changes[0].cosmetic);
        assert!(!m.diff(&[], &[], "a.gds", &DiffOptions::default()).changes[0].cosmetic);
    }
}
