use std::collections::{BTreeMap, HashMap};

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

    let mut netlist = xschem_viewer::extract_netlist(&xv_sch);

    // Resolver escena para obtener posiciones de pines en espacio mundo,
    // luego hacer matching geométrico wire-endpoint → pin → net.
    // Usamos SceneBuilder directamente para reutilizar el xv_sch ya parseado.
    {
        let opts = xschem_viewer::RenderOptions::dark().with_sym_paths_from_xschemrc();
        let scene = xschem_viewer::SceneBuilder::new(&opts).build(&xv_sch);
        xschem_viewer::fill_connectivity(&mut netlist, &scene, &xv_sch);
    }

    // Index components by name — xschem stores the instance name in properties
    // under keys "name", "Name", or as the first property value
    let comp_by_name: HashMap<String, &xschem_viewer::models::Component> = xv_sch
        .components()
        .filter_map(|c| {
            c.properties.get("name")
                .or_else(|| c.properties.get("Name"))
                .map(|n| (n.clone(), c))
        })
        .collect();

    let mut sch = Schematic::default();

    for (name, inst) in &netlist.instances {
        let found = comp_by_name.get(name.as_str());
        let (x, y, rotation, mirror) = found
            .map(|c| (c.x, c.y, c.rotation, c.flip))
            .unwrap_or((0.0, 0.0, 0, 0));

        // Recopilar conectividad pin → net de este componente
        let pins: BTreeMap<String, String> = netlist.nets
            .iter()
            .flat_map(|(net_name, net)| {
                net.pins.iter()
                    .filter(|p| p.instance == *name)
                    .map(move |p| (p.pin.clone(), net_name.clone()))
            })
            .collect();

        sch.components.insert(
            name.clone(),
            Component {
                name: name.clone(),
                symbol: inst.symbol.clone(),
                params: inst.params.clone(),
                pins,
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
