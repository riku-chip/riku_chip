//! Reglas de dispositivos de un PDK, leídas de su `.tech` de Magic:
//!
//! - `cifinput` (su primer estilo, el que Magic usa por defecto) dice cómo
//!   se arma cada tipo de Magic a partir de capas GDS: `layer nfet DIFF`,
//!   `and POLY`, `and-not PSDM`…, con capas intermedias (`templayer`) y
//!   capas que se llenan desde otras (`copyup`); `calma DIFF 65 20` dice qué
//!   capa GDS es cada nombre.
//! - `extract` (su primer estilo) dice qué modelo SPICE es cada tipo:
//!   `device msubcircuit sky130_fd_pr__nfet_01v8 nfet,scnfet … w>=0.42`.
//!   Un tipo puede tener varios modelos según W y L (en SKY130, `scnfet` es
//!   `special_nfet_01v8` con `w<0.42`): se usa el primero cuyas condiciones
//!   se cumplen, como en Magic. Los dos campos siguientes son los tipos de
//!   fuente y drenaje (`*ndiff`: `ndiff` y sus contactos), para los `.mag`,
//!   donde los transistores ya vienen pintados.
//! - `types` dice qué nombres son el mismo tipo (`scnmos,scntransistor,scnfet`):
//!   un `.mag` usa cualquiera de ellos.
//!
//! Magic lo interpreta en `cif/CIFrdtech.c`: cada `layer`/`templayer` es una
//! lista de operaciones sobre una región que empieza vacía (`or` suma, `and`
//! corta, `and-not` resta). Acá se evalúan en un punto: sirve para decidir
//! el tipo de una compuerta, cuyos marcadores (implantes, pozos) la cubren
//! entera en un diseño que pasa DRC. `grow`/`shrink` no cambian el
//! resultado en el punto (aproximación: los marcadores no se agrandan).

use std::collections::HashMap;

/// Una capa GDS: `(layer, datatype)`; `None` en datatype es "cualquiera".
pub type GdsLayer = (u32, Option<u32>);

#[derive(Clone, Debug, PartialEq)]
enum Op {
    Or(Vec<String>),
    And(Vec<String>),
    AndNot(Vec<String>),
    /// Suma la región de este punto de la lista a otra capa.
    CopyUp(Vec<String>),
    /// `grow`, `shrink` y lo que no se evalúa en un punto.
    Ignored,
}

#[derive(Clone, Debug, PartialEq)]
struct Def {
    name: String,
    /// `layer` (un tipo de Magic) o `templayer` (intermedia, usable como operando).
    temp: bool,
    ops: Vec<Op>,
}

/// Un tipo de transistor: su nombre en Magic, sus modelos SPICE (cada uno
/// con las condiciones de W y L en que se usa) y los tipos de su fuente y
/// drenaje (en un `.mag`).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceType {
    pub magic: String,
    pub models: Vec<(String, Vec<Cond>)>,
    pub sd: Vec<String>,
}

/// `w>=0.42`: una condición de un modelo, en µm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cond {
    pub param: char,
    pub op: CondOp,
    pub value: f64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CondOp {
    Lt,
    Le,
    Gt,
    Ge,
}

impl Cond {
    fn parse(token: &str) -> Option<Self> {
        let param = token.chars().next().filter(|c| matches!(c, 'w' | 'l'))?;
        let rest = &token[1..];
        let (op, num) = [(">=", CondOp::Ge), ("<=", CondOp::Le), (">", CondOp::Gt), ("<", CondOp::Lt)]
            .into_iter()
            .find_map(|(s, op)| rest.strip_prefix(s).map(|n| (op, n)))?;
        Some(Self { param, op, value: num.parse().ok()? })
    }

