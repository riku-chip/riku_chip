//! Formas de onda (`.raw`) como SVG: un gráfico por unidad con B continua y
//! A punteada y, en un diff, el error B − A debajo de cada uno (como la
//! vista de la GUI). Muestra las señales pedidas (`--signal`) o, si no, las
//! expresiones que den una curva y las señales que más cambiaron (o las
//! primeras, sin comparación), hasta [`MAX_SIGNALS`].

use std::fmt::Write;

use super::svg::Style;
use crate::i18n::tr;
use crate::modules::spice::compare::{compare_plot, interp, pair_plots, SignalDiff, Status, Tolerance};
use crate::modules::spice::derived;
use crate::modules::spice::expr::{self, Evaluated};
use crate::modules::spice::raw::{Plot, RawFile, Variable};
use crate::text::eng;

const MAX_SIGNALS: usize = 3;
const PALETTE: [&str; 6] = ["#4e9cf5", "#f58f3b", "#3cc47c", "#e04f6a", "#a77bf0", "#d4b83a"];

/// Una curva a dibujar: nombre, unidad, B y (en diff) A.
struct Curve<'a> {
    name: String,
    unit: &'static str,
    b: Option<(&'a [f64], &'a [f64])>,
    a: Option<(&'a [f64], &'a [f64])>,
}

fn is_internal(name: &str) -> bool {
    name.contains('#') || name.contains('.') || name.starts_with('@')
}

fn changed(d: &SignalDiff) -> bool {
    match d.status {
        Status::Compared => !d.within_tolerance,
        _ => true,
    }
}

/// Qué dibujar de un `.raw`: señales por nombre (sin distinguir mayúsculas)
/// y el análisis (`tran`, `ac`…). Vacío = lo que elija la imagen.
#[derive(Clone, Copy, Debug, Default)]
pub struct Pick<'a> {
    pub signals: &'a [String],
    pub analysis: Option<&'a str>,
}

