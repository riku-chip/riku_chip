//! Lo que `log --lvs` y `status --lvs` dicen del LVS de un par, sin depender
//! de cómo se calcula (`crate::lvs`, que necesita los módulos de Xschem y de
//! layouts): el veredicto, cómo cambió respecto de otra versión y qué
//! discrepancias aparecieron, se arreglaron o cambiaron de valor.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    /// Coinciden conectividad y parámetros.
    Match,
    /// La conectividad coincide; algún parámetro (W, L…) no.
    PropertyErrors,
    /// No coinciden: redes, dispositivos o pines.
    Mismatch,
}

impl Verdict {
    /// De mejor a peor: coincide, parámetros distintos, no coincide.
    pub fn severity(self) -> u8 {
        match self {
            Verdict::Match => 0,
            Verdict::PropertyErrors => 1,
            Verdict::Mismatch => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Transition {
    /// Coincidía y dejó de coincidir.
    Broke,
    /// Tenía solo parámetros distintos y ahora tampoco coinciden las
    /// conexiones (un corto, un abierto).
    Worse,
    /// Las conexiones vuelven a coincidir, con parámetros distintos.
    Better,
    /// Volvió a coincidir.
    Fixed,
}

impl Transition {
    /// De `before` a `now`; `None` si el veredicto no cambió.
    pub fn between(before: Verdict, now: Verdict) -> Option<Self> {
        match (before, now) {
            (b, v) if b == v => None,
            (Verdict::Match, _) => Some(Transition::Broke),
            (_, Verdict::Match) => Some(Transition::Fixed),
            (b, v) if v.severity() > b.severity() => Some(Transition::Worse),
            _ => Some(Transition::Better),
        }
    }

    /// Empeoró: lo que un hook o la CI deben frenar.
    pub fn is_regression(self) -> bool {
        matches!(self, Transition::Broke | Transition::Worse)
    }
}

/// De qué lado está un pin que no tiene pareja.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Side {
    Schematic,
    Layout,
}

/// Algo que no coincide, con los nombres del esquemático como identidad: los
/// del layout son índices que se corren cuando se agrega un transistor.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Discrepancy {
    /// Un parámetro distinto de un dispositivo emparejado.
    Property { instance: String, layout_instance: String, model: String, param: String, schematic: String, layout: String },
    /// Redes que no se pudieron emparejar.
    Nets { schematic: Vec<String>, layout: Vec<String> },
    /// Dispositivos que no se pudieron emparejar.
    Devices { schematic: Vec<String>, layout: Vec<String> },
    /// Un pin que está de un solo lado.
    Pin { name: String, only_in: Side },
}

impl Discrepancy {
    /// La misma discrepancia en otra versión tiene la misma clave.
    pub fn key(&self) -> String {
        let names = |tag: &str, s: &[String], l: &[String]| {
            let (side, v) = if s.is_empty() { ("L", l) } else { ("", s) };
            let mut v = v.to_vec();
            v.sort();
            format!("{tag}{side}:{}", v.join(","))
        };
        match self {
            Discrepancy::Property { instance, param, .. } => format!("P:{instance}:{param}"),
            Discrepancy::Nets { schematic, layout } => names("N", schematic, layout),
            Discrepancy::Devices { schematic, layout } => names("D", schematic, layout),
            Discrepancy::Pin { name, only_in } => format!("pin:{only_in:?}:{name}"),
        }
    }

    /// Error del layout respecto del esquemático (que es la intención), si
    /// los dos valores son números: `(layout − esquemático) / esquemático`.
    pub fn relative_error(&self) -> Option<f64> {
        let Discrepancy::Property { schematic, layout, .. } = self else { return None };
        let (s, l) = (schematic.trim().parse::<f64>().ok()?, layout.trim().parse::<f64>().ok()?);
        (s != 0.0 && s.is_finite() && l.is_finite()).then(|| (l - s) / s)
    }
}

/// Una discrepancia que siguió pero con otros valores (`w 18 ≠ 20` → `18 ≠ 19`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Changed {
    pub before: Discrepancy,
    pub now: Discrepancy,
}

/// Qué cambió entre dos resultados del mismo par.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub appeared: Vec<Discrepancy>,
    pub fixed: Vec<Discrepancy>,
    pub changed: Vec<Changed>,
}

impl Delta {
    /// De `before` a `now`, ordenado por clave (no depende del orden en que
    /// Netgen lista las cosas).
    pub fn between(before: &[Discrepancy], now: &[Discrepancy]) -> Self {
        let by_key = |v: &[Discrepancy]| -> BTreeMap<String, Discrepancy> { v.iter().map(|d| (d.key(), d.clone())).collect() };
        let (b, n) = (by_key(before), by_key(now));
        let mut out = Delta::default();
        for (k, d) in &n {
            match b.get(k) {
                None => out.appeared.push(d.clone()),
                Some(old) if !same_values(old, d) => out.changed.push(Changed { before: old.clone(), now: d.clone() }),
                Some(_) => {}
            }
        }
        out.fixed = b.iter().filter(|(k, _)| !n.contains_key(*k)).map(|(_, d)| d.clone()).collect();
        out
    }

