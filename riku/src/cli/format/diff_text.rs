//! Formateador de texto para `riku diff`.
//!
//! Imprime un diff semántico legible: header con conteos, lista de elementos
//! cambiados con marker (`+`, `-`, `~`, `r` para rename) y, según el tipo de
//! elemento, la tabla de parámetros antes/después o las áreas y el bbox de
//! la geometría. Las nets añadidas y eliminadas se listan al final.

use super::common::marker_for_change;
use crate::core::domain::models::{Change, ChangeKind, Element, FileChange, Value};
use crate::i18n::tr;
pub(crate) use crate::text::eng;

pub fn print(report: &FileChange, file_path: &str) -> Result<(), String> {
    if let Some(err) = &report.error {
        print_error(file_path, err);
        return Ok(());
    }
    if report.is_empty() {
        println!("{}", tr!("diff.no_changes"));
        return Ok(());
    }

    // Nets y el cambio "todo el archivo" van aparte (como en la salida v1).
    let listed = |c: &&Change| !matches!(c.element, Element::Net { .. } | Element::Whole);
    let semantic: Vec<&Change> = report.functional().filter(listed).collect();
    let cosmetic_count = report.changes.iter().filter(|c| c.cosmetic).filter(listed).count();

    print_header(file_path, semantic.len(), cosmetic_count);

    for c in &semantic {
        print_change(c);
    }

    print_nets(report);

    Ok(())
}

/// Un archivo que el módulo no pudo comparar (no es "sin cambios").
pub fn print_error(file_path: &str, err: &str) {
    println!("{}", tr!("diff.file", file = super::color::bold(file_path)));
    println!("  {} {err}", super::color::red(&tr!("diff.failed")));
}

fn print_header(file_path: &str, semantic: usize, cosmetic: usize) {
    println!("{}", tr!("diff.file", file = super::color::bold(file_path)));
    println!("{}", tr!("diff.changes", count = semantic));
    if cosmetic > 0 {
        println!("{}", tr!("diff.cosmetic", count = cosmetic));
    }
    println!();
}

/// Nombre como lo muestra la CLI: `cell:INV`, `TOP:L1/0:INV`, `vin → vin_diff`.
fn display_name(c: &Change) -> String {
    let renamed = |name: &str| match (&c.renamed_from, c.kind) {
        (Some(from), ChangeKind::Renamed) => format!("{from} → {name}"),
        _ => name.to_string(),
    };
    match &c.element {
        Element::Component { name } => renamed(name),
        Element::Cell { name } => format!("cell:{}", renamed(name)),
        other => other.name(),
    }
}

fn print_change(c: &Change) {
    let marker = if c.severity.is_some() { "!" } else { marker_for_change(c.kind) };
    println!("  {} {}", super::color::marker(marker), display_name(c));
    match &c.element {
        Element::LayoutNet { .. } => print_layout_net(c),
        Element::Geometry { .. } => print_geometry(c),
        Element::Signal { .. } => print_signal(c),
        Element::Port { .. } | Element::Device { .. } => print_port(c),
        Element::Component { .. } => match c.kind {
            ChangeKind::Modified | ChangeKind::Renamed => print_param_diff(c),
            ChangeKind::Added => {
                if let Some(sym) = c.after("symbol") {
                    println!("      {}", tr!("diff.symbol", symbol = sym));
                }
            }
            ChangeKind::Removed => {}
        },
        _ => {}
    }
}

fn print_geometry(c: &Change) {
    if let Element::Geometry { cell, via: Some(via), .. } = &c.element {
        let pretty = std::iter::once(cell.as_str()).chain(via.path.iter().map(String::as_str)).collect::<Vec<_>>().join(" → ");
        match (via.instances, via.at) {
            (n, _) if n > 1 => println!("      {}", tr!("diff.origin_n", path = pretty, count = n)),
            (_, Some([x, y])) => println!("      {} @ ({x:.3}, {y:.3})", tr!("diff.origin", path = pretty)),
            _ => println!("      {}", tr!("diff.origin", path = pretty)),
        }
    }
    let count = |k: &str| c.after(k).map_or_else(|| "0".to_string(), Value::to_string);
    let area = |k: &str| format!("{:.3}", c.after(k).and_then(Value::as_f64).unwrap_or(0.0));
    println!("      {}", tr!("diff.polys_added", count = count("added_polygons"), area = area("added_area_um2")));
    println!("      {}", tr!("diff.polys_removed", count = count("removed_polygons"), area = area("removed_area_um2")));
    if let Some(b) = c.location {
        println!("      bbox: ({:.3}, {:.3}) → ({:.3}, {:.3}) µm", b.min_x, b.min_y, b.max_x, b.max_y);
    }
}

