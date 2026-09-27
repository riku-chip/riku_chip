//! Forma anterior (v1) de los cambios, para una versión de transición.
//!
//! Hasta la v2, los drivers describían cada cambio con un `element` de texto
//! con convenciones (`"net:vdd"`, `"layout"`, `"cell:INV"`,
//! `"TOP:L1/0:INV"`, `"a → b"`) y mapas `before`/`after` de strings. Este
//! módulo reconstruye exactamente esa forma a partir de un [`FileChange`]
//! para `riku diff -f json-v1` y el `full_report` de `status`/`log`.
//! Se elimina cuando nadie consuma el v1.

use std::collections::BTreeMap;

use serde::Serialize;
use serde_json::{json, Value as Json};

use crate::{Change, ChangeKind, Element, FileChange, Value};

/// Un cambio con la forma v1.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LegacyEntry {
    pub kind: &'static str,
    pub element: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub before: Option<BTreeMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub after: Option<BTreeMap<String, String>>,
    pub cosmetic: bool,
    pub position_changed: bool,
}

/// Cambios con la forma v1, en el mismo orden.
pub fn entries(fc: &FileChange) -> Vec<LegacyEntry> {
    fc.changes.iter().map(entry).collect()
}

/// Reporte de driver v1 (`full_report` de `status`/`log` con `--full --json`),
/// con las claves en el mismo orden que antes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LegacyReport {
    pub file_type: crate::FileFormat,
    pub changes: Vec<LegacyEntry>,
    pub warnings: Vec<String>,
}

/// El reporte de driver v1 de un `FileChange`.
pub fn driver_report(fc: &FileChange) -> LegacyReport {
    LegacyReport { file_type: fc.format.clone(), changes: entries(fc), warnings: fc.warnings.clone() }
}

/// Cuerpo de `riku diff -f json` v1: componentes, nets y `is_move_all`.
pub fn diff_report_json(fc: &FileChange) -> Json {
    let mut components = Vec::new();
    let mut nets_added = Vec::new();
    let mut nets_removed = Vec::new();
    let mut is_move_all = false;
    for c in &fc.changes {
        match (&c.element, c.kind) {
            (Element::Whole, _) => is_move_all |= c.cosmetic,
            (Element::Net { name }, ChangeKind::Added) => nets_added.push(name.clone()),
            (Element::Net { name }, ChangeKind::Removed) => nets_removed.push(name.clone()),
            (Element::Net { .. }, _) => {}
            _ => {
                let e = entry(c);
                // ComponentDiff (v1) serializaba `before`/`after` siempre, como null si faltaban.
                components.push(json!({
                    "name": e.element,
                    "kind": e.kind,
                    "cosmetic": e.cosmetic,
                    "position_changed": e.position_changed,
                    "before": e.before,
                    "after": e.after,
                }));
            }
        }
    }
    json!({
        "components": components,
        "nets_added": nets_added,
        "nets_removed": nets_removed,
        "is_move_all": is_move_all,
    })
}

fn entry(c: &Change) -> LegacyEntry {
    let kind = match c.kind {
        ChangeKind::Added => "added",
        ChangeKind::Removed => "removed",
        ChangeKind::Modified | ChangeKind::Renamed => "modified",
    };
    let renamed = |name: &str| match (&c.renamed_from, c.kind) {
        (Some(from), ChangeKind::Renamed) => format!("{from} → {name}"),
        _ => name.to_string(),
    };
    let values = |pick: fn(&crate::Detail) -> Option<&Value>| -> BTreeMap<String, String> {
        c.details.iter().filter_map(|d| pick(d).map(|v| (d.key.clone(), text(v)))).collect()
    };
    let (element, before, after) = match &c.element {
        Element::Component { name } => (
            renamed(name),
            (c.kind != ChangeKind::Added).then(|| values(|d| d.before.as_ref())),
            (c.kind != ChangeKind::Removed).then(|| values(|d| d.after.as_ref())),
        ),
        Element::Net { name } => (format!("net:{name}"), None, None),
        // v1 no tenía puertos de layout: el nombre y los valores, como una net.
        Element::Port { .. } => (
            c.element.name(),
            (c.kind != ChangeKind::Added).then(|| values(|d| d.before.as_ref())),
            (c.kind != ChangeKind::Removed).then(|| values(|d| d.after.as_ref())),
        ),
        Element::Signal { name, .. } => (
            format!("signal:{name}"),
            (c.kind != ChangeKind::Added).then(|| values(|d| d.before.as_ref())),
            (c.kind != ChangeKind::Removed).then(|| values(|d| d.after.as_ref())),
        ),
        Element::Whole => ("layout".to_string(), None, Some(values(|d| d.after.as_ref()))),
        Element::Cell { name } => (format!("cell:{}", renamed(name)), None, None),
        Element::Geometry { cell, via, .. } => {
            let mut after = values(|d| d.after.as_ref());
            if let Some(b) = c.location {
                after.insert(
                    "bbox_um".into(),
                    format!("{:.3},{:.3},{:.3},{:.3}", b.min_x, b.min_y, b.max_x, b.max_y),
                );
            }
            let mut origin = vec![cell.clone()];
            if let Some(v) = via {
                origin.extend(v.path.iter().cloned());
                if v.instances > 0 {
                    after.insert("instances".into(), v.instances.to_string());
                }
                if let Some([x, y]) = v.at {
                    after.insert("instance_at_um".into(), format!("{x:.3},{y:.3}"));
                }
            }
            after.insert("origin_path".into(), origin.join("/"));
            after.insert("flattened".into(), via.is_some().to_string());
            (c.element.name(), None, Some(after))
        }
    };
    LegacyEntry { kind, element, before, after, cosmetic: c.cosmetic, position_changed: c.position_changed }
}

