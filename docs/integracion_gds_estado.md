# Integración GDS en riku_chip — estado y roadmap

Documento de seguimiento del soporte de archivos GDSII (layouts físicos de chips) en riku, el VCS para circuitos integrados.

**Última actualización:** 2026-09-26 (incluye usabilidad de la GUI)
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
│ riku-gui (egui/eframe, nativa)                                          │
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

### 3.4 Usabilidad de la GUI

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

Scripts en `riku_gds_samples/` del entorno de pruebas (`_cmp_klayout_all.py`, `_cmp_xor_klayout.py`), usando el módulo Python de KLayout 0.30.

| Qué | Dónde | Resultado |
|---|---|---|
| Geometría por celda: bbox, polígonos y área por capa, labels con posición | 437 top cells de `sky130_fd_sc_hd` (49 882 polígonos, 6 146 labels) | idéntico |
| Ídem | 230 top cells de `gf180mcu_fd_sc_mcu7t5v0` (15 070 / 2 664) | idéntico (salvo la representación del bbox de una celda vacía) |
| Ídem | 78 top cells de `sg13g2_stdcell` (6 477 / 407) | idéntico |
| XOR por capa (6 decimales) | `inv_1` con met1 añadido, mcon borrado, poly movido 0,05 µm y licon movido 5 nm (cosmético) | idéntico |
| XOR jerárquico | met1 añadido dentro de `inv_2` → `macro_sparecell` (+0,554750 µm²) | idéntico |

Visualmente (láminas lado a lado con KLayout + `.lyp` oficial): misma geometría y posiciones; cambia el estilo (KLayout usa tramados, Riku rellenos translúcidos por rol de capa).

---

## 5. Tests

| Crate | Tests | Cubren |
|---|---|---|
| `viewer-core` | 9 | eje Y, fit, hit-test y área de primitivas, contrato por defecto de `load_entry`/`load_diff` |
| `gds-renderer` | 39 | diff por celda y jerárquico, lados vacíos, `changed_cells`, escena de diff, paletas y detección de PDK, labels jerárquicos, anchors, catálogo de celdas |
| `riku-gui` | 41 | transformaciones y zoom, relleno cóncavo, selector y filtros, tooltip, encuadre de cambios, colocación de etiquetas, contraste por tema, springs e inercia, fundido de tema, mensajes, filtro del árbol |
| `riku` | 80 | incluye `tests/gds_e2e.rs`: repo git real → `GitService` → `GdsDriver`, y el binario `riku diff -f json` (áreas, bbox absoluto, archivo nuevo, versiones idénticas) |

---

## 6. Entorno de desarrollo

Windows + MSVC 2019 falla de varias formas al compilar gdstk-rs (LNK1171 por `mspdbcore.dll`, OOM de LLVM, DLLs de vcpkg en runtime). En Linux todo compila sin ajustes: el entorno de referencia es el contenedor **iic-osic-tools** (trae Rust vía `rustup`, zlib, qhull, KLayout y los PDKs en `/foss/pdks`).

```bash
docker exec -it <contenedor-iic-osic-tools> bash
cd /foss/designs/riku_chip/riku && cargo test
cd ../riku-gui && cargo run -- /foss/pdks/sky130A/libs.ref/sky130_fd_sc_hd/gds/sky130_fd_sc_hd.gds --cell sky130_fd_sc_hd__inv_1
```

Con WSLg la ventana aparece en el escritorio de Windows. Si hace falta capturarla con herramientas X11, lanzar con `env -u WAYLAND_DISPLAY` para que use XWayland.

---

## 7. Pendiente

La lista completa y priorizada vive en **`docs/roadmap/pendientes.md`**. Lo más urgente:

1. **CI**: no hay integración continua; los tests solo corren a mano.
2. **Warnings `float_literal_f32_fallback`** (18 en `riku-gui`): rustc anuncia que serán error en una versión futura.
3. **Paridad de la vista `.sch`** con la de GDS (tooltip, etiquetas, movimiento, atajos de zoom).

Luego: OASIS, celdas renombradas, un item por instancia en cambios repartidos, scripts de verificación dentro del repo, cache del XOR.

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
