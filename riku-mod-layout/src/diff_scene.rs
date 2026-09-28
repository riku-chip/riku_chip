//! Escena del diff de una celda: la versión nueva atenuada, el XOR encima
//! (verde = añadido, rojo = eliminado) y la lista de cambios (geometría por
//! capa, celdas y puertos de Magic).

use std::collections::{BTreeMap, HashSet};

use gdstk_rs::{GdsTag, Library};
use viewer_core::{
    bbox::BoundingBox as VcBBox,
    diff::{ChangeItem, ChangeKind},
    element::DrawElement,
    error::{Result as VcResult, ViewerError},
    paint::{LayerPaint, Rgba},
    scene::Scene as VcScene,
};

use crate::diff_cache::{CellDiffDto, ChangedCells, DiffCache};
use crate::gds_diff::{changed_cells, diff_cell_as, CellChange, CellDiff, DiffConfig};
use crate::layer_style::{layer_label, layer_names, tag_tuple, LayerNames};
use crate::process::Process;
use crate::viewer_core_compat::{list_cells, vc_scene_from_cell};

/// Colores del overlay de diff. Relleno semitransparente para ver la capa
/// debajo, contorno opaco para ubicar cambios chicos.
const DIFF_ADDED: (Rgba, Rgba) = (Rgba::new(40, 220, 90, 150), Rgba::new(90, 255, 130, 255));
const DIFF_REMOVED: (Rgba, Rgba) = (Rgba::new(240, 60, 60, 150), Rgba::new(255, 100, 100, 255));

/// Lo que se leyó de cada lado (bytes, o los archivos de una jerarquía
/// Magic) y la cache donde guardar lo que cuesta calcular (celdas cambiadas
/// y XOR de la celda mostrada).
pub(crate) struct CachedDiff<'a> {
    pub cache: &'a DiffCache,
    pub inputs: Vec<&'a [u8]>,
    /// Parámetros de lectura (lambda de Magic), parte de la clave.
    pub read_params: String,
}

impl CachedDiff<'_> {
    fn get<T: serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        kind: &str,
        params: &str,
        compute: impl FnOnce() -> T,
    ) -> T {
        let params = format!("{}{params}", self.read_params);
        let r = self.cache.get_or_compute(kind, &self.inputs, &params, || {
            Ok::<_, std::convert::Infallible>(compute())
        });
        match r {
            Ok((v, _)) => v,
            Err(never) => match never {},
        }
    }
}

