//! Formateador de texto para `riku diff`.
//!
//! Imprime un diff semántico legible: header con conteos, lista de componentes
//! cambiados con marker (`+`, `-`, `~`, `r` para rename) y, si aplica, la
//! tabla de parámetros antes/después. Los nets añadidos y eliminados se listan
//! al final.

use std::collections::{BTreeMap, BTreeSet};

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
        println!("Cosméticos: {cosmetic} (solo posición)");
    }
    println!();
}

fn print_component(c: &ComponentDiff) {
    println!("  {} {}", marker_for(c), c.name);

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

fn marker_for(c: &ComponentDiff) -> &'static str {
    let is_rename = c.kind == ChangeKind::Modified && c.name.contains(" → ");
    if is_rename {
        return "r";
    }
    match c.kind {
        ChangeKind::Added => "+",
        ChangeKind::Removed => "-",
        ChangeKind::Modified => "~",
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
