//! Adaptador `riku-mod-layout` ↔ `viewer-core`.
//!
//! Expone `GdsBackend`, que implementa `ViewerBackend` para que `riku-gui`
//! abra archivos `.gds`/`.oas` por la ruta neutra: `Library::from_bytes_any` →
//! `draw_commands` → conversión `DrawCommand → DrawElement` → `Scene`.
//!
//! La escena resultante es Y-up y trae un `LayerPaint` por cada
//! (layer, datatype): el campo `layer` de cada `DrawElement` es la clave de
//! ese estilo, no el numero de layer GDS crudo.

use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use gdstk_rs::{Anchor, GdsTag, Library, Point2D};
use viewer_core::{
    backend::{BackendInfo, ViewerBackend},
    bbox::BoundingBox as VcBBox,
    diff::{ChangeItem, ChangeKind},
    element::{DrawElement, HAlign, Layer, VAlign},
    error::{Result as VcResult, ViewerError},
    paint::{LayerPaint, Rgba},
    scene::{Scene as VcScene, SceneHandle, ViewEntry},
    viewport::YAxis,
    CancellationToken,
};

use crate::diff_cache::{CellDiffDto, ChangedCells, DiffCache};
use crate::gds_diff::{changed_cells, diff_cell_as, CellChange, CellDiff, DiffConfig};
use crate::palette::{detect_pdk, layer_spec, LayerRole};
use crate::scene::DrawCommand;
use crate::style::Pdk;

pub struct GdsBackend {
    /// Cache del diff (celdas cambiadas y XOR) para layouts grandes.
    cache: DiffCache,
}

impl GdsBackend {
    pub fn new() -> Self {
        Self { cache: DiffCache::from_env() }
    }

    /// Backend con una cache concreta (tests, o `DiffCache::disabled()`).
    pub fn with_cache(cache: DiffCache) -> Self {
        Self { cache }
    }
}

impl Default for GdsBackend {
    fn default() -> Self {
        Self::new()
    }
}

/// Opacidad del relleno segun el rol de la capa. Dispositivo: se ve el color
/// y tambien lo que queda debajo. Pozo: apenas un tinte (cubre media celda).
/// Contorno: sin relleno (implantes, marcadores, pines, boundary).
fn fill_alpha(role: LayerRole) -> u8 {
    match role {
        LayerRole::Device => 90,
        LayerRole::Well => 28,
        LayerRole::Outline => 0,
    }
}

/// Asigna una clave `Layer` (u16) por cada (layer, datatype) distinto, en
/// orden de apilado del PDK (y luego (layer, datatype) para desempatar), asi
/// la lista de capas de la UI sale de abajo hacia arriba. `68/20` (met1) y
/// `68/16` (met1.pin) tienen claves y estilos distintos.
///
/// Se indexa por tupla porque `GdsTag` (struct compartido de cxx) no es `Ord`.
struct LayerKeys {
    keys: BTreeMap<(u32, u32), Layer>,
    pdk: Pdk,
}

impl LayerKeys {
    fn new(commands: &[DrawCommand], path_hint: Option<&str>) -> Self {
        let tags: BTreeSet<(u32, u32)> = commands.iter().map(|c| tag_tuple(c.tag())).collect();
        let as_gds: Vec<GdsTag> = tags.iter().map(|&t| gds_tag(t)).collect();
        let pdk = detect_pdk(path_hint, &as_gds);

        let mut ordered: Vec<(u32, u32)> = tags.into_iter().collect();
        ordered.sort_by_key(|&t| (layer_spec(gds_tag(t), pdk).rank, t));
        let keys = ordered
            .into_iter()
            .enumerate()
            .map(|(i, t)| (t, i.min(u16::MAX as usize) as Layer))
            .collect();
        Self { keys, pdk }
    }

    fn key(&self, tag: GdsTag) -> Layer {
        self.keys.get(&tag_tuple(tag)).copied().unwrap_or(u16::MAX)
    }

