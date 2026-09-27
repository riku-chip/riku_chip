//! Adaptador `riku-mod-layout` ↔ `viewer-core`.
//!
//! Expone `GdsBackend`, que implementa `ViewerBackend` para que `riku-gui`
//! abra archivos `.gds`/`.oas`/`.mag` por la ruta neutra: `Library` (de los
//! bytes, o de la jerarquía Magic con sus sub-celdas, ver [`crate::source`])
//! → polígonos aplanados y etiquetas → `Scene`. El diff arma su escena en
//! [`crate::diff_scene`]; el estilo de las capas sale de
//! [`crate::layer_style`].
//!
//! La escena resultante es Y-up y trae un `LayerPaint` por cada
//! (layer, datatype): el campo `layer` de cada `DrawElement` es la clave de
//! ese estilo, no el numero de layer GDS crudo.

use async_trait::async_trait;
use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::sync::Arc;

use gdstk_rs::{Anchor, GdsTag, Library};
use viewer_core::{
    files::{DiffFiles, FileSource},
    backend::{BackendInfo, ViewerBackend},
    bbox::BoundingBox as VcBBox,
    element::{DrawElement, HAlign, Layer, VAlign},
    error::{Result as VcResult, ViewerError},
    scene::{Scene as VcScene, SceneHandle, ViewEntry},
    viewport::YAxis,
    CancellationToken,
};

use crate::diff_cache::DiffCache;
use crate::diff_scene::{build_diff_scene, port_item, CachedDiff};
use crate::layer_style::{tag_tuple, layer_names, LayerKeys};
use crate::source::{self, LibCache, Raw, ReadError};

pub struct GdsBackend {
    /// Cache del diff (celdas cambiadas y XOR) para layouts grandes.
    cache: DiffCache,
    /// Las últimas bibliotecas leídas: cambiar de celda o de pestaña no
    /// vuelve a leer el archivo.
    libs: Arc<LibCache>,
}

impl GdsBackend {
    pub fn new() -> Self {
        Self::with_cache(DiffCache::from_env())
    }

    /// Backend con una cache concreta (tests, o `DiffCache::disabled()`).
    pub fn with_cache(cache: DiffCache) -> Self {
        Self { cache, libs: Arc::default() }
    }
}

