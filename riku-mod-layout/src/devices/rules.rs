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
//! - `contact`, `connect` y `aliases` dicen qué tipos conducen juntos cuando
//!   se tocan; las líneas `labels` de `cifinput`, en qué
//!   capas GDS van las etiquetas de cada tipo, y `substrate` de `extract`
//!   qué tipos son el sustrato: las redes del nivel 3 (`nets/`).
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
pub(super) enum Op {
    Or(Vec<String>),
    And(Vec<String>),
    AndNot(Vec<String>),
    /// Suma la región de este punto de la lista a otra capa.
    CopyUp(Vec<String>),
    /// `grow` y `shrink`, en µm: no cambian el resultado en un punto, pero sí
    /// las regiones (un pozo que se agranda une sus pedazos).
    Grow(f64),
    Shrink(f64),
    /// Lo que no se evalúa (`grow-grid`, `boundary`…).
    Ignored,
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct Def {
    pub(super) name: String,
    /// `layer` (un tipo de Magic) o `templayer` (intermedia, usable como operando).
    pub(super) temp: bool,
    pub(super) ops: Vec<Op>,
    /// `labels LIPIN port`: nombres de `calma` cuyas etiquetas son de este
    /// tipo, y si son pines.
    labels: Vec<(String, bool)>,
}

/// Un tipo de transistor: su nombre en Magic, sus modelos SPICE (cada uno
/// con las condiciones de W y L en que se usa) y los tipos de su fuente y
/// drenaje (en un `.mag`).
#[derive(Clone, Debug, PartialEq)]
pub struct DeviceType {
    pub magic: String,
    pub models: Vec<(String, Vec<Cond>)>,
    pub sd: Vec<String>,
    /// Tipos del sustrato (`pwell`); `space` si puede no haber pozo dibujado.
    pub sub: Vec<String>,
    /// `msubcircuit` (una instancia `X` en SPICE) y no `mosfet` (`M`).
    pub subckt: bool,
}

/// Un tipo de resistor: su nombre en Magic (canónico), su modelo SPICE
/// (`None` en las líneas `None` de IHP: un corto, no un dispositivo) y los
/// tipos de sus terminales (`*metal5`: el metal y sus vías).
#[derive(Clone, Debug, PartialEq)]
pub struct ResistorType {
    pub magic: String,
    pub model: Option<String>,
    pub terminals: Vec<String>,
    /// `rsubcircuit` (una instancia `X` en SPICE) y no `resistor` (`R`).
    pub subckt: bool,
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
    pub(super) calma: HashMap<String, Vec<GdsLayer>>,
    pub(super) defs: Vec<Def>,
    pub(super) temps: HashMap<String, usize>,
    /// `(def, op)` de cada `copyup` a un nombre.
    pub(super) copyups: HashMap<String, Vec<(usize, usize)>>,
    /// Transistores, en el orden de su `layer` en `cifinput` (el de Magic).
    pub devices: Vec<(usize, DeviceType)>,
    /// Resistores (`device resistor|rsubcircuit` de `extract`), uno por tipo.
    pub resistors: Vec<ResistorType>,
    /// Cada nombre de `types` → el primero de su línea (el canónico).
    canonical: HashMap<String, String>,
    /// Cada tipo (canónico) → su plano (`active`, `metal1`…).
    planes: HashMap<String, String>,
    /// Cada tipo de contacto → las dos capas que une (`mcon` → `locali`, `metal1`).
    contacts: HashMap<String, Vec<String>>,
    /// Pares de listas de tipos que conducen juntos, ya expandidos.
    pub connect: Vec<(Vec<String>, Vec<String>)>,
    /// Tipos que son el sustrato (`*psd,space/w,pwell`) y los que lo excluyen
    /// (`-dnwell,isosub`).
    pub substrate: (Vec<String>, Vec<String>),
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
            let mut w = line.split_whitespace();
            let (Some(plane), Some(names)) = (w.next(), w.next()) else { continue };
            let names: Vec<&str> = names.split(',').collect();
            for n in &names {
                rules.canonical.entry(n.to_string()).or_insert_with(|| names[0].to_string());
            }
            // `-active`: un tipo que no se lee ni se escribe en CIF; el plano es el mismo.
            rules.planes.entry(names[0].to_string()).or_insert_with(|| plane.trim_start_matches('-').to_string());
        }
        let extract = sections.iter().find(|(n, _)| n == "extract").map(|(_, l)| first_style(l)).unwrap_or_default();
        let models = device_models(&extract, &rules.canonical);
        for line in sections.iter().filter(|(n, _)| n == "contact").flat_map(|(_, l)| l) {
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.len() >= 3 && !matches!(w[0], "stackable" | "lambda") {
                let c = rules.canonical(w[0]).to_string();
                let residues = w[1..].iter().map(|r| rules.canonical(r).to_string()).collect();
                rules.contacts.insert(c, residues);
            }
        }
        let aliases: HashMap<String, String> = sections
            .iter()
            .filter(|(n, _)| n == "aliases")
            .flat_map(|(_, l)| l)
            .filter_map(|l| l.split_once(char::is_whitespace).map(|(a, b)| (a.to_string(), b.trim().to_string())))
            .collect();
        for line in sections.iter().filter(|(n, _)| n == "connect").flat_map(|(_, l)| l) {
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.len() == 2 {
                let pair = (rules.expand(w[0], &aliases), rules.expand(w[1], &aliases));
                rules.connect.push(pair);
            }
        }
        if let Some(line) = extract.iter().find(|l| l.split_whitespace().next() == Some("substrate")) {
            let w: Vec<&str> = line.split_whitespace().collect();
            let types = w.get(1).map(|t| rules.expand(t, &aliases)).unwrap_or_default();
            let not = w.iter().find_map(|t| t.strip_prefix('-')).map(|t| rules.expand(t, &aliases)).unwrap_or_default();
            rules.substrate = (types, not);
        }

