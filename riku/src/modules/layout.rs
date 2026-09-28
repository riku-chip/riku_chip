//! Módulo de layouts (GDSII, OASIS y Magic): traduce el diff de `riku-mod-layout` al
//! vocabulario del núcleo y ofrece su backend del visor. Toda la lógica de
//! geometría vive en `riku-mod-layout`; aquí no se usa `gdstk_rs`.

use std::sync::Arc;

use riku_mod_layout::{
    diff_layout_sides, DiffCache, DiffConfig, GdsError, GdsGeomDiff, LayoutSide,
    DEFAULT_COSMETIC_THRESHOLD_UM2,
};
use riku_kernel::{DiffFiles, DiffOptions, FormatModule, ModuleInfo};
use viewer_core::ViewerBackend;

use crate::core::domain::models::{Bounds, Change, ChangeKind, Element, FileChange, FileFormat, Via};

fn cell_change(name: &str, kind: ChangeKind) -> Change {
    Change::new(kind, Element::Cell { name: name.to_string() })
}

/// Traduce un cambio de geometría de riku-mod-layout al vocabulario del núcleo:
/// áreas y conteos como números, sub-celda e instancia tipadas.
fn geom_change(g: &GdsGeomDiff) -> Change {
    let kind = match (g.added_polygons, g.removed_polygons) {
        (a, 0) if a > 0 => ChangeKind::Added,
        (0, r) if r > 0 => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    // origin_path = [cell] (geometría propia) o [cell, sub] (vía una instancia).
    let via = (g.origin_path.len() > 1).then(|| Via {
        path: g.origin_path[1..].to_vec(),
        instances: g.instances,
        at: g.instance_at_um.map(|(x, y)| [x, y]),
    });
    let element = Element::Geometry {
        cell: g.cell.clone(),
        layer: g.layer.layer,
        datatype: g.layer.datatype,
        layer_name: g.layer_name.clone(),
        via,
    };
    let mut c = Change::new(kind, element)
        .cosmetic(g.cosmetic)
        .with_detail("added_polygons", None, Some(g.added_polygons.into()))
        .with_detail("removed_polygons", None, Some(g.removed_polygons.into()))
        .with_detail("added_area_um2", None, Some(g.added_area_um2.into()))
        .with_detail("removed_area_um2", None, Some(g.removed_area_um2.into()));
    c.location = g.bbox_um.map(|b| Bounds { min_x: b.min_x, min_y: b.min_y, max_x: b.max_x, max_y: b.max_y });
    c
}

/// Un puerto de Magic: sus atributos antes y después (`class`: input →
/// inout). Si solo se movió, es cosmético.
fn port_change(p: &riku_mod_layout::mag::PortChange) -> Change {
    use riku_kernel::Value;
    let kind = match (&p.before, &p.after) {
        (None, _) => ChangeKind::Added,
        (_, None) => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    let mut c = Change::new(kind, Element::Port { cell: p.cell.clone(), name: p.name.clone() }).cosmetic(p.cosmetic);
    type Field = fn(&riku_mod_layout::mag::PortDesc) -> Option<Value>;
    let fields: [(&str, Field); 6] = [
        ("class", |d| d.class.clone().map(Value::Text)),
        ("use", |d| d.usage.clone().map(Value::Text)),
        ("index", |d| Some(Value::Int(d.index))),
        ("sides", |d| Some(Value::Text(d.sides.clone()))),
        ("layer", |d| Some(Value::Text(d.layers.join(",")))),
        ("position_um", |d| d.rects_um.first().map(|r| Value::Text(format!("{:.3},{:.3}", r[0], r[1])))),
    ];
    for (key, get) in fields {
        let (before, after) = (p.before.as_ref().and_then(get), p.after.as_ref().and_then(get));
        // En uno modificado, solo lo que cambió; en uno nuevo o quitado, todo.
        if kind != ChangeKind::Modified || before != after {
            c = c.with_detail(key, before, after);
        }
    }
    if let Some(r) = p.after.as_ref().or(p.before.as_ref()).and_then(|d| d.rects_um.first()) {
        c.location = Some(Bounds { min_x: r[0], min_y: r[1], max_x: r[2], max_y: r[3] });
    }
    c
}

/// Un transistor que cambió: su modelo, W y L antes y después (solo lo que
/// cambió, si es el mismo transistor).
fn device_change(d: &riku_mod_layout::devices::DeviceChange) -> Change {
    use riku_kernel::Value;
    let kind = match (&d.before, &d.after) {
        (None, _) => ChangeKind::Added,
        (_, None) => ChangeKind::Removed,
        _ => ChangeKind::Modified,
    };
    let shown = d.after.as_ref().or(d.before.as_ref()).expect("un lado al menos");
    let mut c = Change::new(kind, Element::Device { cell: d.cell.clone(), model: shown.model.clone(), at: shown.at_um });
    type Field = fn(&riku_mod_layout::devices::DeviceDesc) -> Value;
    let fields: [(&str, Field); 3] =
        [("model", |x| Value::Text(x.model.clone())), ("w_um", |x| x.w_um.into()), ("l_um", |x| x.l_um.into())];
    for (key, get) in fields {
        let (before, after) = (d.before.as_ref().map(get), d.after.as_ref().map(get));
        let same = match (&before, &after) {
            (Some(Value::Text(a)), Some(Value::Text(b))) => a == b,
            (Some(a), Some(b)) => a.as_f64().zip(b.as_f64()).is_some_and(|(a, b)| (a - b).abs() <= 5e-4),
            _ => false,
        };
        if kind != ChangeKind::Modified || !same {
            c = c.with_detail(key, before, after);
        }
    }
    let b = shown.bbox_um;
    c.location = Some(Bounds { min_x: b[0], min_y: b[1], max_x: b[2], max_y: b[3] });
    c
}

fn translate_error(e: GdsError, path_hint: &str) -> String {
    match e {
        GdsError::NotGdsii { side } => format!(
            "{path_hint} ({side}): no es un layout GDSII, OASIS ni Magic, se omite el diff."
        ),
        GdsError::Parse { side, msg } => format!(
            "{path_hint} ({side}): no se pudo leer el layout: {msg}"
        ),
    }
}

/// Módulo de layouts: GDSII, OASIS y Magic (motor: gdstk vía riku-mod-layout).
/// Un `.mag` lee sus sub-celdas de la misma versión (`diff_with`) y del PDK.
pub struct LayoutModule {
    /// Cache en disco del reporte (solo layouts grandes; ver `DiffCache`).
    cache: DiffCache,
}

impl LayoutModule {
    pub fn new() -> Self {
        Self { cache: DiffCache::from_env() }
    }
}

impl Default for LayoutModule {
    fn default() -> Self {
        Self::new()
    }
}

impl FormatModule for LayoutModule {
    fn extensions(&self) -> &'static [&'static str] {
        &["gds", "oas", "mag"]
    }

    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            name: "layout".into(),
            version: "riku-mod-layout (gdstk cxx; Magic en Rust)".into(),
            format: FileFormat::Gds,
            extensions: vec![".gds".to_string(), ".oas".to_string(), ".mag".to_string()],
            available: true,
        }
    }

    fn detect(&self, content: &[u8]) -> bool {
        riku_mod_layout::is_layout(content)
    }

    fn diff(&self, content_a: &[u8], content_b: &[u8], path_hint: &str, opts: &DiffOptions) -> FileChange {
        self.diff_with(content_a, content_b, path_hint, opts, &DiffFiles::default())
    }

    fn diff_with(
        &self,
        content_a: &[u8],
        content_b: &[u8],
        path_hint: &str,
        opts: &DiffOptions,
        files: &DiffFiles,
    ) -> FileChange {
        let mut report = FileChange::new(FileFormat::Gds);

        let cfg = DiffConfig {
            cosmetic_threshold_um2: opts.cosmetic_threshold.unwrap_or(DEFAULT_COSMETIC_THRESHOLD_UM2),
        };
        let off = DiffCache::disabled();
        let cache = if opts.use_cache { &self.cache } else { &off };
        let a = LayoutSide { bytes: content_a, files: files.before.as_deref() };
        let b = LayoutSide { bytes: content_b, files: files.after.as_deref() };
        let r = match diff_layout_sides(a, b, path_hint, &cfg, cache) {
            Ok(r) => r,
            Err(e) => return FileChange::failed(FileFormat::Gds, translate_error(e, path_hint)),
        };

        for n in r.cells_removed {
            report.changes.push(cell_change(&n, ChangeKind::Removed));
        }
        for n in r.cells_added {
            report.changes.push(cell_change(&n, ChangeKind::Added));
        }
        for (from, to) in r.cells_renamed {
            let mut c = cell_change(&to, ChangeKind::Renamed);
            c.renamed_from = Some(from);
            report.changes.push(c);
        }
        for g in &r.geometry {
            report.changes.push(geom_change(g));
        }
        for p in &r.ports {
            report.changes.push(port_change(p));
        }
        for d in &r.devices {
            report.changes.push(device_change(d));
        }
        report.warnings.extend(r.warnings);
        report
    }

    fn viewer(&self) -> Option<Arc<dyn ViewerBackend>> {
        Some(Arc::new(riku_mod_layout::GdsBackend::new()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof_lib_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("external")
            .join("gdstk")
            .join("tests")
            .join("proof_lib.gds");
        std::fs::read(&path)
            .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", path.display()))
    }

    #[test]
    fn detect_returns_gds_for_magic() {
        let bytes = [0x00u8, 0x06, 0x00, 0x02, 0x01, 0x00];
        assert!(LayoutModule::new().detect(&bytes));
    }

    #[test]
    fn diff_warns_on_non_gds_a() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let gds = proof_lib_bytes();
        let report = LayoutModule::new().diff(svg, &gds, "x.gds", &DiffOptions::default());
        assert!(report.changes.is_empty());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let err = report.error.as_deref().unwrap_or_default();
        assert!(
            err.contains("(A)") && err.contains("GDSII"),
            "el error debe identificar lado A y mencionar GDSII: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_warns_on_non_gds_b() {
        let svg = br#"<svg xmlns='http://www.w3.org/2000/svg'></svg>"#;
        let gds = proof_lib_bytes();
        let report = LayoutModule::new().diff(&gds, svg, "x.gds", &DiffOptions::default());
        assert!(report.changes.is_empty());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        let err = report.error.as_deref().unwrap_or_default();
        assert!(
            err.contains("(B)") && err.contains("GDSII"),
            "el error debe identificar lado B y mencionar GDSII: {:?}",
            report.warnings
        );
    }

    #[test]
    fn diff_identical_returns_empty() {
        let gds = proof_lib_bytes();
        let report = LayoutModule::new().diff(&gds, &gds, "x.gds", &DiffOptions::default());
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
        assert!(
            report.is_empty(),
            "self-vs-self debe ser empty: {:?}",
            report.changes
        );
    }

    fn fixture_bytes(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("riku-mod-layout")
            .join("tests")
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn high_threshold_marks_changes_cosmetic() {
        // Con threshold 200 µm² (mas grande que el cambio del fixture: 100 µm²),
        // el diff debe marcar todos los entries geometricos como cosmetic.
        let a = fixture_bytes("datatype_a.gds");
        let b = fixture_bytes("datatype_b.gds");
        let opts = DiffOptions { cosmetic_threshold: Some(200.0), ..Default::default() };
        let report = LayoutModule::new().diff(&a, &b, "datatype.gds", &opts);
        let geom: Vec<&Change> = report
            .changes
            .iter()
            .filter(|c| matches!(c.element, Element::Geometry { .. }))
            .collect();
        assert!(!geom.is_empty(), "deberia haber entries geom");
        assert!(
            geom.iter().all(|c| c.cosmetic),
            "todos los entries geom deben ser cosmetic con threshold alto"
        );
    }

    #[test]
    fn default_threshold_keeps_real_changes_non_cosmetic() {
        // Con default 0.01 µm², un cambio de 100 µm² NO debe ser cosmetico.
        let a = fixture_bytes("datatype_a.gds");
        let b = fixture_bytes("datatype_b.gds");
        let report = LayoutModule::new().diff(&a, &b, "datatype.gds", &DiffOptions::default());
        let geom: Vec<&Change> = report
            .changes
            .iter()
            .filter(|c| matches!(c.element, Element::Geometry { .. }))
            .collect();
        assert!(!geom.is_empty());
        assert!(
            geom.iter().any(|c| !c.cosmetic),
            "al menos un entry geom no deberia ser cosmetic con threshold bajo"
        );
    }

    #[test]
    fn can_handle_gds_extension() {
        let d = LayoutModule::new();
        assert!(d.handles_path("foo.gds"));
        assert!(d.handles_path("path/to/Bar.GDS"));
        assert!(!d.handles_path("foo.sch"));
    }

    fn renderer_fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../riku-mod-layout/tests/fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn renamed_cell_is_one_rename_entry() {
        let r = LayoutModule::new().diff(
            &renderer_fixture("rename_a.gds"),
            &renderer_fixture("rename_b.gds"),
            "rename.gds",
            &DiffOptions::default(),
        );
        let rename: Vec<&Change> = r.changes.iter().filter(|c| c.kind == ChangeKind::Renamed).collect();
        assert_eq!(rename.len(), 1);
        assert_eq!(rename[0].element, Element::Cell { name: "INV_X1".into() });
        assert_eq!(rename[0].renamed_from.as_deref(), Some("INV"));
        let is_cell = |c: &&Change, n: &str| c.element == Element::Cell { name: n.into() } && c.kind != ChangeKind::Renamed;
        assert!(!r.changes.iter().any(|c| is_cell(&c, "INV") || is_cell(&c, "INV_X1")));
    }

    #[test]
    fn instances_of_the_same_subcell_are_grouped() {
        let r = LayoutModule::new().diff(
            &renderer_fixture("multi_inst_a.gds"),
            &renderer_fixture("multi_inst_b.gds"),
            "multi.gds",
            &DiffOptions::default(),
        );
        let top = r.changes.iter().find(|c| c.element.name() == "TOP:L1/0:INV").expect("TOP");
        let Element::Geometry { via: Some(via), .. } = &top.element else { panic!("sin via: {top:?}") };
        assert_eq!((via.instances, via.at), (2, None));
        assert_eq!(top.after("added_area_um2").and_then(|v| v.as_f64()), Some(2.0));
    }
}
