//! Señales calculadas: evalúa expresiones ([`expr`](super::expr)) en cada
//! análisis de las dos versiones y las compara como una señal más. Lo usan
//! el diff de la CLI y el visor.

use super::compare::{compare_scalar, compare_series, pair_plots, SignalDiff, Status, Tolerance};
use super::expr::{Evaluated, ExprError, Expression};
use super::raw::{Plot, RawFile};

/// Una expresión evaluada en un análisis emparejado.
#[derive(Clone, Debug)]
pub struct Derived {
    /// Índice del par en [`pair_plots`].
    pub pair: usize,
    pub a: Option<Evaluated>,
    pub b: Option<Evaluated>,
    pub diff: SignalDiff,
}

/// Evalúa `exprs` en cada par de análisis de `a` y `b`. Una expresión se
/// aplica a los análisis que tienen sus señales (y al de su prefijo,
/// `tran: …`) y donde da algún valor válido: `v(out)/v(in)` no se compara en
/// un punto de operación con `v(in) = 0`. Solo se avisa de una expresión que
/// no se pudo aplicar a ningún análisis.
pub fn evaluate(exprs: &[Expression], a: &RawFile, b: &RawFile, tol: Tolerance) -> (Vec<Derived>, Vec<String>) {
    let mut out = Vec::new();
    // Por expresión: si se aplicó a algún análisis y el primer problema.
    let mut used = vec![false; exprs.len()];
    let mut problem: Vec<Option<ExprError>> = vec![None; exprs.len()];
    for (pair, (ia, ib)) in pair_plots(a, b).into_iter().enumerate() {
        let (pa, pb) = (ia.map(|i| &a.plots[i]), ib.map(|i| &b.plots[i]));
        let plot_name = pb.or(pa).map(|p| p.name.clone()).unwrap_or_default();
        for (k, e) in exprs.iter().enumerate() {
            if !e.applies_to(&plot_name) {
                continue;
            }
            let mut run = |p: Option<&Plot>| match p.map(|p| e.eval(p)) {
                Some(Ok(v)) if has_values(&v) => Some(v),
                Some(Ok(_)) | None => None,
                Some(Err(err)) => {
                    // Un error de cálculo dice más que una señal que falta.
                    if problem[k].is_none() || matches!(problem[k], Some(ExprError::Missing(_))) {
                        problem[k] = Some(err);
                    }
                    None
                }
            };
            let (va, vb) = (run(pa), run(pb));
            if va.is_none() && vb.is_none() {
                continue;
            }
            used[k] = true;
            let mut diff = compare(&plot_name, e, pa, va.as_ref(), pb, vb.as_ref(), tol);
            diff.expression = Some(e.text.clone());
            out.push(Derived { pair, a: va, b: vb, diff });
        }
    }
    let warnings = exprs
        .iter()
        .zip(used.iter().zip(problem))
        .filter(|(_, (used, _))| !**used)
        .map(|(e, (_, p))| match p {
            Some(err) => format!("{}: {err}", e.name),
            None => format!("{}: no da valores en ningún análisis", e.name),
        })
        .collect();
    (out, warnings)
}

/// `true` si el resultado tiene al menos un valor finito.
fn has_values(v: &Evaluated) -> bool {
    match v {
        Evaluated::Signal(var) => var.values.iter().any(|x| x.is_finite()),
        Evaluated::Scalar(x, _) => x.is_finite(),
    }
}

