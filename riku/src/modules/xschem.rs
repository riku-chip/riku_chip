use std::collections::{BTreeMap, BTreeSet};

use xschem_viewer::semantic::{ChangeKind as XsKind, ComponentDiff, SemanticSchematic as Schematic};

use riku_kernel::{DiffOptions, FormatModule, ModuleInfo};
use crate::core::domain::models::{Change, ChangeKind, Element, FileChange, FileFormat, Value};
use super::xschem_pdk as pdk;

const MOVE_ALL_NOTE: &str = "reorganizacion cosmetica (Move All)";

/// Opciones de render canónicas para un esquemático: tema dark + símbolos de
/// `.xschemrc` + símbolos del PDK (`$PDK_ROOT/$PDK`, o el PDK instalado que
/// tiene los símbolos del archivo si `$PDK` no está definida). Fuente única
/// para el diff y el visor: los dos encuentran los mismos símbolos.
pub(super) fn render_options_for(text: &str) -> (xschem_viewer::RenderOptions, pdk::PdkSource) {
    let mut opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
    let source = pdk::symbol_source_for(text);
    for path in source.paths() {
        opts = opts.with_sym_path(path.to_string_lossy().to_string());
    }
    (opts, source)
}

fn parse_text(text: &str) -> Schematic {
    xschem_viewer::semantic::parse_semantic(text, &render_options_for(text).0)
}

/// Valida un blob como contenido Xschem decodificable y devuelve el `&str`
/// listo para parsear. Usado por `diff` para chequear A y B simétricamente
/// antes de llamar al parser; cualquier error se propaga como warning del
/// `FileChange`.
fn validate_xschem<'a>(content: &'a [u8], side: &str, path_hint: &str) -> Result<&'a str, String> {
    let text = std::str::from_utf8(content).map_err(|_| {
        format!("{path_hint} ({side}): contenido no es UTF-8 valido, se omite el diff semantico.")
    })?;
    // Vacío = el archivo no existía en ese commit (nuevo o eliminado): un
    // esquemático sin nada, así todo aparece añadido o eliminado.
    if !content.is_empty() && !is_xschem(content) {
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
/// necesiten distinguir error vs vacío deben usar `XschemModule::diff`.
pub fn parse(content: &[u8]) -> Schematic {
    match std::str::from_utf8(content) {
        Ok(text) => parse_text(text),
        Err(_) => Schematic::default(),
    }
}

pub struct XschemModule {
    cached_info: std::sync::OnceLock<ModuleInfo>,
}

impl XschemModule {
    pub fn new() -> Self {
        Self {
            cached_info: std::sync::OnceLock::new(),
        }
    }
}

impl Default for XschemModule {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatModule for XschemModule {
    fn extensions(&self) -> &'static [&'static str] {
        &["sch"]
    }

    fn info(&self) -> ModuleInfo {
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
            pdk::PdkStatus::NotConfigured => match pdk::pdk_root() {
                Some(_) => "PDK: se detecta por los símbolos de cada esquemático".to_string(),
                None => "PDK: [no detectado, usa PDK_ROOT/PDK o .xschemrc]".to_string(),
            },
        };

        let info = ModuleInfo {
            name: "xschem".into(),
            version: format!("Native Renderer | {}", pdk_status),
            format: FileFormat::Xschem,
            extensions: vec![".sch".to_string()],
            available: true,
        };

        let _ = self.cached_info.set(info.clone());
        info
    }

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str, _opts: &DiffOptions) -> FileChange {
        let mut report = FileChange::new(FileFormat::Xschem);

        let text_a = match validate_xschem(content_a, "A", path_hint) {
            Ok(t) => t,
            Err(w) => return FileChange::failed(FileFormat::Xschem, w),
        };
        let text_b = match validate_xschem(content_b, "B", path_hint) {
            Ok(t) => t,
            Err(w) => return FileChange::failed(FileFormat::Xschem, w),
        };

        let sch_a = parse_text(text_a);
        let sch_b = parse_text(text_b);
        let result = xschem_viewer::semantic::diff(&sch_a, &sch_b);
        report.changes.extend(result.components.iter().map(component_change));
        for net in result.nets_added {
            report.changes.push(Change::new(ChangeKind::Added, Element::Net { name: net }));
        }
        for net in result.nets_removed {
            report.changes.push(Change::new(ChangeKind::Removed, Element::Net { name: net }));
        }
        if result.is_move_all {
            report.changes.push(
                Change::new(ChangeKind::Modified, Element::Whole)
                    .cosmetic(true)
                    .with_detail("note", None, Some(MOVE_ALL_NOTE.into())),
            );
        }
        report
    }

    fn detect(&self, content: &[u8]) -> bool {
        is_xschem(content)
    }

    /// Visor de símbolos y esquemáticos (ver `xschem_view.rs`); también
    /// para exportar imágenes sin ventana.
    fn viewer(&self) -> Option<std::sync::Arc<dyn viewer_core::ViewerBackend>> {
        Some(std::sync::Arc::new(super::xschem_view::XschemViewer))
    }
}

/// Firma de Xschem: la cabecera `v {xschem version=…}` en las primeras líneas.
pub fn is_xschem(content: &[u8]) -> bool {
    String::from_utf8_lossy(&content[..content.len().min(240)]).contains("xschem version=")
}

