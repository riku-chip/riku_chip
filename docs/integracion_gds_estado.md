# Integración GDS en riku_chip — estado y roadmap

Documento de seguimiento del soporte de archivos GDSII (layouts físicos de chips) en riku, el VCS para circuitos integrados.

**Última actualización:** 2026-09-26 (incluye usabilidad de la GUI, OASIS, renombres, cambios por instancia, cache y CI)
**Autor de los cambios documentados:** Dante (Adriel2503)
**Estado:** los bloques A–D del plan original están terminados. GDS tiene diff geométrico en la CLI y un visor con diff visual en la GUI, ambos verificados contra KLayout.

---

## 1. Contexto y motivación

`riku_chip` es un VCS especializado para diseño de chips IC. Empezó manejando schematics `.sch` (xschem) con un driver dedicado escrito por Carlos Cueva. Este trabajo agrega soporte para `.gds`, el formato dominante para representar la geometría física de un chip.

**Por qué GDS:** los `.gds` son el output canónico de cualquier flujo de diseño de IC. Un commit típico cambia `.sch` y `.gds` juntos; ver solo uno fragmenta la revisión.

**Diferencia clave respecto a xschem:** un schematic es texto que se parsea y compara semánticamente (componentes, nets). Un GDS es geometría binaria: el diff útil es **geométrico** (qué área cambió en qué capa de qué celda), no estructural.

---

## 2. Arquitectura

```
┌───────────────────────────────────────────────────────────────────────┐
│ riku gui (visor egui, dentro del ejecutable riku)                                       │
│   ScreenXform (mundo↔pantalla, eje Y) · polygon_fill (earcut)           │
│   entry_picker (celdas) · Details (capas, cambios, tooltip)             │
│   rutas: Xschem rica (sch_painter) │ neutra (ViewerBackend)             │
└───────────────────────────────────────────────────────────────────────┘
        │ lanza (riku diff -f visual)          │ Arc<dyn ViewerBackend>
        ▼                                      ▼
┌────────────────────────────┐   ┌──────────────────────────────────────┐
│ riku (CLI + core)          │   │ viewer-core (contrato neutro)        │
│  XschemDriver · GdsDriver  │   │  ViewerBackend: load / load_entry /  │
│  diff · status · log · doc │   │                 load_diff            │
└────────────────────────────┘   │  Scene: elementos, y_axis, capas     │
        │                        │   (LayerPaint), metadata, entries    │
        ▼                        │   (ViewEntry), changes (ChangeItem), │
┌──────────────────────────────────┐ world_unit                        │
│ gds-renderer                     └──────────────────────────────────────┘
│  GdsBackend (impl ViewerBackend) · diff_gds / diff_cell /            │
│  changed_cells · labels (jerarquía) · palette (SKY130/GF180/IHP)     │
│  render SVG (render_scene)                                           │
└──────────────────────────────────────────────────────────────────────┘
        │
        ▼
┌──────────────────────────────────────────────────────────────────────┐
│ gdstk-rs (binding cxx → gdstk C++), submódulo external/gdstk          │
│  Library::from_bytes · get_polygons (aplanado) · xor_split_flat       │
│  Label::anchor · Reference (origen, rotación, reflexión, AREF)        │
└──────────────────────────────────────────────────────────────────────┘
```

**Patrón clave:** la GUI tiene dos rutas. Xschem mantiene su ruta rica (fantasmas, anotaciones semánticas); GDS va por la ruta neutra `ViewerBackend`. Todo lo específico de GDS vive en `gds-renderer`: `viewer-core` y `riku-gui` solo conocen tipos neutros. Los métodos nuevos del contrato tienen implementación por defecto, así que `XschemBackend` (submódulo externo) no cambió.

---

## 3. Qué está hecho

### 3.1 Base (bloques A–C)

| Bloque | Contenido | Commits |
|---|---|---|
| A | Submódulo `external/gdstk`, `OwnedPolygon` único, `FileFormat::Gds` | `414ebb0`, `61dde7d` |
| B | `Library::from_bytes` en gdstk-rs | `3f8a4db` |
| C | `GdsDriver`: diff por celda/capa, µm², bbox, umbral cosmético (`--cosmetic-threshold-um2`), XOR jerárquico con origen | `b3739cd` … `7a13432` |

### 3.2 Visor GDS (bloque D)

