use std::collections::{BTreeMap, BTreeSet};

use crate::core::models::{ChangeKind, ComponentDiff, DiffReport};
use crate::parsers::xschem::parse;

// ─── Snapshot ────────────────────────────────────────────────────────────────

/// Captura el estado completo de un componente como mapa plano.
/// `include_coords` controla si se incluyen x/y/rotation/mirror.
/// Los pins se incluyen siempre con prefijo `pin:` (e.g. `pin:DRAIN=Vout`).
fn component_snapshot(
    component: &crate::core::models::Component,
    include_coords: bool,
) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    out.insert("symbol".to_string(), component.symbol.clone());
    if include_coords {
        out.insert("x".to_string(), component.x.to_string());
        out.insert("y".to_string(), component.y.to_string());
        out.insert("rotation".to_string(), component.rotation.to_string());
        out.insert("mirror".to_string(), component.mirror.to_string());
    }
    for (k, v) in &component.params {
        out.insert(k.clone(), v.clone());
    }
    for (pin, net) in &component.pins {
        out.insert(format!("pin:{pin}"), net.clone());
    }
    out
}

/// Similitud entre dos componentes basada en parámetros semánticos (sin coords ni pins).
/// Los pins se excluyen porque una reconexión no implica que sean componentes distintos.
/// Retorna un valor entre 0.0 (nada en común) y 1.0 (idénticos).
fn param_similarity(
    a: &crate::core::models::Component,
    b: &crate::core::models::Component,
) -> f64 {
    if a.symbol != b.symbol {
        return 0.0;
    }
    if a.params.is_empty() && b.params.is_empty() {
        return 1.0;
    }
    let all_keys: BTreeSet<_> = a.params.keys().chain(b.params.keys()).collect();
    if all_keys.is_empty() {
        return 1.0;
    }
    let matching = all_keys.iter()
        .filter(|k| a.params.get(*k) == b.params.get(*k))
        .count();
    matching as f64 / all_keys.len() as f64
}

// ─── Rename detection ─────────────────────────────────────────────────────────

/// Umbral mínimo de similitud para considerar dos componentes como renombrados.
const RENAME_THRESHOLD: f64 = 0.8;

/// Detecta pares (removido, añadido) que son probablemente renombrados.
/// Usa matching greedy por similitud — O(n*m) sobre los conjuntos de removidos/añadidos,
/// que en la práctica son pequeños (<20 elementos típicamente).
fn detect_renames(
    removed: &[&crate::core::models::Component],
    added: &[&crate::core::models::Component],
) -> Vec<(String, String, f64)> { // (nombre_a, nombre_b, similitud)
    let mut used_added = BTreeSet::new();
    let mut renames = Vec::new();

    // Ordenar candidatos por similitud descendente para matching greedy óptimo
    let mut candidates: Vec<(usize, usize, f64)> = removed.iter().enumerate()
        .flat_map(|(i, ra)| {
            added.iter().enumerate().filter_map(move |(j, ab)| {
                let sim = param_similarity(ra, ab);
                if sim >= RENAME_THRESHOLD { Some((i, j, sim)) } else { None }
            })
        })
        .collect();
    candidates.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));

    let mut used_removed = BTreeSet::new();
    for (i, j, sim) in candidates {
        if used_removed.contains(&i) || used_added.contains(&j) { continue; }
        renames.push((removed[i].name.clone(), added[j].name.clone(), sim));
        used_removed.insert(i);
        used_added.insert(j);
    }

    renames
}

// ─── Main diff ───────────────────────────────────────────────────────────────

