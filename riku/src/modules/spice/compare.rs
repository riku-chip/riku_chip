//! Comparación de formas de onda entre dos simulaciones.
//!
//! Dos corridas no usan los mismos pasos de tiempo (ngspice ajusta el paso
//! según la actividad del circuito), así que las curvas se comparan sobre
//! la unión de las dos grillas, interpolando linealmente cada una. Se mide
//! el error máximo (y dónde ocurre) y el RMS ponderado por el paso; el
//! cambio es cosmético si el máximo queda bajo la tolerancia, relativa al
//! rango de la señal.

use super::raw::{Plot, RawFile, Variable};

/// Tolerancia de la comparación.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tolerance {
    /// Fracción del rango de la señal (0.001 = 0,1 %).
    pub rel: f64,
    /// Piso absoluto, para señales casi constantes (ruido numérico).
    pub abs: f64,
}

impl Default for Tolerance {
    fn default() -> Self {
        Self { rel: 1e-3, abs: 1e-12 }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Added,
    Removed,
    /// Presente en las dos; ver `within_tolerance`.
    Compared,
    /// Presente en las dos pero sin eje común (barridos sin orden, rangos
    /// que no se solapan).
    Incomparable,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SignalDiff {
    pub plot: String,
    pub name: String,
    pub unit: &'static str,
    pub x_unit: &'static str,
    pub status: Status,
    /// |B − A| máximo.
    pub max_abs: f64,
    /// Valor del eje X donde ocurre el máximo.
    pub at_x: f64,
    pub rms: f64,
    /// Rango (máx − mín) de la señal, sobre las dos versiones.
    pub range: f64,
    pub within_tolerance: bool,
}

impl SignalDiff {
    /// Error máximo relativo al rango (0 si la señal es constante).
    pub fn rel(&self) -> f64 {
        if self.range > 0.0 { self.max_abs / self.range } else { 0.0 }
    }
}

/// Un análisis emparejado entre las dos versiones.
#[derive(Clone, Debug, PartialEq)]
pub struct PlotDiff {
    pub name: String,
    /// Rango del eje X en A y en B (si cambió la duración de la simulación).
    pub x_range: (Option<(f64, f64)>, Option<(f64, f64)>),
    pub signals: Vec<SignalDiff>,
}

/// Empareja los plots de A y B por nombre (y orden de aparición entre los
/// del mismo nombre): primero los de B, después los que solo están en A.
pub fn pair_plots(a: &RawFile, b: &RawFile) -> Vec<(Option<usize>, Option<usize>)> {
    let key = |plots: &[Plot]| -> Vec<(String, usize)> {
        let mut seen = std::collections::HashMap::new();
        plots
            .iter()
            .map(|p| {
                let n = seen.entry(p.name.clone()).or_insert(0usize);
                *n += 1;
                (p.name.clone(), *n)
            })
            .collect()
    };
    let (ka, kb) = (key(&a.plots), key(&b.plots));
    let mut out: Vec<_> = kb.iter().enumerate().map(|(i, k)| (ka.iter().position(|x| x == k), Some(i))).collect();
    out.extend(ka.iter().enumerate().filter(|(_, k)| !kb.contains(k)).map(|(j, _)| (Some(j), None)));
    out
}

/// Compara señal por señal cada análisis emparejado (ver [`pair_plots`]).
pub fn compare(a: &RawFile, b: &RawFile, tol: Tolerance) -> Vec<PlotDiff> {
    pair_plots(a, b)
        .into_iter()
        .map(|(ia, ib)| compare_plot(ia.map(|i| &a.plots[i]), ib.map(|i| &b.plots[i]), tol))
        .collect()
}

fn x_range(p: Option<&Plot>) -> Option<(f64, f64)> {
    let xs = &p?.x()?.values;
    Some((*xs.first()?, *xs.last()?))
}

pub fn compare_plot(a: Option<&Plot>, b: Option<&Plot>, tol: Tolerance) -> PlotDiff {
    let name = b.or(a).map(|p| p.name.clone()).unwrap_or_default();
    let mut signals = Vec::new();
    let complex = b.or(a).is_some_and(|p| p.complex);
    let x_unit = b.or(a).and_then(Plot::x).map_or("", |x| x.unit(complex));
    let blank = |v: &Variable, status| SignalDiff {
        plot: name.clone(),
        name: v.name.clone(),
        unit: v.unit(complex),
        x_unit,
        status,
        max_abs: 0.0,
        at_x: 0.0,
        rms: 0.0,
        range: 0.0,
        within_tolerance: false,
    };
    if let Some(pb) = b {
        for sb in pb.signals() {
            match a.and_then(|pa| pa.signal(&sb.name).map(|sa| (pa, sa))) {
                Some((pa, sa)) => {
                    let mut d = blank(sb, Status::Compared);
                    match compare_signal(pa.x().map(|x| &x.values[..]), &sa.values, pb.x().map(|x| &x.values[..]), &sb.values) {
                        Some(m) => {
                            d.max_abs = m.max_abs;
                            d.at_x = m.at_x;
                            d.rms = m.rms;
                            d.range = m.range;
                            d.within_tolerance = m.max_abs <= (tol.rel * m.range).max(tol.abs);
                        }
                        None => d.status = Status::Incomparable,
                    }
                    signals.push(d);
                }
                None => signals.push(blank(sb, Status::Added)),
            }
        }
    }
    if let Some(pa) = a {
        for sa in pa.signals() {
            if b.and_then(|pb| pb.signal(&sa.name)).is_none() {
                signals.push(blank(sa, Status::Removed));
            }
        }
    }
    PlotDiff { name, x_range: (x_range(a), x_range(b)), signals }
}

struct Metrics {
    max_abs: f64,
    at_x: f64,
    rms: f64,
    range: f64,
}

fn increasing(xs: &[f64]) -> bool {
    xs.windows(2).all(|w| w[1] >= w[0])
}

fn range_of(vals: &[f64]) -> (f64, f64) {
    vals.iter().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| (lo.min(v), hi.max(v)))
}

/// Interpolación lineal de `(xs, ys)` en `x` (`xs` creciente, `x` dentro).
pub fn interp(xs: &[f64], ys: &[f64], x: f64) -> f64 {
    let i = xs.partition_point(|&v| v < x);
    if i == 0 {
        return ys[0];
    }
    if i >= xs.len() {
        return ys[xs.len() - 1];
    }
    let (x0, x1) = (xs[i - 1], xs[i]);
    if x1 == x0 {
        return ys[i];
    }
    ys[i - 1] + (ys[i] - ys[i - 1]) * (x - x0) / (x1 - x0)
}

fn compare_signal(xa: Option<&[f64]>, ya: &[f64], xb: Option<&[f64]>, yb: &[f64]) -> Option<Metrics> {
    let (xa, xb) = (xa?, xb?);
    if ya.is_empty() || yb.is_empty() || xa.len() != ya.len() || xb.len() != yb.len() {
        return None;
    }
    let (lo_a, hi_a) = range_of(ya);
    let (lo_b, hi_b) = range_of(yb);
    let range = hi_a.max(hi_b) - lo_a.min(lo_b);

    // Punto de operación (un solo punto) o barridos con el mismo eje sin
    // orden creciente: comparar punto a punto.
    let pointwise = ya.len() == 1 && yb.len() == 1 || (!(increasing(xa) && increasing(xb)) && xa == xb);
    if pointwise {
        // Un punto de operación no tiene "rango": la escala es su magnitud.
        let range = if ya.len() == 1 { ya[0].abs().max(yb[0].abs()) } else { range };
        let (mut max_abs, mut at_x, mut sum) = (0.0f64, xb[0], 0.0);
        for i in 0..ya.len() {
            let e = (yb[i] - ya[i]).abs();
            sum += e * e;
            if e > max_abs {
                max_abs = e;
                at_x = xb[i];
            }
        }
        return Some(Metrics { max_abs, at_x, rms: (sum / ya.len() as f64).sqrt(), range });
    }
    if !(increasing(xa) && increasing(xb)) {
        return None;
    }
    // Unión de las grillas dentro del tramo común.
    let lo = xa[0].max(xb[0]);
    let hi = xa[xa.len() - 1].min(xb[xb.len() - 1]);
    if hi < lo {
        return None;
    }
    let mut grid: Vec<f64> = xa.iter().chain(xb.iter()).copied().filter(|&x| x >= lo && x <= hi).collect();
    grid.sort_by(f64::total_cmp);
    grid.dedup();
    if grid.is_empty() {
        return None;
    }
    let (mut max_abs, mut at_x) = (0.0f64, grid[0]);
    let mut prev: Option<(f64, f64)> = None;
    let (mut integral, mut span) = (0.0, 0.0);
    for &x in &grid {
        let e = interp(xb, yb, x) - interp(xa, ya, x);
        if e.abs() > max_abs {
            max_abs = e.abs();
            at_x = x;
        }
        if let Some((px, pe)) = prev {
            let dx = x - px;
            // Trapecio de e².
            integral += dx * (pe * pe + e * e) / 2.0;
            span += dx;
        }
        prev = Some((x, e));
    }
    let rms = if span > 0.0 { (integral / span).sqrt() } else { max_abs };
    Some(Metrics { max_abs, at_x, rms, range })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plot(name: &str, x: Vec<f64>, sigs: &[(&str, Vec<f64>)]) -> Plot {
        let mut vars = vec![Variable { name: "time".into(), kind: "time".into(), values: x }];
        for (n, v) in sigs {
            vars.push(Variable { name: n.to_string(), kind: "voltage".into(), values: v.clone() });
        }
        Plot { title: String::new(), name: name.into(), command: None, complex: false, vars }
    }

    #[test]
    fn identical_runs_are_within_tolerance() {
        let p = plot("Transient Analysis", vec![0.0, 1.0, 2.0], &[("v(out)", vec![0.0, 1.0, 0.0])]);
        let d = compare(&RawFile { plots: vec![p.clone()] }, &RawFile { plots: vec![p] }, Tolerance::default());
        let s = &d[0].signals[0];
        assert_eq!(s.status, Status::Compared);
        assert!(s.within_tolerance && s.max_abs == 0.0);
    }

    #[test]
    fn different_grids_are_interpolated() {
        // Misma recta muestreada distinto: sin diferencia.
        let a = plot("T", vec![0.0, 2.0], &[("v", vec![0.0, 2.0])]);
        let b = plot("T", vec![0.0, 0.5, 1.0, 2.0], &[("v", vec![0.0, 0.5, 1.0, 2.0])]);
        let s = &compare_plot(Some(&a), Some(&b), Tolerance::default()).signals[0];
        assert!(s.max_abs < 1e-12, "{s:?}");
    }

    #[test]
    fn a_bump_is_found_where_it_happens() {
        let a = plot("T", vec![0.0, 1.0, 2.0, 3.0], &[("v", vec![0.0, 0.0, 0.0, 1.0])]);
        let b = plot("T", vec![0.0, 1.0, 2.0, 3.0], &[("v", vec![0.0, 0.3, 0.0, 1.0])]);
        let s = &compare_plot(Some(&a), Some(&b), Tolerance::default()).signals[0];
        assert!((s.max_abs - 0.3).abs() < 1e-12 && s.at_x == 1.0);
        assert!(!s.within_tolerance);
        assert!((s.rel() - 0.3).abs() < 1e-12);
    }

    #[test]
    fn added_removed_and_unmatched_plots() {
        let a = RawFile { plots: vec![plot("T", vec![0.0, 1.0], &[("v(a)", vec![0.0, 1.0])])] };
        let b = RawFile {
            plots: vec![
                plot("T", vec![0.0, 1.0], &[("v(b)", vec![0.0, 1.0])]),
                plot("Operating Point", vec![0.0], &[("v(b)", vec![1.8])]),
            ],
        };
        let d = compare(&a, &b, Tolerance::default());
        assert_eq!(d.len(), 2);
        let st: Vec<_> = d[0].signals.iter().map(|s| (s.name.as_str(), s.status)).collect();
        assert_eq!(st, vec![("v(b)", Status::Added), ("v(a)", Status::Removed)]);
        assert_eq!(d[1].signals[0].status, Status::Added);
    }

    #[test]
    fn operating_point_compares_directly() {
        let a = plot("Operating Point", vec![0.0], &[("v(a)", vec![1.0])]);
        let b = plot("Operating Point", vec![0.0], &[("v(a)", vec![1.25])]);
        let s = &compare_plot(Some(&a), Some(&b), Tolerance::default()).signals[0];
        assert_eq!(s.status, Status::Compared);
        assert!((s.max_abs - 0.25).abs() < 1e-12);
    }
}
