use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::core::domain::models::{ChangeKind, DriverKind, FileFormat};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiffEntry {
    pub kind: ChangeKind,
    pub element: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<BTreeMap<String, String>>,
    pub cosmetic: bool,
    #[serde(default)]
    pub position_changed: bool,
}

// ─── Protocolo driver → core ─────────────────────────────────────────────────
//
// Los drivers reportan `DiffEntry.element` con dos convenciones especiales que
// el core interpreta:
// - prefijo `"net:"` → el elemento es una net, no un componente
// - literal `"layout"` → cambio global del layout (típicamente cosmético)

/// Prefijo que los drivers usan en `DiffEntry.element` para marcar nets.
pub const NET_PREFIX: &str = "net:";

/// Valor especial de `DiffEntry.element` que indica "todo el layout cambió"
/// (típicamente un reposicionamiento global; usado para `is_move_all`).
pub const LAYOUT_ELEMENT: &str = "layout";

/// `true` si el elemento representa una net (empieza con `"net:"`).
pub fn is_net_element(element: &str) -> bool {
    element.starts_with(NET_PREFIX)
}

/// Devuelve el nombre de la net sin el prefijo `"net:"`. Si no tiene prefijo,
/// devuelve el elemento tal cual.
pub fn net_name(element: &str) -> &str {
    element.strip_prefix(NET_PREFIX).unwrap_or(element)
}

/// `true` si el elemento es el marcador especial `"layout"`.
pub fn is_layout_element(element: &str) -> bool {
    element == LAYOUT_ELEMENT
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DriverDiffReport {
    pub file_type: FileFormat,
    pub changes: Vec<DiffEntry>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_a: Option<std::path::PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_b: Option<std::path::PathBuf>,
    pub warnings: Vec<String>,
}

impl DriverDiffReport {
    pub fn is_empty(&self) -> bool {
        !self.changes.iter().any(|change| !change.cosmetic)
    }

    pub fn has_visuals(&self) -> bool {
        self.visual_a.is_some() || self.visual_b.is_some()
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DriverInfo {
    pub name: DriverKind,
    pub available: bool,
    pub version: String,
    pub extensions: Vec<String>,
}

pub trait RikuDriver: Send + Sync {
    fn info(&self) -> DriverInfo;

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str) -> DriverDiffReport;

    fn normalize(&self, content: &[u8], path_hint: &str) -> Vec<u8>;

    /// Renderiza el contenido a un SVG en memoria. Default `None` para drivers
    /// que no soportan render visual.
    fn render(&self, content: &[u8], path_hint: &str) -> Option<String> {
        let _ = (content, path_hint);
        None
    }

    fn can_handle(&self, filename: &str) -> bool {
        let suffix = Path::new(filename)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let suffix = if suffix.is_empty() {
            String::new()
        } else {
            format!(".{suffix}")
        };
        self.info().extensions.iter().any(|ext| ext == &suffix)
    }
}