    fn paints(&self) -> BTreeMap<Layer, LayerPaint> {
        self.keys
            .iter()
            .map(|(&t, key)| (*key, layer_paint(gds_tag(t), self.pdk)))
            .collect()
    }
}

fn tag_tuple(tag: GdsTag) -> (u32, u32) {
    (tag.layer, tag.datatype)
}

fn gds_tag((layer, datatype): (u32, u32)) -> GdsTag {
    GdsTag { layer, datatype }
}

fn layer_paint(tag: GdsTag, pdk: Pdk) -> LayerPaint {
    let spec = layer_spec(tag, pdk);
    let c = spec.color;
    let stroke = Rgba::new(c.r, c.g, c.b, 255);
    let name = match spec.name {
        Some(name) => format!("{name} {}/{}", tag.layer, tag.datatype),
        None => format!("{}/{}", tag.layer, tag.datatype),
    };
    LayerPaint { name, fill: stroke.with_alpha(fill_alpha(spec.role)), stroke }
}

/// Alto de las etiquetas como fraccion del lado mayor de la cell. GDS no
/// guarda tamano de texto util (los LABEL suelen ser puntos), asi que se
/// escala con la geometria para que no tapen el layout.
const LABEL_FRACTION: f64 = 0.03;

fn label_size(bbox: &VcBBox) -> f64 {
    let side = bbox.width().max(bbox.height());
    if side.is_finite() && side > 0.0 {
        side * LABEL_FRACTION
    } else {
        1.0
    }
}

/// Anchor GDS → alineacion neutra. En Y-up "N" (norte) es el borde superior
/// del texto, que en pantalla sigue siendo `Top`.
fn anchor_align(anchor: Anchor) -> (HAlign, VAlign) {
    let h = match anchor {
        Anchor::NW | Anchor::W | Anchor::SW => HAlign::Start,
        Anchor::N | Anchor::O | Anchor::S => HAlign::Middle,
        Anchor::NE | Anchor::E | Anchor::SE => HAlign::End,
    };
    // El anchor nombra el punto del texto que cae en `origin`: NW = la esquina
    // superior izquierda del texto esta en origin.
    let v = match anchor {
        Anchor::NW | Anchor::N | Anchor::NE => VAlign::Top,
        Anchor::W | Anchor::O | Anchor::E => VAlign::Middle,
        Anchor::SW | Anchor::S | Anchor::SE => VAlign::Bottom,
    };
    (h, v)
}

fn command_to_element(cmd: &DrawCommand, keys: &LayerKeys, text_size: f64) -> Option<DrawElement> {
    match cmd {
        DrawCommand::Polygon { tag, points } => Some(DrawElement::Polygon {
            points: points.iter().map(|p: &Point2D| (p.x, p.y)).collect(),
            layer: keys.key(*tag),
            filled: true,
        }),
        DrawCommand::Label { tag, text, origin, anchor } => {
            let (h_align, v_align) = anchor_align(*anchor);
            Some(DrawElement::Text {
                x: origin.x,
                y: origin.y,
                content: text.clone(),
                size: text_size,
                angle_deg: 0.0,
                h_align,
                v_align,
                layer: keys.key(*tag),
            })
        }
    }
}