| Tema | Qué se resolvió |
|---|---|
| Encuadre | `ScreenXform` suma el offset del panel; auto-fit con flag y re-fit al cambiar el tamaño del lienzo; zoom anclado al cursor; botón Fit |
| Eje Y | `YAxis` en `viewer-core`: GDS es Y-up, la pantalla Y-down |
| Colores | Paletas por PDK con **rol de capa**: dispositivo (relleno), pozo (tinte tenue), contorno (implantes, marcadores, boundary, pines). Orden de apilado físico |
| PDKs | SKY130, GF180MCU e IHP SG13G2 con nombres y colores de sus `.lyp` oficiales. Detección por path o por capas presentes |
| Polígonos cóncavos | Triangulación earcut (L, U, anillos "keyhole"); convexos por el camino rápido |
| Labels | Anchor del GDS y labels de **toda la jerarquía**, transformados a la celda raíz |
| Celdas | Selector con buscador y filtro "solo top cells"; `--cell NOMBRE` |
| Paneles | Details con celda, PDK, conteos, tamaño y capas con checkbox (se conservan al cambiar de celda); árbol de proyecto con scroll |
| Tooltip | Capa, tamaño y área del polígono bajo el cursor (prioriza capas con relleno) |

### 3.3 Diff visual de GDS

`riku diff A B archivo.gds -f visual` abre la GUI en modo diff:

- **Diff:** layout "después" atenuado + capas `Δ añadido` (verde) y `Δ eliminado` (rojo) con el XOR.
- **Before / After:** cada versión sola, conservando la vista para comparar la misma zona.
- **Cambios:** lista por capa y origen (relevantes primero, cosméticos en gris); clic encuadra el cambio.
- **Librerías:** `changed_cells` detecta qué celdas cambiaron (incluidos cambios heredados de sub-celdas) con una huella de la geometría aplanada + confirmación por XOR (32 ms para 441 celdas en release). El selector las marca (`+` `−` `~`), filtra "solo con cambios" y abre la primera cambiada.
- **Archivo nuevo o borrado:** un lado vacío cuenta como librería vacía (CLI y GUI).

### 3.4 Diff: OASIS, renombres, instancias y cache

| Tema | Qué hace |
|---|---|
| OASIS | `.oas` en CLI, visor y árbol del proyecto. gdstk-rs elige el lector por la firma del archivo (`Library::from_bytes_any`), así que A y B pueden ser de formatos distintos |
| Celdas renombradas | Una celda que desaparece y otra con la misma geometría aplanada que aparece se reportan como renombre (`r cell:INV → INV_X1`). La huella propone y el XOR confirma; si hay varias candidatas o no hay geometría, no se adivina. La GUI compara la celda contra su nombre anterior |
| Cambio por instancia | Cada instancia de la sub-celda, y cada repetición de un AREF, tiene su item y su recuadro en la GUI (`met1 · en INV @ (10.00, 10.00)`). La CLI los agrupa (`en N instancias`) y el JSON agrega `instances` / `instance_at_um` |
| Cache | Reporte de la CLI, celdas cambiadas y XOR de la celda de la GUI en `~/.cache/riku/diff`, con clave por bytes y parámetros. Solo para layouts de más de 1 MiB; tope de 512 MiB; se desactiva con `--no-cache` / `RIKU_NO_CACHE=1`. `diff_gds` además salta el XOR de las celdas con la misma huella de geometría |
| Paletas | Todas las capas de `gf180mcu.lyp` (116) y `sg13g2.lyp` (376), generadas con `tools/palettes/gen_palettes.py`; las tablas curadas siguen mandando |

### 3.5 Usabilidad de la GUI

Dos pasadas de diseño, verificadas con capturas antes/después (criterios de la guía *Designing Fluid Interfaces* y los principios de diseño de Apple, adaptados a un visor de escritorio):

| Tema | Qué hace |
|---|---|
| Etiquetas | Tamaño fijo en pantalla, fusión de las que comparten punto (`VPB · VPWR`), pastilla con halo desplazada del pin, sin solaparse; aviso de cuántas quedaron ocultas |
| Tema | Claro / Oscuro / Sistema, persistente; contraste de etiquetas WCAG AA verificado en tests; fundido de 250 ms al cambiar |
| Movimiento | Encuadrar e ir a un cambio animados (spring sin rebote, interrumpible); inercia al soltar un arrastre; "Reducir movimiento" |
| Orientación | Ruta `commits › archivo › celda › vista`, título de ventana, pantalla inicial con recientes, barra de estado con coordenadas en µm y escala |
| Organización | Barra de herramientas en español, Detalles en secciones plegables, árbol de proyecto filtrado a archivos abribles |
| Feedback | Mensajes temporales sobre el lienzo (estado, completado, aviso, error) con errores en lenguaje claro |
| Acceso rápido | Atajos `F`, `L`, `+`/`−`; arrastrar un archivo a la ventana lo abre |

---

## 4. Verificación contra KLayout

Scripts en **`tools/verify/`** (ver su README), con el módulo Python de KLayout 0.30. `tools/verify/compare.sh` vuelca cada librería con Riku (ejemplo `verify_dump` de gds-renderer) y con KLayout, y compara los dos volcados.