/// Arma la escena de diff de una celda. `None` = el archivo no existia de
/// ese lado. Sincrona para poder testearla sin runtime.
pub(crate) fn build_diff_scene(
    lib_a: Option<&Library>,
    lib_b: Option<&Library>,
    entry: Option<&str>,
    path_hint: Option<&str>,
    cached: &CachedDiff<'_>,
) -> VcResult<VcScene> {
    let Some(base_lib) = lib_b.or(lib_a) else {
        return Err(ViewerError::Parse("diff sin contenido en ninguno de los dos lados".into()));
    };

    // Catalogo: celdas de "after" y, a continuacion, las que solo existian antes.
    let mut entries = list_cells(base_lib);
    if let (Some(a), Some(_)) = (lib_a, lib_b) {
        let known: HashSet<String> = entries.iter().map(|e| e.id.clone()).collect();
        entries.extend(list_cells(a).into_iter().filter(|e| !known.contains(&e.id)));
    }

    // Marcar que celdas cambiaron: en una libreria de cientos es lo que
    // permite encontrar el cambio.
    let changed: ChangedCells = cached.get("changed", "", || changed_cells(lib_a, lib_b));
    for e in &mut entries {
        e.change = changed.get(&e.id).map(|c| match c {
            CellChange::Added => ChangeKind::Added,
            CellChange::Removed => ChangeKind::Removed,
            CellChange::Modified | CellChange::Renamed { .. } => ChangeKind::Modified,
        });
        e.renamed_from = match changed.get(&e.id) {
            Some(CellChange::Renamed { from }) => Some(from.clone()),
            _ => None,
        };
    }
    // El nombre viejo de una celda renombrada ya no es una entrada propia.
    let renamed_from: HashSet<&str> = changed
        .values()
        .filter_map(|c| match c {
            CellChange::Renamed { from } => Some(from.as_str()),
            _ => None,
        })
        .collect();
    entries.retain(|e| !renamed_from.contains(e.id.as_str()));

    // Por defecto, la primera celda con cambios (las top van primero en el
    // catalogo); si no hay cambios, la top-cell determinista.
    let name = match entry {
        Some(n) => n.to_string(),
        None => match entries.iter().find(|e| e.change.is_some()) {
            Some(e) => e.id.clone(),
            None => lib_b
                .and_then(crate::select_top_cell)
                .or_else(|| lib_a.and_then(crate::select_top_cell))
                .map(|c| c.name().to_string())
                .ok_or_else(|| ViewerError::Parse("library sin top cells".into()))?,
        },
    };

    // Base visual: la celda en "after"; si ya no existe, la de "before".
    let (lib, cell) = lib_b
        .and_then(|l| l.find_cell(&name).map(|c| (l, c)))
        .or_else(|| lib_a.and_then(|l| l.find_cell(&name).map(|c| (l, c))))
        .ok_or_else(|| ViewerError::Backend(format!("la celda '{name}' no existe en ninguna versión")))?;
    let (mut scene, keys) = vc_scene_from_cell(lib, &cell, path_hint);

    // Atenuar el layout para que resalten los cambios.
    for paint in scene.layers.values_mut() {
        paint.fill.a /= 3;
        paint.stroke.a = 110;
    }

    let cfg = DiffConfig::default();
    // Celda renombrada: comparar con su nombre anterior, no contra vacio.
    let name_a = match changed.get(&name) {
        Some(CellChange::Renamed { from }) => from.as_str(),
        _ => name.as_str(),
    };
    let params = format!("{name_a}\n{name}\n{}", cfg.cosmetic_threshold_um2);
    let diff: CellDiff = cached
        .get("cell", &params, || CellDiffDto::from(&diff_cell_as(lib_a, name_a, lib_b, &name, &cfg)))
        .into();
    let unit_factor = base_lib.unit() / 1e-6;

    // Capas del overlay, al final de la lista (y por encima al pintar).
    let next = scene.layers.keys().next_back().map_or(0, |k| k.saturating_add(1));
    let (k_removed, k_added) = (next, next.saturating_add(1));
    scene.layers.insert(
        k_removed,
        LayerPaint { name: "Δ eliminado".into(), fill: DIFF_REMOVED.0, stroke: DIFF_REMOVED.1, hidden: false },
    );
    scene.layers.insert(
        k_added,
        LayerPaint { name: "Δ añadido".into(), fill: DIFF_ADDED.0, stroke: DIFF_ADDED.1, hidden: false },
    );
    let polygon = |p: &gdstk_rs::OwnedPolygon, layer| DrawElement::Polygon {
        points: p.points.iter().map(|q| (q.x, q.y)).collect(),
        layer,
        filled: true,
    };
    for lp in &diff.polygons {
        for p in &lp.removed {
            scene.push(polygon(p, k_removed));
        }
        for p in &lp.added {
            scene.push(polygon(p, k_added));
        }
    }

    let mut names = lib_a.map(layer_names).unwrap_or_default();
    names.extend(lib_b.map(layer_names).unwrap_or_default());
    scene.changes = change_items(&diff, keys.process, unit_factor, &names);
    scene.notices.extend(diff.failure_notes(&name));
    scene.changes.extend(cell_presence_items(&changed));

    let relevant = diff.geometry.iter().filter(|g| !g.cosmetic).count();
    let cosmetic = diff.geometry.len() - relevant;
    // fold desde +0.0: `sum()` de f64 vacio da -0.0 ("+-0.0000").
    let area_add = diff.geometry.iter().fold(0.0, |acc, g| acc + g.added_area_um2);
    let area_rem = diff.geometry.iter().fold(0.0, |acc, g| acc + g.removed_area_um2);
    let summary = if diff.geometry.is_empty() {
        "sin cambios geométricos".to_string()
    } else {
        format!("{relevant} relevantes · {cosmetic} cosméticos")
    };
    let mut head = vec![("Diff".to_string(), summary)];
    if !diff.geometry.is_empty() {
        head.push(("Área".to_string(), format!("+{area_add:.4} / −{area_rem:.4} µm²")));
    }
    head.push(("Cambiadas".to_string(), format!("{} de {} celdas", changed.len(), entries.len())));
    scene.metadata.splice(0..0, head);
    scene.current_entry = Some(name);
    scene.entries = entries;
    Ok(scene)
}

