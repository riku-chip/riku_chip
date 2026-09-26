# Pendientes técnicos

Lista única de trabajo abierto de riku_chip, ordenada por prioridad. Cada item dice por qué importa, dónde tocar, cuánto cuesta y cuándo se considera terminado.

**Última revisión:** 2026-09-26 · **Estado de `main`:** ver `git log` (esta revisión cierra el diseño `docs/roadmap/diseno_pendientes.md`)

Esfuerzo: **S** = horas, **M** = 1–2 días, **L** = varios días.

---

## Alta prioridad

### 1. Paridad de la vista de esquemáticos (`.sch`) con la de GDS
- **Por qué:** la ruta Xschem usa su painter propio (`sch_painter.rs`) y no recibió lo que se agregó para GDS. No tiene tooltip; las etiquetas van sin pastillas ni anti-solapamiento; no hay animación ni inercia; `+`/`−` no hacen zoom; y conserva un slider de zoom que la vista GDS no tiene. La experiencia cambia según el tipo de archivo.
- **Dónde:** `riku-gui/src/sch_painter.rs` y la rama `self.sch` de `app.rs`. Opción de fondo: pasar Xschem por la ruta neutra (`XschemBackend` ya existe), conservando fantasmas y anotaciones como overlays.
- **Esfuerzo:** M–L.
- **Listo cuando:** las mismas interacciones funcionan igual en `.sch` y en `.gds`.

### 2. Diff de layouts muy grandes
- **Por qué:** en un `user_project_wrapper` de 42 MB (Caravel), el primer `riku diff` tarda BENCH_COLD. La cache (#9 del diseño) hace que la segunda vez tarde BENCH_WARM, pero la primera sigue siendo lenta: se aplana la jerarquía completa de cada celda, y las celdas que instancian a otras repiten el trabajo.
- **Dónde:** `gds-renderer/src/gds_diff.rs`. Idea: una huella *estructural* por celda (polígonos propios + references con su transformación, combinada con la de sus hijas y memoizada), que detecta las celdas sin cambios en O(formas) y sin aplanar. El XOR queda solo para las celdas que cambiaron de verdad, empezando por las hojas.
- **Esfuerzo:** M.
- **Listo cuando:** el primer diff de ese layout baja de un minuto y los tests contra KLayout (`tools/verify/compare.sh --xor`) siguen idénticos.

### 3. Empaquetado e instalación
- **Por qué:** hoy Riku se instala compilando (`cargo install --path riku` y `--path riku-gui`). Los binarios de release pesan 3,3 MB (`riku`) y 11 MB (`riku-gui`) sin símbolos. En Linux dependen de `libssl`, `zlib`, `libqhull_r` y `libstdc++`.
- **Dónde:** un workflow `release.yml` que, con cada tag `v*`, publique:
  - un `.tar.gz` con los dos binarios y un `install.sh` que los copia a `~/.local/bin`;
  - un `.deb` generado con `cargo-deb`, con `libqhull-r8.0` como dependencia.

  `riku` ya encuentra `riku-gui` junto a su propio ejecutable o en el `PATH`.
- **Esfuerzo:** M.
- **Listo cuando:** en una máquina Linux limpia, `tar xf riku-*.tar.gz && ./install.sh`, o `apt install ./riku_*.deb`, deja `riku` y `riku-gui` listos para usar desde cualquier terminal.

---

## Media prioridad

### 4. Build en Windows
- **Por qué:** con MSVC 2019 local falla (LNK1171, OOM, DLLs de vcpkg). La CI tiene un job `windows (no bloqueante)` con VS 2022 y vcpkg. Resultado del primer intento: WINDOWS_RESULT.
- **Dónde:** `.github/workflows/ci.yml` (job `windows`) y `external/gdstk/rust/build.rs`. Si el problema es el linker, probar `rust-lld` en `.cargo/config.toml` solo para Windows.
- **Esfuerzo:** M (exploratorio).
- **Listo cuando:** el job queda en verde una semana y se le quita `continue-on-error`.

### 5. Pestaña "Antes" de una celda renombrada
- **Por qué:** en el diff de la GUI, la pestaña **Diff** de una celda renombrada compara bien contra su nombre anterior. La pestaña **Antes**, en cambio, busca el nombre nuevo en la versión A, donde no existe.
- **Dónde:** `riku-gui/src/app.rs` (`select_diff_tab`) y `GdsBackend::load_entry`: pasar el nombre anterior cuando la entrada está marcada como renombrada.
- **Esfuerzo:** S.

### 6. Renombres con cambios
- **Por qué:** hoy solo se detecta el renombre *puro*, con geometría idéntica. Una celda renombrada que además cambió sigue apareciendo como baja + alta.
- **Dónde:** `gds_diff.rs` (`detect_renames`). Emparejar por similitud: misma bbox y la mayoría de las huellas de polígono en común.
- **Esfuerzo:** M.

### 7. Nivel de detalle (LOD) para layouts enormes
- **Por qué:** con 6,2 M de polígonos (el mismo wrapper de 42 MB), cargar la escena tarda ~5 s. Con el zoom alejado se dibujan millones de polígonos más chicos que un píxel.
- **Dónde:** `riku-gui/src/scene_painter.rs`: no dibujar polígonos de menos de ~1 px (o dibujarlos como un punto) y agrupar por celda al alejarse.
- **Esfuerzo:** M.

---

## Baja prioridad

| # | Tema | Por qué | Esfuerzo |
|---|---|---|---|
| 8 | Formato con `cargo fmt` en la CI | El código no está formateado de forma uniforme; activarlo implica un commit grande solo de formato | S |
| 9 | Clippy en la CI | Primero como job no bloqueante, hasta dejarlo limpio | S |
| 10 | Paleta SKY130 completa desde su `.lyp` | `tools/palettes/gen_palettes.py` ya lo hace para GF180 e IHP; agregar SKY130 (429 capas) es sumar una entrada a `PDKS` | S |

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