/// Abierto o corto de una red: qué redes había y cuáles hay, y dónde.
fn print_layout_net(c: &Change) {
    let show = |v: Option<&Value>| v.map_or_else(|| "—".to_string(), Value::to_string);
    let kind = c.after("kind").map(Value::to_string).unwrap_or_default();
    let what = match kind.as_str() {
        "open" => tr!("diff.net_open"),
        "short" => tr!("diff.net_short"),
        "separated" => tr!("diff.net_separated"),
        _ => tr!("diff.net_renamed"),
    };
    println!("      {what}: {} → {}", show(c.before("nets")), show(c.after("nets")));
    if let Some(b) = c.location {
        println!("      bbox: ({:.3}, {:.3}) → ({:.3}, {:.3}) µm", b.min_x, b.min_y, b.max_x, b.max_y);
    }
}

/// Puerto de un layout: los atributos que cambiaron (`class: input → inout`).
fn print_port(c: &Change) {
    for d in &c.details {
        let show = |v: &Option<Value>| v.as_ref().map_or_else(|| "—".to_string(), Value::to_string);
        match c.kind {
            ChangeKind::Modified => println!("      {}: {} → {}", d.key, show(&d.before), show(&d.after)),
            ChangeKind::Removed => println!("      {}: {}", d.key, show(&d.before)),
            _ => println!("      {}: {}", d.key, show(&d.after)),
        }
    }
}

/// Señal de simulación: error máximo (y dónde), RMS y porcentaje del rango.
fn print_signal(c: &Change) {
    let text = |k: &str| c.after(k).map(Value::to_string).unwrap_or_default();
    let num = |k: &str| c.after(k).and_then(Value::as_f64);
    let plot = text("plot");
    let unit = text("unit");
    // Señal calculada con nombre (`gain = v(out)/v(in)`): mostrar la fórmula.
    if let (Some(expr), Element::Signal { name, .. }) = (c.after("expression"), &c.element) {
        if expr.to_string() != *name {
            println!("      = {expr}");
        }
    }
    // Escalar (`max(v(out))`, `v(out)[0]`): el valor antes y después.
    if c.before("value").is_some() || c.after("value").is_some() {
        let v = |x: Option<&Value>| x.and_then(Value::as_f64).map_or_else(|| "—".to_string(), |f| eng(f, &unit));
        let delta =
            num("max_abs_diff").map(|d| format!(" · Δ {} ({:.2} %)", eng(d, &unit), num("rel_diff").unwrap_or(0.0) * 100.0));
        println!("      {} → {}{}  ({plot})", v(c.before("value")), v(c.after("value")), delta.unwrap_or_default());
        return;
    }
    match c.kind {
        ChangeKind::Added => println!("      {}", tr!("diff.signal_new", plot = plot)),
        ChangeKind::Removed => println!("      {}", tr!("diff.signal_gone", plot = plot)),
        _ => match (num("max_abs_diff"), num("at")) {
            (Some(max), Some(at)) => {
                let unit = text("unit");
                let line = tr!(
                    "diff.signal_delta",
                    max = eng(max, &unit),
                    at = eng(at, &text("x_unit")),
                    rms = eng(num("rms_diff").unwrap_or(0.0), &unit),
                    rel = format!("{:.2}", num("rel_diff").unwrap_or(0.0) * 100.0),
                    plot = plot
                );
                println!("      {line}");
            }
            _ => println!("      {}  ({plot})", text("note")),
        },
    }
}

fn print_param_diff(c: &Change) {
    // Un sub-esquemático que cambió (por sí mismo o por dentro).
    if let Some(inside) = c.after("inside") {
        println!("      {}", tr!("diff.inside", path = inside));
    }
    let mut details: Vec<_> = c.params().filter(|d| d.key != "inside").collect();
    details.sort_by(|a, b| a.key.cmp(&b.key));
    for d in details {
        let key = &d.key;
        match (&d.before, &d.after) {
            (Some(a), Some(b)) if a != b => println!("      {key}: {a} → {b}"),
            (None, Some(b)) => println!("      {key}: {} → {b}", tr!("diff.new")),
            (Some(a), None) => println!("      {key}: {a} → {}", tr!("diff.deleted")),
            _ => {}
        }
    }
}

fn print_nets(report: &FileChange) {
    let nets = |kind: ChangeKind| -> Vec<&str> {
        report
            .changes
            .iter()
            .filter(|c| c.kind == kind)
            .filter_map(|c| match &c.element {
                Element::Net { name } => Some(name.as_str()),
                _ => None,
            })
            .collect()
    };
    let (added, removed) = (nets(ChangeKind::Added), nets(ChangeKind::Removed));
    if !added.is_empty() {
        println!();
        for net in &added {
            println!("  + net:{net}");
        }
    }
    if !removed.is_empty() {
        if added.is_empty() {
            println!();
        }
        for net in &removed {
            println!("  - net:{net}");
        }
    }
}