/// Escena de una cell y el PDK detectado (lo reusa el diff para nombrar capas).
fn vc_scene_from_cell(lib: &Library, cell: &gdstk_rs::Cell<'_>, path_hint: Option<&str>) -> (VcScene, Pdk) {
    let draw = crate::scene::draw_commands(lib, cell);
    let keys = LayerKeys::new(&draw, path_hint);

    let mut scene = VcScene::new();
    // GDS usa la convencion matematica: Y crece hacia arriba.
    scene.y_axis = YAxis::Up;
    scene.world_unit = Some(unit_label(lib.unit()));
    scene.layers = keys.paints();
    // Sembrar el bbox con el de la cell aunque algún DrawCommand no contribuya
    // (Scene::push lo expandirá igualmente con cada elemento).
    let cb = cell.bbox();
    if cb.min_x.is_finite()
        && cb.min_y.is_finite()
        && cb.max_x.is_finite()
        && cb.max_y.is_finite()
    {
        scene.bbox.expand(&VcBBox {
            min_x: cb.min_x,
            min_y: cb.min_y,
            max_x: cb.max_x,
            max_y: cb.max_y,
        });
    }

    // Orden de pintado: poligonos por apilado (las claves ya siguen el rank)
    // y los textos al final para que queden encima. Sort estable: dentro de
    // una capa se conserva el orden del archivo.
    let mut commands: Vec<&DrawCommand> = draw.iter().collect();
    commands.sort_by_key(|c| (matches!(c, DrawCommand::Label { .. }), keys.key(c.tag())));

    let text_size = label_size(&scene.bbox);
    let (mut polygons, mut labels) = (0usize, 0usize);
    for cmd in commands {
        match cmd {
            DrawCommand::Polygon { .. } => polygons += 1,
            DrawCommand::Label { .. } => labels += 1,
        }
        if let Some(el) = command_to_element(cmd, &keys, text_size) {
            scene.push(el);
        }
    }

    scene.metadata = vec![
        ("Celda".into(), cell.name().to_string()),
        ("PDK".into(), pdk_name(keys.pdk).into()),
        ("Polígonos".into(), polygons.to_string()),
        ("Etiquetas".into(), labels.to_string()),
        ("Capas".into(), scene.layers.len().to_string()),
        (
            "Tamaño".into(),
            format!("{:.3} × {:.3} µm", scene.bbox.width(), scene.bbox.height()),
        ),
    ];
    (scene, keys.pdk)
}

/// Unidad de las coordenadas de usuario (`Library::unit()` = metros por
/// unidad). Los PDKs usan micrometros; el resto se muestra explicito.
fn unit_label(meters: f64) -> String {
    let near = |v: f64| (meters / v - 1.0).abs() < 1e-9;
    if near(1e-6) {
        "µm".into()
    } else if near(1e-9) {
        "nm".into()
    } else {
        format!("×{meters:e} m")
    }
}

fn pdk_name(pdk: Pdk) -> &'static str {
    match pdk {
        Pdk::Sky130 => "SKY130",
        Pdk::Gf180 => "GF180MCU",
        Pdk::Ihp => "IHP SG13G2",
        Pdk::Generic => "genérico",
    }
}