    fn holds(&self, w_um: f64, l_um: f64) -> bool {
        let v = if self.param == 'w' { w_um } else { l_um };
        // Medio nanómetro de margen: W y L salen de áreas y bordes en punto flotante.
        let eps = 5e-4;
        match self.op {
            CondOp::Lt => v < self.value - eps,
            CondOp::Le => v <= self.value + eps,
            CondOp::Gt => v > self.value + eps,
            CondOp::Ge => v >= self.value - eps,
        }
    }
}

impl DeviceType {
    /// El modelo para un transistor de esas medidas: el primero cuyas
    /// condiciones se cumplen (o el primero, si ninguno).
    pub fn model(&self, w_um: f64, l_um: f64) -> &str {
        self.models
            .iter()
            .find(|(_, conds)| conds.iter().all(|c| c.holds(w_um, l_um)))
            .or(self.models.first())
            .map_or("", |(m, _)| m.as_str())
    }
}

/// Las reglas de un PDK.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct DeviceRules {
    calma: HashMap<String, Vec<GdsLayer>>,
    defs: Vec<Def>,
    temps: HashMap<String, usize>,
    /// `(def, op)` de cada `copyup` a un nombre.
    copyups: HashMap<String, Vec<(usize, usize)>>,
    /// Transistores, en el orden de su `layer` en `cifinput` (el de Magic).
    pub devices: Vec<(usize, DeviceType)>,
    /// Cada nombre de `types` → el primero de su línea (el canónico).
    canonical: HashMap<String, String>,
}

/// Clases de `device` que son transistores MOS.
const MOS_CLASSES: &[&str] = &["mosfet", "msubcircuit"];

impl DeviceRules {
    /// Reglas del texto de un `.tech` (con sus `include` ya puestos).
    /// `None` si no tiene `cifinput` o ningún transistor.
    pub fn parse(tech: &str) -> Option<Self> {
        let sections = sections(tech);
        let cif = first_style(&sections.iter().find(|(n, _)| n == "cifinput")?.1);
        let mut rules = DeviceRules::default();
        for line in sections.iter().filter(|(n, _)| n == "types").flat_map(|(_, l)| l) {
            let Some(names) = line.split_whitespace().nth(1) else { continue };
            let names: Vec<&str> = names.split(',').collect();
            for n in &names {
                rules.canonical.entry(n.to_string()).or_insert_with(|| names[0].to_string());
            }
        }
        let models = sections
            .iter()
            .find(|(n, _)| n == "extract")
            .map(|(_, l)| device_models(&first_style(l), &rules.canonical))
            .unwrap_or_default();

        for line in &cif {
            let w: Vec<&str> = line.split_whitespace().collect();
            let names = |i: usize| w.get(i).map(|s| s.split(',').map(str::to_string).collect::<Vec<_>>()).unwrap_or_default();
            match w.first().copied() {
                Some("calma") | Some("gds") if w.len() >= 4 => {
                    let tags = gds_layers(w[2], w[3]);
                    rules.calma.entry(w[1].to_string()).or_default().extend(tags);
                }
                Some(k @ ("layer" | "templayer" | "fault")) if w.len() >= 2 => {
                    let temp = k != "layer";
                    let ops = if w.len() >= 3 { vec![Op::Or(names(2))] } else { Vec::new() };
                    if temp {
                        rules.temps.insert(w[1].to_string(), rules.defs.len());
                    }
                    rules.defs.push(Def { name: w[1].to_string(), temp, ops });
                }
                Some(op @ ("and" | "and-not" | "or" | "copyup")) => {
                    let Some(def) = rules.defs.last_mut() else { continue };
                    let n = names(1);
                    def.ops.push(match op {
                        "and" => Op::And(n),
                        "and-not" => Op::AndNot(n),
                        "or" => Op::Or(n),
                        _ => Op::CopyUp(n),
                    });
                }
                Some("grow" | "grow-grid" | "shrink" | "boundary" | "not-square" | "mask-hints" | "tagged") => {
                    if let Some(def) = rules.defs.last_mut() {
                        def.ops.push(Op::Ignored);
                    }
                }
                _ => {} // style, scalefactor, labels, ignore, options…
            }
        }
        for (d, def) in rules.defs.iter().enumerate() {
            for (o, op) in def.ops.iter().enumerate() {
                if let Op::CopyUp(targets) = op {
                    for t in targets {
                        rules.copyups.entry(t.clone()).or_default().push((d, o));
                    }
                }
            }
        }
        rules.devices = rules
            .defs
            .iter()
            .enumerate()
            .filter(|(_, d)| !d.temp)
            .filter_map(|(i, d)| {
                let (models, sd) = models.get(rules.canonical(&d.name))?;
                Some((i, DeviceType { magic: d.name.clone(), models: models.clone(), sd: sd.clone() }))
            })
            .collect();
        (!rules.devices.is_empty()).then_some(rules)
    }

