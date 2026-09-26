# Pendientes técnicos

Lista única de trabajo abierto de riku_chip, por prioridad. Cada item dice por qué importa, dónde tocar, cuánto cuesta y cuándo se considera terminado.

**Última revisión:** 2026-09-26 · **Estado de `main`:** `7b341ae`

Esfuerzo: **S** = horas, **M** = 1–2 días, **L** = varios días.

---

## Alta prioridad

### 1. Integración continua (CI)
- **Por qué:** no hay CI (`.github/` no existe). Los tests solo corren a mano; una regresión en un crate que otro consume (p.ej. `viewer-core` → Xschem) puede llegar a `main` sin que nadie lo note.
- **Dónde:** `.github/workflows/ci.yml` nuevo. Linux (Ubuntu) con `zlib1g-dev libqhull-dev pkg-config`, checkout con submódulos, `cargo test` en `viewer-core`, `gds-renderer`, `riku`, `riku-gui` y `cargo check --features viewer-core-compat` en `external/xschem-viewer-rust`. Cachear `~/.cargo` y los `target/`.
- **Esfuerzo:** M.
- **Listo cuando:** cada push y PR a `main` corre todo y marca rojo si algo falla.

### 2. Warnings `float_literal_f32_fallback` (futuro error de compilación)
- **Por qué:** 18 warnings en `riku-gui` que rustc anuncia que **se volverán error** ("will become a hard error in a future release"). Una actualización de Rust podría romper el build.
- **Dónde:** `riku-gui/src/sch_painter.rs` (16) y `riku-gui/src/app.rs` (2): literales como `Stroke::new(1.0, …)` → `Stroke::new(1.0_f32, …)`.
- **Esfuerzo:** S (mecánico).
- **Listo cuando:** `cargo build` de `riku-gui` sin warnings.

### 3. Paridad de la vista de esquemáticos (`.sch`) con la de GDS
- **Por qué:** la ruta Xschem usa su painter propio (`sch_painter.rs`) y no recibió lo agregado para GDS: sin tooltip, etiquetas sin pastillas ni anti-solapamiento, sin animación/inercia, `+`/`−` no hacen zoom, y mantiene un slider de zoom que la vista GDS no tiene. La experiencia cambia según el tipo de archivo.
- **Dónde:** `riku-gui/src/sch_painter.rs`, `app.rs` (rama `self.sch`). Opción de fondo: pasar Xschem por la ruta neutra (`XschemBackend` ya existe) conservando fantasmas y anotaciones como overlays.
- **Esfuerzo:** M–L.
- **Listo cuando:** las mismas interacciones funcionan igual en `.sch` y `.gds`.

---

## Media prioridad

### 4. Soporte OASIS (`.oas`)
- **Por qué:** formato de las foundries modernas; gdstk lo soporta.
- **Dónde:** `external/gdstk/rust` (lectura OASIS desde bytes), `GdsDriver::info().extensions`, `GdsBackend::accepts`, filtro del árbol (`riku-gui/src/project.rs`).
- **Esfuerzo:** M. **Listo cuando:** `riku diff` y la GUI abren `.oas` con los mismos tests que `.gds`.

### 5. Celdas renombradas
- **Por qué:** hoy un rename aparece como "eliminada + añadida", con todo su contenido como cambio.
- **Dónde:** `gds-renderer/src/gds_diff.rs`. La huella de `changed_cells` (geometría aplanada) sirve para emparejar celdas con contenido idéntico o casi.
- **Esfuerzo:** M. **Listo cuando:** un rename puro se reporta como "renombrada A → B" sin cambios geométricos.

### 6. Un cambio repartido en varias instancias
- **Por qué:** si una sub-celda cambia y está instanciada N veces, el diff da **un** item con un bbox que abarca todas; "ir al cambio" encuadra todo.
- **Dónde:** `gds-renderer/src/gds_diff.rs` (bucketing por origen), `viewer_core_compat.rs` (`change_items`).
- **Esfuerzo:** S–M. **Listo cuando:** un item por instancia, cada uno con su bbox.

### 7. Scripts de verificación dentro del repo
- **Por qué:** la verificación contra KLayout y las pruebas de GUI se hicieron con scripts que viven fuera del repo (en el entorno de pruebas): comparación de geometría/labels/XOR (`_cmp_klayout*.py`, `_cmp_xor_klayout.py`), driver XTest para clics y teclado (`xt.py`) y conversión de capturas. Sin ellos, nadie más puede repetir la verificación.
- **Dónde:** `tools/verify/` nuevo, con un README de uso en el contenedor iic-osic-tools.
- **Esfuerzo:** S. **Listo cuando:** un comando reproduce la comparación contra KLayout de las librerías SKY130/GF180/IHP.

### 8. Orden al fusionar etiquetas
- **Por qué:** las etiquetas del mismo punto se unen en el orden del archivo (`VPB · VPWR`); el pin de alimentación debería ir primero.
- **Dónde:** `riku-gui/src/label_layout.rs` (`merge_coincident`) + prioridad por capa desde el backend.
- **Esfuerzo:** S.

---

## Baja prioridad

| # | Tema | Por qué | Esfuerzo |
|---|---|---|---|
| 9 | Cache del XOR | GDS de cientos de MB pueden tardar decenas de segundos por diff. Cache por hash de contenido en `~/.cache/riku/` | M |
| 10 | Triangular cóncavos al cargar | Hoy se triangulan en cada frame; solo pesa en layouts grandes | S |
| 11 | Autocompletado en el shell (`riku` sin argumentos) | El shell ya tiene historial (rustyline); falta completar comandos y rutas | S |
| 12 | Build en Windows | MSVC 2019 falla (LNK1171/OOM/DLLs vcpkg). Documentado en `docs/integracion_gds_estado.md` §6; solución real: VS 2022 o `rust-lld` | M |
| 13 | Tracking tipográfico por tamaño | La guía de diseño lo pide, pero egui no permite ajustar el espaciado entre letras | — (limitación del toolkit) |

---

## Decisiones pendientes (no son código)

- **`.agents/` y `skills-lock.json`** en la raíz: los creó el instalador de la skill `apple-design`. Decidir si se versionan (compartir la skill con el equipo) o se agregan al `.gitignore`.
- **Reescritura de historial del 2026-09-26:** avisar a colaboradores (p.ej. Carlos) que resincronicen con `git fetch && git reset --hard origin/main` si tenían los commits anteriores. La rama local `backup/antes-de-quitar-coautor` se puede borrar cuando ya no haga falta.
- **Pruebas con usuarios reales:** la GUI se probó con clics y teclado simulados y en una sola pantalla (1366×768). Falta ver a diseñadores usándola en su flujo real y en otras resoluciones.

---

## Hecho recientemente (referencia)

Resumen en `docs/integracion_gds_estado.md`. Hitos: diff GDS geométrico y jerárquico (CLI y GUI, verificado contra KLayout), selector de celdas, paletas SKY130/GF180/IHP, tooltip, etiquetas legibles, tema claro/oscuro, movimiento fluido, feedback con mensajes, test end-to-end.