    /// Entre dos resultados con su veredicto. Si en alguno no coinciden las
    /// conexiones, Netgen no llega a comparar parámetros: los parámetros no
    /// cuentan (si no, un corto "arreglaría" todos los W distintos y quitar
    /// el corto los haría "aparecer").
    pub fn between_results(before: (Verdict, &[Discrepancy]), now: (Verdict, &[Discrepancy])) -> Self {
        let connections_only = before.0 == Verdict::Mismatch || now.0 == Verdict::Mismatch;
        let keep = |v: &[Discrepancy]| -> Vec<Discrepancy> {
            v.iter().filter(|d| !(connections_only && matches!(d, Discrepancy::Property { .. }))).cloned().collect()
        };
        Self::between(&keep(before.1), &keep(now.1))
    }

    pub fn is_empty(&self) -> bool {
        self.appeared.is_empty() && self.fixed.is_empty() && self.changed.is_empty()
    }

    pub fn len(&self) -> usize {
        self.appeared.len() + self.fixed.len() + self.changed.len()
    }
}

/// Mismos valores, sin mirar el nombre del lado del layout (un índice que se
/// corre sin que cambie nada).
fn same_values(a: &Discrepancy, b: &Discrepancy) -> bool {
    match (a, b) {
        (
            Discrepancy::Property { schematic: s1, layout: l1, model: m1, .. },
            Discrepancy::Property { schematic: s2, layout: l2, model: m2, .. },
        ) => s1 == s2 && l1 == l2 && m1 == m2,
        (Discrepancy::Nets { layout: a, .. }, Discrepancy::Nets { layout: b, .. })
        | (Discrepancy::Devices { layout: a, .. }, Discrepancy::Devices { layout: b, .. }) => {
            let (mut a, mut b) = (a.clone(), b.clone());
            a.sort();
            b.sort();
            a == b
        }
        _ => a == b,
    }
}

/// El LVS de un par en una versión.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum LvsState {
    Done {
        verdict: Verdict,
    },
    /// El esquemático o el layout no están en esa versión.
    Missing,
    Error {
        error: String,
    },
}

impl LvsState {
    pub fn verdict(&self) -> Option<Verdict> {
        match self {
            LvsState::Done { verdict } => Some(*verdict),
            _ => None,
        }
    }
}

/// El LVS de un par en un commit de `riku log --lvs`, respecto de su primer padre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairLvs {
    pub schematic: String,
    pub layout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    #[serde(flatten)]
    pub state: LvsState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    /// `None`: el padre (o este commit) no tiene resultado para comparar.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    /// Todas las discrepancias de esta versión (solo con `--full`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discrepancies: Vec<Discrepancy>,
}

/// El LVS de un par en `riku status --lvs`: `HEAD` contra el working tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PairStatusLvs {
    pub schematic: String,
    pub layout: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cell: Option<String>,
    pub head: LvsState,
    pub worktree: LvsState,
    /// El par no cambió desde `HEAD` (mismo resultado).
    pub unchanged: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition: Option<Transition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta: Option<Delta>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub discrepancies: Vec<Discrepancy>,
}