pub fn diff(content_a: &[u8], content_b: &[u8]) -> DiffReport {
    let sch_a = parse(content_a);
    let sch_b = parse(content_b);
    let mut report = DiffReport::default();

    let names_a: BTreeSet<_> = sch_a.components.keys().cloned().collect();
    let names_b: BTreeSet<_> = sch_b.components.keys().cloned().collect();

    let only_a: BTreeSet<_> = names_a.difference(&names_b).cloned().collect();
    let only_b: BTreeSet<_> = names_b.difference(&names_a).cloned().collect();
    let common: BTreeSet<_> = names_a.intersection(&names_b).cloned().collect();

    // ── Detección de renombrados ──────────────────────────────────────────────
    let removed_comps: Vec<_> = only_a.iter()
        .filter_map(|n| sch_a.components.get(n))
        .collect();
    let added_comps: Vec<_> = only_b.iter()
        .filter_map(|n| sch_b.components.get(n))
        .collect();

    let renames = detect_renames(&removed_comps, &added_comps);
    let renamed_from: BTreeSet<_> = renames.iter().map(|(a, _, _)| a.clone()).collect();
    let renamed_to: BTreeSet<_> = renames.iter().map(|(_, b, _)| b.clone()).collect();

    for (name_a, name_b, _) in &renames {
        let ca = &sch_a.components[name_a];
        let cb = &sch_b.components[name_b];
        // Renombrado: reportamos como Modified con "name" en before/after
        let mut before = component_snapshot(ca, false);
        let mut after = component_snapshot(cb, false);
        before.insert("name".to_string(), name_a.clone());
        after.insert("name".to_string(), name_b.clone());
        report.components.push(ComponentDiff {
            name: format!("{name_a} → {name_b}"),
            kind: ChangeKind::Modified,
            cosmetic: false,
            before: Some(before),
            after: Some(after),
        });
    }

    // ── Removidos (sin renombrar) ─────────────────────────────────────────────
    for name in only_a.iter().filter(|n| !renamed_from.contains(*n)) {
        if let Some(c) = sch_a.components.get(name) {
            report.components.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Removed,
                cosmetic: false,
                before: Some(component_snapshot(c, false)),
                after: None,
            });
        }
    }

    // ── Añadidos (sin renombrar) ──────────────────────────────────────────────
    for name in only_b.iter().filter(|n| !renamed_to.contains(*n)) {
        if let Some(c) = sch_b.components.get(name) {
            report.components.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Added,
                cosmetic: false,
                before: None,
                after: Some(component_snapshot(c, false)),
            });
        }
    }

    // ── Modificados / cosméticos ──────────────────────────────────────────────
    let mut coord_only_count = 0usize;
    let mut coord_only_entries = Vec::new();

    for name in &common {
        let ca = &sch_a.components[name];
        let cb = &sch_b.components[name];

        let coords_changed =
            (ca.x, ca.y, ca.rotation, ca.mirror) != (cb.x, cb.y, cb.rotation, cb.mirror);
        let params_changed = ca.params != cb.params || ca.symbol != cb.symbol;

        if params_changed {
            report.components.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Modified,
                cosmetic: false,
                before: Some(component_snapshot(ca, false)),
                after: Some(component_snapshot(cb, false)),
            });
        } else if coords_changed {
            coord_only_count += 1;
            coord_only_entries.push(ComponentDiff {
                name: name.clone(),
                kind: ChangeKind::Modified,
                cosmetic: true,
                before: Some(component_snapshot(ca, true)),
                after: Some(component_snapshot(cb, true)),
            });
        }
    }

    report.components.extend(coord_only_entries);

    // Move-all: >80% de componentes comunes solo movieron coordenadas
    if !common.is_empty() && coord_only_count as f64 / common.len() as f64 > 0.8 {
        report.is_move_all = true;
    }

    // ── Nets ─────────────────────────────────────────────────────────────────
    report.nets_added = sch_b.nets.difference(&sch_a.nets).cloned().collect();
    report.nets_removed = sch_a.nets.difference(&sch_b.nets).cloned().collect();

    report
}

// ─── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::models::Component;
    use std::collections::BTreeMap;

    fn make_component(name: &str, symbol: &str, params: &[(&str, &str)]) -> Component {
        Component {
            name: name.to_string(),
            symbol: symbol.to_string(),
            params: params.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect(),
            pins: BTreeMap::new(),
            x: 0.0, y: 0.0, rotation: 0, mirror: 0,
        }
    }

    #[test]
    fn detecta_renombrado_mismo_simbolo_y_valor() {
        use crate::core::models::Schematic;
        let mut sch_a = Schematic::default();
        sch_a.components.insert("R1".to_string(), make_component("R1", "res", &[("value", "10k")]));

        let mut sch_b = Schematic::default();
        sch_b.components.insert("R2".to_string(), make_component("R2", "res", &[("value", "10k")]));

        let removed = vec![sch_a.components.get("R1").unwrap()];
        let added = vec![sch_b.components.get("R2").unwrap()];
        let renames = detect_renames(&removed, &added);

        assert_eq!(renames.len(), 1);
        assert_eq!(renames[0].0, "R1");
        assert_eq!(renames[0].1, "R2");
    }

    #[test]
    fn no_detecta_renombrado_diferente_simbolo() {
        use crate::core::models::Schematic;
        let mut sch_a = Schematic::default();
        sch_a.components.insert("R1".to_string(), make_component("R1", "res", &[("value", "10k")]));

        let mut sch_b = Schematic::default();
        sch_b.components.insert("C1".to_string(), make_component("C1", "cap", &[("value", "10k")]));

        let removed = vec![sch_a.components.get("R1").unwrap()];
        let added = vec![sch_b.components.get("C1").unwrap()];
        let renames = detect_renames(&removed, &added);

        assert!(renames.is_empty());
    }

    #[test]
    fn similitud_identico_es_uno() {
        let a = make_component("R1", "res", &[("value", "10k"), ("footprint", "0402")]);
        let b = make_component("R2", "res", &[("value", "10k"), ("footprint", "0402")]);
        assert_eq!(param_similarity(&a, &b), 1.0);
    }

    #[test]
    fn similitud_diferente_simbolo_es_cero() {
        let a = make_component("R1", "res", &[("value", "10k")]);
        let b = make_component("C1", "cap", &[("value", "10k")]);
        assert_eq!(param_similarity(&a, &b), 0.0);
    }
}
