use std::collections::HashMap;

use crate::core::models::{Component, FileFormat, Schematic, Wire};
use crate::core::ports::SchematicParser;

pub fn detect_format(content: &[u8]) -> FileFormat {
    let header = String::from_utf8_lossy(&content[..content.len().min(240)]);
    if header.contains("xschem version=") {
        FileFormat::Xschem
    } else if header.contains("<Qucs Schematic") {
        FileFormat::Qucs
    } else if header.contains("EESchema Schematic File Version") {
        FileFormat::KicadLegacy
    } else {
        FileFormat::Unknown
    }
}

pub fn parse(content: &[u8]) -> Schematic {
    let text = match std::str::from_utf8(content) {
        Ok(s) => s,
        Err(_) => return Schematic::default(),
    };

    let xv_sch = match xschem_viewer::parser::parse(text) {
        Ok(s) => s,
        Err(_) => return Schematic::default(),
    };

    let netlist = xschem_viewer::extract_netlist(&xv_sch);

    // Index components by name for O(1) lookup of position/rotation
    let comp_by_name: HashMap<&str, &xschem_viewer::models::Component> = xv_sch
        .components()
        .filter_map(|c| c.properties.get("name").map(|n| (n.as_str(), c)))
        .collect();

    let mut sch = Schematic::default();

    for (name, inst) in &netlist.instances {
        let (x, y, rotation, mirror) = comp_by_name
            .get(name.as_str())
            .map(|c| (c.x, c.y, c.rotation, c.flip))
            .unwrap_or((0.0, 0.0, 0, 0));

        sch.components.insert(
            name.clone(),
            Component {
                name: name.clone(),
                symbol: inst.symbol.clone(),
                params: inst.params.clone(),
                x,
                y,
                rotation,
                mirror,
            },
        );
    }

    for wire in xv_sch.wires() {
        let label = wire.properties.get("lab").cloned().unwrap_or_default();
        if !label.is_empty() {
            sch.nets.insert(label.clone());
        }
        sch.wires.push(Wire {
            x1: wire.x1,
            y1: wire.y1,
            x2: wire.x2,
            y2: wire.y2,
            label,
        });
    }

    for net in netlist.nets.keys() {
        sch.nets.insert(net.clone());
    }

    sch
}

pub struct XschemParser;

impl SchematicParser for XschemParser {
    fn detect_format(&self, content: &[u8]) -> FileFormat {
        detect_format(content)
    }

    fn parse(&self, content: &[u8]) -> Schematic {
        parse(content)
    }
}