/// El código de salida de `status --lvs` (0, 1 o 2) y si hay que avisar
/// que aparecieron discrepancias nuevas sin que empeorara el veredicto.
pub fn status_outcome(pairs: &[PairStatusLvs]) -> (u8, bool) {
    if pairs.iter().any(|p| matches!(p.worktree, LvsState::Error { .. })) {
        return (2, false);
    }
    if pairs.iter().any(|p| p.transition.is_some_and(Transition::is_regression)) {
        return (1, false);
    }
    let new = pairs.iter().any(|p| p.delta.as_ref().is_some_and(|d| !d.appeared.is_empty()));
    (0, new)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn prop(inst: &str, param: &str, s: &str, l: &str) -> Discrepancy {
        Discrepancy::Property {
            instance: inst.into(),
            layout_instance: "19".into(),
            model: "pfet".into(),
            param: param.into(),
            schematic: s.into(),
            layout: l.into(),
        }
    }

    fn nets(s: &[&str], l: &[&str]) -> Discrepancy {
        Discrepancy::Nets {
            schematic: s.iter().map(|x| x.to_string()).collect(),
            layout: l.iter().map(|x| x.to_string()).collect(),
        }
    }

    #[test]
    fn transitions_between_verdicts() {
        use Transition::*;
        use Verdict::*;
        for (b, n, want) in [
            (Match, Match, None),
            (Match, PropertyErrors, Some(Broke)),
            (Match, Mismatch, Some(Broke)),
            (PropertyErrors, Mismatch, Some(Worse)),
            (Mismatch, PropertyErrors, Some(Better)),
            (PropertyErrors, Match, Some(Fixed)),
            (Mismatch, Match, Some(Fixed)),
            (PropertyErrors, PropertyErrors, None),
        ] {
            assert_eq!(Transition::between(b, n), want, "{b:?} → {n:?}");
        }
        assert!(Broke.is_regression() && Worse.is_regression() && !Better.is_regression() && !Fixed.is_regression());
    }

    #[test]
    fn keys_use_the_schematic_names() {
        assert_eq!(prop("M3", "w", "18", "19").key(), "P:M3:w");
        assert_eq!(nets(&["Vp", "Vout"], &["Vout"]).key(), "N:Vout,Vp", "ordenados");
        assert_eq!(nets(&[], &["n12"]).key(), "NL:n12", "sin nombres del esquemático, los del layout");
        assert_eq!(Discrepancy::Pin { name: "Ib".into(), only_in: Side::Schematic }.key(), "pin:Schematic:Ib");
    }

    #[test]
    fn delta_says_what_appeared_was_fixed_or_changed() {
        let before = [prop("M3", "w", "18", "20"), prop("M1", "w", "4", "2"), nets(&["Vout", "Vp"], &["Vout"])];
        let now = [prop("M1", "w", "4", "2"), prop("M3", "w", "18", "19"), prop("M8", "w", "20", "19")];
        let d = Delta::between(&before, &now);
        assert_eq!(d.appeared, [prop("M8", "w", "20", "19")]);
        assert_eq!(d.fixed, [nets(&["Vout", "Vp"], &["Vout"])]);
        assert_eq!(d.changed, [Changed { before: prop("M3", "w", "18", "20"), now: prop("M3", "w", "18", "19") }]);
        assert_eq!(d.len(), 3);
        // El orden en que vienen no importa.
        let mut rev_before = before.to_vec();
        rev_before.reverse();
        let mut rev_now = now.to_vec();
        rev_now.reverse();
        assert_eq!(Delta::between(&rev_before, &rev_now), d);
        // El índice del layout que se corre no es un cambio.
        let mut moved = prop("M1", "w", "4", "2");
        if let Discrepancy::Property { layout_instance, .. } = &mut moved {
            *layout_instance = "21".into();
        }
        assert!(Delta::between(&[prop("M1", "w", "4", "2")], &[moved]).is_empty());
    }

    #[test]
    fn with_a_short_the_parameters_are_not_compared() {
        let props = [prop("M1", "w", "4", "2"), prop("M2", "w", "4", "2")];
        let short = [nets(&["Vout", "Vp"], &["Vout"])];
        // Corto: Netgen no da los parámetros; no se "arreglan".
        let broke = Delta::between_results((Verdict::PropertyErrors, &props), (Verdict::Mismatch, &short));
        assert_eq!((broke.appeared.len(), broke.fixed.len()), (1, 0), "{broke:?}");
        // Sin el corto vuelven; no "aparecen".
        let fixed = Delta::between_results((Verdict::Mismatch, &short), (Verdict::PropertyErrors, &props));
        assert_eq!((fixed.appeared.len(), fixed.fixed.len()), (0, 1), "{fixed:?}");
        // Sin corto, sí cuentan.
        assert_eq!(Delta::between_results((Verdict::Match, &[]), (Verdict::PropertyErrors, &props)).appeared.len(), 2);
    }

    #[test]
    fn relative_error_of_the_layout_against_the_schematic() {
        let pct = |s: &str, l: &str| prop("M1", "w", s, l).relative_error().map(|e| (e * 1000.0).round() / 10.0);
        assert_eq!(pct("4", "2"), Some(-50.0));
        assert_eq!(pct("18", "19"), Some(5.6));
        assert_eq!(pct("1e-06", "2e-06"), Some(100.0));
        assert_eq!(pct("0", "2"), None);
        assert_eq!(pct("a", "2"), None);
        assert_eq!(nets(&["A"], &[]).relative_error(), None);
    }

    #[test]
    fn status_exit_code_blocks_regressions_and_warns_new_ones() {
        let pair = |head: Verdict, wt: LvsState, appeared: usize| PairStatusLvs {
            schematic: "a.sch".into(),
            layout: "a.gds".into(),
            cell: None,
            head: LvsState::Done { verdict: head },
            transition: wt.verdict().and_then(|v| Transition::between(head, v)),
            worktree: wt,
            unchanged: false,
            delta: Some(Delta { appeared: vec![prop("M9", "w", "1", "2"); appeared], ..Default::default() }),
            discrepancies: Vec::new(),
        };
        let done = |v| LvsState::Done { verdict: v };
        assert_eq!(status_outcome(&[pair(Verdict::Match, done(Verdict::Match), 0)]), (0, false));
        assert_eq!(status_outcome(&[pair(Verdict::Match, done(Verdict::PropertyErrors), 1)]), (1, false), "dejó de coincidir");
        assert_eq!(status_outcome(&[pair(Verdict::PropertyErrors, done(Verdict::Mismatch), 1)]), (1, false), "empeoró");
        assert_eq!(
            status_outcome(&[pair(Verdict::PropertyErrors, done(Verdict::PropertyErrors), 1)]),
            (0, true),
            "igual de mal, con nuevas: aviso"
        );
        assert_eq!(status_outcome(&[pair(Verdict::Mismatch, done(Verdict::PropertyErrors), 0)]), (0, false), "mejoró");
        assert_eq!(status_outcome(&[pair(Verdict::Match, LvsState::Error { error: "x".into() }, 0)]), (2, false));
    }
}