/// Traduce un cambio del motor de Xschem al vocabulario del núcleo. Es el
/// único lugar que conoce `ComponentDiff`.
fn component_change(c: &ComponentDiff) -> Change {
    // El motor marca un renombre como `Modified` con nombre "viejo → nuevo".
    let (kind, name, renamed_from) = match (&c.kind, c.name.split_once(" → ")) {
        (XsKind::Modified, Some((from, to))) => (ChangeKind::Renamed, to.to_string(), Some(from.to_string())),
        (XsKind::Added, _) => (ChangeKind::Added, c.name.clone(), None),
        (XsKind::Removed, _) => (ChangeKind::Removed, c.name.clone(), None),
        (XsKind::Modified, _) => (ChangeKind::Modified, c.name.clone(), None),
    };
    let empty = BTreeMap::new();
    let (before, after) = (c.before.as_ref().unwrap_or(&empty), c.after.as_ref().unwrap_or(&empty));
    let keys: BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    let mut change = Change::new(kind, Element::Component { name }).cosmetic(c.cosmetic);
    change.position_changed = c.position_changed;
    change.renamed_from = renamed_from;
    for k in keys {
        let text = |m: &BTreeMap<String, String>| m.get(k).map(|v| Value::Text(v.clone()));
        change = if PLACEMENT_KEYS.contains(&k.as_str()) {
            change.with_placement(k.clone(), text(before), text(after))
        } else {
            change.with_detail(k.clone(), text(before), text(after))
        };
    }
    change
}

/// Claves del motor de Xschem que son la ubicación de un componente (no
/// parámetros): se marcan como tales para que nadie más tenga que saberlas.
const PLACEMENT_KEYS: &[&str] = &["x", "y", "rotation", "mirror"];

#[cfg(test)]
mod tests {
    use super::*;

    const VALID_SCH: &[u8] = br#"v {xschem version=3.0.0 file_version=1.2}
"#;

    fn driver() -> XschemModule {
        XschemModule::new()
    }

    #[test]
    fn a_new_or_deleted_schematic_lists_everything() {
        let sch = b"v {xschem version=3.0.0 file_version=1.2}\n\
C {res.sym} 10 20 0 0 {name=R1 value=10k}\n\
N 0 0 10 0 {lab=OUT}\n";
        let added = driver().diff(b"", sch, "x.sch", &DiffOptions::default());
        assert!(added.warnings.is_empty(), "{:?}", added.warnings);
        assert!(added.changes.iter().any(|c| c.kind == ChangeKind::Added && c.element.name() == "R1"));
        assert!(added.changes.iter().any(|c| c.kind == ChangeKind::Added && matches!(c.element, Element::Net { .. })));
        assert!(added.functional().next().is_some());

        let removed = driver().diff(sch, b"", "x.sch", &DiffOptions::default());
        assert!(removed.changes.iter().any(|c| c.kind == ChangeKind::Removed && c.element.name() == "R1"));
    }

    #[test]
    fn diff_warns_on_invalid_utf8_in_a() {
        let invalid: &[u8] = &[0xFF, 0xFE, 0x00, 0x80];
        let report = driver().diff(invalid, VALID_SCH, "x.sch", &DiffOptions::default());
        assert!(report.changes.is_empty(), "no debe inventar cambios");
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let err = report.error.as_deref().unwrap_or_default();
        assert!(
            err.contains("(A)") && err.contains("UTF-8"),
            "el error debe identificar lado A y mencionar UTF-8: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_invalid_utf8_in_b() {
        let invalid: &[u8] = &[0xFF, 0xFE, 0x00, 0x80];
        let report = driver().diff(VALID_SCH, invalid, "x.sch", &DiffOptions::default());
        assert!(report.changes.is_empty());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let err = report.error.as_deref().unwrap_or_default();
        assert!(
            err.contains("(B)") && err.contains("UTF-8"),
            "el error debe identificar lado B y mencionar UTF-8: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_non_xschem_b() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let report = driver().diff(VALID_SCH, svg, "x.sch", &DiffOptions::default());
        assert!(
            report.changes.is_empty(),
            "no debe reportar 'todo removido' falso: {:?}",
            report.changes
        );
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let err = report.error.as_deref().unwrap_or_default();
        assert!(
            err.contains("(B)") && err.contains("Xschem"),
            "el error debe identificar lado B y mencionar formato: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_non_xschem_a() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let report = driver().diff(svg, VALID_SCH, "x.sch", &DiffOptions::default());
        assert!(report.changes.is_empty());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let err = report.error.as_deref().unwrap_or_default();
        assert!(err.contains("(A)"));
    }

    #[test]
    fn parse_returns_default_on_invalid_utf8() {
        let invalid: &[u8] = &[0xFF, 0xFE, 0x00];
        let s = parse(invalid);
        assert!(s.components.is_empty());
        assert!(s.wires.is_empty());
    }

    /// Dónde se va el tiempo al parsear un esquemático real (medición).
    #[test]
    #[ignore = "medición"]
    fn parse_cost_breakdown() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../examples/SH/op_sim.sch");
        let text = std::fs::read_to_string(path).unwrap();
        let n = 20;
        let t = std::time::Instant::now();
        let opts: Vec<_> = (0..n).map(|_| render_options_for(&text).0).collect();
        let t_opts = t.elapsed() / n;
        let t = std::time::Instant::now();
        for o in &opts {
            std::hint::black_box(xschem_viewer::semantic::parse_semantic(&text, o));
        }
        let t_parse = t.elapsed() / n;
        eprintln!("[P8] opciones de render: {t_opts:?} · parse_semantic: {t_parse:?} (por esquemático)");
    }
}