/// Un item por (capa, origen): relevantes primero y, dentro de cada grupo,
/// los de mayor area.
fn change_items(diff: &CellDiff, process: &'static Process, unit_factor: f64, names: &LayerNames) -> Vec<ChangeItem> {
    let mut geo: Vec<&crate::GdsGeomDiff> = diff.geometry.iter().collect();
    geo.sort_by(|a, b| {
        let area = |g: &crate::GdsGeomDiff| g.added_area_um2 + g.removed_area_um2;
        a.cosmetic.cmp(&b.cosmetic).then(area(b).total_cmp(&area(a)))
    });
    geo.into_iter()
        .map(|g| {
            let tag = GdsTag { layer: g.layer.layer, datatype: g.layer.datatype };
            let layer = layer_label(tag, &process.layer_spec(tag), names.get(&tag_tuple(tag)).map(String::as_str));
            // Un item por instancia: la posicion distingue las copias.
            let label = match (g.origin_path.get(1), g.instance_at_um) {
                (Some(sub), Some((x, y))) => format!("{layer} · en {sub} @ ({x:.2}, {y:.2})"),
                (Some(sub), None) => format!("{layer} · en {sub}"),
                (None, _) => layer,
            };
            let kind = match (g.added_polygons > 0, g.removed_polygons > 0) {
                (true, false) => ChangeKind::Added,
                (false, true) => ChangeKind::Removed,
                _ => ChangeKind::Modified,
            };
            let detail = format!(
                "+{} / −{} pol · +{:.4} / −{:.4} µm²",
                g.added_polygons, g.removed_polygons, g.added_area_um2, g.removed_area_um2
            );
            // bbox_um esta en µm; la escena, en unidades de usuario.
            let bbox = g.bbox_um.map(|b| VcBBox {
                min_x: b.min_x / unit_factor,
                min_y: b.min_y / unit_factor,
                max_x: b.max_x / unit_factor,
                max_y: b.max_y / unit_factor,
            });
            ChangeItem { kind, label, detail, bbox, cosmetic: g.cosmetic }
        })
        .collect()
}

/// Celdas que aparecen o desaparecen en la library (no tienen ubicacion en
/// la celda mostrada).
fn cell_presence_items(changed: &BTreeMap<String, CellChange>) -> Vec<ChangeItem> {
    let item = |kind, what: &str, name: &String| ChangeItem {
        kind,
        label: format!("celda {what}: {name}"),
        detail: String::new(),
        bbox: None,
        cosmetic: false,
    };
    changed
        .iter()
        .filter_map(|(name, c)| match c {
            CellChange::Added => Some(item(ChangeKind::Added, "añadida", name)),
            CellChange::Removed => Some(item(ChangeKind::Removed, "eliminada", name)),
            CellChange::Renamed { from } => Some(item(ChangeKind::Modified, "renombrada", &format!("{from} → {name}"))),
            CellChange::Modified => None,
        })
        .collect()
}

/// Un puerto de Magic que cambió, para la lista del visor (coordenadas en
/// µm, las de la escena de un `.mag`).
pub(crate) fn port_item(p: &crate::mag::PortChange) -> ChangeItem {
    let (kind, label) = match (&p.before, &p.after) {
        (None, Some(d)) => (ChangeKind::Added, format!("puerto {} ({})", p.name, d.class.as_deref().unwrap_or("—"))),
        (Some(_), None) => (ChangeKind::Removed, format!("puerto {}", p.name)),
        (Some(x), Some(y)) if x.class != y.class => (
            ChangeKind::Modified,
            format!("puerto {}: {} → {}", p.name, x.class.as_deref().unwrap_or("—"), y.class.as_deref().unwrap_or("—")),
        ),
        _ => (ChangeKind::Modified, format!("puerto {}", p.name)),
    };
    let detail = match p.after.as_ref().or(p.before.as_ref()) {
        _ if p.cosmetic => "se movió".to_string(),
        Some(d) => format!("{} {} · {}", d.index, d.usage.as_deref().unwrap_or(""), d.layers.join(",")),
        None => String::new(),
    };
    let bbox = p.after.as_ref().or(p.before.as_ref()).and_then(|d| d.rects_um.first()).map(|r| VcBBox {
        min_x: r[0],
        min_y: r[1],
        max_x: r[2],
        max_y: r[3],
    });
    ChangeItem { kind, label, detail, bbox, cosmetic: p.cosmetic }
}