pub fn wave_svg(
    before: Option<&RawFile>,
    after: &RawFile,
    expressions: &[String],
    tol: Tolerance,
    pick: Pick<'_>,
    style: &Style,
) -> Result<String, String> {
    let empty = RawFile { plots: Vec::new() };
    let a_file = before.unwrap_or(&empty);
    let pairs = pair_plots(a_file, after);
    let with_b = |b: Option<usize>, f: &dyn Fn(&Plot) -> bool| b.is_some_and(|i| f(&after.plots[i]));
    let found = match pick.analysis {
        // El análisis pedido: si no está o no tiene curvas, se dice por qué.
        Some(name) => {
            if expr::analysis_is(name, "").is_none() {
                return Err(tr!("image.unknown_analysis", analysis = name, known = expr::analysis_prefixes()));
            }
            let is = |p: &Plot| expr::analysis_is(name, &p.name) == Some(true);
            if !pairs.iter().any(|(_, b)| with_b(*b, &is)) {
                let available: Vec<&str> = after.plots.iter().map(|p| p.name.as_str()).collect();
                return Err(tr!("image.no_analysis", analysis = name, available = available.join(", ")));
            }
            let curves = pairs.iter().copied().enumerate().find(|(_, (_, b))| with_b(*b, &|p| is(p) && p.points() > 1));
            Some(curves.ok_or_else(|| tr!("image.analysis_no_curves", analysis = name))?)
        }
        // Primer análisis con curvas (el punto de operación tiene un solo punto).
        None => pairs.iter().copied().enumerate().find(|(_, (_, b))| with_b(*b, &|p| p.points() > 1)),
    };
    let Some((pair, (ia, ib))) = found else {
        return Ok(empty_svg(style, &tr!("image.no_curves")));
    };
    let (pa, pb) = (ia.map(|i| &a_file.plots[i]), ib.map(|i| &after.plots[i]).expect("par con B"));
    let complex = pb.complex;
    fn series<'a>(p: &'a Plot, v: &'a Variable) -> Option<(&'a [f64], &'a [f64])> {
        p.x().map(|x| (&x.values[..], &v.values[..]))
    }

    let exprs: Vec<_> = expressions.iter().filter_map(|e| expr::parse(e).ok()).collect();
    let (derived, _) = derived::evaluate(&exprs, a_file, after, tol);
    let derived: Vec<_> = derived.into_iter().filter(|d| d.pair == pair && d.diff.scalar.is_none()).collect();

    // Candidatas con su diff (si hay A) para elegir las que más cambiaron:
    // primero las expresiones (las pidió el usuario), después las señales.
    let diffs = before.map(|_| compare_plot(pa, Some(pb), tol));
    let mut candidates: Vec<(Curve, Option<&SignalDiff>)> = Vec::new();
    for d in &derived {
        let b = match (&d.b, pb.x()) {
            (Some(Evaluated::Signal(v)), Some(x)) => Some((&x.values[..], v)),
            _ => None,
        };
        let a = match (&d.a, pa.and_then(Plot::x)) {
            (Some(Evaluated::Signal(v)), Some(x)) => Some((&x.values[..], v)),
            _ => None,
        };
        let unit = b.map(|(_, v)| v.unit(complex)).unwrap_or("");
        candidates.push((
            Curve {
                name: format!("ƒ {}", d.diff.name),
                unit,
                b: b.map(|(x, v)| (x, &v.values[..])),
                a: a.map(|(x, v)| (x, &v.values[..])),
            },
            Some(&d.diff),
        ));
    }
    let derived_count = candidates.len();
    for v in pb.signals() {
        let a = pa.and_then(|p| p.signal(&v.name).and_then(|s| series(p, s)));
        let d = diffs.as_ref().and_then(|d| d.signals.iter().find(|s| s.name == v.name));
        candidates.push((Curve { name: v.name.clone(), unit: v.unit(complex), b: series(pb, v), a }, d));
    }
    let is_diff = before.is_some();
    let chosen: Vec<&(Curve, Option<&SignalDiff>)> = if !pick.signals.is_empty() {
        // Las pedidas, en ese orden, y las expresiones que den una curva.
        let mut v: Vec<_> = candidates[..derived_count].iter().collect();
        for name in pick.signals {
            let Some(c) = candidates[derived_count..].iter().find(|(c, _)| c.name.eq_ignore_ascii_case(name.trim())) else {
                let available: Vec<&str> =
                    candidates[derived_count..].iter().map(|(c, _)| c.name.as_str()).filter(|n| !is_internal(n)).take(12).collect();
                return Err(tr!("image.no_signal", signal = name, analysis = pb.name, available = available.join(", ")));
            };
            if !v.iter().any(|(x, _)| x.name == c.0.name) {
                v.push(c);
            }
        }
        v
    } else {
        let mut v: Vec<_> = if is_diff && candidates.iter().any(|(_, d)| d.is_some_and(changed)) {
            let mut v: Vec<_> = candidates.iter().filter(|(_, d)| d.is_some_and(changed)).collect();
            // Estable: a igual cambio, las expresiones siguen delante.
            v.sort_by(|x, y| y.1.map_or(0.0, |d| d.rel()).total_cmp(&x.1.map_or(0.0, |d| d.rel())));
            v
        } else {
            candidates.iter().filter(|(c, _)| !is_internal(&c.name) || c.name.starts_with('ƒ')).collect()
        };
        v.truncate(MAX_SIGNALS);
        v
    };
    if chosen.is_empty() {
        return Ok(empty_svg(style, &tr!("image.no_signals")));
    }

    // Un grupo por unidad; en diff, cada grupo con su panel de error.
    let mut groups: Vec<(&'static str, Vec<(usize, &Curve)>)> = Vec::new();
    for (i, (c, _)) in chosen.iter().enumerate() {
        match groups.iter_mut().find(|(u, _)| *u == c.unit) {
            Some((_, v)) => v.push((i, c)),
            None => groups.push((c.unit, vec![(i, c)])),
        }
    }
    let log_x = pb.log_x();
    let x_unit = pb.x().map_or("", |x| x.unit(complex));
    let x_name = pb.x().map_or(String::new(), |x| x.name.clone());

    let (w, h) = (style.width as f64, style.height as f64);
    let bg = if style.dark { "#141418" } else { "#fafaf7" };
    let fg = if style.dark { "#e6e6e6" } else { "#1e1e1e" };
    let grid = if style.dark { "#34343a" } else { "#dcdcd6" };
    let mut out = String::new();
    let _ = write!(
        out,
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}" viewBox="0 0 {w} {h}" font-family="DejaVu Sans, sans-serif">"#
    );
    let _ = write!(out, r#"<rect width="{w}" height="{h}" fill="{bg}"/>"#);
    let legend = if is_diff { format!("  ·  {}", tr!("image.b_solid_a_dashed")) } else { String::new() };
    let caption = format!("{}  ·  {}{legend}", style.caption, pb.name);
    let _ = write!(out, r#"<text x="16" y="20" font-size="14" fill="{fg}">{}</text>"#, esc(&caption));

    let rows = groups.len() * if is_diff { 2 } else { 1 };
    let (left, right, top, bottom) = (78.0, 16.0, 34.0, 40.0);
    let gap = 26.0;
    let row_h = ((h - top - bottom - gap * (rows as f64 - 1.0)) / rows as f64).max(40.0);
    let tx = |x: f64| if log_x { x.max(1e-300).log10() } else { x };
    // Eje X común: el tramo de B.
    let xs = &pb.x().map(|x| x.values.clone()).unwrap_or_default();
    let (x0, x1) = (tx(*xs.first().unwrap_or(&0.0)), tx(*xs.last().unwrap_or(&1.0)));

    let mut row = 0;
    for (unit, curves) in &groups {
        let y_top = top + row as f64 * (row_h + gap);
        let mut lines: Vec<(Vec<(f64, f64)>, &str, bool)> = Vec::new();
        for (i, c) in curves {
            let color = PALETTE[i % PALETTE.len()];
            if let Some((cx, cy)) = c.a {
                lines.push((points(cx, cy, tx), color, true));
            }
            if let Some((cx, cy)) = c.b {
                lines.push((points(cx, cy, tx), color, false));
            }
        }
        let legend: Vec<(String, &str)> = curves.iter().map(|(i, c)| (c.name.clone(), PALETTE[i % PALETTE.len()])).collect();
        panel(&mut out, (left, y_top, w - left - right, row_h), (x0, x1), &lines, unit, unit, &legend, log_x, x_unit, fg, grid);
        row += 1;
        if is_diff {
            let y_top = top + row as f64 * (row_h + gap);
            let mut err = Vec::new();
            for (i, c) in curves {
                if let (Some((xa, ya)), Some((xb, yb))) = (c.a, c.b) {
                    err.push((error_points(xa, ya, xb, yb, tx), PALETTE[i % PALETTE.len()], false));
                }
            }
            let legend: Vec<(String, &str)> =
                curves.iter().map(|(i, c)| (format!("Δ {}", c.name), PALETTE[i % PALETTE.len()])).collect();
            let label = if unit.is_empty() { "B − A".to_string() } else { format!("B − A [{unit}]") };
            panel(
                &mut out,
                (left, y_top, w - left - right, row_h),
                (x0, x1),
                &err,
                &label,
                unit,
                &legend,
                log_x,
                x_unit,
                fg,
                grid,
            );
            row += 1;
        }
    }
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-size="12" fill="{fg}" text-anchor="middle">{}</text>"#,
        left + (w - left - right) / 2.0,
        h - 8.0,
        esc(&format!("{x_name} [{x_unit}]"))
    );
    out.push_str("</svg>");
    Ok(out)
}

