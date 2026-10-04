//! El LVS en el texto de `log --lvs`, `status --lvs` y `lvs --log`: el
//! veredicto, la transición y qué discrepancias aparecieron (`+`), se
//! arreglaron (`−`) o cambiaron de valor (`~`).

use super::color;
use crate::core::analysis::lvs_types::{Delta, Discrepancy, LvsState, PairLvs, PairStatusLvs, Side, Transition, Verdict};
use crate::core::analysis::summary::DetailLevel;
use crate::i18n::tr;

/// Líneas de delta que se muestran sin `--detail`.
const SUMMARY_LINES: usize = 3;

/// `xschem/ota-5t.sch` → `ota-5t` (y la celda, si se eligió otra).
pub fn pair_label(schematic: &str, cell: Option<&str>) -> String {
    let stem = std::path::Path::new(schematic).file_stem().map_or_else(|| schematic.to_string(), |s| s.to_string_lossy().into());
    match cell {
        Some(c) if c != stem => format!("{stem} ({c})"),
        _ => stem,
    }
}

pub fn state_text(s: &LvsState) -> String {
    match s {
        LvsState::Done { verdict: Verdict::Match } => color::green(&tr!("lvs.v_match")),
        LvsState::Done { verdict: Verdict::PropertyErrors } => color::yellow(&tr!("lvs.v_properties")),
        LvsState::Done { verdict: Verdict::Mismatch } => color::red(&tr!("lvs.v_mismatch")),
        LvsState::Missing => tr!("lvs.v_missing"),
        LvsState::Error { error } => color::red(&tr!("lvs.v_error", error = error)),
    }
}

pub fn transition_text(t: Option<Transition>) -> String {
    let label = match t {
        Some(Transition::Broke) => tr!("lvs.broke"),
        Some(Transition::Worse) => tr!("lvs.worse"),
        Some(Transition::Better) => tr!("lvs.better"),
        Some(Transition::Fixed) => tr!("lvs.fixed"),
        None => return String::new(),
    };
    let label = if t.is_some_and(Transition::is_regression) { color::red(&label) } else { color::green(&label) };
    format!("  ← {label}")
}

/// `−50 %`, `+5,6 %`: un decimal por debajo de 10 %, con la coma del idioma.
fn percent(e: f64) -> String {
    percent_with(e, crate::i18n::current().starts_with("es"))
}

fn percent_with(e: f64, comma: bool) -> String {
    let v = e * 100.0;
    let text = if v.abs() < 10.0 { format!("{:.1}", v.abs()) } else { format!("{:.0}", v.abs()) };
    let text = if comma { text.replace('.', ",") } else { text };
    format!("{}{text} %", if v < 0.0 { "−" } else { "+" })
}

fn names(v: &[String]) -> String {
    if v.is_empty() {
        "—".into()
    } else {
        v.join(", ")
    }
}

/// Una discrepancia en una línea. `before`: la misma en la versión anterior,
/// para mostrar qué valor cambió (`layout 20 → 19`).
pub fn discrepancy_text(d: &Discrepancy, before: Option<&Discrepancy>) -> String {
    match d {
        Discrepancy::Property { instance, param, schematic, layout, .. } => {
            let (s0, l0) = match before {
                Some(Discrepancy::Property { schematic, layout, .. }) => (Some(schematic), Some(layout)),
                _ => (None, None),
            };
            let arrow = |old: Option<&String>, new: &String| match old {
                Some(o) if o != new => format!("{o} → {new}"),
                _ => new.clone(),
            };
            let pct = d.relative_error().map(|e| format!(" ({})", percent(e))).unwrap_or_default();
            tr!(
                "lvs.d_property",
                instance = instance,
                param = param,
                schematic = arrow(s0, schematic),
                layout = arrow(l0, layout)
            ) + &pct
        }
        Discrepancy::Nets { schematic, layout } => tr!("lvs.d_nets", schematic = names(schematic), layout = names(layout)),
        Discrepancy::Devices { schematic, layout } => tr!("lvs.d_devices", schematic = names(schematic), layout = names(layout)),
        Discrepancy::Pin { name, only_in: Side::Schematic } => tr!("lvs.d_pin_schematic", name = name),
        Discrepancy::Pin { name, only_in: Side::Layout } => tr!("lvs.d_pin_layout", name = name),
    }
}

/// Las líneas del delta: lo que apareció, lo que cambió y lo que se arregló.
/// `limit`: cuántas como mucho (el resto, `… y N más`).
pub fn delta_lines(delta: &Delta, limit: Option<usize>, indent: &str) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    lines.extend(delta.appeared.iter().map(|d| format!("{indent}{} {}", color::red("+"), discrepancy_text(d, None))));
    lines.extend(
        delta.changed.iter().map(|c| format!("{indent}{} {}", color::yellow("~"), discrepancy_text(&c.now, Some(&c.before)))),
    );
    lines.extend(delta.fixed.iter().map(|d| format!("{indent}{} {}", color::green("−"), discrepancy_text(d, None))));
    if let Some(max) = limit.filter(|&m| lines.len() > m) {
        let more = lines.len() - max;
        lines.truncate(max);
        lines.push(format!("{indent}{}", tr!("lvs.d_more", count = more)));
    }
    lines
}