impl Default for GdsBackend {
    fn default() -> Self {
        Self::new()
    }
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

fn label_element(label: crate::labels::FlatLabel, layer: Layer, text_size: f64) -> DrawElement {
    let (h_align, v_align) = anchor_align(label.anchor);
    DrawElement::Text {
        x: label.origin.x,
        y: label.origin.y,
        content: label.text,
        size: text_size,
        angle_deg: 0.0,
        h_align,
        v_align,
        layer,
    }
}

/// Escena de una cell y el estilo de sus capas (lo reusa el diff para
/// nombrar capas).
///
/// Los polígonos aplanados por gdstk pasan directo a elementos de la escena
/// (sin una lista intermedia: en el chip de 42 MB eran ~330 MB de pico).
pub(crate) fn vc_scene_from_cell(lib: &Library, cell: &gdstk_rs::Cell<'_>, path_hint: Option<&str>) -> (VcScene, LayerKeys) {
    let flat = cell.get_polygons().build();
    let labels = crate::labels::flatten_labels(lib, cell);
    let tags: BTreeSet<(u32, u32)> = flat
        .polygons()
        .map(|p| (p.layer(), p.datatype()))
        .chain(labels.iter().map(|l| tag_tuple(l.tag)))
        .collect();
    let keys = LayerKeys::new(tags, path_hint, layer_names(lib));

    let mut scene = VcScene::new();
    // GDS usa la convencion matematica: Y crece hacia arriba.
    scene.y_axis = YAxis::Up;
    scene.world_unit = Some(unit_label(lib.unit()));
    scene.layers = keys.paints();
    // Sembrar el bbox con el de la cell aunque algún polígono no contribuya
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
    // y los textos al final para que queden encima. Dentro de una capa, el
    // orden del archivo (una lista por capa, en orden de clave).
    let text_size = label_size(&scene.bbox);
    let polygons = flat.count() as usize;
    let mut by_layer: BTreeMap<Layer, Vec<DrawElement>> = BTreeMap::new();
    for p in flat.polygons() {
        let layer = keys.key(GdsTag { layer: p.layer(), datatype: p.datatype() });
        let points = p.points().map(|q| (q.x, q.y)).collect();
        by_layer.entry(layer).or_default().push(DrawElement::Polygon { points, layer, filled: true });
    }
    drop(flat);
    for el in by_layer.into_values().flatten() {
        scene.push(el);
    }
    let labels_count = labels.len();
    let mut by_layer: BTreeMap<Layer, Vec<DrawElement>> = BTreeMap::new();
    for l in labels {
        let layer = keys.key(l.tag);
        by_layer.entry(layer).or_default().push(label_element(l, layer, text_size));
    }
    for el in by_layer.into_values().flatten() {
        scene.push(el);
    }
    let labels = labels_count;

    scene.metadata = vec![
        ("Celda".into(), cell.name().to_string()),
        ("PDK".into(), keys.process.name.clone()),
        ("Polígonos".into(), polygons.to_string()),
        ("Etiquetas".into(), labels.to_string()),
        ("Capas".into(), scene.layers.len().to_string()),
        (
            "Tamaño".into(),
            format!("{:.3} × {:.3} µm", scene.bbox.width(), scene.bbox.height()),
        ),
    ];
    (scene, keys)
}

/// Devuelve al sistema la memoria libre del heap. Armar una escena grande
/// aplana en C++ y convierte a elementos: glibc se queda con lo liberado y
/// el visor seguía ocupando el pico (chip de 42 MB: 2,25 → 1,22 GB).
pub(crate) fn release_free_memory() {
    #[cfg(all(target_os = "linux", target_env = "gnu"))]
    {
        unsafe extern "C" {
            fn malloc_trim(pad: usize) -> i32;
        }
        // SAFETY: malloc_trim solo recorre el heap de glibc; no toca
        // punteros vivos.
        unsafe {
            malloc_trim(0);
        }
    }
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

/// Los archivos de un lado de un diff; `None` si no existía (0 bytes).
fn collect_side<'a>(
    bytes: &'a [u8],
    path: Option<&str>,
    files: Option<&dyn FileSource>,
    label: &'static str,
) -> VcResult<Option<Raw<'a>>> {
    if bytes.is_empty() {
        return Ok(None);
    }
    source::collect(bytes, path, files).map(Some).map_err(|e| read_error(e, label))
}

/// Error de lectura para el visor. `label`: el lado de un diff, si lo es.
fn read_error(e: ReadError, label: &str) -> ViewerError {
    let side = if label.is_empty() { String::new() } else { format!(" ({label})") };
    match e {
        ReadError::NotLayout => ViewerError::Parse(format!("layout{side}: no es un layout GDSII, OASIS ni Magic")),
        // Es un layout pero no se puede leer (truncado, con ciclos, un .mag mal formado).
        ReadError::Parse(msg) => ViewerError::Corrupt(format!("layout{side}: {msg}")),
    }
}

#[async_trait]
impl ViewerBackend for GdsBackend {
    fn info(&self) -> BackendInfo {
        BackendInfo {
            name: "gds",
            version: env!("CARGO_PKG_VERSION"),
            extensions: &["gds", "oas", "mag"],
        }
    }

