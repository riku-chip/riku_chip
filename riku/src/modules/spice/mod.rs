//! Módulo de simulación: resultados `.raw` de ngspice (formas de onda).
//!
//! El diff compara cada señal entre las dos versiones ([`compare`]) y
//! reporta un [`Element::Signal`] por señal que cambió más que la
//! tolerancia; los cambios dentro de la tolerancia quedan como cosméticos.
//! Además compara señales calculadas ([`expr`]: `v(out)/v(in)`,
//! `max(v(out))`) que llegan en `DiffOptions::expressions`.
//! El visor no pasa por `ViewerBackend` (que dibuja planos: esquemáticos y
//! layouts): la GUI grafica las curvas con su vista propia.

pub mod compare;
pub mod derived;
pub mod expr;
pub mod raw;

use riku_kernel::{DiffOptions, FormatModule, ModuleInfo};

use crate::core::domain::models::{Change, ChangeKind, Element, FileChange, FileFormat, Value};
use riku_kernel::Detail;
use compare::{Status, Tolerance};

pub struct WaveformModule;

impl WaveformModule {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WaveformModule {
    fn default() -> Self {
        Self::new()
    }
}

/// Lee un lado del diff; vacío = el archivo no existía en esa versión.
fn read_side(content: &[u8], side: &str, path: &str) -> Result<raw::RawFile, String> {
    if content.is_empty() {
        return Ok(raw::RawFile { plots: Vec::new() });
    }
    raw::parse(content).map_err(|e| format!("{path} ({side}): {e}"))
}

impl FormatModule for WaveformModule {
    fn info(&self) -> ModuleInfo {
        ModuleInfo {
            name: "spice".into(),
            version: "formas de onda (.raw de ngspice)".into(),
            format: FileFormat::Waveform,
            extensions: vec![".raw".into()],
            available: true,
        }
    }

    fn detect(&self, content: &[u8]) -> bool {
        raw::looks_like_raw(content)
    }