#[async_trait]
impl ViewerBackend for GdsBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: "gds",
            version: env!("CARGO_PKG_VERSION"),
            extensions: &["gds", "oas"],
        }
    }

    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool {
        if let Some(p) = path_hint {
            let p = p.to_ascii_lowercase();
            if p.ends_with(".gds") || p.ends_with(".oas") {
                return true;
            }
        }
        crate::is_layout(content)
    }

    async fn load(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        self.load_entry(content, path_hint, None, token).await
    }

    /// `entry` es el nombre de una celda. Se re-parsea el archivo en cada
    /// llamada: `Library` no es `Send` y el parseo es barato (~30 ms para la
    /// libreria completa de celdas estandar de SKY130, 4 MB).
    async fn load_entry(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        if token.is_cancelled() {
            return Err(ViewerError::Cancelled);
        }
        let scene = tokio::task::spawn_blocking(move || -> VcResult<VcScene> {
            let lib = Library::from_bytes_any(&content)
                .map_err(|e| ViewerError::Parse(format!("GDSII parse: {e}")))?;

            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }

            let entries = list_cells(&lib);
            let cell = match entry.as_deref() {
                // Celda pedida explicitamente: si no existe es un error, no se
                // sustituye en silencio por otra.
                Some(name) => lib
                    .find_cell(name)
                    .ok_or_else(|| ViewerError::Backend(format!("la celda '{name}' no existe en el archivo")))?,
                // Por defecto: top-cell determinista (alfabetica en empate).
                // Library vacia o ciclica -> escena vacia (no es error).
                None => match crate::select_top_cell(&lib) {
                    Some(cell) => cell,
                    None => {
                        let mut scene = VcScene::default();
                        scene.entries = entries;
                        return Ok(scene);
                    }
                },
            };

            let (mut scene, _) = vc_scene_from_cell(&lib, &cell, path_hint.as_deref());
            let tops = entries.iter().filter(|e| e.is_root).count();
            // Justo despues de "Celda": cuantas celdas hay para elegir.
            scene.metadata.insert(1, ("Celdas".into(), format!("{tops} top / {} total", entries.len())));
            scene.current_entry = Some(cell.name().to_string());
            scene.entries = entries;
            // Índice espacial (culling y nivel de detalle): aquí, fuera del hilo de la UI.
            scene.build_index();
            Ok(scene)
        })
        .await??;

        Ok(Arc::new(scene) as SceneHandle)
    }

    /// Diff geometrico de una celda: la version "after" atenuada con el XOR
    /// encima (verde = anadido, rojo = eliminado) y la lista de cambios.
    async fn load_diff(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        if token.is_cancelled() {
            return Err(ViewerError::Cancelled);
        }
        let cache = self.cache.clone();
        let scene = tokio::task::spawn_blocking(move || -> VcResult<VcScene> {
            let parse = |bytes: &[u8], side: &str| -> VcResult<Option<Library>> {
                if bytes.is_empty() {
                    return Ok(None);
                }
                Library::from_bytes_any(bytes)
                    .map(Some)
                    .map_err(|e| ViewerError::Parse(format!("GDSII ({side}): {e}")))
            };
            let lib_a = parse(&before, "antes")?;
            let lib_b = parse(&after, "después")?;
            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }
            let diff = CachedDiff { cache: &cache, before: &before, after: &after };
            build_diff_scene(lib_a.as_ref(), lib_b.as_ref(), entry.as_deref(), path_hint.as_deref(), &diff).map(|mut s| {
                s.build_index();
                s
            })
        })
        .await??;

        Ok(Arc::new(scene) as SceneHandle)
    }
}

/// Colores del overlay de diff. Relleno semitransparente para ver la capa
/// debajo, contorno opaco para ubicar cambios chicos.
const DIFF_ADDED: (Rgba, Rgba) = (Rgba::new(40, 220, 90, 150), Rgba::new(90, 255, 130, 255));
const DIFF_REMOVED: (Rgba, Rgba) = (Rgba::new(240, 60, 60, 150), Rgba::new(255, 100, 100, 255));

/// Bytes crudos de cada lado y la cache donde guardar lo que cuesta calcular
/// (celdas cambiadas y XOR de la celda mostrada).
struct CachedDiff<'a> {
    cache: &'a DiffCache,
    before: &'a [u8],
    after: &'a [u8],
}

