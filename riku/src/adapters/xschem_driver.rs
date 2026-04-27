use crate::core::domain::driver::{
    DiffEntry, DriverDiffReport, DriverInfo, LAYOUT_ELEMENT, NET_PREFIX, RikuDriver,
};
use crate::core::domain::models::{ChangeKind, DriverKind, FileFormat, Schematic};
use crate::core::format::detect_format;
use crate::core::pdk;

const MOVE_ALL_NOTE: &str = "reorganizacion cosmetica (Move All)";

/// Opciones de render canónicas para Xschem: tema dark + símbolos de
/// `.xschemrc` + ruta del PDK desde `$PDK_ROOT/$PDK` si está disponible.
/// Fuente única para `parse`, `render` y diff — evita que el render
/// encuentre símbolos que el diff semántico no.
fn render_options() -> xschem_viewer::RenderOptions {
    let mut opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
    if let Some(path) = pdk::pdk_symbol_path() {
        opts = opts.with_sym_path(path.to_string_lossy().to_string());
    }
    opts
}

fn parse_text(text: &str) -> Schematic {
    xschem_viewer::semantic::parse_semantic(text, &render_options())
}

/// Valida un blob como contenido Xschem decodificable y devuelve el `&str`
/// listo para parsear. Usado por `diff` para chequear A y B simétricamente
/// antes de llamar al parser; cualquier error se propaga como warning del
/// `DriverDiffReport`.
fn validate_xschem<'a>(content: &'a [u8], side: &str, path_hint: &str) -> Result<&'a str, String> {
    let text = std::str::from_utf8(content).map_err(|_| {
        format!("{path_hint} ({side}): contenido no es UTF-8 valido, se omite el diff semantico.")
    })?;
    if detect_format(content) != FileFormat::Xschem {
        return Err(format!(
            "{path_hint} ({side}): no es formato Xschem, se omite el diff semantico."
        ));
    }
    Ok(text)
}

/// Parsea un .sch a su vista semántica usando las opciones por defecto de
/// riku (tema dark + símbolos de `.xschemrc` + PDK). Expuesto como helper
/// para que los consumidores no tengan que duplicar esta configuración.
/// En blobs no-UTF-8 devuelve `Schematic::default()`; los callers que
/// necesiten distinguir error vs vacío deben usar `XschemDriver::diff`.
pub fn parse(content: &[u8]) -> Schematic {
    match std::str::from_utf8(content) {
        Ok(text) => parse_text(text),
        Err(_) => Schematic::default(),
    }
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

        let pdk_status = match pdk::pdk_status() {
            pdk::PdkStatus::Found(_) => {
                let name = std::env::var("PDK").unwrap_or_default();
                format!("PDK: {} [ok]", name)
            }
            pdk::PdkStatus::Misconfigured(_) => {
                let name = std::env::var("PDK").unwrap_or_default();
                format!("PDK: {} [error: ruta no encontrada]", name)
            }
            pdk::PdkStatus::NotConfigured => {
                "PDK: [no detectado, usa PDK_ROOT/PDK o .xschemrc]".to_string()
            }
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

        let text_a = match validate_xschem(content_a, "A", path_hint) {
            Ok(t) => t,
            Err(w) => {
                report.warnings.push(w);
                return report;
            }
        };
        let text_b = match validate_xschem(content_b, "B", path_hint) {
            Ok(t) => t,
            Err(w) => {
                report.warnings.push(w);
                return report;
            }
        };

        let sch_a = parse_text(text_a);
        let sch_b = parse_text(text_b);
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
                element: format!("{NET_PREFIX}{net}"),
                before: None,
                after: None,
                cosmetic: false,
                position_changed: false,
            });
        }

        for net in result.nets_removed {
            report.changes.push(DiffEntry {
                kind: ChangeKind::Removed,
                element: format!("{NET_PREFIX}{net}"),
                before: None,
                after: None,
                cosmetic: false,
                position_changed: false,
            });
        }

        if result.is_move_all {
            report.changes.push(DiffEntry {
                kind: ChangeKind::Modified,
                element: LAYOUT_ELEMENT.to_string(),
                before: None,
                after: Some(
                    [("note".to_string(), MOVE_ALL_NOTE.to_string())]
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

    fn render(&self, content: &[u8], _path_hint: &str) -> Option<String> {
        let text = std::str::from_utf8(content).ok()?;
        xschem_viewer::Renderer::new(render_options())
            .render(text)
            .ok()
            .map(|r| r.svg)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SCH: &[u8] = br#"v {xschem version=3.0.0 file_version=1.2}
"#;

    fn driver() -> XschemDriver {
        XschemDriver::new()
    }

    #[test]
    fn diff_warns_on_invalid_utf8_in_a() {
        let invalid: &[u8] = &[0xFF, 0xFE, 0x00, 0x80];
        let report = driver().diff(invalid, VALID_SCH, "x.sch");
        assert!(report.changes.is_empty(), "no debe inventar cambios");
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("(A)") && report.warnings[0].contains("UTF-8"),
            "warning debe identificar lado A y mencionar UTF-8: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_invalid_utf8_in_b() {
        let invalid: &[u8] = &[0xFF, 0xFE, 0x00, 0x80];
        let report = driver().diff(VALID_SCH, invalid, "x.sch");
        assert!(report.changes.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("(B)") && report.warnings[0].contains("UTF-8"),
            "warning debe identificar lado B y mencionar UTF-8: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_non_xschem_b() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let report = driver().diff(VALID_SCH, svg, "x.sch");
        assert!(
            report.changes.is_empty(),
            "no debe reportar 'todo removido' falso: {:?}",
            report.changes
        );
        assert_eq!(report.warnings.len(), 1);
        assert!(
            report.warnings[0].contains("(B)") && report.warnings[0].contains("Xschem"),
            "warning debe identificar lado B y mencionar formato: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_non_xschem_a() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let report = driver().diff(svg, VALID_SCH, "x.sch");
        assert!(report.changes.is_empty());
        assert_eq!(report.warnings.len(), 1);
        assert!(report.warnings[0].contains("(A)"));
    }

    #[test]
    fn parse_returns_default_on_invalid_utf8() {
        let invalid: &[u8] = &[0xFF, 0xFE, 0x00];
        let s = parse(invalid);
        assert!(s.components.is_empty());
        assert!(s.wires.is_empty());
    }
}

