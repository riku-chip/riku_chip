# Fase 6.2: el visor con layouts grandes

Índice espacial, nivel de detalle y triangulación en cache. Detalla el punto 4.2 de [`diseno_fase6_rendimiento.md`](diseno_fase6_rendimiento.md). Estado: **propuesto** (2026-09-26).

---

## 1. Qué pasa hoy (medido con el layout de 42 MB: 6,2 millones de elementos, 43 capas)

| Medida | Valor |
|---|---|
| Armar la escena (`GdsBackend::load`) | 4,4 s · 2,1 GB |
| `paint_scene` con el chip completo en pantalla | **560–660 ms por cuadro** |
| RAM del proceso con la ventana abierta | **11 GB** (el contenedor tiene 11) |

Cada cuadro (`riku/src/gui/scene_painter.rs`), con el chip completo a la vista y un píxel = 3,8 µm:

1. `Scene::visit` recorre los 6,2 millones de elementos y recalcula el bbox de cada uno (`DrawElement::bounding_box` recorre los vértices del polígono) para descartar los que no se ven. Con todo a la vista no descarta nada.
2. `draw_element` proyecta cada polígono a pantalla (un `Vec<Pos2>` nuevo), decide si es convexo (`is_convex`, recorre los vértices) y crea una `Shape` de egui **por elemento**: relleno más contorno. Los cóncavos se triangulan con earcut **en cada cuadro**.
3. egui teselá esas 6 millones de formas a triángulos (con suavizado de bordes) en un solo hilo. La GPU solo pinta el resultado.

De ahí salen los 11 GB: la escena (2,1 GB) más las formas y mallas de un cuadro entero, que egui guarda como `Shape` (con su `Vec` de puntos cada una) y como `Mesh` (cada rectángulo con contorno son ~12 vértices por el suavizado). Del orden de 1 KB por elemento.

Y el mismo recorrido lineal lo hace `pick_at` (el tooltip) en cada movimiento del mouse.

---

## 2. Objetivos

| # | Objetivo | Cómo se mide |
|---|---|---|
| V1 | Cuadro con el chip completo **< 50 ms**; en zoom cercano, igual o mejor que hoy | `RIKU_PROFILE=1` (queda como opción permanente del visor) |
| V2 | RAM con el layout de 42 MB **< 3 GB** | `ps` durante `riku gui` |
| V3 | Armar el índice **< 2 s** para 6,2 millones de elementos, en paralelo | `profile_view` |
| V4 | Tooltip y "ir al cambio" instantáneos (< 1 ms) | `RIKU_PROFILE=1` |
| V5 | Mismo dibujo en zoom cercano (píxel a píxel) para `.sch` y para celdas estándar; en zoom lejano, la misma silueta | capturas con `riku_shot.sh` antes y después, en tres zooms |
| V6 | El crate de Carlos y `riku-kernel` no cambian; la CI `xschem-compat` sigue verde | CI |

---

## 3. Idea

Trabajar una sola vez al cargar y, en cada cuadro, tocar solo lo que se ve **y** lo que se distingue.

```
  carga (spawn_blocking, con rayon)              cada cuadro (hilo de egui)
  ┌───────────────────────────────┐              ┌──────────────────────────────────┐
  │ Scene::build_index()          │              │ index.visit_lod(bbox visible,    │
  │  · bbox f32 y tamaño de cada  │   ──────▶    │                 tamaño de píxel) │
  │    elemento                   │              │  · elementos ≥ umbral: uno a uno │
  │  · grilla de culling          │              │    (mallas por capa, sin Shape   │
  │  · pirámide de cobertura      │              │     por elemento)                │
  │    por capa (LOD)             │              │  · lo menor a un píxel: bloques  │
  │  · triangulación de cóncavos  │              │    de la pirámide, una malla     │
  └───────────────────────────────┘              │    por capa                      │
                                                 └──────────────────────────────────┘
```

---

## 4. Diseño

### 4.1 `SceneIndex` (`viewer-core/src/index.rs`, nuevo)