fn compare(
    plot_name: &str,
    e: &Expression,
    pa: Option<&Plot>,
    va: Option<&Evaluated>,
    pb: Option<&Plot>,
    vb: Option<&Evaluated>,
    tol: Tolerance,
) -> SignalDiff {
    let complex = pb.or(pa).is_some_and(|p| p.complex);
    let x_unit = pb.or(pa).and_then(Plot::x).map_or("", |x| x.unit(complex));
    fn series<'a>(p: Option<&'a Plot>, v: &'a Evaluated) -> Option<(&'a [f64], &'a [f64])> {
        match (p, v) {
            (Some(p), Evaluated::Signal(var)) => Some((p.x().map_or(&[][..], |x| &x.values[..]), &var.values[..])),
            _ => None,
        }
    }
    let scalar = |v: &Evaluated| match v {
        Evaluated::Scalar(x, _) => Some(*x),
        Evaluated::Signal(_) => None,
    };
    match (va, vb) {
        (Some(Evaluated::Signal(sa)), Some(Evaluated::Signal(_))) | (Some(Evaluated::Signal(sa)), None) => {
            compare_series(plot_name, &e.name, sa.unit(complex), x_unit, series(pa, va.unwrap()), vb.and_then(|v| series(pb, v)), tol)
        }
        (None, Some(Evaluated::Signal(sb))) => compare_series(plot_name, &e.name, sb.unit(complex), x_unit, None, series(pb, vb.unwrap()), tol),
        (Some(Evaluated::Scalar(_, u)), Some(Evaluated::Scalar(..)))
        | (Some(Evaluated::Scalar(_, u)), None)
        | (None, Some(Evaluated::Scalar(_, u))) => compare_scalar(plot_name, &e.name, u, va.and_then(scalar), vb.and_then(scalar), tol),
        // Señal en una versión y número en la otra: no se pueden comparar.
        _ => {
            let mut d = compare_scalar(plot_name, &e.name, "", None, None, tol);
            d.status = Status::Incomparable;
            d
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modules::spice::expr::parse;
    use crate::modules::spice::raw::Variable;

    fn raw(vin: Vec<f64>, vout: Vec<f64>) -> RawFile {
        let v = |n: &str, k: &str, vals: Vec<f64>| Variable { name: n.into(), kind: k.into(), values: vals, complex: None };
        RawFile {
            plots: vec![Plot {
                title: String::new(),
                name: "Transient Analysis".into(),
                command: None,
                complex: false,
                vars: vec![v("time", "time", vec![0.0, 1.0, 2.0]), v("v(in)", "voltage", vin), v("v(out)", "voltage", vout)],
            }],
        }
    }

    #[test]
    fn gain_changes_are_found_and_missing_signals_are_skipped() {
        let a = raw(vec![1.0, 1.0, 1.0], vec![0.5, 0.5, 0.5]);
        let b = raw(vec![1.0, 1.0, 1.0], vec![0.5, 0.6, 0.5]);
        let exprs = vec![parse("gain = v(out)/v(in)").unwrap(), parse("peak = max(v(out))").unwrap(), parse("v(nada)*2").unwrap()];
        let (d, warnings) = evaluate(&exprs, &a, &b, Tolerance::default());
        assert_eq!(d.len(), 2, "la de v(nada) no aplica a este análisis");
        assert_eq!(warnings, vec!["v(nada)*2: no existe la señal v(nada)".to_string()]);
        let gain = &d[0].diff;
        assert_eq!((gain.name.as_str(), gain.status), ("gain", Status::Compared));
        assert!((gain.max_abs - 0.1).abs() < 1e-12 && gain.at_x == 1.0 && !gain.within_tolerance);
        assert_eq!(gain.expression.as_deref(), Some("v(out)/v(in)"));
        let peak = &d[1].diff;
        assert_eq!(peak.scalar, Some((Some(0.5), Some(0.6))));
        assert_eq!(peak.unit, "V");
    }

    #[test]
    fn no_valid_values_means_not_applicable() {
        // v(in) = 0 en todos los puntos: la división no da ningún valor.
        let a = raw(vec![0.0; 3], vec![1.0; 3]);
        let (d, warnings) = evaluate(&[parse("v(out)/v(in)").unwrap()], &a, &a, Tolerance::default());
        assert!(d.is_empty());
        assert_eq!(warnings.len(), 1);
        // Limitada a otro análisis: no se evalúa.
        let (d, _) = evaluate(&[parse("ac: v(out)").unwrap()], &a, &a, Tolerance::default());
        assert!(d.is_empty());
    }

    #[test]
    fn eval_errors_become_warnings() {
        let a = raw(vec![1.0; 3], vec![1.0; 3]);
        let (d, warnings) = evaluate(&[parse("v(out)[7]").unwrap()], &a, &a, Tolerance::default());
        assert!(d.is_empty());
        assert_eq!(warnings.len(), 1, "{warnings:?}");
    }
}
