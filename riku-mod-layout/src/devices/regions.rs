//! Regiones de los tipos de Magic en una celda aplanada: las reglas de
//! `cifinput` evaluadas con polígonos (`or`, `and`, `and-not` con
//! [`boolean_owned`]). Para decidir el tipo de una compuerta alcanza con un
//! punto ([`DeviceRules::device_at`]); las redes necesitan las regiones
//! enteras (`ndiff`, `locali`, `mcon`…), con `grow` y `shrink` (el pozo P de
//! SKY130 es la difusión fuera del pozo N agrandada 0,13 µm, y así sus
//! pedazos se unen).

use std::collections::HashMap;

use gdstk_rs::{boolean_owned, offset_owned, BoolOp, GdsTag, OwnedPolygon};

use super::extract::LayerPolys;
use super::rules::{DeviceRules, Op};

const TAG: GdsTag = GdsTag { layer: 0, datatype: 0 };

fn boolean(a: &[OwnedPolygon], b: &[OwnedPolygon], op: BoolOp) -> Vec<OwnedPolygon> {
    boolean_owned(a, b, op, TAG).unwrap_or_default()
}

/// Evalúa regiones de tipos sobre los polígonos de una celda, recordando
/// las capas intermedias (`templayer`) ya evaluadas.
pub struct RegionEval<'a> {
    rules: &'a DeviceRules,
    layers: &'a LayerPolys,
    /// µm por unidad de las coordenadas.
    unit_um: f64,
    memo: HashMap<String, Vec<OwnedPolygon>>,
    visiting: Vec<String>,
    /// Si un operando puede tener algo (ver [`Self::maybe_def`]).
    maybe: HashMap<String, bool>,
}

impl<'a> RegionEval<'a> {
    /// `unit_um`: µm por unidad de las coordenadas de `layers`.
    pub fn new(rules: &'a DeviceRules, layers: &'a LayerPolys, unit_um: f64) -> Self {
        Self { rules, layers, unit_um, memo: HashMap::new(), visiting: Vec::new(), maybe: HashMap::new() }
    }

    /// La región de un tipo de Magic (cualquiera de sus nombres): la unión
    /// de sus `layer` en `cifinput` (un tipo puede tener más de una).
    pub fn type_region(&mut self, name: &str) -> Vec<OwnedPolygon> {
        let rules = self.rules;
        let c = rules.canonical(name);
        let defs: Vec<usize> =
            rules.defs.iter().enumerate().filter(|(_, d)| !d.temp && rules.canonical(&d.name) == c).map(|(i, _)| i).collect();
        let mut out = Vec::new();
        for &d in &defs {
            if self.maybe_def(d, None, &mut Vec::new()) {
                out.extend(self.def(d, None));
            }
        }
        if defs.len() > 1 && !out.is_empty() {
            out = boolean(&out, &[], BoolOp::Or);
        }
        out
    }

    /// La región de una sola `layer` (por su índice en `cifinput`).
    pub fn def_region(&mut self, def: usize) -> Vec<OwnedPolygon> {
        self.def(def, None)
    }

    /// Las operaciones de una `layer`/`templayer`, hasta `upto` (sin incluirla).
    fn def(&mut self, def: usize, upto: Option<usize>) -> Vec<OwnedPolygon> {
        let ops = &self.rules.defs[def].ops;
        let ops = &ops[..upto.unwrap_or(ops.len()).min(ops.len())];
        let mut state: Vec<OwnedPolygon> = Vec::new();
        for op in ops {
            match op {
                Op::Or(ns) => {
                    let r = self.names(ns);
                    if !r.is_empty() {
                        state = boolean(&state, &r, BoolOp::Or);
                    }
                }
                Op::And(ns) if !state.is_empty() => {
                    let r = self.names(ns);
                    state = if r.is_empty() { Vec::new() } else { boolean(&state, &r, BoolOp::And) };
                }
                Op::AndNot(ns) if !state.is_empty() => {
                    let r = self.names(ns);
                    if !r.is_empty() {
                        state = boolean(&state, &r, BoolOp::Not);
                    }
                }
                Op::Grow(um) if !state.is_empty() => state = offset_owned(&state, um / self.unit_um, TAG).unwrap_or_default(),
                Op::Shrink(um) if !state.is_empty() => state = offset_owned(&state, -um / self.unit_um, TAG).unwrap_or_default(),
                _ => {}
            }
        }
        state
    }

    /// Sin tocar polígonos: la regla puede dar algo con las capas que hay.
    /// Un tipo que necesita una capa ausente (`and HVI` sin alto voltaje en
    /// la celda) se descarta antes de hacer las operaciones, que son lo caro.
    fn maybe_def(&mut self, def: usize, upto: Option<usize>, visiting: &mut Vec<String>) -> bool {
        let ops = &self.rules.defs[def].ops;
        let ops = &ops[..upto.unwrap_or(ops.len()).min(ops.len())];
        let mut state = false;
        for op in ops {
            match op {
                Op::Or(ns) => state = state || ns.iter().any(|n| self.maybe_name(n, visiting)),
                Op::And(ns) => state = state && ns.iter().any(|n| self.maybe_name(n, visiting)),
                _ => {}
            }
        }
        state
    }

