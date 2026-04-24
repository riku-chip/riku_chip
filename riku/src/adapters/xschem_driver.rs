use std::path::PathBuf;

use crate::core::driver::{DiffEntry, DriverDiffReport, DriverInfo, RikuDriver};
use crate::core::format::detect_format;
use crate::core::models::{ChangeKind, DriverKind, FileFormat, Schematic};
use crate::core::svg_cache;

/// Parsea un .sch a su vista semántica usando las opciones por defecto de
/// riku (tema dark + símbolos de `.xschemrc`). Expuesto como helper para
/// que los consumidores no tengan que duplicar esta configuración.
pub fn parse(content: &[u8]) -> Schematic {
    let text = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(_) => return Schematic::default(),
    };
    let opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
    xschem_viewer::semantic::parse_semantic(text, &opts)
}

pub struct XschemDriver {
    cached_info: std::sync::OnceLock<DriverInfo>,
}

impl XschemDriver {
    pub fn new() -> Self {
        Self {
            cached_info: std::sync::OnceLock::new(),
        }
    }
}

impl Default for XschemDriver {
    fn default() -> Self {
        Self::new()
    }
}

impl RikuDriver for XschemDriver {
    fn info(&self) -> DriverInfo {
        if let Some(info) = self.cached_info.get() {
            return info.clone();
        }

        let pdk_root = std::env::var("PDK_ROOT").ok();
        let pdk_name = std::env::var("PDK").ok();

        let pdk_status = match (&pdk_root, &pdk_name) {
            (Some(root), Some(pdk)) => {
                let path = std::path::Path::new(root)
                    .join(pdk)
                    .join("libs.tech/xschem");
                if path.exists() {
                    format!("PDK: {} [ok]", pdk)
                } else {
                    format!("PDK: {} [error: ruta no encontrada]", pdk)
                }
            }
            _ => "PDK: [no detectado, usa PDK_ROOT/PDK o .xschemrc]".to_string(),
        };

        let info = DriverInfo {
            name: DriverKind::Xschem,
            available: true,
            version: format!("Native Renderer | {}", pdk_status),
            extensions: vec![".sch".to_string()],
        };

        let _ = self.cached_info.set(info.clone());
        info
    }

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str) -> DriverDiffReport {
        let mut report = DriverDiffReport {
            file_type: FileFormat::Xschem,
            ..Default::default()
        };

        if detect_format(content_a) != FileFormat::Xschem {
            report.warnings.push(format!(
                "{path_hint}: no es formato Xschem, usando diff de texto."
            ));
            return report;
        }

        let sch_a = parse(content_a);
        let sch_b = parse(content_b);
        let result = xschem_viewer::semantic::diff(&sch_a, &sch_b);
        for component in result.components {
            report.changes.push(DiffEntry {
                kind: component.kind,
                element: component.name,
                before: component.before,
                after: component.after,
                cosmetic: component.cosmetic,
                position_changed: component.position_changed,
            });
        }

        for net in result.nets_added {
            report.changes.push(DiffEntry {
                kind: ChangeKind::Added,
                element: format!("net:{net}"),
                before: None,
                after: None,
                cosmetic: false,
                position_changed: false,
            });
        }

        for net in result.nets_removed {
            report.changes.push(DiffEntry {
                kind: ChangeKind::Removed,
                element: format!("net:{net}"),
                before: None,
                after: None,
                cosmetic: false,
                position_changed: false,
            });
        }

        if result.is_move_all {
            report.changes.push(DiffEntry {
                kind: ChangeKind::Modified,
                element: "layout".to_string(),
                before: None,
                after: Some(
                    [(
                        "note".to_string(),
                        "reorganizacion cosmetica (Move All)".to_string(),
                    )]
                    .into_iter()
                    .collect(),
                ),
                cosmetic: true,
                position_changed: false,
            });
        }

        report
    }

    fn normalize(&self, content: &[u8], _path_hint: &str) -> Vec<u8> {
        content.to_vec()
    }

    fn render(&self, content: &[u8], _path_hint: &str) -> Option<PathBuf> {
        let text = std::str::from_utf8(content).ok()?;
        svg_cache::get_or_render(content, || {
            let mut opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
            if let (Ok(root), Ok(pdk)) = (std::env::var("PDK_ROOT"), std::env::var("PDK")) {
                let pdk_path = std::path::Path::new(&root).join(pdk).join("libs.tech/xschem");
                if pdk_path.exists() {
                    opts = opts.with_sym_path(pdk_path.to_string_lossy().to_string());
                }
            }
            xschem_viewer::Renderer::new(opts).render(text).ok().map(|r| r.svg)
        })
    }
}