fn limit(level: DetailLevel) -> Option<usize> {
    (level == DetailLevel::Resumen).then_some(SUMMARY_LINES)
}

/// Las líneas de un par en un commit de `log --lvs`. Vacío si no cambió nada
/// respecto del padre (salvo con `--detail`).
pub fn log_lines(p: &PairLvs, level: DetailLevel, indent: &str) -> Vec<String> {
    let delta = p.delta.as_ref().filter(|d| !d.is_empty());
    let compared = p.delta.is_some();
    let quiet = compared && p.transition.is_none() && delta.is_none() && !matches!(p.state, LvsState::Error { .. });
    if quiet && level == DetailLevel::Resumen {
        return Vec::new();
    }
    let unchanged = if quiet { tr!("lvs.unchanged") } else { String::new() };
    let mut out = vec![format!(
        "{indent}{}{unchanged}{}",
        tr!("lvs.line", pair = pair_label(&p.schematic, p.cell.as_deref()), state = state_text(&p.state)),
        transition_text(p.transition)
    )];
    let deeper = format!("{indent}  ");
    if let Some(d) = delta {
        out.extend(delta_lines(d, limit(level), &deeper));
    }
    out.extend(full_list(&p.discrepancies, &deeper));
    out
}

fn full_list(all: &[Discrepancy], indent: &str) -> Vec<String> {
    if all.is_empty() {
        return Vec::new();
    }
    let mut out = vec![format!("{indent}{}", tr!("lvs.all", count = all.len()))];
    out.extend(all.iter().map(|d| format!("{indent}  · {}", discrepancy_text(d, None))));
    out
}

/// Las líneas de un par en `status --lvs`.
pub fn status_lines(p: &PairStatusLvs, level: DetailLevel) -> Vec<String> {
    let label = pair_label(&p.schematic, p.cell.as_deref());
    if p.unchanged {
        return vec![format!("  {label:12}  {}{}", state_text(&p.worktree), tr!("lvs.unchanged"))];
    }
    let states = match (&p.head, &p.worktree) {
        (LvsState::Missing, w) => state_text(w),
        (h, w) if h == w => state_text(w),
        (h, w) => format!("{} → {}", state_text(h), state_text(w)),
    };
    let mut out = vec![format!("  {label:12}  {states}{}", transition_text(p.transition))];
    if let Some(d) = p.delta.as_ref().filter(|d| !d.is_empty()) {
        out.extend(delta_lines(d, limit(level), "                "));
    }
    out.extend(full_list(&p.discrepancies, "                "));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percent_of_the_layout_against_the_schematic() {
        assert_eq!(percent_with(-0.5, true), "−50 %");
        assert_eq!(percent_with(1.0 / 18.0, true), "+5,6 %");
        assert_eq!(percent_with(1.0 / 18.0, false), "+5.6 %");
        assert_eq!(percent_with(1.0, false), "+100 %");
    }

    #[test]
    fn a_changed_value_shows_the_arrow() {
        let p = |l: &str| Discrepancy::Property {
            instance: "M3".into(),
            layout_instance: "7".into(),
            model: "nfet".into(),
            param: "w".into(),
            schematic: "18".into(),
            layout: l.into(),
        };
        let t = discrepancy_text(&p("19"), Some(&p("20")));
        assert!(t.contains("20 → 19") && t.contains("18") && !t.contains("18 → 18"), "{t}");
        assert_eq!(pair_label("xschem/ota-5t.sch", Some("ota-5t")), "ota-5t");
        assert_eq!(pair_label("xschem/ota-5t.sch", Some("core")), "ota-5t (core)");
    }

    #[test]
    fn quiet_commits_say_nothing_unless_asked() {
        let p = PairLvs {
            schematic: "a.sch".into(),
            layout: "a.gds".into(),
            cell: None,
            state: LvsState::Done { verdict: Verdict::PropertyErrors },
            transition: None,
            delta: Some(Delta::default()),
            discrepancies: Vec::new(),
        };
        assert!(log_lines(&p, DetailLevel::Resumen, "").is_empty());
        assert_eq!(log_lines(&p, DetailLevel::Detalle, "").len(), 1);
        let first = PairLvs { delta: None, ..p };
        assert_eq!(log_lines(&first, DetailLevel::Resumen, "").len(), 1, "sin con qué comparar: el estado");
    }
}