```rust
pub struct SceneIndex {
    /// Por elemento: bbox en f32 (redondeado hacia afuera) y lado mayor.
    bboxes: Vec<[f32; 4]>,
    size: Vec<f32>,
    layer: Vec<Layer>,
    /// Culling: grilla uniforme sobre el bbox de la escena; cada celda lista los
    /// elementos que la tocan. Los que tocan más de 64 celdas van a `large`
    /// (pocos: boundaries, pozos, anillos) y se prueban uno a uno.
    grid: Grid { origin, cell, cols, rows, cells: Vec<Vec<u32>> },
    large: Vec<u32>,
    /// LOD: por nivel (celda = base × 2^n) y capa, qué celdas contienen algún
    /// elemento menor que la celda de ese nivel. Bitset por (nivel, capa).
    coverage: Vec<CoverageLevel>,
    /// Relleno de polígonos: convexo (abanico) o índices de earcut. Solo para
    /// polígonos rellenos; `None` en el resto.
    fill: Vec<Option<PolygonFill>>,   // enum PolygonFill { Convex, Triangles(Box<[u32]>) }
}

pub enum Visible<'a> {
    /// Se dibuja tal cual (con `idx` se busca la triangulación en cache).
    Element { idx: usize, el: &'a DrawElement },
    /// Zona con geometría menor a un píxel, resumida: se pinta como un rectángulo.
    Block { layer: Layer, bbox: BoundingBox },
}

pub struct LodQuery {
    pub bbox: BoundingBox,      // región visible del mundo
    pub px_world: f64,          // unidades de mundo que ocupa un píxel
    pub lod: bool,              // false = todo uno a uno (Ajustes)
}

impl SceneIndex {
    pub fn build(elements: &[DrawElement], bbox: &BoundingBox) -> Self;   // con rayon si la feature `parallel` está
    pub fn visit_lod(&self, elements: &[DrawElement], q: &LodQuery, hidden: &dyn Fn(Layer) -> bool, f: &mut dyn FnMut(Visible<'_>) -> bool);
    pub fn candidates_at(&self, x: f64, y: f64) -> impl Iterator<Item = usize>;  // para pick_at
    pub fn fill(&self, idx: usize) -> Option<&PolygonFill>;
}
```

**Grilla de culling.** Tamaño de celda tal que haya ~64 elementos por celda en promedio (`cols × rows ≈ N / 64`, tope 2048 × 2048). Un elemento chico cae en 1–4 celdas; al recorrer se evita repetirlo con un sello por elemento (`Vec<u32>` de "visto en el cuadro k", 25 MB para 6,2 millones). Los grandes (más de 64 celdas) van aparte y se prueban contra el bbox visible; en un chip son decenas.

**Pirámide de cobertura (LOD).** Nivel 0 con celda = lado mayor de la escena / 2048; cada nivel duplica la celda. Por nivel y capa, un bitset de celdas ocupadas, construido con los elementos cuyo lado mayor es **menor** que la celda de ese nivel. Memoria: 2048² bits = 512 KB por capa en el nivel 0; con 43 capas y la suma de niveles, unos 30 MB.

**Consulta por cuadro.** Con `px_world` se elige el nivel ℓ con la celda más grande que no supere `BLOCK_PX × px_world` (`BLOCK_PX = 1.5`). Entonces:
- los elementos con lado ≥ celda(ℓ) salen uno a uno (de la grilla, filtrados por tamaño y por capa oculta);
- lo demás sale como bloques: por capa y por fila de celdas visibles del nivel ℓ, las corridas de celdas ocupadas contiguas se funden en un solo `Block`.
- Si un píxel es más chico que la celda del nivel 0 (zoom cercano), no hay bloques: todo uno a uno, como hoy, pero solo lo visible.

Los bloques son de a lo sumo 1,5 px: nada que el usuario pueda distinguir desaparece; al acercarse, aparece todo.

**Triangulación en cache.** Al construir: `is_convex` y, si no, earcut, una sola vez por polígono relleno (en paralelo). El painter usa el resultado.

### 4.2 `Scene` y `RenderableScene` (`viewer-core`), sin romper a nadie

- `Scene` gana `pub index: Option<Arc<SceneIndex>>` (con `Scene::new()` sigue en `None`) y `pub fn build_index(&mut self)`. Carlos arma sus escenas con `Scene::new()` + `push` (`viewer_core_adapter.rs:124`), así que un campo más no lo afecta.
- `RenderableScene` gana `fn index(&self) -> Option<&SceneIndex> { None }`: quien implemente el trait por su cuenta sigue compilando.
- `DrawElement` **no cambia** (agregarle un campo rompería a quien lo construye). Lo precalculado vive en tablas paralelas indexadas por posición en `elements`.
- Feature nueva `parallel = ["dep:rayon"]` en `viewer-core`; `riku` la activa con `gui`. Sin la feature, `build` es secuencial (mismo resultado).

### 4.3 El painter (`riku/src/gui/scene_painter.rs` y `polygon_fill.rs`)