    fn maybe_name(&mut self, n: &str, visiting: &mut Vec<String>) -> bool {
        if let Some(&v) = self.maybe.get(n) {
            return v;
        }
        if visiting.iter().any(|v| v == n) {
            return false;
        }
        visiting.push(n.to_string());
        let rules = self.rules;
        let mut v = rules.gds_layers(n).iter().any(|&gl| self.layers.has(gl));
        if !v {
            if let Some(&t) = rules.temps.get(n) {
                v = self.maybe_def(t, None, visiting);
            }
        }
        if !v {
            for &(d, o) in rules.copyups.get(n).into_iter().flatten() {
                if self.maybe_def(d, Some(o), visiting) {
                    v = true;
                    break;
                }
            }
        }
        visiting.pop();
        self.maybe.insert(n.to_string(), v);
        v
    }

    fn names(&mut self, names: &[String]) -> Vec<OwnedPolygon> {
        names.iter().flat_map(|n| self.name(n)).collect()
    }

    /// Un operando: su capa GDS, su `templayer` y lo que otras le suman con
    /// `copyup`. Un nombre que se refiere a sí mismo (un ciclo de `copyup`)
    /// no suma.
    fn name(&mut self, n: &str) -> Vec<OwnedPolygon> {
        if let Some(v) = self.memo.get(n) {
            return v.clone();
        }
        if self.visiting.iter().any(|v| v == n) {
            return Vec::new();
        }
        self.visiting.push(n.to_string());
        let rules = self.rules;
        let mut v = self.layers.polys(rules.gds_layers(n));
        if let Some(&t) = rules.temps.get(n) {
            v.extend(self.def(t, None));
        }
        for &(d, o) in rules.copyups.get(n).into_iter().flatten() {
            v.extend(self.def(d, Some(o)));
        }
        self.visiting.pop();
        self.memo.insert(n.to_string(), v.clone());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::super::rules::tests::TECH;
    use super::*;
    use gdstk_rs::Point2D;

    fn rect(tag: (u32, u32), x0: f64, y0: f64, x1: f64, y1: f64) -> OwnedPolygon {
        let p = |x, y| Point2D { x, y };
        OwnedPolygon { layer: tag.0, datatype: tag.1, points: vec![p(x0, y0), p(x1, y0), p(x1, y1), p(x0, y1)] }
    }

    fn area(polys: &[OwnedPolygon]) -> f64 {
        polys
            .iter()
            .map(|p| {
                let n = p.points.len();
                (0..n).map(|i| p.points[i].x * p.points[(i + 1) % n].y - p.points[(i + 1) % n].x * p.points[i].y).sum::<f64>()
                    / 2.0
            })
            .sum::<f64>()
            .abs()
    }

    #[test]
    fn regions_follow_the_cifinput_rules() {
        let rules = DeviceRules::parse(TECH).unwrap();
        // Difusión N de 2 × 1 con un poly vertical de 0,2 en el medio.
        let layers = LayerPolys::new([
            rect((65, 20), 0.0, 0.0, 2.0, 1.0),
            rect((93, 44), -1.0, -1.0, 3.0, 2.0),
            rect((66, 20), 0.9, -0.5, 1.1, 1.5),
        ]);
        let mut ev = RegionEval::new(&rules, &layers, 1.0);
        // ndiff = ndiffarea (templayer) = DIFF and-not POLY and NSDM: dos pedazos.
        let ndiff = ev.type_region("ndiffusion");
        assert_eq!(ndiff.len(), 2, "{ndiff:?}");
        assert!((area(&ndiff) - 1.8).abs() < 1e-9);
        // La compuerta: DIFF and POLY and NSDM.
        assert!((area(&ev.type_region("nfet")) - 0.2).abs() < 1e-9);
        assert!(ev.type_region("pdiff").is_empty(), "sin pozo N");
        // pwell = DIFF,TAP and-not NWELL: la difusión entera.
        assert!((area(&ev.type_region("pwell")) - 2.0).abs() < 1e-9);
        assert!(ev.type_region("nada").is_empty());
    }

    #[test]
    fn grow_and_shrink_close_the_gaps_of_a_well() {
        // Dos tomas separadas 0,2 µm (en nm): `pwell2` las agranda 0,13 µm
        // (en la regla, 13 centimicrones) y las achica: quedan unidas.
        let tech = TECH.replace(
            " layer pwell DIFF,TAP\n and-not NWELL\n",
            " layer pwell DIFF,TAP\n and-not NWELL\n layer pwell2 TAP\n grow 13\n shrink 13\n",
        );
        let rules = DeviceRules::parse(&tech).unwrap();
        let layers = LayerPolys::new([rect((65, 44), 0.0, 0.0, 1000.0, 500.0), rect((65, 44), 1200.0, 0.0, 2000.0, 500.0)]);
        let mut ev = RegionEval::new(&rules, &layers, 1e-3);
        assert_eq!(ev.type_region("pwell").len(), 2, "sin grow: dos pedazos");
        let well = ev.type_region("pwell2");
        assert_eq!(well.len(), 1, "{well:?}");
        assert!((area(&well) - 2000.0 * 500.0).abs() < 1.0, "{}", area(&well));
    }
}