        let mut unit_um = 1e-2;
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
                    rules.defs.push(Def { name: w[1].to_string(), temp, ops, labels: Vec::new() });
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
                Some("scalefactor") => {
                    // `grow`/`shrink` van en centimicrones o, con la palabra, en nm o Å.
                    unit_um = if w.contains(&"nanometers") {
                        1e-3
                    } else if w.contains(&"angstroms") {
                        1e-4
                    } else {
                        1e-2
                    };
                }
                Some(op @ ("grow" | "shrink")) if w.len() >= 2 => {
                    if let (Some(def), Ok(v)) = (rules.defs.last_mut(), w[1].parse::<f64>()) {
                        def.ops.push(if op == "grow" { Op::Grow(v * unit_um) } else { Op::Shrink(v * unit_um) });
                    }
                }
                Some("labels") if w.len() >= 2 => {
                    if let Some(def) = rules.defs.last_mut() {
                        def.labels.push((w[1].to_string(), w.get(2) == Some(&"port")));
                    }
                }
                Some("grow" | "grow-grid" | "grow-min" | "shrink" | "boundary" | "not-square" | "mask-hints" | "tagged") => {
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
                let m = models.get(rules.canonical(&d.name))?;
                // Los alias (`allpsub` en GF180 es `space/w,pwell,pbase`).
                let expand = |v: &[String]| {
                    let mut out: Vec<String> = Vec::new();
                    for x in v.iter().flat_map(|s| rules.expand(s, &aliases)) {
                        if !out.contains(&x) {
                            out.push(x);
                        }
                    }
                    out
                };
                let t = DeviceType { magic: d.name.clone(), models: m.models.clone(), sd: expand(&m.sd), sub: expand(&m.sub), subckt: m.subckt };
                Some((i, t))
            })
            .collect();
        for line in &extract {
            let w: Vec<&str> = line.split_whitespace().collect();
            if w.first() != Some(&"device") || w.len() < 5 || !matches!(w[1], "resistor" | "rsubcircuit") {
                continue;
            }
            let terminals = rules.expand(w[4], &aliases);
            for t in w[3].split(',') {
                let magic = rules.canonical(t).to_string();
                // Un tipo con dos líneas (`rsubcircuit` y `resistor` en SKY130): la primera, como Magic.
                if rules.resistors.iter().any(|r| r.magic == magic) {
                    continue;
                }
                let model = (w[2] != "None").then(|| w[2].to_string());
                rules.resistors.push(ResistorType { magic, model, terminals: terminals.clone(), subckt: w[1] == "rsubcircuit" });
            }
        }
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

    /// Una lista de tipos de `connect` o `substrate` (`*li,coreli`,
    /// `allnactivenonfet`, `space/w`), en nombres canónicos: los alias de
    /// `aliases` se expanden, `*x` es `x` y los contactos que lo unen, y el
    /// plano (`/w`) se descarta. `space` queda como está.
    fn expand(&self, list: &str, aliases: &HashMap<String, String>) -> Vec<String> {
        let mut out = Vec::new();
        self.expand_into(list, aliases, &mut out, 0);
        out
    }

    fn expand_into(&self, list: &str, aliases: &HashMap<String, String>, out: &mut Vec<String>, depth: u32) {
        for item in list.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let item = item.split('/').next().unwrap_or(item);
            let (star, name) = match item.strip_prefix('*') {
                Some(n) => (true, n),
                None => (false, item),
            };
            if let Some(a) = aliases.get(name).filter(|_| depth < 8) {
                self.expand_into(a, aliases, out, depth + 1);
                continue;
            }
            let name = self.canonical(name).to_string();
            if star {
                let mut cs: Vec<&String> = self.contacts.iter().filter(|(_, r)| r.contains(&name)).map(|(c, _)| c).collect();
                cs.sort();
                for c in cs {
                    if !out.contains(c) {
                        out.push(c.clone());
                    }
                }
            }
            if !out.contains(&name) {
                out.push(name);
            }
        }
    }

    /// El plano de un tipo (`active`), según `types`.
    pub fn plane(&self, name: &str) -> Option<&str> {
        self.planes.get(self.canonical(name)).map(String::as_str)
    }

    /// La posición de la primera `layer` de un tipo en `cifinput`: Magic pinta
    /// en ese orden, y en un mismo plano un tipo posterior tapa al anterior.
    pub fn layer_index(&self, name: &str) -> Option<usize> {
        let c = self.canonical(name);
        self.defs.iter().position(|d| !d.temp && self.canonical(&d.name) == c)
    }

    /// El tipo y los contactos que lo tocan (el `*metal1` de Magic): una
    /// etiqueta de `metal1` puede caer sobre una vía, que tapa al metal.
    pub fn with_contacts(&self, name: &str) -> Vec<String> {
        self.expand(&format!("*{name}"), &HashMap::new())
    }

    /// Las dos capas que une un contacto (vacío si no es un contacto).
    pub fn contact_residues(&self, name: &str) -> &[String] {
        self.contacts.get(self.canonical(name)).map_or(&[], Vec::as_slice)
    }

    /// Los tipos que conducen (los de `connect`), sin `space`.
    pub fn conductors(&self) -> Vec<String> {
        let mut out: Vec<String> = self.connect.iter().flat_map(|(a, b)| a.iter().chain(b)).filter(|t| *t != "space").cloned().collect();
        out.sort();
        out.dedup();
        out
    }

    /// Los tipos de Magic de las `layer` de `cifinput` (en el orden del
    /// archivo, sin repetir).
    pub fn layer_types(&self) -> Vec<&str> {
        let mut out: Vec<&str> = Vec::new();
        for d in self.defs.iter().filter(|d| !d.temp) {
            if !out.contains(&d.name.as_str()) {
                out.push(&d.name);
            }
        }
        out
    }

    /// Las capas GDS de pines (`labels LIPIN port`): una etiqueta sobre un
    /// polígono de estas capas es un pin, como en Magic.
    pub fn port_layers(&self) -> Vec<GdsLayer> {
        let mut out: Vec<GdsLayer> =
            self.defs.iter().flat_map(|d| d.labels.iter().filter(|(_, p)| *p)).flat_map(|(n, _)| self.gds_layers(n).iter().copied()).collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Los tipos cuyas etiquetas van en la capa GDS `tag`, y si son pines.
    /// Una etiqueta de una `templayer` (`ndiffarea`) es de los tipos que la
    /// usan como base (`ndiff`).
    pub fn label_types(&self, tag: (u32, u32)) -> Vec<(String, bool)> {
        let matches = |name: &str| self.gds_layers(name).iter().any(|&(l, d)| l == tag.0 && d.is_none_or(|d| d == tag.1));
        let mut out: Vec<(String, bool)> = Vec::new();
        for def in &self.defs {
            for (_, port) in def.labels.iter().filter(|(n, _)| matches(n)) {
                let types: Vec<&str> = if def.temp {
                    self.defs
                        .iter()
                        .filter(|d| !d.temp && matches!(d.ops.first(), Some(Op::Or(ns)) if ns.contains(&def.name)))
                        .map(|d| d.name.as_str())
                        .collect()
                } else {
                    vec![def.name.as_str()]
                };
                for t in types {
                    let t = self.canonical(t).to_string();
                    match out.iter_mut().find(|(n, _)| *n == t) {
                        Some(e) => e.1 |= *port,
                        None => out.push((t, *port)),
                    }
                }
            }
        }
        out
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

    /// Las capas GDS donde se dibuja un tipo: las del primer `or` de sus
    /// `layer`, siguiendo las `templayer` (`ndiff` → `ndiffarea` → `DIFF`,
    /// `DIFFPIN`…), sin los marcadores que solo lo restringen (`NSDM`).
    pub fn base_layers(&self, name: &str) -> Vec<GdsLayer> {
        let c = self.canonical(name);
        let mut out = Vec::new();
        for (i, d) in self.defs.iter().enumerate() {
            if !d.temp && self.canonical(&d.name) == c {
                self.base_into(i, &mut out, 0);
            }
        }
        out.sort_unstable();
        out.dedup();
        out
    }

    fn base_into(&self, def: usize, out: &mut Vec<GdsLayer>, depth: u32) {
        let Some(Op::Or(names)) = self.defs[def].ops.first() else { return };
        for n in names {
            out.extend(self.gds_layers(n).iter().copied());
            if let Some(&t) = self.temps.get(n).filter(|_| depth < 8) {
                self.base_into(t, out, depth + 1);
            }
        }
    }

    /// Las capas GDS que usan las reglas de esos tipos (cualquiera de sus nombres).
    pub fn type_layers(&self, types: &[String]) -> Vec<GdsLayer> {
        let wanted: Vec<&str> = types.iter().map(|t| self.canonical(t)).collect();
        let mut names = Vec::new();
        for (d, def) in self.defs.iter().enumerate() {
            if !def.temp && wanted.contains(&self.canonical(&def.name)) {
                self.collect_names(d, &mut names, &mut Vec::new());
            }
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
        self.devices_at(inside).last().map(|(_, t)| *t)
    }

    /// Todos los transistores cuya regla incluye el punto, en el orden de
    /// `cifinput`, con el índice de su `layer`.
    pub fn devices_at(&self, inside: &dyn Fn(GdsLayer) -> bool) -> Vec<(usize, &DeviceType)> {
        let mut memo = HashMap::new();
        self.devices.iter().filter(|(d, _)| self.eval_def(*d, None, inside, &mut memo, &mut Vec::new())).map(|(d, t)| (*d, t)).collect()
    }

    /// La regla de esa `layer` agranda o achica la región: en un punto no
    /// alcanza (`npd` de SKY130 es `npass` con `shrink 70`/`grow 70`: solo las
    /// compuertas de más de 0,14 µm).
    pub fn resizes(&self, def: usize) -> bool {
        self.defs[def].ops.iter().any(|o| matches!(o, Op::Grow(_) | Op::Shrink(_)))
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
                Op::CopyUp(_) | Op::Grow(_) | Op::Shrink(_) | Op::Ignored => {}
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

/// Lo que dicen las líneas `device` de un tipo de transistor.
#[derive(Default)]
struct Models {
    models: Vec<(String, Vec<Cond>)>,
    sd: Vec<String>,
    sub: Vec<String>,
    subckt: bool,
}

/// Modelos (con sus condiciones), tipos de fuente/drenaje y de sustrato de
/// cada tipo, por nombre canónico, según las líneas `device` de MOS de
/// `extract`: `device msubcircuit MODELO tipos sd [sd…] sustrato nodo…`.
fn device_models(lines: &[String], canonical: &HashMap<String, String>) -> HashMap<String, Models> {
    let canon = |n: &str| canonical.get(n).cloned().unwrap_or_else(|| n.to_string());
    let names = |t: &str| t.split(',').map(|t| canon(t.trim_start_matches('*').split('/').next().unwrap_or(""))).collect::<Vec<_>>();
    let mut out: HashMap<String, Models> = HashMap::new();
    for line in lines {
        let w: Vec<&str> = line.split_whitespace().collect();
        if w.first() != Some(&"device") || w.len() < 4 || !MOS_CLASSES.contains(&w[1]) {
            continue;
        }
        let conds: Vec<Cond> = w[4..].iter().filter_map(|t| Cond::parse(t)).collect();
        // Fuente y drenaje: el campo que sigue a los tipos, repetido una vez
        // por terminal (`*ndiff *ndiff`, `ndiff,ndc ndiff,ndc`); el siguiente
        // es el sustrato (`pwell,space/w`), si no es un nodo (`error`,
        // `$SUB`) ni un parámetro (`w>=0.42`, `l=l`).
        let sd_field = w.get(4).copied().filter(|t| !t.contains('=') && !t.contains('<') && !t.contains('>'));
        let sd: Vec<String> = sd_field.map(names).unwrap_or_default();
        let after = 4 + w[4..].iter().take_while(|t| Some(**t) == sd_field).count();
        let sub: Vec<String> = w
            .get(after)
            .filter(|t| sd_field.is_some() && !t.starts_with('$') && !t.contains('=') && !t.contains('<') && !t.contains('>') && **t != "error")
            .map(|t| names(t))
            .unwrap_or_default();
        for t in w[3].split(',') {
            let m = out.entry(canon(t)).or_default();
            // La misma línea repetida con otros terminales (npd en SKY130).
            if !m.models.iter().any(|(mm, c)| mm == w[2] && *c == conds) {
                m.models.push((w[2].to_string(), conds.clone()));
            }
            m.subckt |= w[1] == "msubcircuit";
            for s in &sd {
                if !m.sd.contains(s) {
                    m.sd.push(s.clone());
                }
            }
            for s in &sub {
                if !m.sub.contains(s) {
                    m.sub.push(s.clone());
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
    /// `templayer`, un marcador de bajo Vt y un `copyup` cíclico; y para las
    /// redes, difusión, poly, `locali` y `metal1` con sus contactos, pozo N
    /// y sustrato.
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
 active psubstratepdiff,psd
 active ndc,ndcontact2
end
contact
 ndc ndiff locali
 pdc pdiff locali
 psc psd locali
 pc poly locali
 mcon locali metal1
 stackable
end
aliases
 allnactivenonfet *ndiff
 allfets allnfets,pfet
 allnfets nfet,nfetlvt
end
connect
 nwell nwell
 pwell,*psd pwell,*psd
 allnactivenonfet allnactivenonfet
 *pdiff *pdiff
 *poly,allfets *poly,allfets
 *locali *locali
 *metal1 *metal1
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
 layer nwell NWELL
 labels NWELL
 templayer ndiffarea DIFF
 and-not POLY
 and NSDM
 labels DIFF
 layer ndiff ndiffarea
 layer pdiff DIFF
 and-not POLY
 and NWELL
 layer psd TAP
 and-not NWELL
 layer pwell DIFF,TAP
 and-not NWELL
 or SUBTXT
 labels SUBTXT text
 layer poly POLY
 layer ndc CONT
 and DIFF
 and NSDM
 layer pdc CONT
 and DIFF
 and NWELL
 layer psc CONT
 and TAP
 layer pc CONT
 and POLY
 layer locali LI
 labels LI
 labels LIPIN port
 layer mcon MCON
 layer metal1 MET1
 labels MET1TXT text
 calma DIFF 65 20
 calma TAP 65 44
 calma CONT 66 44
 calma LI 67 20
 calma LIPIN 67 16
 calma MCON 67 44
 calma MET1 68 20
 calma MET1TXT 68 5
 calma SUBTXT 64 59
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
 substrate *psd,space/w,pwell well $SUB -dnwell
 device msubcircuit mini__nfet nfet *ndiff *ndiff \
    pwell,space/w error w>=0.42 l=l w=w
 device msubcircuit mini__nfet_small nfet *ndiff *ndiff pwell error w<0.42
 device mosfet mini__nfet_lvt nfetlvt *ndiff
 device msubcircuit mini__pfet pfet *pdiff *pdiff nwell error
 device mosfet mini__nfet_gf nfetgf ndiff,ndc ndiff,ndc pwell error
 device resistor mini__res rpoly *poly
 device resistor None rm1 *metal1
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
    fn terminals_and_substrate_come_from_the_device_lines() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        let nfet = r.device_type("nfet").unwrap();
        assert_eq!((nfet.sd.as_slice(), nfet.sub.as_slice()), (&["ndiff".to_string()][..], &["pwell".to_string(), "space".to_string()][..]));
        assert!(nfet.subckt);
        let pfet = r.device_type("pfet").unwrap();
        assert_eq!(pfet.sub, ["nwell"]);
        // Como en GF180: fuente y drenaje sin `*`.
        let m = device_models(&["device mosfet m nfetgf ndiff,ndc ndiff,ndc pwell error".to_string()], &HashMap::new());
        let gf = &m["nfetgf"];
        assert_eq!((gf.sd.as_slice(), gf.sub.as_slice(), gf.subckt), (&["ndiff".to_string(), "ndc".to_string()][..], &["pwell".to_string()][..], false));
        let lvt = r.device_type("nfetlvt").unwrap();
        assert_eq!((lvt.sd.len(), lvt.sub.len()), (1, 0), "sin sustrato en la línea");
        // El orden de pintado y los planos.
        assert_eq!((r.plane("nmos"), r.plane("ndiffusion"), r.plane("metal1")), (Some("active"), Some("active"), None));
        assert!(r.layer_index("nfet") < r.layer_index("ndiff") && r.layer_index("ndiff") < r.layer_index("ndc"));
        assert_eq!(r.layer_index("nada"), None);
    }

    #[test]
    fn resistors_come_from_their_device_lines() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        let got: Vec<(&str, Option<&str>, bool)> = r.resistors.iter().map(|x| (x.magic.as_str(), x.model.as_deref(), x.subckt)).collect();
        assert_eq!(got, [("rpoly", Some("mini__res")), ("rm1", None)].map(|(m, x)| (m, x, false)));
        assert_eq!(r.resistors[0].terminals, ["pc", "poly"], "*poly: el poly y su contacto");
    }

    #[test]
    fn connectivity_expands_aliases_and_contacts() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        assert_eq!(r.contact_residues("mcon"), ["locali", "metal1"]);
        assert_eq!(r.contact_residues("ndcontact2"), ["ndiff", "locali"], "alias de types");
        assert!(r.contact_residues("metal1").is_empty());
        let group = |t: &str| r.connect.iter().find(|(a, _)| a.iter().any(|x| x == t)).map(|(a, _)| a.clone()).unwrap_or_default();
        // `*locali`: la capa y los contactos que la tocan.
        let mut li = group("locali");
        li.sort();
        assert_eq!(li, ["locali", "mcon", "ndc", "pc", "pdc", "psc"]);
        // `allnactivenonfet` → `*ndiff`; `allfets` → alias dentro de alias.
        assert_eq!(group("ndiff"), ["ndc", "ndiff"]);
        let mut poly = group("poly");
        poly.sort();
        assert_eq!(poly, ["nmos", "nmoslvt", "pc", "pmos", "poly"], "en nombres canónicos");
        assert!(r.conductors().contains(&"metal1".to_string()) && !r.conductors().contains(&"space".to_string()));
        assert_eq!(r.substrate, (vec!["psc".into(), "psubstratepdiff".into(), "space".into(), "pwell".into()], vec!["dnwell".into()]));
    }

    #[test]
    fn labels_belong_to_the_types_of_their_layer() {
        let r = DeviceRules::parse(TECH).expect("reglas");
        assert_eq!(r.label_types((67, 16)), [("locali".to_string(), true)]);
        assert_eq!(r.label_types((67, 20)), [("locali".to_string(), false)]);
        assert_eq!(r.label_types((68, 5)), [("metal1".to_string(), false)]);
        // Una etiqueta de una templayer: de los tipos que la usan como base.
        assert_eq!(r.label_types((65, 20)), [("ndiff".to_string(), false)]);
        assert_eq!(r.label_types((64, 59)), [("pwell".to_string(), false)]);
        assert!(r.label_types((99, 0)).is_empty());
        assert_eq!(r.port_layers(), [(67, Some(16))]);
        // Dónde se dibuja cada tipo: sin los marcadores (NSDM) ni lo que resta (POLY).
        assert_eq!(r.base_layers("ndiff"), [(65, Some(20))]);
        assert_eq!(r.base_layers("mcon"), [(67, Some(44))]);
    }

    #[test]
    fn without_transistors_there_are_no_rules() {
        assert!(DeviceRules::parse("tech\n x\nend\n").is_none());
        assert!(DeviceRules::parse("cifinput\nstyle a\n layer m1 M1\n calma M1 68 20\nend\n").is_none());
    }
}