- Si `scene.index()` es `Some`, `paint_scene` usa `visit_lod`; si no, el `visit` de siempre (compatibilidad con cualquier escena externa).
- **Una malla por capa en vez de una `Shape` por elemento.** Los polígonos rellenos visibles se acumulan en un `egui::Mesh` por capa (abanico si es convexo, índices de la cache si no) y los bloques en otra malla por capa (dos triángulos por bloque). Cada capa termina siendo una o dos formas para egui, no cientos de miles. Sin suavizado de bordes en las mallas: con 1 px por elemento era ruido de todos modos.
- **Contorno solo cuando se ve.** Hoy cada polígono lleva relleno más contorno. El contorno se dibuja si el elemento mide al menos `OUTLINE_MIN_PX = 6` px; por debajo, solo relleno. Las capas de solo contorno (implantes, pines) siguen como líneas, pero solo las visibles.
- **Color de los bloques:** el relleno de la capa; en capas de solo contorno, el color de contorno con alfa 60 (se ve la silueta, no tapa).
- **Orden de pintado:** bloques primero (por capa, de abajo hacia arriba), después los elementos uno a uno en el orden de la escena (ya viene por apilado), después fantasmas, anotaciones y etiquetas como hoy.
- `pick_at` y `hover_info` pasan por `candidates_at` (la celda de la grilla más `large`): de recorrer 6,2 millones a probar decenas.
- `RIKU_PROFILE=1` queda como opción permanente: imprime `paint_scene`, tiempo entre cuadros y formas emitidas. Es la medición de V1 y V4.
- Ajustes: casilla "Simplificar al alejar" (por defecto activada) que pone `lod: false` para comparar o para inspecciones puntuales.

### 4.4 Backends

- `GdsBackend` (`riku-mod-layout/src/viewer_core_compat.rs`) y `XschemViewer` (`riku/src/modules/xschem_view.rs`) llaman a `scene.build_index()` al terminar de armar la escena, dentro del `spawn_blocking` que ya usan. Un esquemático es chico, pero gana el culling y el picking por grilla sin costo apreciable.
- Nada cambia en gdstk-rs ni en el motor de Carlos.

### 4.5 Memoria, estimada para el layout de 42 MB

| Parte | Hoy | Después |
|---|---|---|
| Escena (`elements`, f64) | 2,1 GB | 2,1 GB (bajarla a f32 es otro paso, ver 6) |
| Índice: bboxes f32 + tamaño + capa + grilla + sellos | — | ~250 MB |
| Pirámide de cobertura | — | ~30 MB |
| Triangulaciones (solo cóncavos, ~5 % de los polígonos) | — | ~50 MB |
| Formas y mallas de un cuadro | **~8 GB** | < 100 MB (una o dos mallas por capa) |
| **Total** | **11 GB** | **~2,6 GB** |

---

## 5. Pasos y criterio de listo

| Paso | Qué | Listo cuando |
|---|---|---|
| a | `SceneIndex` con bboxes, grilla, `candidates_at` y triangulación en cache; `Scene::build_index`; `RenderableScene::index`; painter usa culling por grilla y la cache; `pick_at` por grilla | tests de `viewer-core` (grilla, elementos grandes, picking, triangulación); V3, V4; capturas idénticas en los tres zooms |
| b | Pirámide de cobertura, `visit_lod`, bloques y mallas por capa, contorno por tamaño, opción en Ajustes, `RIKU_PROFILE` | V1, V2; capturas: idénticas en zoom cercano, misma silueta de lejos |
| c | `build_index` en paralelo (`parallel`), backends lo llaman; `profile_view` mide el índice | V3; CI verde en las cuatro combinaciones de features y en `xschem-compat` |
| d | Documentación (`docs/gui.md`, README, avance) | — |

Esfuerzo total: L (tres commits, uno por paso a–c).

---

## 6. Fuera de alcance, anotado

- **Escena en f32:** `DrawElement::Polygon` guarda `Vec<(f64, f64)>`; en f32 la escena bajaría de 2,1 GB a ~1,1 GB. Cambiaría el tipo público que usa Carlos: se decide aparte.
- **Etiquetas:** ya se filtran por tamaño natural (`LABEL_MIN_NATURAL_PX`); con 10 mil etiquetas no pesan. No entran en la pirámide.
- **Carga más rápida** (4,4 s): el aplanado es una llamada a gdstk; queda para 6.4 junto con el diff paralelo.

---

## 7. Riesgos

| Riesgo | Mitigación |
|---|---|
| Los bloques esconden algo que se buscaba | Solo lo menor a 1,5 px; casilla para desactivar; al acercarse aparece todo |
| El contorno por tamaño cambia el dibujo de los `.sch` | Los elementos de un esquemático miden decenas de píxeles: no los alcanza. Se verifica con capturas |
| Un elemento enorme en miles de celdas infla la grilla | Los que tocan más de 64 celdas van a `large` |
| `f32` en los bboxes deja fuera algo por redondeo | Redondeo hacia afuera (`min` hacia abajo, `max` hacia arriba) |
| Mallas gigantes en zoom medio (muchos elementos de 2–10 px) | Cada malla se corta en trozos de 64 k vértices (límite práctico de egui) |