/// Texto v1 de un valor: los reales con 3 decimales (áreas en µm²).
fn text(v: &Value) -> String {
    match v {
        Value::Float(x) => format!("{x:.3}"),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Bounds, FileFormat, Via};

    #[test]
    fn geometry_keeps_the_v1_strings() {
        let mut fc = FileChange::new(FileFormat::Gds);
        let mut c = Change::new(
            ChangeKind::Added,
            Element::Geometry {
                cell: "TOP".into(),
                layer: 1,
                datatype: 0,
                layer_name: None,
                via: Some(Via { path: vec!["INV".into()], instances: 1, at: Some([10.0, 10.0]) }),
            },
        )
        .with_detail("added_polygons", None, Some(1usize.into()))
        .with_detail("added_area_um2", None, Some(1.0.into()))
        .with_detail("removed_area_um2", None, Some(0.0.into()));
        c.location = Some(Bounds { min_x: 12.0, min_y: 10.0, max_x: 13.0, max_y: 11.0 });
        fc.changes.push(c);
        let e = &entries(&fc)[0];
        assert_eq!(e.element, "TOP:L1/0:INV");
        let a = e.after.as_ref().unwrap();
        assert_eq!(a["added_area_um2"], "1.000");
        assert_eq!(a["removed_area_um2"], "0.000");
        assert_eq!(a["bbox_um"], "12.000,10.000,13.000,11.000");
        assert_eq!(a["origin_path"], "TOP/INV");
        assert_eq!(a["flattened"], "true");
        assert_eq!(a["instance_at_um"], "10.000,10.000");
        assert!(e.before.is_none());
    }

    #[test]
    fn nets_whole_and_renames_map_to_the_old_protocol() {
        let mut fc = FileChange::new(FileFormat::Xschem);
        fc.changes.push(Change::new(ChangeKind::Added, Element::Net { name: "vdd".into() }));
        fc.changes.push(
            Change::new(ChangeKind::Modified, Element::Whole)
                .cosmetic(true)
                .with_detail("note", None, Some("move all".into())),
        );
        let mut r = Change::new(ChangeKind::Renamed, Element::Component { name: "vin_diff".into() })
            .with_detail("value", Some("1".into()), Some("1".into()));
        r.renamed_from = Some("vin".into());
        fc.changes.push(r);
        let mut cell = Change::new(ChangeKind::Renamed, Element::Cell { name: "INV_X1".into() });
        cell.renamed_from = Some("INV".into());
        fc.changes.push(cell);

        let names: Vec<_> = entries(&fc).into_iter().map(|e| (e.kind, e.element)).collect();
        assert_eq!(
            names,
            vec![
                ("added", "net:vdd".to_string()),
                ("modified", "layout".to_string()),
                ("modified", "vin → vin_diff".to_string()),
                ("modified", "cell:INV → INV_X1".to_string()),
            ]
        );
        let d = diff_report_json(&fc);
        assert_eq!(d["nets_added"], json!(["vdd"]));
        assert_eq!(d["is_move_all"], true);
        assert_eq!(d["components"][0]["name"], "vin → vin_diff");
        assert_eq!(d["components"][0]["before"]["value"], "1");
        assert_eq!(d["components"][1]["before"], Json::Null);
    }
}
