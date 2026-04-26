use crate::core::domain::models::{ChangeKind, ComponentDiff, DiffReport, Schematic};
use crate::core::rendering::styles::annotation_style;

// Half-size of the bounding box drawn around each changed component, in schematic units.
const BBOX_HALF: f64 = 20.0;
// Stroke width for wire highlights, in schematic units.
const WIRE_STROKE: f64 = 3.5;

fn component_rect(
    cd: &ComponentDiff,
    sch_b: &Schematic,
    sch_a: Option<&Schematic>,
) -> Option<String> {
    let source = if cd.kind == ChangeKind::Removed {
        sch_a.unwrap_or(sch_b)
    } else {
        sch_b
    };
    let comp = source.components.get(&cd.name)?;
    let style = annotation_style(&cd.kind, cd.cosmetic);
    let (fill, stroke) = (style.fill, style.stroke);
    let x = comp.x - BBOX_HALF;
    let y = comp.y - BBOX_HALF;
    let size = BBOX_HALF * 2.0;
    Some(format!(
        r#"<rect x="{x:.2}" y="{y:.2}" width="{size:.2}" height="{size:.2}" fill="{fill}" stroke="{stroke}" stroke-width="1.5" rx="3" ry="3"/>"#
    ))
}

fn wire_lines(net_names: &[String], sch: &Schematic, color: &str) -> Vec<String> {
    let set: std::collections::HashSet<&str> = net_names.iter().map(|s| s.as_str()).collect();
    sch.wires
        .iter()
        .filter(|w| set.contains(w.label.as_str()))
        .map(|w| {
            format!(
                r#"<line x1="{:.2}" y1="{:.2}" x2="{:.2}" y2="{:.2}" stroke="{color}" stroke-width="{WIRE_STROKE}" stroke-linecap="round"/>"#,
                w.x1, w.y1, w.x2, w.y2
            )
        })
        .collect()
}

/// Injects a `<g id="riku-diff-annotations">` layer into an SVG produced by
/// xschem_viewer. Because xschem_viewer uses schematic coordinates directly
/// (viewBox = bbox of all primitives, no extra transform), annotation
/// coordinates map 1-to-1 to SVG coordinates.
pub fn annotate(
    svg_content: &str,
    sch_b: &Schematic,
    diff_report: &DiffReport,
    sch_a: Option<&Schematic>,
) -> String {
    let mut elements: Vec<String> = Vec::new();

    for cd in &diff_report.components {
        if let Some(rect) = component_rect(cd, sch_b, sch_a) {
            elements.push(rect);
        }
    }

    if !diff_report.nets_added.is_empty() {
        elements.extend(wire_lines(
            &diff_report.nets_added,
            sch_b,
            "rgba(0,200,0,0.9)",
        ));
    }
    if let Some(sch_a) = sch_a {
        if !diff_report.nets_removed.is_empty() {
            elements.extend(wire_lines(
                &diff_report.nets_removed,
                sch_a,
                "rgba(200,0,0,0.9)",
            ));
        }
    }

    if elements.is_empty() {
        return svg_content.to_string();
    }

    let layer = format!(
        "\n<g id=\"riku-diff-annotations\">\n{}\n</g>\n",
        elements.join("\n")
    );
    svg_content.replacen("</svg>", &(layer + "</svg>"), 1)
}

#[cfg(test)]
mod tests {
    use super::annotate;
    use crate::core::domain::models::{
        ChangeKind, Component, ComponentDiff, DiffReport, Schematic, Wire,
    };
    use std::collections::{BTreeMap, BTreeSet};

    #[test]
    fn annotate_injects_layer() {
        let mut components = BTreeMap::new();
        components.insert(
            "R1".to_string(),
            Component {
                name: "R1".to_string(),
                symbol: "res.sym".to_string(),
                params: BTreeMap::new(),
                pins: BTreeMap::new(),
                x: 10.0,
                y: 20.0,
                rotation: 0,
                mirror: 0,
            },
        );
        let schematic = Schematic {
            components,
            wires: vec![Wire {
                x1: 0.0,
                y1: 0.0,
                x2: 10.0,
                y2: 0.0,
                label: "NET1".to_string(),
            }],
            nets: BTreeSet::from(["NET1".to_string()]),
        };
        let diff = DiffReport {
            components: vec![ComponentDiff {
                name: "R1".to_string(),
                kind: ChangeKind::Added,
                cosmetic: false,
                position_changed: false,
                before: None,
                after: None,
            }],
            nets_added: vec!["NET1".to_string()],
            nets_removed: vec![],
            is_move_all: false,
        };
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100"></svg>"#;

        let out = annotate(svg, &schematic, &diff, None);
        assert!(out.contains("riku-diff-annotations"));
        assert!(out.contains("<rect"));
        assert!(out.contains("<line"));
    }
}
