# Pendientes técnicos

Lista única de trabajo abierto de riku_chip, ordenada por prioridad. Cada item dice por qué importa, dónde tocar, cuánto cuesta y cuándo se considera terminado.

**Última revisión:** 2026-09-26 · **Estado de `main`:** ver `git log` (esta revisión cierra el diseño `docs/roadmap/diseno_pendientes.md`)

Esfuerzo: **S** = horas, **M** = 1–2 días, **L** = varios días.

---

## Alta prioridad

### 1. Paridad de la vista de esquemáticos con la de GDS — hecho (2026-09-26)
- El visor dibuja `.sch` y `.gds` por la misma ruta (fase 4 de `diseno_arquitectura_final.md`): los esquemáticos ganaron tooltip, capas activables, animación, inercia, atajos `+`/`−`/`F` y la lista de cambios con "ir al cambio" y el detalle de parámetros. Fantasmas, recuadros por componente y nets resaltadas se conservan como overlays de la escena.
- Un `.sch` nuevo o eliminado entre dos commits ahora lista todos sus componentes y nets como añadidos o eliminados (un lado vacío es un esquemático sin nada, como en los layouts).

### 2. Diff de layouts muy grandes — hecho en lo principal (2026-09-26)
- **Por qué:** en un `user_project_wrapper` de 42 MB (IHP SG13G2), el primer `riku diff` **no terminó en 45 minutos**. Medido por etapas (2026-09-26): leer 0,5 s, huellas de celda 4,2 s, aplanar por capa unos segundos, y **XOR ~22 min**, casi todo sobre capas idénticas en A y B; la capa 19/0 (124 mil rectángulos) sola tarda 358 s por el peor caso de Clipper. El visor con ese layout usa 11 GB y ~600 ms por cuadro.
- **Dónde y cómo:** Fase 6, [`diseno_fase6_rendimiento.md`](diseno_fase6_rendimiento.md): huella por capa antes del XOR (~22 min → ~12 s en un núcleo), índice espacial y nivel de detalle en el visor, gdstk-rs seguro entre hilos, `rayon`, XOR por cuadrantes.
- **Resultado (fase 6.1 + 6.5.a):** el primer diff de ese layout baja de ~22 min a **6,4 s** (1,7 GB de pico), con resultado igual a KLayout (área de diferencia 0; KLayout tarda 26 s). Con un cambio real en las capas 6/0 y 19/0: 16 s y áreas idénticas a KLayout. La causa real: B era una reexportación (los mismos polígonos escritos con otro vértice de inicio o sentido de giro), así que nada coincidía y todo pasaba por el XOR completo.
- **Visor (fase 6.2, hecho):** ese layout abre con 2,5 GB (antes 11) y ~2 ms por cuadro con el chip completo (antes ~600). Límite: zoom cercano sobre una zona muy densa, ~45 ms por cuadro.
- **Queda (resto de la fase 6):** aprovechar varios núcleos en el diff y en `log`.

### 3. Empaquetado e instalación — hecho (2026-09-26)
- Un solo ejecutable `riku` (visor incluido). `.github/workflows/release.yml` compila en Ubuntu 22.04 con zlib, qhull y libstdc++ estáticas (`GDSTK_STATIC`) y verifica que el binario solo dependa de glibc. Publica `.tar.gz` (5,6 MB, con `install.sh`, entrada de menú e icono), `.deb` (4,1 MB) y `SHA256SUMS` con cada tag `v*`. Probado en un Ubuntu 22.04 limpio (tar y deb) y con el visor abierto.
- **Falta:** publicar la primera versión (crear el tag) y agregar el archivo `LICENSE` (el README dice MIT pero el archivo no existe).

---

## Media prioridad

### 4. Build en Windows
- **Por qué:** con MSVC 2019 local falla (LNK1171, OOM, DLLs de vcpkg). La CI tiene un job `windows (no bloqueante)` con VS 2022 y vcpkg. Tras aceptar el `z.lib` de zlib 1.3.2 (commit `ca86886` en gdstk_rust), **los tests de riku-mod-layout, riku y riku-gui pasan en Windows** (run `36271142299`). Falta ver que se mantenga estable y documentar la instalación en Windows (vcpkg + DLLs junto al `.exe`).
- **Dónde:** `.github/workflows/ci.yml` (job `windows`) y `external/gdstk/rust/build.rs`. Si el problema es el linker, probar `rust-lld` en `.cargo/config.toml` solo para Windows.
- **Esfuerzo:** M (exploratorio).
- **Listo cuando:** el job queda en verde una semana y se le quita `continue-on-error`.

