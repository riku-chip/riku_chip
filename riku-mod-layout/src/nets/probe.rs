//! La red bajo un punto del visor: los pedazos conductores de cada red en
//! una grilla por tipo, con las capas de la escena donde se dibuja cada tipo
//! (para elegir la red del metal que está bajo el cursor y no la del pozo de
//! abajo).

use std::collections::HashMap;

use viewer_core::element::Layer;
use viewer_core::{NetHit, NetProbe};

use super::{net_label, Netlist};
use crate::devices::extract::Grid;

/// Los pedazos de un tipo, la red de cada uno y las capas donde se ve.
struct TypePieces {
    layers: Vec<Layer>,
    grid: Grid,
    nets: Vec<usize>,
}

/// La sonda de redes de una celda (ver [`NetProbe`]).
pub struct LayoutNets {
    types: Vec<TypePieces>,
    names: Vec<String>,
    /// Por red, sus pedazos: `(tipo, polígono)`.
    by_net: Vec<Vec<(usize, usize)>>,
}

impl std::fmt::Debug for LayoutNets {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LayoutNets").field("nets", &self.names.len()).field("types", &self.types.len()).finish()
    }
}

impl LayoutNets {
    /// `layers_of(tipo)`: las capas de la escena donde se dibuja ese tipo de
    /// Magic. `unit_um`: µm por unidad, para nombrar las redes sin etiqueta.
    pub fn new(nl: &Netlist, layers_of: &dyn Fn(&str) -> Vec<Layer>, unit_um: f64) -> Self {
        let mut grouped: HashMap<&str, (Vec<gdstk_rs::OwnedPolygon>, Vec<usize>)> = HashMap::new();
        for p in &nl.pieces {
            let e = grouped.entry(p.magic.as_str()).or_default();
            e.0.push(p.poly.clone());
            e.1.push(p.net);
        }
        let mut keys: Vec<&str> = grouped.keys().copied().collect();
        keys.sort_unstable();
        let mut by_net = vec![Vec::new(); nl.nets.len()];
        let mut types = Vec::new();
        for (ti, t) in keys.into_iter().enumerate() {
            let (polys, nets) = grouped.remove(t).unwrap_or_default();
            for (pi, &n) in nets.iter().enumerate() {
                by_net[n].push((ti, pi));
            }
            types.push(TypePieces { layers: layers_of(t), grid: Grid::new(polys), nets });
        }
        let names = (0..nl.nets.len()).map(|i| net_label(nl, i, unit_um)).collect();
        Self { types, names, by_net }
    }

    fn hit(&self, net: usize) -> NetHit {
        let outline = self.by_net[net]
            .iter()
            .map(|&(t, p)| self.types[t].grid.polys[p].points.iter().map(|q| (q.x, q.y)).collect())
            .collect();
        NetHit { name: self.names[net].clone(), outline }
    }
}

impl NetProbe for LayoutNets {
    fn at(&self, x: f64, y: f64, layer: Option<Layer>) -> Option<NetHit> {
        let net = self
            .types
            .iter()
            .filter(|t| layer.is_none_or(|l| t.layers.contains(&l)))
            .find_map(|t| t.grid.find(x, y).map(|i| t.nets[i as usize]))?;
        Some(self.hit(net))
    }
}

#[cfg(test)]
mod tests {
    use super::super::{Net, NetPiece, Netlist};
    use super::*;
    use gdstk_rs::{OwnedPolygon, Point2D};

    fn square(x: f64) -> OwnedPolygon {
        let p = |x, y| Point2D { x, y };
        OwnedPolygon { layer: 0, datatype: 0, points: vec![p(x, 0.0), p(x + 1.0, 0.0), p(x + 1.0, 1.0), p(x, 1.0)] }
    }

    #[test]
    fn finds_the_net_of_the_layer_under_the_cursor() {
        let net = |n: &str| Net { name: Some(n.into()), labels: vec![n.into()], port: true, substrate: false, bbox: [0.0; 4] };
        let nl = Netlist {
            pieces: vec![
                NetPiece { magic: "metal1".into(), poly: square(0.0), net: 0 },
                NetPiece { magic: "metal1".into(), poly: square(5.0), net: 0 },
                // Un pozo debajo del mismo punto, de otra red.
                NetPiece { magic: "nwell".into(), poly: square(0.0), net: 1 },
            ],
            nets: vec![net("Y"), net("VPB")],
            ..Default::default()
        };
        let probe = LayoutNets::new(&nl, &|t| if t == "metal1" { vec![3] } else { vec![1] }, 1.0);
        let hit = probe.at(0.5, 0.5, Some(3)).expect("metal1");
        assert_eq!((hit.name.as_str(), hit.outline.len()), ("Y", 2), "la red entera: sus dos pedazos");
        assert_eq!(probe.at(0.5, 0.5, Some(1)).map(|h| h.name), Some("VPB".into()), "sobre el pozo, su red");
        assert_eq!(probe.at(0.5, 0.5, Some(9)), None, "una capa que no conduce");
        assert_eq!(probe.at(3.0, 0.5, None), None);
    }
}