impl CachedDiff<'_> {
    fn get<T: serde::Serialize + serde::de::DeserializeOwned>(
        &self,
        kind: &str,
        params: &str,
        compute: impl FnOnce() -> T,
    ) -> T {
        let r = self.cache.get_or_compute(kind, &[self.before, self.after], params, || {
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
fn build_diff_scene(
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
    let (mut scene, pdk) = vc_scene_from_cell(lib, &cell, path_hint);

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
        LayerPaint { name: "Δ eliminado".into(), fill: DIFF_REMOVED.0, stroke: DIFF_REMOVED.1 },
    );
    scene.layers.insert(
        k_added,
        LayerPaint { name: "Δ añadido".into(), fill: DIFF_ADDED.0, stroke: DIFF_ADDED.1 },
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

    scene.changes = change_items(&diff, pdk, unit_factor);
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
fn change_items(diff: &CellDiff, pdk: Pdk, unit_factor: f64) -> Vec<ChangeItem> {
    let mut geo: Vec<&crate::GdsGeomDiff> = diff.geometry.iter().collect();
    geo.sort_by(|a, b| {
        let area = |g: &crate::GdsGeomDiff| g.added_area_um2 + g.removed_area_um2;
        a.cosmetic.cmp(&b.cosmetic).then(area(b).total_cmp(&area(a)))
    });
    geo.into_iter()
        .map(|g| {
            let tag = GdsTag { layer: g.layer.layer, datatype: g.layer.datatype };
            let layer = match layer_spec(tag, pdk).name {
                Some(n) => format!("{n} {}/{}", tag.layer, tag.datatype),
                None => format!("{}/{}", tag.layer, tag.datatype),
            };
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

/// Todas las celdas de la library como `ViewEntry`: primero las top cells y
/// luego el resto, cada grupo en orden alfabetico. El tamano sale del bbox
/// (celdas sin geometria -> `None`).
pub fn list_cells(lib: &Library) -> Vec<ViewEntry> {
    let tops = lib.top_level();
    let top_names: HashSet<String> = (0..tops.count()).map(|i| tops.cell(i).name().to_string()).collect();

    let mut entries: Vec<ViewEntry> = lib
        .cells()
        .map(|cell| {
            let b = cell.bbox();
            // gdstk da bbox (0,0,0,0) para celdas vacias: sin tamano real.
            let (w, h) = (b.max_x - b.min_x, b.max_y - b.min_y);
            let size = ([b.min_x, b.min_y, b.max_x, b.max_y].iter().all(|v| v.is_finite())
                && (w > 0.0 || h > 0.0))
                .then_some((w, h));
            let id = cell.name().to_string();
            ViewEntry { is_root: top_names.contains(&id), id, size, change: None }
        })
        .collect();
    entries.sort_by(|a, b| b.is_root.cmp(&a.is_root).then_with(|| a.id.cmp(&b.id)));
    entries
}

#[cfg(test)]
mod tests {
    use super::*;

    fn proof_lib_bytes() -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("external")
            .join("gdstk")
            .join("tests")
            .join("proof_lib.gds");
        std::fs::read(&path)
            .unwrap_or_else(|e| panic!("no se pudo leer {}: {e}", path.display()))
    }

    #[test]
    fn accepts_gds_extension() {
        let b = GdsBackend::new();
        assert!(b.accepts(b"", Some("foo.gds")));
        assert!(b.accepts(b"", Some("path/to/Bar.GDS")));
        assert!(!b.accepts(b"", Some("foo.sch")));
    }

    #[test]
    fn accepts_gds_magic_without_hint() {
        let b = GdsBackend::new();
        let bytes = [0x00u8, 0x06, 0x00, 0x02, 0x01, 0x00];
        assert!(b.accepts(&bytes, None));
        assert!(!b.accepts(b"NOT_A_GDS", None));
    }

    #[tokio::test]
    async fn oasis_file_loads_like_its_gds_twin() {
        let b = GdsBackend::new();
        let oas = fixture("hier_inv_b.oas");
        assert!(b.accepts(&oas, None) && b.accepts(&[], Some("chip.OAS")));
        let h = b
            .load_entry(oas, Some("hier_inv_b.oas".into()), Some("TOP".into()), CancellationToken::new())
            .await
            .expect("load .oas");
        let g = b
            .load_entry(fixture("hier_inv_b.gds"), None, Some("TOP".into()), CancellationToken::new())
            .await
            .expect("load .gds");
        assert_eq!(h.current_entry(), Some("TOP"));
        assert_eq!(h.bbox(), g.bbox());
        assert_eq!(h.entries().len(), g.entries().len());
    }

    #[tokio::test]
    async fn load_proof_lib_returns_nonempty_scene() {
        let bytes = proof_lib_bytes();
        let backend = GdsBackend::new();
        let handle = backend
            .load(bytes, None, CancellationToken::new())
            .await
            .expect("load proof_lib");
        assert!(handle.len() > 0, "esperaba elementos, got {}", handle.len());
        assert!(!handle.bbox().is_empty(), "bbox no debe estar vacío");
    }

    #[tokio::test]
    async fn load_invalid_returns_parse_error() {
        let backend = GdsBackend::new();
        let res = backend
            .load(b"NOT_A_GDS_FILE".to_vec(), None, CancellationToken::new())
            .await;
        match res {
            Err(ViewerError::Parse(_)) => {}
            Err(e) => panic!("esperaba ViewerError::Parse, got {e:?}"),
            Ok(_) => panic!("esperaba error, got Ok"),
        }
    }

    #[tokio::test]
    async fn load_respects_pre_cancelled_token() {
        let backend = GdsBackend::new();
        let token = CancellationToken::new();
        token.cancel();
        let res = backend.load(proof_lib_bytes(), None, token).await;
        match res {
            Err(ViewerError::Cancelled) => {}
            Err(e) => panic!("esperaba ViewerError::Cancelled, got {e:?}"),
            Ok(_) => panic!("esperaba error, got Ok"),
        }
    }

    fn fixture(name: &str) -> Vec<u8> {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("leer {}: {e}", path.display()))
    }

    async fn load(bytes: Vec<u8>, hint: Option<&str>) -> SceneHandle {
        GdsBackend::new()
            .load(bytes, hint.map(str::to_string), CancellationToken::new())
            .await
            .expect("load")
    }

    #[tokio::test]
    async fn scene_is_y_up_and_every_element_has_paint() {
        let handle = load(proof_lib_bytes(), None).await;
        assert_eq!(handle.y_axis(), YAxis::Up);
        let mut missing = 0;
        handle.visit(&VcBBox::empty(), &mut |el| {
            match handle.layer_paint(el.layer()) {
                Some(p) => assert!(p.fill.a < 255 && p.stroke.a == 255, "relleno translucido, borde opaco"),
                None => missing += 1,
            }
            true
        });
        assert_eq!(missing, 0, "elementos sin LayerPaint");
    }

    #[tokio::test]
    async fn datatype_is_part_of_layer_style() {
        let a = load(fixture("datatype_a.gds"), None).await;
        let b = load(fixture("datatype_b.gds"), None).await;
        let name = |h: &SceneHandle| {
            let mut n = String::new();
            h.visit(&VcBBox::empty(), &mut |el| {
                n = h.layer_paint(el.layer()).unwrap().name.clone();
                false
            });
            n
        };
        assert_eq!(name(&a), "1/0");
        assert_eq!(name(&b), "1/1");
    }

    #[test]
    fn anchor_maps_to_alignment() {
        assert_eq!(anchor_align(Anchor::O), (HAlign::Middle, VAlign::Middle));
        assert_eq!(anchor_align(Anchor::NW), (HAlign::Start, VAlign::Top));
        assert_eq!(anchor_align(Anchor::SE), (HAlign::End, VAlign::Bottom));
    }

    #[tokio::test]
    async fn labels_are_painted_after_polygons_and_metadata_is_set() {
        let handle = load(proof_lib_bytes(), None).await;
        let mut seen_text = false;
        let mut polygon_after_text = false;
        handle.visit(&VcBBox::empty(), &mut |el| {
            match el {
                DrawElement::Text { .. } => seen_text = true,
                _ if seen_text => polygon_after_text = true,
                _ => {}
            }
            true
        });
        assert!(!polygon_after_text, "los textos deben ir al final");
        let meta = handle.metadata();
        assert!(meta.iter().any(|(k, _)| k == "Celda"));
        let capas: usize = meta.iter().find(|(k, _)| k == "Capas").unwrap().1.parse().unwrap();
        assert_eq!(handle.layer_list().len(), capas);
    }

    #[tokio::test]
    async fn sky130_path_hint_names_layers() {
        // El fixture usa layer 1 (no es de SKY130): sin match cae al nombre crudo,
        // pero el hint no debe romper la carga.
        let h = load(fixture("datatype_a.gds"), Some("/foss/pdks/sky130A/x.gds")).await;
        assert!(!h.is_empty());
    }

    async fn load_cell(name: &str, cell: Option<&str>) -> VcResult<SceneHandle> {
        GdsBackend::new()
            .load_entry(fixture(name), None, cell.map(str::to_string), CancellationToken::new())
            .await
    }

    fn ids(h: &SceneHandle) -> Vec<(&str, bool)> {
        h.entries().iter().map(|e| (e.id.as_str(), e.is_root)).collect()
    }

    #[tokio::test]
    async fn entries_list_tops_first_then_rest_alphabetically() {
        let multi = load(fixture("top_multi.gds"), None).await;
        assert_eq!(ids(&multi), vec![("ALPHA", true), ("BETA", true), ("ZETA", true)]);

        let nested = load(fixture("top_nested.gds"), None).await;
        assert_eq!(ids(&nested), vec![("TOP", true), ("GATE", false), ("INV", false)]);
        let gate = &nested.entries()[1];
        let (w, h) = gate.size.expect("GATE tiene geometria");
        assert!((w - 1.0).abs() < 1e-9 && (h - 1.0).abs() < 1e-9);
    }

    #[tokio::test]
    async fn default_entry_matches_select_top_cell() {
        let h = load_cell("top_multi.gds", None).await.expect("load");
        assert_eq!(h.current_entry(), Some("ALPHA"));
        let celdas = h.metadata().iter().find(|(k, _)| k == "Celdas").map(|(_, v)| v.as_str());
        assert_eq!(celdas, Some("3 top / 3 total"));
    }

    #[tokio::test]
    async fn explicit_entry_loads_that_cell_even_if_not_top() {
        let h = load_cell("top_nested.gds", Some("INV")).await.expect("load INV");
        assert_eq!(h.current_entry(), Some("INV"));
        // INV contiene GATE desplazado (5,5): el bbox no es el de TOP (+10,+10).
        let b = h.bbox();
        assert!((b.min_x - 5.0).abs() < 1e-9 && (b.min_y - 5.0).abs() < 1e-9, "{b:?}");
        assert_eq!(h.entries().len(), 3, "el catalogo viaja con cualquier celda");
    }

    #[tokio::test]
    async fn unknown_entry_is_an_error() {
        match load_cell("top_multi.gds", Some("NO_EXISTE")).await {
            Err(ViewerError::Backend(msg)) => assert!(msg.contains("NO_EXISTE")),
            Err(e) => panic!("esperaba Backend, got {e:?}"),
            Ok(_) => panic!("esperaba error"),
        }
    }

    async fn diff(a: Option<&str>, b: &str, cell: Option<&str>) -> SceneHandle {
        let before = a.map(fixture).unwrap_or_default();
        GdsBackend::new()
            .load_diff(before, fixture(b), None, cell.map(str::to_string), CancellationToken::new())
            .await
            .expect("load_diff")
    }

    /// Cantidad de poligonos pintados en la capa de overlay `name`.
    fn overlay_count(h: &SceneHandle, name: &str) -> usize {
        let key = h.layer_list().into_iter().find(|(_, p)| p.name == name).map(|(k, _)| k).expect(name);
        let mut n = 0;
        h.visit(&VcBBox::empty(), &mut |el| {
            n += usize::from(el.layer() == key);
            true
        });
        n
    }

    fn meta<'a>(h: &'a SceneHandle, key: &str) -> &'a str {
        h.metadata().iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str()).unwrap_or("")
    }

    #[tokio::test]
    async fn diff_datatype_change_is_one_removed_and_one_added() {
        let h = diff(Some("datatype_a.gds"), "datatype_b.gds", None).await;
        assert_eq!(overlay_count(&h, "Δ eliminado"), 1);
        assert_eq!(overlay_count(&h, "Δ añadido"), 1);
        let kinds: Vec<_> = h.changes().iter().map(|c| (c.label.as_str(), c.kind)).collect();
        assert!(kinds.contains(&("1/0", ChangeKind::Removed)), "{kinds:?}");
        assert!(kinds.contains(&("1/1", ChangeKind::Added)), "{kinds:?}");
        assert!(h.changes().iter().all(|c| c.bbox.is_some() && !c.cosmetic));
        assert_eq!(meta(&h, "Diff"), "2 relevantes · 0 cosméticos");
    }

    #[tokio::test]
    async fn identical_files_have_no_changes() {
        let h = diff(Some("datatype_a.gds"), "datatype_a.gds", None).await;
        assert!(h.changes().is_empty());
        assert_eq!(overlay_count(&h, "Δ añadido"), 0);
        assert_eq!(meta(&h, "Diff"), "sin cambios geométricos");
    }

    #[tokio::test]
    async fn diff_scene_read_from_cache_is_identical() {
        let dir = std::env::temp_dir().join(format!("riku-backend-cache-{}", std::process::id()));
        let b = GdsBackend::with_cache(DiffCache::at(&dir));
        let load = || {
            b.load_diff(
                fixture("multi_inst_a.gds"),
                fixture("multi_inst_b.gds"),
                None,
                Some("ARR".into()),
                CancellationToken::new(),
            )
        };
        let first = load().await.expect("primera carga");
        let entries = std::fs::read_dir(&dir).expect("cache escrita").count();
        assert_eq!(entries, 2, "celdas cambiadas + XOR de ARR");
        let second = load().await.expect("desde la cache");
        assert_eq!(first.changes(), second.changes());
        assert_eq!(overlay_count(&first, "Δ añadido"), 6);
        assert_eq!(overlay_count(&second, "Δ añadido"), 6);
        assert_eq!(first.entries(), second.entries());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[tokio::test]
    async fn renamed_cell_is_shown_as_rename_without_geometry_changes() {
        let h = diff(Some("rename_a.gds"), "rename_b.gds", Some("INV_X1")).await;
        assert_eq!(overlay_count(&h, "Δ añadido"), 0, "comparada contra su nombre anterior");
        assert!(h.changes().iter().any(|c| c.label == "celda renombrada: INV → INV_X1"));
        let ids: Vec<&str> = h.entries().iter().map(|e| e.id.as_str()).collect();
        assert!(ids.contains(&"INV_X1") && !ids.contains(&"INV"), "{ids:?}");
    }

    #[tokio::test]
    async fn missing_before_means_everything_added() {
        let h = diff(None, "datatype_b.gds", None).await;
        assert_eq!(overlay_count(&h, "Δ añadido"), 1);
        assert_eq!(overlay_count(&h, "Δ eliminado"), 0);
        assert!(h.changes().iter().any(|c| c.label == "celda añadida: TOP"));
    }

    #[tokio::test]
    async fn change_inside_subcell_is_located_in_top_coordinates() {
        let h = diff(Some("hier_inv_a.gds"), "hier_inv_b.gds", Some("TOP")).await;
        let c = &h.changes()[0];
        assert_eq!((c.label.as_str(), c.kind), ("1/0 · en INV @ (10.00, 10.00)", ChangeKind::Added));
        let b = c.bbox.expect("bbox");
        // Rect (2,0)-(3,1) de INV instanciado en (10,10).
        assert!((b.min_x - 12.0).abs() < 1e-9 && (b.max_y - 11.0).abs() < 1e-9, "{b:?}");
        assert_eq!(h.current_entry(), Some("TOP"));
    }

    #[tokio::test]
    async fn diff_marks_changed_cells_and_opens_first_changed() {
        // INV cambia y TOP la instancia: ambas modificadas.
        let h = diff(Some("hier_inv_a.gds"), "hier_inv_b.gds", None).await;
        let marks: Vec<_> = h.entries().iter().map(|e| (e.id.as_str(), e.change)).collect();
        assert_eq!(marks, vec![("TOP", Some(ChangeKind::Modified)), ("INV", Some(ChangeKind::Modified))]);
        assert_eq!(h.current_entry(), Some("TOP"), "top cambiada primero");
        assert_eq!(meta(&h, "Cambiadas"), "2 de 2 celdas");

        let same = diff(Some("hier_inv_a.gds"), "hier_inv_a.gds", None).await;
        assert!(same.entries().iter().all(|e| e.change.is_none()));
        assert_eq!(meta(&same, "Área"), "", "sin cambios no hay fila de área");
    }
}
