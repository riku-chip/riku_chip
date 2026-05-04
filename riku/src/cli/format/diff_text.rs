//! Formateador de texto para `riku diff`.
//!
//! Imprime un diff semántico legible: header con conteos, lista de componentes
//! cambiados con marker (`+`, `-`, `~`, `r` para rename) y, si aplica, la
//! tabla de parámetros antes/después. Los nets añadidos y eliminados se listan
//! al final.

use std::collections::{BTreeMap, BTreeSet};

use super::common::marker_for_change;
use crate::core::analysis::diff_view::DiffView;
use crate::core::domain::models::{ChangeKind, ComponentDiff};

pub fn print(view: &DiffView, file_path: &str) -> Result<(), String> {
    if view.report.is_empty() {
        println!("Sin cambios semanticos.");
        return Ok(());
    }

    let semantic: Vec<&ComponentDiff> = view
        .report
        .components
        .iter()
        .filter(|c| !c.cosmetic)
        .collect();
    let cosmetic_count = view.report.components.iter().filter(|c| c.cosmetic).count();

    print_header(file_path, semantic.len(), cosmetic_count);

    for c in &semantic {
        print_component(c);
    }

    print_nets(&view.report.nets_added, &view.report.nets_removed);

    Ok(())
}

fn print_header(file_path: &str, semantic: usize, cosmetic: usize) {
    println!("Archivo : {file_path}");
    println!("Cambios : {semantic}");
    if cosmetic > 0 {
        println!("Cosméticos: {cosmetic}");
    }
    println!();
}

fn print_component(c: &ComponentDiff) {
    println!("  {} {}", marker_for_change(&c.kind, &c.name), c.name);

    // Elementos GDS tienen forma "<cell>:L<layer>/<datatype>" — los renderiza
    // print_gds_geom con áreas + bbox.
    if is_gds_geom_element(&c.name) {
        if let Some(after) = &c.after {
            print_gds_geom(after);
        }
        return;
    }

    if let (Some(before), Some(after)) = (&c.before, &c.after) {
        print_param_diff(before, after);
    } else if c.kind == ChangeKind::Added {
        if let Some(after) = &c.after {
            if let Some(sym) = after.get("symbol") {
                println!("      símbolo: {sym}");
            }
        }
    }
}

/// Detecta elementos con forma `<cell>:L<layer>/<datatype>` o
/// `<cell>:L<layer>/<datatype>:<origin_tail>` (output de GdsDriver).
fn is_gds_geom_element(name: &str) -> bool {
    let Some((_, tail)) = name.split_once(':') else {
        return false;
    };
    let Some(rest) = tail.strip_prefix('L') else {
        return false;
    };
    // Solo nos interesa el primer segmento despues de 'L': "<layer>/<datatype>".
    let layer_dt = rest.split(':').next().unwrap_or("");
    let Some((l, dt)) = layer_dt.split_once('/') else {
        return false;
    };
    !l.is_empty() && !dt.is_empty() && l.chars().all(|c| c.is_ascii_digit())
        && dt.chars().all(|c| c.is_ascii_digit())
}

fn print_gds_geom(after: &BTreeMap<String, String>) {
    if let Some(origin) = after.get("origin_path") {
        // origin_path = "<cell>" o "<cell>/<sub>"; mostrar solo si tiene >1 segmento.
        if origin.contains('/') {
            let pretty = origin.replace('/', " → ");
            println!("      origen: {pretty}");
        }
    }
    let added_n = after.get("added_polygons").map(String::as_str).unwrap_or("0");
    let removed_n = after.get("removed_polygons").map(String::as_str).unwrap_or("0");
    let added_a = after.get("added_area_um2").map(String::as_str).unwrap_or("0.000");
    let removed_a = after.get("removed_area_um2").map(String::as_str).unwrap_or("0.000");
    println!("      +{added_n} polys / +{added_a} µm²");
    println!("      -{removed_n} polys / -{removed_a} µm²");
    if let Some(b) = after.get("bbox_um") {
        // bbox_um viene como "min_x,min_y,max_x,max_y" (3 decimales).
        let parts: Vec<&str> = b.split(',').collect();
        if parts.len() == 4 {
            println!(
                "      bbox: ({}, {}) → ({}, {}) µm",
                parts[0], parts[1], parts[2], parts[3]
            );
        }
    }
}

fn print_param_diff(before: &BTreeMap<String, String>, after: &BTreeMap<String, String>) {
    let all_keys: BTreeSet<_> = before.keys().chain(after.keys()).collect();
    for key in all_keys {
        if matches!(key.as_str(), "x" | "y" | "rotation" | "mirror") {
            continue;
        }
        match (before.get(key), after.get(key)) {
            (Some(a), Some(b)) if a != b => println!("      {key}: {a} → {b}"),
            (None, Some(b)) => println!("      {key}: (nuevo) → {b}"),
            (Some(a), None) => println!("      {key}: {a} → (eliminado)"),
            _ => {}
        }
    }
}

fn print_nets(added: &[String], removed: &[String]) {
    if !added.is_empty() {
        println!();
        for net in added {
            println!("  + net:{net}");
        }
    }
    if !removed.is_empty() {
        if added.is_empty() {
            println!();
        }
        for net in removed {
            println!("  - net:{net}");
        }
    }
}