    fn accepts(&self, content: &[u8], path_hint: Option<&str>) -> bool {
        if let Some(p) = path_hint {
            let p = p.to_ascii_lowercase();
            if p.ends_with(".gds") || p.ends_with(".oas") || p.ends_with(".mag") {
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
    /// llamada: el parseo es barato (~30 ms para la libreria completa de
    /// celdas estandar de SKY130, 4 MB) y la escena no guarda la `Library`.
    async fn load_entry(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        self.load_with(content, path_hint, entry, None, token).await
    }

    /// Como `load_entry`; un `.mag` lee sus sub-celdas de `files` (o del
    /// disco si su ruta es absoluta) y del PDK.
    async fn load_with(
        &self,
        content: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        files: Option<Arc<dyn FileSource>>,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        if token.is_cancelled() {
            return Err(ViewerError::Cancelled);
        }
        let libs = self.libs.clone();
        let scene = tokio::task::spawn_blocking(move || -> VcResult<VcScene> {
            let raw = source::collect(&content, path_hint.as_deref(), files.as_deref()).map_err(|e| read_error(e, ""))?;
            let side = raw.read(Some(&libs)).map_err(|e| read_error(e, ""))?;
            let notices = side.notices;
            let lib = side.lib;

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
            scene.notices.extend(notices);
            let tops = entries.iter().filter(|e| e.is_root).count();
            // Justo despues de "Celda": cuantas celdas hay para elegir.
            scene.metadata.insert(1, ("Celdas".into(), format!("{tops} top / {} total", entries.len())));
            scene.current_entry = Some(cell.name().to_string());
            scene.entries = entries;
            // Índice espacial (culling y nivel de detalle): aquí, fuera del hilo de la UI.
            // Es lo más caro de la carga: si ya la cancelaron (otra celda,
            // otro archivo), no se arma.
            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }
            scene.build_index();
            release_free_memory();
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
        self.load_diff_with(before, after, path_hint, entry, DiffFiles::default(), token).await
    }

    /// Como `load_diff`; un `.mag` lee las sub-celdas de cada versión de su
    /// fuente (`files.before`, `files.after`).
    async fn load_diff_with(
        &self,
        before: Vec<u8>,
        after: Vec<u8>,
        path_hint: Option<String>,
        entry: Option<String>,
        files: DiffFiles,
        token: CancellationToken,
    ) -> VcResult<SceneHandle> {
        if token.is_cancelled() {
            return Err(ViewerError::Cancelled);
        }
        let cache = self.cache.clone();
        let libs = self.libs.clone();
        let scene = tokio::task::spawn_blocking(move || -> VcResult<VcScene> {
            let path = path_hint.as_deref();
            let (ra, rb) = rayon::join(
                || collect_side(&before, path, files.before.as_deref(), "antes"),
                || collect_side(&after, path, files.after.as_deref(), "después"),
            );
            let (ra, rb) = (ra?, rb?);
            let read = |raw: &Option<Raw<'_>>, label| raw.as_ref().map(|r| r.read(Some(&libs)).map_err(|e| read_error(e, label))).transpose();
            let (a, b) = rayon::join(|| read(&ra, "antes"), || read(&rb, "después"));
            let (a, b) = (a?, b?);
            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }
            let mut notices = Vec::new();
            for (side, label) in [(&a, "antes"), (&b, "después")] {
                if let Some(s) = side {
                    notices.extend(s.notices.iter().map(|n| format!("{label}: {n}")));
                }
            }
            let (inputs, read_params) = source::pair_key(ra.as_ref(), rb.as_ref());
            let diff = CachedDiff { cache: &cache, inputs, read_params };
            let lib_a = a.as_ref().map(|s| &*s.lib);
            let lib_b = b.as_ref().map(|s| &*s.lib);
            let ports = crate::mag::port_changes(
                a.as_ref().and_then(|s| s.info.as_ref()),
                b.as_ref().and_then(|s| s.info.as_ref()),
            );
            let mut s = build_diff_scene(lib_a, lib_b, entry.as_deref(), path, &diff)?;
            let cell = s.current_entry.clone();
            s.changes.extend(ports.iter().filter(|p| Some(&p.cell) == cell.as_ref()).map(port_item));
            s.notices.extend(notices);
            if token.is_cancelled() {
                return Err(ViewerError::Cancelled);
            }
            s.build_index();
            release_free_memory();
            Ok(s)
        })
        .await??;

        Ok(Arc::new(scene) as SceneHandle)
    }
}

/// Todas las celdas de la library como `ViewEntry`: primero las top cells y
/// luego el resto, cada grupo en orden alfabetico. El tamano sale del bbox
/// (celdas sin geometria -> `None`).
pub(crate) fn list_cells(lib: &Library) -> Vec<ViewEntry> {
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
    use viewer_core::diff::ChangeKind;

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

    /// Archivos en memoria, como los de un commit.
    struct Mem(std::collections::HashMap<&'static str, &'static str>);
    impl FileSource for Mem {
        fn read(&self, path: &str) -> Option<Vec<u8>> {
            self.0.get(path).map(|s| s.as_bytes().to_vec())
        }
    }

    const MAG_TOP: &str = "magic
tech sky130A
magscale 1 2
<< metal1 >>
rect 0 0 100 20
use inv  i0
transform 1 0 200 0 1 0
box 0 0 1 1
<< end >>
";
    const MAG_INV: &str = "magic
tech sky130A
magscale 1 2
<< locali >>
rect 0 0 40 10
<< viali >>
rect 5 2 10 8
<< end >>
";
    const MAG_INV_2: &str = "magic
tech sky130A
magscale 1 2
<< locali >>
rect 0 0 40 30
<< viali >>
rect 5 2 10 8
<< end >>
";

    #[tokio::test]
    async fn magic_hierarchy_opens_with_named_layers_and_pdk_colors() {
        let b = GdsBackend::new();
        let files: Arc<dyn FileSource> = Arc::new(Mem([("chip/inv.mag", MAG_INV)].into()));
        let h = b
            .load_with(MAG_TOP.as_bytes().to_vec(), Some("chip/top.mag".into()), None, Some(files), CancellationToken::new())
            .await
            .expect("load .mag");
        assert!(h.notices().is_empty(), "{:?}", h.notices());
        let names: Vec<String> = h.layer_list().into_iter().map(|(_, p)| p.name.clone()).collect();
        // Nombres de Magic, apilados como en SKY130: li1 debajo de mcon y met1.
        assert_eq!(names, ["locali", "viali", "metal1"]);
        let met1 = h.layer_list().into_iter().find(|(_, p)| p.name == "metal1").unwrap().1.stroke;
        assert_eq!((met1.r, met1.g, met1.b), (60, 130, 240), "color de met1 de SKY130");

        // Sin los archivos del commit, la sub-celda falta y se avisa.
        let h = b.load_entry(MAG_TOP.as_bytes().to_vec(), Some("chip/top.mag".into()), None, CancellationToken::new()).await.unwrap();
        assert!(h.notices().iter().any(|n| n.contains("inv")), "{:?}", h.notices());
    }

    #[tokio::test]
    async fn magic_diff_reads_each_side_from_its_version() {
        let b = GdsBackend::new();
        let before: Arc<dyn FileSource> = Arc::new(Mem([("chip/inv.mag", MAG_INV)].into()));
        let after: Arc<dyn FileSource> = Arc::new(Mem([("chip/inv.mag", MAG_INV_2)].into()));
        let files = DiffFiles::new(Some(before), Some(after));
        let top = MAG_TOP.as_bytes().to_vec();
        let h = b
            .load_diff_with(top.clone(), top, Some("chip/top.mag".into()), None, files, CancellationToken::new())
            .await
            .expect("diff .mag");
        // El archivo de arriba es igual; el cambio viene de inv, en locali.
        assert!(h.changes().iter().any(|c| c.label.starts_with("locali")), "{:?}", h.changes());

        // Un puerto que cambia de clase aparece en la lista de cambios.
        let inv = |class: &str| format!("magic
tech sky130A
magscale 1 2
<< locali >>
rect 0 0 40 10
<< labels >>
rlabel locali s 0 0 40 10 0 A
port 1 nsew signal {class}
<< end >>
");
        let h = b
            .load_diff(inv("input").into_bytes(), inv("inout").into_bytes(), Some("inv.mag".into()), None, CancellationToken::new())
            .await
            .expect("diff de puertos");
        assert!(h.changes().iter().any(|c| c.label == "puerto A: input → inout"), "{:?}", h.changes());
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

    /// Un GDS truncado es un layout que no se puede leer: `Corrupt`, para
    /// que el visor lo explique sin buscar palabras en el mensaje.
    #[tokio::test]
    async fn load_truncated_gds_is_corrupt() {
        let mut bytes = proof_lib_bytes();
        bytes.truncate(bytes.len() / 2);
        let res = GdsBackend::new().load(bytes, None, CancellationToken::new()).await;
        assert!(matches!(res, Err(ViewerError::Corrupt(_))), "{:?}", res.err());
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

    #[test]
    fn libraries_are_read_once_per_content() {
        let cache = LibCache::default();
        let reads = std::cell::Cell::new(0);
        let read = |bytes: &[u8]| {
            cache.get_or_read(bytes, || {
                reads.set(reads.get() + 1);
                Ok::<_, ViewerError>((Library::from_bytes(&fixture("hier_inv_a.gds")).unwrap(), Vec::new()))
            })
        };
        let first = read(b"A").unwrap().0;
        assert!(Arc::ptr_eq(&first, &read(b"A").unwrap().0), "mismo contenido: la misma Library");
        assert_eq!(reads.get(), 1);
        read(b"B").unwrap();
        read(b"C").unwrap();
        // Solo guarda las dos más recientes: A se volvió a leer.
        read(b"A").unwrap();
        assert_eq!(reads.get(), 4);
    }

    /// Cambiar de celda en el diff del chip de 42 MB. Con los archivos en
    /// `$RIKU_BIG_A`/`$RIKU_BIG_B` (p. ej. /tmp/big_a.gds y big_b.gds).
    #[tokio::test]
    #[ignore = "necesita $RIKU_BIG_A y $RIKU_BIG_B"]
    async fn cell_switch_on_a_big_diff() {
        let (Some(a), Some(b)) = (std::env::var_os("RIKU_BIG_A"), std::env::var_os("RIKU_BIG_B")) else { return };
        let (a, b) = (std::fs::read(a).unwrap(), std::fs::read(b).unwrap());
        let rss = || -> u64 {
            std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
                s.lines().find(|l| l.starts_with("VmRSS")).and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
            }).unwrap_or(0) / 1024
        };
        let r0 = rss();
        let one = Library::from_bytes(&a).unwrap();
        eprintln!("[P5] una Library en memoria: {} MB", rss().saturating_sub(r0));
        drop(one);
        let backend = GdsBackend::with_cache(DiffCache::disabled());
        let load = |entry: Option<String>| {
            backend.load_diff(a.clone(), b.clone(), Some("big.gds".into()), entry, CancellationToken::new())
        };
        let t = std::time::Instant::now();
        let scene = load(None).await.unwrap();
        eprintln!("[P5] primera carga: {:?}", t.elapsed());
        let other = scene.entries().iter().map(|e| e.id.clone()).find(|id| Some(id.as_str()) != scene.current_entry()).unwrap();
        let t = std::time::Instant::now();
        load(Some(other.clone())).await.unwrap();
        eprintln!("[P5] otra celda: {:?}", t.elapsed());
        // Lo mismo sin las bibliotecas en memoria (como antes).
        let fresh = GdsBackend::with_cache(DiffCache::disabled());
        let t = std::time::Instant::now();
        fresh.load_diff(a.clone(), b.clone(), Some("big.gds".into()), Some(other), CancellationToken::new()).await.unwrap();
        eprintln!("[P5] otra celda, releyendo: {:?}", t.elapsed());
    }

    /// RAM por fase al abrir la top del chip grande (`$RIKU_BIG_A`).
    #[test]
    #[ignore = "necesita $RIKU_BIG_A"]
    fn memory_by_phase_on_a_big_layout() {
        let Some(a) = std::env::var_os("RIKU_BIG_A") else { return };
        let status = |key: &str| -> u64 {
            std::fs::read_to_string("/proc/self/status").ok().and_then(|s| {
                s.lines().find(|l| l.starts_with(key)).and_then(|l| l.split_whitespace().nth(1)?.parse().ok())
            }).unwrap_or(0) / 1024
        };
        let show = |what: &str| eprintln!("[P6] {what:<28} RSS {:>5} MB  pico {:>5} MB", status("VmRSS"), status("VmHWM"));
        let lib = Library::from_bytes(&std::fs::read(a).unwrap()).unwrap();
        show("Library");
        let top = crate::select_top_cell(&lib).unwrap();
        let t = std::time::Instant::now();
        let (mut scene, _) = vc_scene_from_cell(&lib, &top, None);
        eprintln!("[P6] armar la escena: {:?}", t.elapsed());
        show("escena (elementos)");
        scene.build_index();
        show("índice");
        release_free_memory();
        show("tras devolver la memoria");
        eprintln!("[P6] elementos: {}", scene.elements.len());
    }
}