| Qué | Dónde | Resultado |
|---|---|---|
| Geometría por celda: bbox, polígonos y área por capa, labels con posición | 437 top cells de `sky130_fd_sc_hd` (49 882 polígonos, 6 146 labels) | idéntico |
| Ídem | 230 top cells de `gf180mcu_fd_sc_mcu7t5v0` (15 070 / 2 664) | idéntico |
| Ídem | 78 top cells de `sg13g2_stdcell` (6 477 / 407) | idéntico |
| Ídem, leyendo OASIS | `hier_inv_b.oas` | idéntico |
| XOR por capa (6 decimales) | `inv_1` con met1 añadido, mcon borrado, poly movido 0,05 µm y licon movido 5 nm (cosmético) | idéntico |
| XOR jerárquico | met1 añadido dentro de `inv_2` → `macro_sparecell` (+0,554750 µm²); AREF 3×2 (`multi_inst`) | idéntico |

Visualmente (`tools/verify/klayout_snapshot.py`, láminas lado a lado con KLayout y el `.lyp` oficial) la geometría y las posiciones coinciden. Lo que cambia es el estilo: KLayout usa tramados y Riku rellenos translúcidos según el rol de la capa.

---

## 5. Tests

Todos corren en la CI (GitHub Actions) con cada push y cada PR, con `-D warnings`.

| Crate | Tests | Cubren |
|---|---|---|
| `viewer-core` | 9 | eje Y, fit, hit-test y área de primitivas, contrato por defecto de `load_entry`/`load_diff` |
| `gds-renderer` | 54 | diff por celda y jerárquico, un item por instancia (SREF y AREF), renombres, OASIS contra GDS, cache (aciertos, corrupción, límite, escena idéntica), lados vacíos, `changed_cells`, escena de diff, paletas curadas y generadas, detección de PDK, labels jerárquicos, anchors, catálogo de celdas |
| `riku` (visor, `src/gui`) | 43 | transformaciones y zoom, relleno cóncavo, selector y filtros, tooltip, encuadre de cambios, colocación y prioridad de etiquetas, contraste por tema, springs e inercia, fundido de tema, mensajes, filtro del árbol |
| `riku` | 87 | incluye `tests/gds_e2e.rs`: repo git real → `GitService` → `GdsDriver`, y el binario `riku diff -f json` (áreas, bbox absoluto, archivo nuevo, versiones idénticas, OASIS igual a GDS); renombres e instancias en el driver; autocompletado del shell |
| `gdstk-rs` (submódulo) | +3 | OASIS: detección de formato, geometría idéntica a GDSII, bytes inválidos |

---

## 6. Entorno de desarrollo

Windows + MSVC 2019 falla de varias formas al compilar gdstk-rs: LNK1171 por `mspdbcore.dll`, OOM de LLVM y DLLs de vcpkg en runtime. Con VS 2022 y zlib/qhull de vcpkg sí compila y pasan los tests: la CI lo prueba en el job `windows (no bloqueante)` (ver `pendientes.md`, #4). En Linux todo compila sin ajustes. El entorno de referencia es el contenedor **iic-osic-tools**, que trae Rust vía `rustup`, zlib, qhull, KLayout y los PDKs en `/foss/pdks`.

```bash
docker exec -it <contenedor-iic-osic-tools> bash
cd /foss/designs/riku_chip && cargo test --workspace
cargo run --release -- gui /foss/pdks/sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds --cell sky130_fd_sc_hd__inv_1
```

Para usarlo instalado: `cargo install --path riku`. Es un solo ejecutable con CLI, shell y visor (`riku gui`, `riku open`); `--no-default-features` compila la versión solo de terminal.

Con WSLg la ventana aparece en el escritorio de Windows. Si hace falta capturarla con herramientas X11, lanzar con `env -u WAYLAND_DISPLAY` para que use XWayland.

---

## 7. Pendiente

La lista completa y priorizada vive en **`docs/roadmap/pendientes.md`**. Lo más urgente:

1. **Paridad de la vista `.sch`** con la de GDS: tooltip, etiquetas, movimiento y atajos de zoom.
2. **Diff de layouts muy grandes:** el primer diff de un wrapper de 42 MB sigue siendo lento. La cache resuelve las repeticiones; falta una huella estructural por celda.
3. **Empaquetado:** `.tar.gz` y `.deb` generados por la CI con cada release.

---

## 8. Referencias

- `docs/arquitectura_gds.md` — contratos originales entre crates.
- `docs/research/architecture/gdstk_rust_decisiones.md` — decisiones de diseño del binding.
- `docs/research/herramientas/gds_klayout_magic_diff.md` — investigación sobre diff de GDS con KLayout/Magic.
- `external/gdstk/rust/README.md` — setup del binding (vcpkg, pkg-config).

**Repos:**
- riku_chip: https://github.com/riku-chip/riku_chip
- gdstk-rs: https://github.com/Adriel2503/gdstk_rust
- xschem-viewer-rust: https://github.com/carloscl03/xschem-viewer-rust
