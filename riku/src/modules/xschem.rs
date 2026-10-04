use std::collections::{BTreeMap, BTreeSet};

use xschem_viewer::semantic::{ChangeKind as XsKind, ComponentDiff, SemanticSchematic as Schematic};

use super::xschem_hier as hier;
use super::xschem_pdk as pdk;
use crate::core::domain::models::{Change, ChangeKind, Element, FileChange, FileFormat, Value};
use crate::i18n::tr;
use riku_kernel::{DiffFiles, DiffOptions, FormatModule, ModuleInfo};

/// Opciones de render canónicas para un esquemático: tema dark + símbolos de
/// `.xschemrc` + símbolos del PDK (`$PDK_ROOT/$PDK`, o el PDK instalado que
/// tiene los símbolos del archivo si `$PDK` no está definida). Fuente única
/// para el diff y el visor: los dos encuentran los mismos símbolos.
pub(crate) fn render_options_for(text: &str) -> (xschem_viewer::RenderOptions, pdk::PdkSource) {
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
    let text = std::str::from_utf8(content).map_err(|_| tr!("xschem.side_not_utf8", file = path_hint, side = side))?;
    // Vacío = el archivo no existía en ese commit (nuevo o eliminado): un
    // esquemático sin nada, así todo aparece añadido o eliminado.
    if !content.is_empty() && !is_xschem(content) {
        return Err(tr!("xschem.side_not_xschem", file = path_hint, side = side));
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
        Self { cached_info: std::sync::OnceLock::new() }
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
                tr!("xschem.info_pdk_ok", name = name)
            }
            pdk::PdkStatus::Misconfigured(_) => {
                let name = std::env::var("PDK").unwrap_or_default();
                tr!("xschem.info_pdk_missing", name = name)
            }
            pdk::PdkStatus::NotConfigured => match pdk::pdk_root() {
                Some(_) => tr!("xschem.info_pdk_detect"),
                None => tr!("xschem.info_pdk_none"),
            },
        };

        let info = ModuleInfo {
            name: "xschem".into(),
            version: tr!("xschem.info_version", pdk = pdk_status),
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
            report.changes.push(Change::new(ChangeKind::Modified, Element::Whole).cosmetic(true).with_detail(
                "note",
                None,
                Some(tr!("change.move_all_detail").into()),
            ));
        }
        report
    }

    /// Como [`Self::diff`], y además las instancias cuyo sub-esquemático
    /// del proyecto cambió en la misma versión (por sí mismo o por dentro):
    /// salen modificadas con `inside` = qué cambió (`amp.sch → mirror.sch`).
    fn diff_with(&self, before: &[u8], after: &[u8], path_hint: &str, opts: &DiffOptions, files: &DiffFiles) -> FileChange {
        let mut report = self.diff(before, after, path_hint, opts);
        if report.error.is_none() {
            note_changes_inside(&mut report, before, after, path_hint, files);
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

/// Las instancias de `path` cuyo sub-esquemático cambió entre las dos
/// versiones (ver `xschem_hier`), marcadas en el reporte. Una instancia
/// nueva, borrada o que ahora usa otro sub-esquemático ya está en el diff.
fn note_changes_inside(report: &mut FileChange, before: &[u8], after: &[u8], path: &str, files: &DiffFiles) {
    let (Some(fa), Some(fb)) = (files.before.as_deref(), files.after.as_deref()) else { return };
    if before.is_empty() || after.is_empty() {
        return;
    }
    let (ha, hb) = (hier::collect(before, path, Some(fa)), hier::collect(after, path, Some(fb)));
    let changed = hier::changes(&ha, &hb);
    let subs = |h: &hier::Hierarchy| -> BTreeMap<String, String> {
        h.nodes
            .get(path)
            .map_or_else(BTreeMap::new, |n| n.children.iter().map(|c| (c.instance.clone(), c.schematic.clone())).collect())
    };
    let (sa, sb) = (subs(&ha), subs(&hb));
    for (instance, schematic) in &sb {
        if sa.get(instance) != Some(schematic) || !changed.contains_key(schematic) {
            continue;
        }
        let inside = Some(Value::Text(hier::inside_path(schematic, &ha, &hb, &changed)));
        let functional = functional_inside(schematic, &ha, &hb, &changed);
        let existing = report.changes.iter_mut().find(|c| matches!(&c.element, Element::Component { name } if name == instance));
        match existing {
            Some(c) => {
                c.details.push(riku_kernel::Detail::new(INSIDE_KEY, None, inside));
                c.cosmetic &= !functional;
            }
            None => report.changes.push(
                Change::new(ChangeKind::Modified, Element::Component { name: instance.clone() })
                    .cosmetic(!functional)
                    .with_detail(INSIDE_KEY, None, inside),
            ),
        }
    }
}

/// Si algo de lo que cambió dentro de `schematic` (él o sus
/// sub-esquemáticos) es funcional. Solo cosmético (un Move All, textos
/// movidos) no cambia el circuito: la instancia cambia solo en lo cosmético.
fn functional_inside(schematic: &str, a: &hier::Hierarchy, b: &hier::Hierarchy, changed: &BTreeMap<String, ChangeKind>) -> bool {
    let mut stack = vec![schematic.to_string()];
    let mut seen = BTreeSet::new();
    while let Some(p) = stack.pop() {
        if !changed.contains_key(&p) || !seen.insert(p.clone()) {
            continue;
        }
        match (a.nodes.get(&p), b.nodes.get(&p)) {
            (Some(x), Some(y)) => {
                // Otros pines en el símbolo cambian cómo se conecta.
                if x.symbol_bytes != y.symbol_bytes {
                    return true;
                }
                let own = XschemModule::new().diff(&x.bytes, &y.bytes, &p, &DiffOptions::default());
                if x.bytes != y.bytes && (own.error.is_some() || own.functional().next().is_some()) {
                    return true;
                }
            }
            _ => return true,
        }
        for n in [a.nodes.get(&p), b.nodes.get(&p)].into_iter().flatten() {
            stack.extend(n.children.iter().map(|c| c.schematic.clone()));
        }
    }
    false
}

/// Detalle de un componente cuyo sub-esquemático cambió: qué cambió.
pub const INSIDE_KEY: &str = "inside";

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
        assert!(report.changes.is_empty(), "no debe reportar 'todo removido' falso: {:?}", report.changes);
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
    fn una_instancia_cuyo_sub_esquematico_cambio_sale_modificada() {
        use std::collections::HashMap;
        use std::sync::Arc;
        struct Mem(HashMap<&'static str, Vec<u8>>);
        impl riku_kernel::FileSource for Mem {
            fn read(&self, path: &str) -> Option<Vec<u8>> {
                self.0.get(path).cloned()
            }
        }
        let top = b"v {xschem version=3.4.5 file_version=1.2}
C {amp.sym} 0 0 0 0 {name=x1}
"
        .to_vec();
        let amp = |r: &str| {
            format!(
                "v {{xschem version=3.4.5 file_version=1.2}}
C {{res.sym}} 0 0 0 0 {{name=R1 value={r}}}
"
            )
            .into_bytes()
        };
        let version = |r: &str| -> Arc<dyn riku_kernel::FileSource> {
            Arc::new(Mem(HashMap::from([
                ("top.sch", top.clone()),
                ("amp.sch", amp(r)),
                (
                    "amp.sym",
                    b"v {xschem version=3.4.5}
"
                    .to_vec(),
                ),
            ])))
        };
        let files = DiffFiles::new(Some(version("1k")), Some(version("2k")));
        let report = driver().diff_with(&top, &top, "top.sch", &DiffOptions::default(), &files);
        let x1 = report.changes.iter().find(|c| c.element.name() == "x1").expect("x1 cambió por dentro");
        assert_eq!((x1.kind, x1.cosmetic), (ChangeKind::Modified, false));
        assert_eq!(x1.after(INSIDE_KEY).map(|v| v.to_string()).as_deref(), Some("amp.sch"));

        let same = DiffFiles::new(Some(version("1k")), Some(version("1k")));
        assert!(driver().diff_with(&top, &top, "top.sch", &DiffOptions::default(), &same).changes.is_empty());
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