fn empty_svg(style: &Style, why: &str) -> String {
    let (w, h) = (style.width, style.height);
    format!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="{w}" height="{h}"><rect width="{w}" height="{h}" fill="{}"/><text x="16" y="24" font-size="14" fill="gray">{}: {}</text></svg>"#,
        if style.dark { "#141418" } else { "#fafaf7" },
        esc(&style.caption),
        esc(why)
    )
}

fn esc(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Puntos finitos, con la X transformada; como mucho unos 3000 (mín/máx por tramo).
fn points(xs: &[f64], ys: &[f64], tx: impl Fn(f64) -> f64) -> Vec<(f64, f64)> {
    let pts: Vec<(f64, f64)> = xs.iter().zip(ys).filter(|(_, y)| y.is_finite()).map(|(x, y)| (tx(*x), *y)).collect();
    decimate(pts, 3000)
}

fn error_points(xa: &[f64], ya: &[f64], xb: &[f64], yb: &[f64], tx: impl Fn(f64) -> f64) -> Vec<(f64, f64)> {
    if xa.len() < 2 || xb.len() < 2 {
        return Vec::new();
    }
    let (lo, hi) = (xa[0].max(xb[0]), xa[xa.len() - 1].min(xb[xb.len() - 1]));
    let mut grid: Vec<f64> = xa.iter().chain(xb).copied().filter(|&x| x >= lo && x <= hi).collect();
    grid.sort_by(f64::total_cmp);
    grid.dedup();
    let pts = grid.iter().map(|&x| (tx(x), interp(xb, yb, x) - interp(xa, ya, x))).filter(|(_, e)| e.is_finite()).collect();
    decimate(pts, 3000)
}

fn decimate(pts: Vec<(f64, f64)>, max: usize) -> Vec<(f64, f64)> {
    if pts.len() <= max {
        return pts;
    }
    let per = pts.len().div_ceil(max / 2);
    let mut out = Vec::with_capacity(max);
    for chunk in pts.chunks(per) {
        let lo = chunk.iter().copied().min_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        let hi = chunk.iter().copied().max_by(|a, b| a.1.total_cmp(&b.1)).unwrap();
        if lo.0 <= hi.0 {
            out.extend([lo, hi]);
        } else {
            out.extend([hi, lo]);
        }
    }
    out
}

/// Paso "redondo" (1, 2, 5 × 10ⁿ) para unas `n` marcas en `span`.
fn nice_step(span: f64, n: f64) -> f64 {
    let raw = (span / n).abs().max(1e-300);
    let mag = 10f64.powf(raw.log10().floor());
    let f = raw / mag;
    mag * if f < 1.5 {
        1.0
    } else if f < 3.5 {
        2.0
    } else if f < 7.5 {
        5.0
    } else {
        10.0
    }
}

#[allow(clippy::too_many_arguments)]
fn panel(
    out: &mut String,
    (px, py, pw, ph): (f64, f64, f64, f64),
    (x0, x1): (f64, f64),
    lines: &[(Vec<(f64, f64)>, &str, bool)],
    y_label: &str,
    unit: &str,
    legend: &[(String, &str)],
    log_x: bool,
    x_unit: &str,
    fg: &str,
    grid: &str,
) {
    let (mut y0, mut y1) = (f64::INFINITY, f64::NEG_INFINITY);
    for (pts, _, _) in lines {
        for &(_, y) in pts {
            y0 = y0.min(y);
            y1 = y1.max(y);
        }
    }
    if !y0.is_finite() {
        (y0, y1) = (-1.0, 1.0);
    }
    if y1 - y0 < 1e-300 {
        (y0, y1) = (y0 - 1.0, y1 + 1.0);
    }
    let pad = (y1 - y0) * 0.06;
    let (y0, y1) = (y0 - pad, y1 + pad);
    let xspan = if x1 > x0 { x1 - x0 } else { 1.0 };
    let sx = |x: f64| px + (x - x0) / xspan * pw;
    let sy = |y: f64| py + ph - (y - y0) / (y1 - y0) * ph;

    let _ = write!(out, r#"<rect x="{px:.1}" y="{py:.1}" width="{pw:.1}" height="{ph:.1}" fill="none" stroke="{grid}"/>"#);
    // Marcas del eje Y (un 0 que salió como 1e-18 por redondeo es 0).
    let step = nice_step(y1 - y0, 4.0);
    let mut v = (y0 / step).ceil() * step;
    while v <= y1 {
        let v_label = if v.abs() < step * 1e-6 { 0.0 } else { v };
        let y = sy(v);
        let _ = write!(out, r#"<line x1="{px:.1}" y1="{y:.1}" x2="{:.1}" y2="{y:.1}" stroke="{grid}"/>"#, px + pw);
        let _ = write!(
            out,
            r#"<text x="{:.1}" y="{:.1}" font-size="10" fill="{fg}" text-anchor="end">{}</text>"#,
            px - 4.0,
            y + 3.5,
            esc(&short(v_label, unit))
        );
        v += step;
    }
    // Marcas del eje X (décadas en escala log).
    let step = if log_x { 1.0 } else { nice_step(xspan, 6.0) };
    let mut v = (x0 / step).ceil() * step;
    while v <= x1 + step * 1e-9 {
        let x = sx(v);
        let _ = write!(out, r#"<line x1="{x:.1}" y1="{py:.1}" x2="{x:.1}" y2="{:.1}" stroke="{grid}"/>"#, py + ph);
        let value = if log_x { 10f64.powf(v) } else { v };
        let _ = write!(
            out,
            r#"<text x="{x:.1}" y="{:.1}" font-size="10" fill="{fg}" text-anchor="middle">{}</text>"#,
            py + ph + 13.0,
            esc(&short(value, x_unit))
        );
        v += step;
    }
    let _ = write!(
        out,
        r#"<text x="{:.1}" y="{:.1}" font-size="11" fill="{fg}" text-anchor="middle" transform="rotate(-90 {:.1} {:.1})">{}</text>"#,
        px - 62.0,
        py + ph / 2.0,
        px - 62.0,
        py + ph / 2.0,
        esc(y_label)
    );
    for (pts, color, dashed) in lines {
        if pts.len() < 2 {
            continue;
        }
        let list: String = pts.iter().map(|(x, y)| format!("{:.1},{:.1}", sx(*x), sy(*y))).collect::<Vec<_>>().join(" ");
        let dash = if *dashed { r#" stroke-dasharray="6 4" stroke-opacity="0.8""# } else { "" };
        let width = if *dashed { 1.2 } else { 1.6 };
        let _ = write!(out, r#"<polyline points="{list}" fill="none" stroke="{color}" stroke-width="{width}"{dash}/>"#);
    }
    for (i, (name, color)) in legend.iter().enumerate() {
        let y = py + 14.0 + i as f64 * 14.0;
        let x = px + pw - 8.0;
        let _ =
            write!(out, r#"<text x="{x:.1}" y="{y:.1}" font-size="11" fill="{color}" text-anchor="end">{}</text>"#, esc(name));
    }
}

/// Número corto para una marca de eje (`1.5 µs`, `0`).
fn short(v: f64, unit: &str) -> String {
    if v.abs() < 1e-21 {
        return "0".into();
    }
    let full = eng(v, unit);
    let trim = |n: &str| if n.contains('.') { n.trim_end_matches('0').trim_end_matches('.').to_string() } else { n.to_string() };
    match full.split_once(' ') {
        Some((num, rest)) => format!("{} {rest}", trim(num)),
        None => trim(&full),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(vout: Vec<f64>) -> RawFile {
        let v = |n: &str, k: &str, vals: Vec<f64>| Variable { name: n.into(), kind: k.into(), values: vals, complex: None };
        RawFile {
            plots: vec![Plot {
                title: String::new(),
                name: "Transient Analysis".into(),
                command: None,
                complex: false,
                vars: vec![v("time", "time", vec![0.0, 1e-6, 2e-6]), v("v(out)", "voltage", vout)],
            }],
        }
    }

    fn style() -> Style {
        Style { width: 800, height: 500, dark: false, caption: "tb.raw".into() }
    }

    fn svg(a: Option<&RawFile>, b: &RawFile, exprs: &[String], pick: Pick<'_>) -> Result<String, String> {
        wave_svg(a, b, exprs, Tolerance::default(), pick, &style())
    }

    /// Un `.raw` con muchas señales de nivel superior, como un `save all`.
    fn many(n: usize) -> RawFile {
        let mut f = raw(vec![0.0, 1.0, 0.0]);
        for i in 0..n {
            let values = vec![0.0, i as f64, 0.0];
            f.plots[0].vars.push(Variable { name: format!("v(n{i})"), kind: "voltage".into(), values, complex: None });
        }
        f
    }

    #[test]
    fn diff_draws_both_versions_and_the_error() {
        let (a, b) = (raw(vec![0.0, 1.0, 0.0]), raw(vec![0.0, 1.5, 0.0]));
        let svg = svg(Some(&a), &b, &[], Pick::default()).unwrap();
        assert!(svg.contains("stroke-dasharray"), "A punteada");
        assert!(svg.contains("B − A [V]"), "panel de error");
        assert!(svg.contains("v(out)"));
    }

    #[test]
    fn single_file_and_expressions() {
        let b = raw(vec![0.0, 1.0, 0.0]);
        let svg = svg(None, &b, &["twice = 2*v(out)".into()], Pick::default()).unwrap();
        assert!(!svg.contains("stroke-dasharray") && svg.contains("ƒ twice"));
    }

    #[test]
    fn an_expression_is_drawn_even_when_the_file_has_many_signals() {
        let svg = svg(None, &many(5), &["twice = 2*v(out)".into()], Pick::default()).unwrap();
        assert!(svg.contains("ƒ twice"), "la expresión va antes que las señales del archivo");
    }

    #[test]
    fn picked_signals_and_analysis() {
        let f = many(5);
        let signals = ["V(N4)".to_string(), "v(out)".to_string()];
        let svg = svg(None, &f, &[], Pick { signals: &signals, analysis: Some("tran") }).unwrap();
        assert!(svg.contains("v(n4)") && svg.contains("v(out)") && !svg.contains("v(n0)"));
        // Errores que se entienden, no una imagen vacía.
        let missing = ["v(nada)".to_string()];
        assert!(svg_err(&f, Pick { signals: &missing, analysis: None }).contains("v(nada)"));
        assert!(svg_err(&f, Pick { signals: &[], analysis: Some("ac") }).contains("Transient Analysis"));
        assert!(svg_err(&f, Pick { signals: &[], analysis: Some("xyz") }).contains("tran"));
    }

    fn svg_err(f: &RawFile, pick: Pick<'_>) -> String {
        svg(None, f, &[], pick).unwrap_err()
    }

    #[test]
    fn nice_steps() {
        assert_eq!(nice_step(10.0, 5.0), 2.0);
        assert!((nice_step(1e-6, 4.0) - 2e-7).abs() < 1e-18);
    }
}