### 5. Pestaña "Antes" de una celda renombrada
- **Por qué:** en el diff de la GUI, la pestaña **Diff** de una celda renombrada compara bien contra su nombre anterior. La pestaña **Antes**, en cambio, busca el nombre nuevo en la versión A, donde no existe.
- **Dónde:** `riku/src/gui/app.rs` (`select_diff_tab`) y `GdsBackend::load_entry`: pasar el nombre anterior cuando la entrada está marcada como renombrada.
- **Esfuerzo:** S.

### 6. Renombres con cambios
- **Por qué:** hoy solo se detecta el renombre *puro*, con geometría idéntica. Una celda renombrada que además cambió sigue apareciendo como baja + alta.
- **Dónde:** `gds_diff.rs` (`detect_renames`). Emparejar por similitud: misma bbox y la mayoría de las huellas de polígono en común.
- **Esfuerzo:** M.

### 7. Nivel de detalle (LOD) para layouts enormes
- **Por qué:** con 6,2 M de polígonos (el mismo wrapper de 42 MB), cargar la escena tarda ~5 s. Con el zoom alejado se dibujan millones de polígonos más chicos que un píxel.
- **Dónde:** `riku/src/gui/scene_painter.rs`: no dibujar polígonos de menos de ~1 px (o dibujarlos como un punto) y agrupar por celda al alejarse.
- **Esfuerzo:** M.

---

## Baja prioridad

| # | Tema | Por qué | Esfuerzo |
|---|---|---|---|
| 8 | Formato con `cargo fmt` en la CI | El código no está formateado de forma uniforme; activarlo implica un commit grande solo de formato | S |
| 9 | Clippy en la CI | Primero como job no bloqueante, hasta dejarlo limpio | S |
| 10 | Paleta SKY130 completa desde su `.lyp` | `tools/palettes/gen_palettes.py` ya lo hace para GF180 e IHP; agregar SKY130 (429 capas) es sumar una entrada a `PDKS` | S |
| 11 | Exportar SVG/PNG desde el visor | Botón "Exportar…" que escriba la escena neutra de `viewer-core` (sirve para `.sch` y `.gds`) o capture el lienzo; opcional `riku render` para scripts. El render SVG antiguo de riku-mod-layout nunca se usó y se borra | S |

---

## Limitaciones conocidas (no se van a resolver por ahora)

- **Espaciado entre letras por tamaño:** la guía de diseño lo pide, pero egui 0.34 no permite ajustar el espaciado entre letras.
- **Triangular los polígonos cóncavos al cargar:** se midió y no vale la pena. En 6,2 M de polígonos, triangular los 3 861 cóncavos cuesta 4 ms por frame y detectar si un polígono es convexo, 119 ms, frente a segundos para dibujar todo. La ganancia queda por debajo del 10 % que se había puesto como umbral; lo que de verdad ayuda es el LOD (#7).

---

## Decisiones pendientes (no son código)

- **`.agents/` y `skills-lock.json`** en la raíz: los creó el instalador de la skill `apple-design`. Falta decidir si se versionan (para compartir la skill con el equipo) o se agregan al `.gitignore`.
- **Reescritura del historial del 2026-09-26:** avisar a los colaboradores (p. ej. Carlos) que resincronicen con `git fetch && git reset --hard origin/main` si tenían los commits anteriores. La rama local `backup/antes-de-quitar-coautor` se puede borrar cuando ya no haga falta.
- **Pruebas con usuarios reales:** la GUI se probó con clics y teclado simulados, en una sola pantalla (1366×768). Falta ver a diseñadores usándola en su flujo real y en otras resoluciones.

---

## Hecho recientemente (referencia)

Resumen en `docs/integracion_gds_estado.md`. En esta tanda (diseño en `docs/roadmap/diseno_pendientes.md`):
- warnings `f32` resueltos;
- CI en GitHub Actions: tests de los 4 crates con `-D warnings`, compatibilidad del visor Xschem y job de Windows;
- orden de etiquetas con la alimentación primero;
- un cambio por instancia;
- celdas renombradas;
- soporte OASIS;
- `tools/verify/` con la verificación contra KLayout (idéntico en SKY130, GF180 e IHP);
- autocompletado con Tab en el shell;
- paletas GF180/IHP completas;
- cache del diff.