    /// El nombre canónico de un tipo de Magic (el mismo si no está en `types`).
    pub fn canonical<'a>(&'a self, name: &'a str) -> &'a str {
        self.canonical.get(name).map_or(name, String::as_str)
    }

    /// El transistor de un tipo de Magic con cualquiera de sus nombres.
    pub fn device_type(&self, name: &str) -> Option<&DeviceType> {
        let c = self.canonical(name);
        self.devices.iter().map(|(_, t)| t).find(|t| self.canonical(&t.magic) == c)
    }

    /// El tipo `name` es fuente o drenaje de `t` (`*ndiff` incluye sus
    /// contactos, `ndiffc`: se toma por prefijo).
    pub fn is_sd_of(&self, t: &DeviceType, name: &str) -> bool {
        let c = self.canonical(name);
        t.sd.iter().any(|s| c.starts_with(s.as_str()))
    }

    /// Capas GDS de un nombre de `cifinput` (vacío si no está en ningún `calma`).
    pub fn gds_layers(&self, name: &str) -> &[GdsLayer] {
        self.calma.get(name).map_or(&[], Vec::as_slice)
    }

    /// Todas las capas GDS que usan las reglas de los transistores.
    pub fn used_layers(&self) -> Vec<GdsLayer> {
        let mut names = Vec::new();
        for (d, _) in &self.devices {
            self.collect_names(*d, &mut names, &mut Vec::new());
        }
        let mut out: Vec<GdsLayer> = names.iter().flat_map(|n| self.gds_layers(n).iter().copied()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    fn collect_names(&self, def: usize, out: &mut Vec<String>, visiting: &mut Vec<String>) {
        for op in &self.defs[def].ops {
            let (Op::Or(ns) | Op::And(ns) | Op::AndNot(ns)) = op else { continue };
            for n in ns {
                if visiting.contains(n) {
                    continue;
                }
                visiting.push(n.clone());
                if !out.contains(n) {
                    out.push(n.clone());
                }
                if let Some(&t) = self.temps.get(n) {
                    self.collect_names(t, out, visiting);
                }
                for &(d, _) in self.copyups.get(n).into_iter().flatten() {
                    self.collect_names(d, out, visiting);
                }
                visiting.pop();
            }
        }
    }

    /// Las dos capas cuya intersección es la compuerta de un transistor: el
    /// primer `or` (la difusión) y el primer `and` (el poly), siguiendo las
    /// `templayer`. En capas GDS.
    pub fn gate_layers(&self, device: usize) -> Option<(Vec<GdsLayer>, Vec<GdsLayer>)> {
        let (active, gate) = self.core(device, 0)?;
        let tags = |names: &[String]| {
            let mut v: Vec<GdsLayer> = names.iter().flat_map(|n| self.gds_layers(n).iter().copied()).collect();
            v.sort_unstable();
            v.dedup();
            v
        };
        let (a, g) = (tags(&active), tags(&gate));
        (!a.is_empty() && !g.is_empty()).then_some((a, g))
    }

    fn core(&self, def: usize, depth: u32) -> Option<(Vec<String>, Vec<String>)> {
        if depth > 8 {
            return None;
        }
        let ops = &self.defs[def].ops;
        let Some(Op::Or(first)) = ops.first() else { return None };
        // Una sola templayer como base: su núcleo (`pfet` = `pfetarea` and-not…).
        // Los `and` que la capa suma después solo restringen el tipo.
        if let [only] = first.as_slice() {
            if let Some(&t) = self.temps.get(only) {
                return self.core(t, depth + 1);
            }
        }
        let and = ops.iter().find_map(|o| if let Op::And(n) = o { Some(n.clone()) } else { None })?;
        Some((first.clone(), and))
    }

    /// El tipo de transistor en un punto: el último cuya regla lo incluye
    /// (Magic pinta las capas en orden). `inside(capa)` dice si el punto está
    /// en esa capa GDS.
    pub fn device_at(&self, inside: &dyn Fn(GdsLayer) -> bool) -> Option<&DeviceType> {
        let mut memo = HashMap::new();
        self.devices
            .iter()
            .filter(|(d, _)| self.eval_def(*d, None, inside, &mut memo, &mut Vec::new()))
            .last()
            .map(|(_, t)| t)
    }

    /// Una lista de operaciones en el punto, hasta `upto` (sin incluirla).
    fn eval_def(
        &self,
        def: usize,
        upto: Option<usize>,
        inside: &dyn Fn(GdsLayer) -> bool,
        memo: &mut HashMap<String, bool>,
        visiting: &mut Vec<String>,
    ) -> bool {
        let mut state = false;
        let ops = &self.defs[def].ops;
        for op in &ops[..upto.unwrap_or(ops.len()).min(ops.len())] {
            match op {
                Op::Or(ns) => state = state || self.any_in(ns, inside, memo, visiting),
                Op::And(ns) => state = state && self.any_in(ns, inside, memo, visiting),
                Op::AndNot(ns) => state = state && !self.any_in(ns, inside, memo, visiting),
                Op::CopyUp(_) | Op::Ignored => {}
            }
        }
        state
    }

    fn any_in(&self, names: &[String], inside: &dyn Fn(GdsLayer) -> bool, memo: &mut HashMap<String, bool>, visiting: &mut Vec<String>) -> bool {
        names.iter().any(|n| self.name_in(n, inside, memo, visiting))
    }

    /// El punto está en el nombre `n`: su capa GDS, su `templayer` o lo que
    /// otras le suman con `copyup`. Un nombre que se refiere a sí mismo (por
    /// un ciclo de `copyup`) no suma.
    fn name_in(&self, n: &str, inside: &dyn Fn(GdsLayer) -> bool, memo: &mut HashMap<String, bool>, visiting: &mut Vec<String>) -> bool {
        if let Some(&v) = memo.get(n) {
            return v;
        }
        if visiting.iter().any(|v| v == n) {
            return false;
        }
        visiting.push(n.to_string());
        let mut v = self.gds_layers(n).iter().any(|&t| inside(t));
        if !v {
            if let Some(&t) = self.temps.get(n) {
                v = self.eval_def(t, None, inside, memo, visiting);
            }
        }
        if !v {
            for &(d, o) in self.copyups.get(n).into_iter().flatten() {
                if self.eval_def(d, Some(o), inside, memo, visiting) {
                    v = true;
                    break;
                }
            }
        }
        visiting.pop();
        memo.insert(n.to_string(), v);
        v
    }
}

/// `calma NAME 65 20`: capas y datatypes, con listas separadas por comas y
/// `*` (cualquiera).
fn gds_layers(layers: &str, types: &str) -> Vec<GdsLayer> {
    let nums = |s: &str| s.split(',').filter_map(|x| x.trim().parse::<u32>().ok()).collect::<Vec<_>>();
    let types: Vec<Option<u32>> = if types.trim() == "*" { vec![None] } else { nums(types).into_iter().map(Some).collect() };
    nums(layers).into_iter().flat_map(|l| types.iter().map(move |&t| (l, t))).collect()
}

/// Modelos (con sus condiciones) y tipos de fuente/drenaje de cada tipo,
/// por nombre canónico, según las líneas `device` de MOS de `extract`.
#[allow(clippy::type_complexity)]
fn device_models(lines: &[String], canonical: &HashMap<String, String>) -> HashMap<String, (Vec<(String, Vec<Cond>)>, Vec<String>)> {
    let canon = |n: &str| canonical.get(n).cloned().unwrap_or_else(|| n.to_string());
    let mut out: HashMap<String, (Vec<(String, Vec<Cond>)>, Vec<String>)> = HashMap::new();
    for line in lines {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.first() != Some(&"device") || w.len() < 4 || !MOS_CLASSES.contains(&w[1]) {
            continue;
        }
        let conds: Vec<Cond> = w[4..].iter().filter_map(|t| Cond::parse(t)).collect();
        // Fuente y drenaje: los campos que siguen a los tipos y empiezan con
        // `*` (`*ndiff,ndiffres`); el que sigue sin `*` es el sustrato
        // (`pwell,space/w`).
        let sd: Vec<String> = w[4..]
            .iter()
            .take_while(|t| t.starts_with('*'))
            .flat_map(|t| t.split(','))
            .map(|t| canon(t.trim_start_matches('*')))
            .collect();
        for t in w[3].split(',') {
            let (models, sds) = out.entry(canon(t)).or_default();
            // La misma línea repetida con otros terminales (npd en SKY130).
            if !models.iter().any(|(m, c)| m == w[2] && *c == conds) {
                models.push((w[2].to_string(), conds.clone()));
            }
            for s in &sd {
                if !sds.contains(s) {
                    sds.push(s.clone());
                }
            }
        }
    }
    out
}

/// Las líneas del primer estilo de una sección (hasta el segundo `style`).
fn first_style(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = 0;
    for l in lines {
        if l.split_whitespace().next() == Some("style") {
            seen += 1;
            if seen > 1 {
                break;
            }
            continue;
        }
        out.push(l.clone());
    }
    out
}

/// Secciones de primer nivel del `.tech` (`cifinput`, `extract`…) con sus
/// líneas, sin comentarios y con las continuaciones (`\` al final) unidas.
fn sections(text: &str) -> Vec<(String, Vec<String>)> {
    let mut out: Vec<(String, Vec<String>)> = Vec::new();
    let mut open = false;
    let mut pending = String::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        let line = if pending.is_empty() { line.to_string() } else { std::mem::take(&mut pending) + line };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if !open {
            out.push((line.split_whitespace().next().unwrap_or("").to_string(), Vec::new()));
            open = true;
        } else if line == "end" {
            open = false;
        } else if let Some(last) = out.last_mut() {
            last.1.push(line.to_string());
        }
    }
    out
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Un `.tech` mínimo con la forma del de SKY130: nfet, pfet por una
    /// `templayer`, un marcador de bajo Vt y un `copyup` cíclico.
    pub(crate) const TECH: &str = r"
tech
  mini
end
types
 active nmos,nfet
 active nmoslvt,nfetlvt
 active pmos,pfet
 active ndiff,ndiffusion
 active ndiffc,ndcontact
 active pdiff
end
cifinput
style mini
 scalefactor 10
 templayer hvarea HVI
 copyup hvcheck
 templayer xhvcheck hvcheck
 copyup hvcheck
 templayer pfetarea DIFF
 and POLY
 and-not NSDM
 and-not HVI,hvcheck
 layer nfet DIFF,barediff
 and POLY
 and NSDM
 and-not LVTN
 and-not HVI,hvcheck
 layer nfetlvt DIFF
 and POLY
 and NSDM
 and LVTN
 layer pfet pfetarea
 and NWELL
 grow 10
 calma DIFF 65 20
 calma POLY 66 20
 calma NSDM 93 44
 calma NWELL 64 20
 calma LVTN 125 44
 calma HVI 75 *
style other
 layer nfet POLY
end
extract
style mini
 device msubcircuit mini__nfet nfet *ndiff *ndiff \
    pwell error w>=0.42 l=l w=w
 device msubcircuit mini__nfet_small nfet *ndiff *ndiff pwell error w<0.42
 device mosfet mini__nfet_lvt nfetlvt *ndiff
 device msubcircuit mini__pfet pfet *pdiff
 device resistor mini__res rpoly *poly
style other
 device mosfet other__nfet nfet *ndiff
end
";

    fn at<'a>(rules: &'a DeviceRules, layers: &[(u32, u32)]) -> Option<&'a str> {
        let inside = |(l, d): GdsLayer| layers.iter().any(|&(ll, dd)| ll == l && d.is_none_or(|d| d == dd));
        rules.device_at(&inside).map(|t| t.model(1.0, 0.15))
    }

    #[test]
    fn reads_the_first_style_and_mos_devices_only() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        let names: Vec<(&str, &str)> = r.devices.iter().map(|(_, t)| (t.magic.as_str(), t.model(1.0, 0.15))).collect();
        assert_eq!(names, [("nfet", "mini__nfet"), ("nfetlvt", "mini__nfet_lvt"), ("pfet", "mini__pfet")]);
        // El modelo depende de W, como scnfet en SKY130.
        let nfet = &r.devices[0].1;
        assert_eq!((nfet.model(0.42, 0.15), nfet.model(0.36, 0.15)), ("mini__nfet", "mini__nfet_small"));
        assert_eq!(nfet.model(0.4199999, 0.15), "mini__nfet", "medio nm de margen");
        assert_eq!(r.gds_layers("HVI"), &[(75, None)]);
        assert!(r.used_layers().contains(&(125, Some(44))) && r.used_layers().contains(&(64, Some(20))));
    }

    #[test]
    fn classifies_a_point_like_magic() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        let (diff, poly, nsdm, nwell, lvtn, hvi) = ((65, 20), (66, 20), (93, 44), (64, 20), (125, 44), (75, 3));
        assert_eq!(at(&r, &[diff, poly, nsdm]), Some("mini__nfet"));
        assert_eq!(at(&r, &[diff, poly, nsdm, lvtn]), Some("mini__nfet_lvt"));
        assert_eq!(at(&r, &[diff, poly, nwell]), Some("mini__pfet"), "por la templayer");
        assert_eq!(at(&r, &[diff, poly, nsdm, hvi]), None, "alto voltaje: ninguna de estas");
        assert_eq!(at(&r, &[diff, nsdm]), None, "sin poly no hay compuerta");
        assert_eq!(at(&r, &[poly]), None);
    }

    #[test]
    fn gate_is_diffusion_and_poly_even_through_a_templayer() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        for (d, t) in &r.devices {
            let (a, g) = r.gate_layers(*d).expect(&t.magic);
            assert_eq!(a, [(65, Some(20))], "{}", t.magic);
            assert_eq!(g, [(66, Some(20))], "{}", t.magic);
        }
    }

    #[test]
    fn names_in_a_mag_are_resolved_with_their_aliases() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        // El .mag dice `nmos`; la regla y el `device`, `nfet`.
        let t = r.device_type("nmos").expect("alias");
        assert_eq!(t.magic, "nfet");
        assert!(r.device_type("nfet").is_some() && r.device_type("ndiff").is_none());
        assert!(r.is_sd_of(t, "ndiff") && r.is_sd_of(t, "ndiffc") && r.is_sd_of(t, "ndcontact"), "{:?}", t.sd);
        assert!(!r.is_sd_of(t, "pdiff"));
        assert!(!r.is_sd_of(t, "pwell"), "el sustrato no es fuente ni drenaje");
    }

    #[test]
    fn without_transistors_there_are_no_rules() {
        assert!(DeviceRules::parse("tech\n x\nend\n").is_none());
        assert!(DeviceRules::parse("cifinput\nstyle a\n layer m1 M1\n calma M1 68 20\nend\n").is_none());
    }
}