    fn diff(&self, before: &[u8], after: &[u8], path_hint: &str, opts: &DiffOptions) -> FileChange {
        let mut report = FileChange::new(FileFormat::Waveform);
        let (a, b) = match (read_side(before, "A", path_hint), read_side(after, "B", path_hint)) {
            (Ok(a), Ok(b)) => (a, b),
            (Err(e), _) | (_, Err(e)) => return FileChange::failed(FileFormat::Waveform, e),
        };
        // Tolerancia del proyecto (`.riku.toml`, `--tolerance`) o la del módulo.
        let tol = opts.tolerance.map_or_else(Tolerance::default, |rel| Tolerance { rel, ..Tolerance::default() });
        for plot in compare::compare(&a, &b, tol) {
            if let (Some((a0, a1)), Some((b0, b1))) = plot.x_range {
                if a0 != b0 || a1 != b1 {
                    report.warnings.push(format!(
                        "{}: el eje cambió de [{a0:e}, {a1:e}] a [{b0:e}, {b1:e}]; se compara el tramo común",
                        plot.name
                    ));
                }
            }
            for s in &plot.signals {
                report.changes.push(signal_change(s));
            }
        }
        let mut exprs = Vec::new();
        for text in &opts.expressions {
            match expr::parse(text) {
                Ok(e) => exprs.push(e),
                Err(e) => report.warnings.push(format!("{text}: {e}")),
            }
        }
        let (derived, warnings) = derived::evaluate(&exprs, &a, &b, tol);
        report.warnings.extend(warnings);
        for d in &derived {
            report.changes.push(signal_change(&d.diff));
        }
        report
    }
}

fn signal_change(s: &compare::SignalDiff) -> Change {
    let element = Element::Signal { plot: s.plot.clone(), name: s.name.clone() };
    let kind = match s.status {
        Status::Added => ChangeKind::Added,
        Status::Removed => ChangeKind::Removed,
        Status::Compared | Status::Incomparable => ChangeKind::Modified,
    };
    let mut c = Change::new(kind, element).cosmetic(s.status == Status::Compared && s.within_tolerance);
    let mut put = |k: &str, v: Value| c.details.push(Detail { key: k.into(), before: None, after: Some(v) });
    put("plot", Value::Text(s.plot.clone()));
    put("unit", Value::Text(s.unit.to_string()));
    if let Some(text) = &s.expression {
        put("expression", Value::Text(text.clone()));
    }
    // Escalar: el valor en cada versión (antes → después).
    if let Some((va, vb)) = s.scalar {
        c.details.push(Detail { key: "value".into(), before: va.map(Value::Float), after: vb.map(Value::Float) });
    }
    let mut put = |k: &str, v: Value| c.details.push(Detail { key: k.into(), before: None, after: Some(v) });
    if s.status == Status::Compared && s.scalar.is_none() {
        put("max_abs_diff", Value::Float(s.max_abs));
        put("at", Value::Float(s.at_x));
        put("x_unit", Value::Text(s.x_unit.to_string()));
        put("rms_diff", Value::Float(s.rms));
        put("rel_diff", Value::Float(s.rel()));
        if s.skipped > 0 {
            put("skipped_points", Value::Int(s.skipped as i64));
        }
    }
    if s.status == Status::Compared && s.scalar.is_some() {
        put("max_abs_diff", Value::Float(s.max_abs));
        put("rel_diff", Value::Float(s.rel()));
    }
    if s.status == Status::Incomparable {
        put("note", Value::Text("sin eje común para comparar".into()));
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;
    use raw::tests::binary_raw;

    fn run(ys_a: Vec<f64>, ys_b: Vec<f64>) -> FileChange {
        let vars = [("time", "time"), ("v(out)", "voltage")];
        let x = vec![0.0, 1e-6, 2e-6];
        let a = binary_raw("Transient Analysis", &vars, &[x.clone(), ys_a]);
        let b = binary_raw("Transient Analysis", &vars, &[x, ys_b]);
        WaveformModule::new().diff(&a, &b, "tb.raw", &DiffOptions::default())
    }

    #[test]
    fn same_waveform_is_clean() {
        let r = run(vec![0.0, 1.8, 0.0], vec![0.0, 1.8, 0.0]);
        assert!(r.is_empty(), "{r:?}");
        assert_eq!(r.changes.len(), 1);
    }

    #[test]
    fn changed_waveform_reports_the_signal() {
        let r = run(vec![0.0, 1.8, 0.0], vec![0.0, 1.5, 0.0]);
        let c = r.functional().next().expect("un cambio funcional");
        assert_eq!(c.element, Element::Signal { plot: "Transient Analysis".into(), name: "v(out)".into() });
        assert!((c.after("max_abs_diff").and_then(Value::as_f64).unwrap() - 0.3).abs() < 1e-12);
        assert_eq!(c.after("at").and_then(Value::as_f64), Some(1e-6));
    }

    #[test]
    fn new_file_adds_every_signal() {
        let x = vec![0.0, 1.0];
        let b = binary_raw("Transient Analysis", &[("time", "time"), ("v(a)", "voltage")], &[x.clone(), x]);
        let r = WaveformModule::new().diff(&[], &b, "tb.raw", &DiffOptions::default());
        assert_eq!(r.changes[0].kind, ChangeKind::Added);
    }

    #[test]
    fn expressions_are_compared_too() {
        let vars = [("time", "time"), ("v(in)", "voltage"), ("v(out)", "voltage")];
        let x = vec![0.0, 1e-6, 2e-6];
        let a = binary_raw("Transient Analysis", &vars, &[x.clone(), vec![1.0; 3], vec![0.5; 3]]);
        let b = binary_raw("Transient Analysis", &vars, &[x, vec![1.0; 3], vec![0.5, 0.6, 0.5]]);
        let opts = DiffOptions {
            expressions: vec!["gain = v(out)/v(in)".into(), "max(v(out))".into(), "v(out) +".into()],
            ..Default::default()
        };
        let r = WaveformModule::new().diff(&a, &b, "tb.raw", &opts);
        let gain = r.changes.iter().find(|c| c.element.name() == "gain").expect("gain");
        assert!(!gain.cosmetic);
        assert_eq!(gain.after("expression"), Some(&Value::Text("v(out)/v(in)".into())));
        let peak = r.changes.iter().find(|c| c.element.name() == "max(v(out))").expect("max");
        assert_eq!((peak.before("value"), peak.after("value")), (Some(&Value::Float(0.5)), Some(&Value::Float(0.6))));
        assert!(r.warnings.iter().any(|w| w.contains("v(out) +")), "{:?}", r.warnings);
    }

    #[test]
    fn garbage_is_an_error_not_a_panic() {
        let r = WaveformModule::new().diff(b"basura", b"basura", "x.raw", &DiffOptions::default());
        assert!(r.changes.is_empty() && r.error.is_some());
        assert!(!r.is_empty(), "un archivo ilegible no es 'sin cambios'");
    }
}
